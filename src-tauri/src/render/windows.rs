//! WebView2 renderer (FR-055): `ICoreWebView2_7::PrintToPdf` in a hidden
//! window of a windowless Tauri app (no plugins, no IPC for the page).
//!
//! 1. The web view gets a private user-data folder inside the job's work
//!    directory, its `Local State` stamped first so Chromium's blank-password
//!    logon probe never runs (it records a failed Windows logon per start;
//!    [`crate::util::chromium`]).
//! 2. Before anything loads: scripts, dialogs, web messages, dev tools and
//!    context menus off; the staged page's folder is mapped to
//!    `https://`[`STAGE_HOST`] and the document's folder to
//!    `https://`[`DOC_HOST`] (no CORS); every request goes through
//!    [`super::is_allowed_request`], which answers anything else with a 403
//!    before it leaves the process; every navigation other than the page's
//!    own is cancelled.
//! 3. When the page's navigation completes, `PrintToPdf` writes the PDF on
//!    the job's paper with no header or footer, and the app exits.
//!
//! Serving the folders over a private `https` origin instead of `file:` is
//! what keeps a page from reaching a file share (module docs of
//! [`super`]): Chromium refuses `file:` sub-resources to a non-`file` page,
//! so `file://host/share/x.png` is never opened, and a protocol-relative
//! `//host/x.png` becomes `https://host/x.png`, which the filter refuses.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{WebviewUrl, WebviewWindowBuilder};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Environment, ICoreWebView2Environment6, ICoreWebView2_3,
    ICoreWebView2_7, COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS,
    COREWEBVIEW2_WEB_ERROR_STATUS, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
};
use webview2_com::{
    take_pwstr, NavigationCompletedEventHandler, NavigationStartingEventHandler,
    PrintToPdfCompletedHandler, WebResourceRequestedEventHandler,
};
use windows_core::{Interface, BOOL, HSTRING, PWSTR};

use super::{RenderRequest, DOC_HOST, STAGE_HOST};

/// How long the whole render may take before the worker gives up by itself.
/// The engine's conversion timeout normally kills it earlier.
const RENDER_LIMIT: Duration = Duration::from_secs(300);

/// Chromium switches for the worker's web view. The first group is what wry
/// passes by default (replaced, not added to, when switches are given). The
/// rest is defence in depth behind the request filter: no background
/// fetches, no component updates, and any request that still got out would
/// meet a dead proxy on loopback and a resolver that resolves nothing (the
/// same switches the browser fallback uses, [`crate::engine::convert::html`]).
const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
     --disable-background-networking --disable-component-update --disable-sync \
     --no-first-run --proxy-server=127.0.0.1:9 --proxy-bypass-list=<-loopback> \
     --host-resolver-rules=\"MAP * ~NOTFOUND\" --disable-quic";

#[derive(Default)]
struct State {
    /// The id of the page's own navigation, once it started.
    navigation: Option<u64>,
    /// `PrintToPdf` was issued.
    printing: bool,
    /// How the render ended; set once.
    outcome: Option<Result<(), String>>,
}

type Shared = Arc<Mutex<State>>;

/// Record the outcome (first one wins) and end the worker.
///
/// The process exits right here, with the worker's exit code: asking the
/// Tauri app to exit (`AppHandle::exit`) leaves `run_return` waiting on the
/// hidden window (seen on Windows 11 ARM64), and nothing is left to do once
/// `PrintToPdf` has reported. The web view's own processes follow their host.
fn finish(state: &Shared, app: &tauri::AppHandle, outcome: Result<(), String>) {
    let mut st = state.lock().unwrap_or_else(|p| p.into_inner());
    if st.outcome.is_none() {
        st.outcome = Some(outcome);
    }
    let code = match st.outcome.as_ref() {
        Some(Ok(())) => crate::worker::EXIT_OK,
        Some(Err(message)) => {
            eprintln!("pluryprint worker render-pdf: {message}");
            crate::worker::EXIT_FAILED
        }
        None => crate::worker::EXIT_FAILED,
    };
    drop(st);
    trace(format_args!("exit {code}"));
    app.exit(code);
    std::process::exit(code);
}

/// The URL the staged page is loaded from.
fn page_url(page: &Path) -> String {
    let name = page
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "page.html".into());
    const KEEP: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'.')
        .remove(b'-')
        .remove(b'_');
    let encoded = percent_encoding::utf8_percent_encode(&name, KEEP);
    format!("https://{STAGE_HOST}/{encoded}")
}

fn hr(context: &str) -> impl Fn(windows_core::Error) -> String + '_ {
    move |e| format!("{context}: {e}")
}

