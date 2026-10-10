//! WebView2 renderer (FR-055): `ICoreWebView2_7::PrintToPdf` in a hidden
//! window of a windowless Tauri app (no plugins, no IPC for the page).
//!
//! 1. The web view gets a private user-data folder inside the job's work
//!    directory, its `Local State` stamped first so Chromium's blank-password
//!    logon probe never runs (it records a failed Windows logon per start;
//!    [`crate::util::chromium`]).
//! 2. Before anything loads: scripts, dialogs, web messages, dev tools and
//!    context menus off; every request goes through
//!    [`super::is_allowed_request`], which answers anything else with a 403
//!    before it leaves the process, and every request to `https://`[`DOC_HOST`]
//!    is answered by the worker itself ([`super::serve`]): the staged page at
//!    [`super::page_url`] (with the page's Content-Security-Policy as a
//!    response header too), and files inside the document's folder, streamed
//!    from disk. Every navigation other than the page's own is cancelled.
//! 3. When the page's navigation completes, `PrintToPdf` writes the PDF on
//!    the job's paper with no header or footer, and the app exits.
//!
//! Serving over a private `https` origin instead of `file:` is what keeps a
//! page from reaching a file share (module docs of [`super`]): Chromium
//! refuses `file:` sub-resources to a non-`file` page, so
//! `file://host/share/x.png` is never opened, and a protocol-relative
//! `//host/x.png` becomes `https://host/x.png`, which the filter refuses.
//! Serving the page and its folder from that *one* origin (instead of two
//! folder mappings) is what lets same-folder web fonts and external SVG
//! `<use>` load: both need the same origin, and a second origin with
//! `DENY_CORS` refused them.

use std::path::Path;
use std::sync::{Arc, Mutex};

use tauri::{WebviewUrl, WebviewWindowBuilder};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Environment, ICoreWebView2Environment6,
    ICoreWebView2WebResourceResponse, ICoreWebView2_7, COREWEBVIEW2_WEB_ERROR_STATUS,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
};
use webview2_com::{
    take_pwstr, NavigationCompletedEventHandler, NavigationStartingEventHandler,
    PrintToPdfCompletedHandler, WebResourceRequestedEventHandler,
};
use windows_core::{Interface, BOOL, HSTRING, PWSTR};
use windows_webview::Win32::System::Com::{IStream, STGM_READ, STGM_SHARE_DENY_WRITE};
use windows_webview::Win32::UI::Shell::SHCreateStreamOnFileEx;

use super::{RenderRequest, Served};

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

/// The response to a request the worker answers itself ([`super::serve`]):
/// the file streamed from disk with its content type (and the page's policy
/// as a header for the staged page), or a bodiless 403/404.
///
/// # Safety
/// WebView2 COM calls; run on the web view's UI thread.
unsafe fn respond(
    env: &ICoreWebView2Environment,
    served: Served,
) -> windows_core::Result<ICoreWebView2WebResourceResponse> {
    let (path, headers) = match served {
        Served::Page(path) => {
            let headers = format!(
                "Content-Type: text/html\nContent-Security-Policy: {}",
                crate::engine::convert::html::page_csp()
            );
            (path, headers)
        }
        Served::File(path) => {
            let headers = format!("Content-Type: {}", super::content_type(&path));
            (path, headers)
        }
        Served::NotFound => {
            return env.CreateWebResourceResponse(
                None,
                404,
                &HSTRING::from("Not Found"),
                &HSTRING::new(),
            )
        }
        Served::Refused => {
            return env.CreateWebResourceResponse(
                None,
                403,
                &HSTRING::from("Blocked"),
                &HSTRING::new(),
            )
        }
    };
    let stream: IStream = match SHCreateStreamOnFileEx(
        &HSTRING::from(path.as_os_str()),
        (STGM_READ | STGM_SHARE_DENY_WRITE).0,
        0,
        false,
        None::<&IStream>,
    ) {
        Ok(stream) => stream,
        Err(e) => {
            trace(format_args!("cannot open {}: {e}", path.display()));
            return env.CreateWebResourceResponse(
                None,
                404,
                &HSTRING::from("Not Found"),
                &HSTRING::new(),
            );
        }
    };
    env.CreateWebResourceResponse(&stream, 200, &HSTRING::from("OK"), &HSTRING::from(headers))
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
    // The job's conversion timeout bounds the whole render, also when the
    // engine that started this worker is gone; the setting 0 means no limit.
    // Running out prints the timeout marker, so the job ends as a timeout,
    // not as a render failure the queue would retry.
    super::start_watchdog(request.limit());

    let profile = request.data_dir.join("EBWebView");
    crate::util::chromium::suppress_blank_password_probe(&profile)
        .map_err(|e| format!("cannot prepare the web view folder: {e}"))?;

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

    // Every request, whatever its kind, is checked; refused ones get a 403
    // without ever reaching the network, and the private origin is answered
    // here, from the staged page and the document's folder only.
    core.AddWebResourceRequestedFilter(&HSTRING::from("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)
        .map_err(hr("request filter"))?;
    let filter_env = env.clone();
    let page = request.page.clone();
    let base_dir = request.base_dir.clone();
    let mut token = 0i64;
    core.add_WebResourceRequested(
        &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let request = args.Request()?;
            let mut uri = PWSTR::null();
            request.Uri(&mut uri)?;
            let uri = take_pwstr(uri);
            let allowed = super::is_allowed_request(&uri);
            let lower = uri.trim().to_ascii_lowercase();
            let local = lower.starts_with("data:") || lower == "about:blank";
            let served = match (allowed, local) {
                // `data:` and `about:blank` load as they are.
                (true, true) => None,
                (true, false) => Some(super::serve(&uri, &page, &base_dir)),
                (false, _) => Some(Served::Refused),
            };
            trace(format_args!("request {uri} -> {served:?}"));
            if let Some(served) = served {
                let response = respond(&filter_env, served)?;
                args.SetResponse(&response)?;
            }
            Ok(())
        })),
        &mut token,
    )
    .map_err(hr("request handler"))?;

    let target = super::page_url(&request.page);
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
    fn fr_055_page_url_is_on_the_document_origin_and_encoded() {
        assert_eq!(
            super::super::page_url(Path::new(r"C:\w\stage\page.html")),
            "https://doc.pluryprint.example/.pluryprint-stage/page.html"
        );
        let page = Path::new(r"C:\w\stage\a b.html");
        let url = super::super::page_url(page);
        assert!(super::super::is_allowed_request(&url));
        assert_eq!(
            super::super::serve(&url, page, Path::new(r"C:\docs")),
            Served::Page(page.to_path_buf())
        );
    }
}