/// With `PLURYPRINT_RENDER_TRACE` set, each stage of the render is written to
/// stderr (which the engine keeps in the job's `render.log`), for diagnosing
/// a web view that never finishes on some machine.
fn trace(message: impl std::fmt::Display) {
    static ENABLED: std::sync::OnceLock<Option<std::time::Instant>> = std::sync::OnceLock::new();
    if let Some(started) = ENABLED.get_or_init(|| {
        std::env::var_os("PLURYPRINT_RENDER_TRACE").map(|_| std::time::Instant::now())
    }) {
        eprintln!(
            "render-trace {:>8.3}s {message}",
            started.elapsed().as_secs_f64()
        );
    }
}

/// Render `request` (see [`super::render`]).
pub fn render(request: &RenderRequest) -> Result<(), String> {
    // A wedged web view must not keep the worker alive forever when the
    // engine runs without a conversion timeout.
    std::thread::spawn(|| {
        std::thread::sleep(RENDER_LIMIT);
        eprintln!("pluryprint worker render-pdf: the render did not finish in time");
        std::process::exit(crate::worker::EXIT_FAILED);
    });

    let profile = request.data_dir.join("EBWebView");
    crate::util::chromium::suppress_blank_password_probe(&profile)
        .map_err(|e| format!("cannot prepare the web view folder: {e}"))?;
    let stage_dir = request
        .page
        .parent()
        .ok_or("the staged page has no folder")?
        .to_path_buf();

    trace(format_args!("start, page {}", request.page.display()));
    let app = crate::render_worker_app().map_err(|e| format!("cannot start the web view: {e}"))?;
    trace("app built");
    let handle = app.handle().clone();
    let state: Shared = Arc::default();

    let window = WebviewWindowBuilder::new(
        &app,
        "render",
        WebviewUrl::External("about:blank".parse().expect("valid URL")),
    )
    .visible(false)
    .focused(false)
    .skip_taskbar(true)
    .data_directory(request.data_dir.clone())
    .additional_browser_args(BROWSER_ARGS)
    .build()
    .map_err(|e| format!("cannot create the web view: {e}"))?;
    trace("window built");

    let request = request.clone();
    let st = state.clone();
    let h = handle.clone();
    window
        .with_webview(move |pw| {
            trace("web view ready");
            // SAFETY: WebView2 COM calls on the UI thread that owns the view.
            let started = unsafe {
                start(
                    pw.controller().CoreWebView2(),
                    pw.environment(),
                    &request,
                    &stage_dir,
                    st.clone(),
                    h.clone(),
                )
            };
            if let Err(e) = started {
                finish(&st, &h, Err(e));
            }
        })
        .map_err(|e| format!("cannot reach the web view: {e}"))?;

    let code = app.run_return(|_, _| {});
    trace(format_args!("event loop ended ({code})"));
    let outcome = state
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .outcome
        .take();
    outcome.unwrap_or_else(|| Err("the web view closed before printing".into()))
}

/// Configure the web view and start loading the page (module docs).
unsafe fn start(
    core: windows_core::Result<ICoreWebView2>,
    env: ICoreWebView2Environment,
    request: &RenderRequest,
    stage_dir: &Path,
    state: Shared,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let core = core.map_err(hr("no web view"))?;

    let settings = core.Settings().map_err(hr("settings"))?;
    settings
        .SetIsScriptEnabled(false)
        .map_err(hr("disabling scripts"))?;
    let _ = settings.SetAreDefaultScriptDialogsEnabled(false);
    let _ = settings.SetIsWebMessageEnabled(false);
    let _ = settings.SetAreDevToolsEnabled(false);
    let _ = settings.SetAreDefaultContextMenusEnabled(false);
    let _ = settings.SetIsStatusBarEnabled(false);

    let core3: ICoreWebView2_3 = core
        .cast()
        .map_err(hr("WebView2 too old (no folder mapping)"))?;
    core3
        .SetVirtualHostNameToFolderMapping(
            &HSTRING::from(STAGE_HOST),
            &HSTRING::from(stage_dir.as_os_str()),
            COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS,
        )
        .map_err(hr("mapping the page folder"))?;
    core3
        .SetVirtualHostNameToFolderMapping(
            &HSTRING::from(DOC_HOST),
            &HSTRING::from(request.base_dir.as_os_str()),
            COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS,
        )
        .map_err(hr("mapping the document folder"))?;

    // Every request, whatever its kind, is checked; refused ones get a 403
    // without ever reaching the network.
    core.AddWebResourceRequestedFilter(&HSTRING::from("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)
        .map_err(hr("request filter"))?;
    let filter_env = env.clone();
    let mut token = 0i64;
    core.add_WebResourceRequested(
        &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let request = args.Request()?;
            let mut uri = PWSTR::null();
            request.Uri(&mut uri)?;
            let uri = take_pwstr(uri);
            let allowed = super::is_allowed_request(&uri);
            trace(format_args!("request {uri} allowed={allowed}"));
            if !allowed {
                let response = filter_env.CreateWebResourceResponse(
                    None,
                    403,
                    &HSTRING::from("Blocked"),
                    &HSTRING::new(),
                )?;
                args.SetResponse(&response)?;
            }
            Ok(())
        })),
        &mut token,
    )
    .map_err(hr("request handler"))?;

    let target = page_url(&request.page);
    let nav_state = state.clone();
    let nav_target = target.clone();
    core.add_NavigationStarting(
        &NavigationStartingEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut uri = PWSTR::null();
            args.Uri(&mut uri)?;
            let uri = take_pwstr(uri);
            let mut id = 0u64;
            args.NavigationId(&mut id)?;
            let mut st = nav_state.lock().unwrap_or_else(|p| p.into_inner());
            if st.navigation.is_none() && uri.eq_ignore_ascii_case(&nav_target) {
                st.navigation = Some(id);
                trace(format_args!("navigation {id} to the page {uri}"));
            } else if st.navigation.is_some() || uri != "about:blank" {
                // A refresh, a link target, a redirect: nothing but the page.
                trace(format_args!("navigation {id} to {uri} cancelled"));
                args.SetCancel(true)?;
            } else {
                trace(format_args!("navigation {id} to {uri} allowed"));
            }
            Ok(())
        })),
        &mut token,
    )
    .map_err(hr("navigation handler"))?;

    let done_state = state.clone();
    let done_core = core.clone();
    let paper = request.paper_in;
    let output = request.output.clone();
    let done_app = app.clone();
    core.add_NavigationCompleted(
        &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut id = 0u64;
            args.NavigationId(&mut id)?;
            let mut ok = BOOL(0);
            args.IsSuccess(&mut ok)?;
            let mut status = COREWEBVIEW2_WEB_ERROR_STATUS(0);
            let _ = args.WebErrorStatus(&mut status);
            trace(format_args!(
                "navigation {id} completed ok={} status={}",
                ok.as_bool(),
                status.0
            ));
            {
                let mut st = done_state.lock().unwrap_or_else(|p| p.into_inner());
                if st.navigation != Some(id) || st.printing {
                    return Ok(());
                }
                st.printing = true;
            }
            if !ok.as_bool() {
                finish(
                    &done_state,
                    &done_app,
                    Err(format!(
                        "the page could not be loaded (status {})",
                        status.0
                    )),
                );
                return Ok(());
            }
            if let Err(e) = print(&done_core, &env, paper, &output, &done_state, &done_app) {
                finish(&done_state, &done_app, Err(e));
            }
            Ok(())
        })),
        &mut token,
    )
    .map_err(hr("completion handler"))?;

    trace(format_args!("navigating to {target}"));
    core.Navigate(&HSTRING::from(target.as_str()))
        .map_err(hr("loading the page"))?;
    Ok(())
}

/// Issue `PrintToPdf` for the loaded page.
fn print(
    core: &ICoreWebView2,
    env: &ICoreWebView2Environment,
    paper: Option<(f64, f64)>,
    output: &Path,
    state: &Shared,
    app: &tauri::AppHandle,
) -> Result<(), String> {
    // SAFETY: WebView2 COM calls on the UI thread (inside its own callback).
    unsafe {
        let core7: ICoreWebView2_7 = core.cast().map_err(hr("WebView2 too old (no PDF)"))?;
        let env6: ICoreWebView2Environment6 = env
            .cast()
            .map_err(hr("WebView2 too old (no print settings)"))?;
        let settings = env6.CreatePrintSettings().map_err(hr("print settings"))?;
        if let Some((width, height)) = paper {
            settings.SetPageWidth(width).map_err(hr("page width"))?;
            settings.SetPageHeight(height).map_err(hr("page height"))?;
        }
        settings
            .SetShouldPrintHeaderAndFooter(false)
            .map_err(hr("header and footer"))?;
        settings
            .SetShouldPrintBackgrounds(true)
            .map_err(hr("backgrounds"))?;
        let _ = std::fs::remove_file(output);
        let st = state.clone();
        let app = app.clone();
        trace("PrintToPdf issued");
        core7
            .PrintToPdf(
                &HSTRING::from(output.as_os_str()),
                &settings,
                &PrintToPdfCompletedHandler::create(Box::new(move |result, ok| {
                    trace(format_args!("PrintToPdf finished: {result:?} ok={ok}"));
                    let outcome = if result.is_ok() && ok {
                        Ok(())
                    } else {
                        Err(format!("WebView2 could not print the page ({result:?})"))
                    };
                    finish(&st, &app, outcome);
                    Ok(())
                })),
            )
            .map_err(hr("printing"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fr_055_page_url_is_on_the_stage_origin_and_encoded() {
        assert_eq!(
            page_url(Path::new(r"C:\w\stage\page.html")),
            "https://stage.pluryprint.example/page.html"
        );
        assert!(super::super::is_allowed_request(&page_url(Path::new(
            r"C:\w\stage\a b.html"
        ))));
    }
}
