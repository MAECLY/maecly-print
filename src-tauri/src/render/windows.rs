//! WebView2 renderer (FR-055): `ICoreWebView2_7::PrintToPdf` in a hidden
//! window of a windowless Tauri app (no plugins, no IPC for the page).
//!
//! 1. The web view gets a private user-data folder inside the job's work
//!    directory, its `Local State` stamped first so Chromium's blank-password
//!    logon probe never runs (it records a failed Windows logon per start;
//!    [`crate::util::chromium`]).
//! 2. Before anything loads: scripts, dialogs, web messages, dev tools and
//!    context menus off; a `WebResourceRequested` filter on `*` for every
//!    resource context (and every request source, where the runtime has
//!    `ICoreWebView2_22`) sends **every** request through
//!    [`super::answer_request`]: inline `data:`/`blob:`/`about:blank` load as
//!    they are, a request to `https://`[`DOC_HOST`] is answered by the worker
//!    itself ([`super::serve`]: the staged page at [`super::page_url`], with
//!    the page's Content-Security-Policy as a response header too, and files
//!    inside the document's folder, streamed from disk), and anything else
//!    gets a synthetic 403 before it leaves the process, logged to stderr
//!    ([`super::log_refusal`], the job's `render.log`). Every navigation
//!    other than the page's own, every frame navigation and every new window
//!    is cancelled and logged too.
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
    ICoreWebView2WebResourceResponse, ICoreWebView2_22, ICoreWebView2_7,
    COREWEBVIEW2_WEB_ERROR_STATUS, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
};
use webview2_com::{
    take_pwstr, NavigationCompletedEventHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, PrintToPdfCompletedHandler, WebResourceRequestedEventHandler,
};
use windows_core::{Interface, BOOL, HSTRING, PWSTR};
use windows_webview::Win32::System::Com::IStream;
use windows_webview::Win32::UI::Shell::SHCreateMemStream;

use super::{RenderRequest, Served};

/// Chromium switches for the worker's web view. The first group is what wry
/// passes by default (replaced, not added to, when switches are given). The
/// rest is defence in depth behind the request filter: no background
/// fetches, no component updates, and any request that still got out would
/// meet a dead proxy on loopback and a resolver that resolves nothing.
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

/// Debug builds only: set to `1`, the staged page is served without its
/// Content-Security-Policy header, so a test can show the request filter
/// alone refusing what the page asks for. Ignored by release builds.
const TEST_NO_CSP_HEADER_ENV: &str = "PLURYPRINT_RENDER_TEST_NO_CSP_HEADER";

/// Largest file the worker serves to the page. A bigger one is answered
/// 404 (it is read into memory once it has been checked on its handle).
const MAX_SERVED_BYTES: u64 = 256 * 1024 * 1024;

/// The bytes of `path`, a [`Served::File`] inside `root`, checked on the
/// open handle itself (security note 9): the final path Windows resolved for
/// that handle (junctions, symbolic links, 8.3 names and `..` all resolved)
/// must still lie inside the canonical `root`, and the bytes are read from
/// that same handle, so a file swapped for a link between [`super::serve`]'s
/// check and the read cannot reach outside the document's folder.
fn read_contained(path: &Path, root: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        GetFinalPathNameByHandleW, FILE_NAME_NORMALIZED, GETFINALPATHNAMEBYHANDLE_FLAGS,
        VOLUME_NAME_DOS,
    };

    let root = root
        .canonicalize()
        .map_err(|e| format!("the document folder: {e}"))?;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buf = vec![0u16; 1024];
    let final_path = loop {
        // SAFETY: a live handle and a buffer of the length passed.
        let n = unsafe {
            GetFinalPathNameByHandleW(
                HANDLE(file.as_raw_handle()),
                &mut buf,
                GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0),
            )
        } as usize;
        if n == 0 {
            return Err(format!("{}", std::io::Error::last_os_error()));
        }
        if n < buf.len() {
            break std::path::PathBuf::from(std::ffi::OsString::from_wide(&buf[..n]));
        }
        buf.resize(n + 1, 0);
    };
    if !final_path.starts_with(&root) {
        return Err(format!(
            "{} resolves outside the document folder",
            path.display()
        ));
    }
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_SERVED_BYTES {
        return Err(format!("{} is not a servable file", path.display()));
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(MAX_SERVED_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_SERVED_BYTES {
        return Err(format!("{} grew past the limit", path.display()));
    }
    Ok(bytes)
}

/// The response to a request the worker answers itself ([`super::serve`]):
/// the file's bytes (checked on their handle, [`read_contained`]) with its
/// content type (and the page's policy as a header for the staged page), or
/// a bodiless 403/404.
///
/// # Safety
/// WebView2 COM calls; run on the web view's UI thread.
unsafe fn respond(
    env: &ICoreWebView2Environment,
    served: Served,
    page: &Path,
    base_dir: &Path,
) -> windows_core::Result<ICoreWebView2WebResourceResponse> {
    let (path, root, headers) = match served {
        Served::Page(path) => {
            // Debug builds only: a test can drop the policy header to prove
            // that the request filter alone keeps a page off the network
            // (the policy otherwise stops most requests before the filter
            // sees them). Release builds always send it.
            let without_policy = cfg!(debug_assertions)
                && std::env::var_os(TEST_NO_CSP_HEADER_ENV).is_some_and(|v| v == "1");
            let headers = if without_policy {
                "Content-Type: text/html".to_string()
            } else {
                format!(
                    "Content-Type: text/html\nContent-Security-Policy: {}",
                    crate::engine::convert::html::page_csp()
                )
            };
            let root = page.parent().unwrap_or(page).to_path_buf();
            (path, root, headers)
        }
        Served::File(path) => {
            let headers = format!("Content-Type: {}", super::content_type(&path));
            (path, base_dir.to_path_buf(), headers)
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
    let bytes = match read_contained(&path, &root) {
        Ok(bytes) => bytes,
        Err(e) => {
            trace(format_args!("not serving {}: {e}", path.display()));
            super::log_refusal("file", &path.display().to_string());
            return env.CreateWebResourceResponse(
                None,
                404,
                &HSTRING::from("Not Found"),
                &HSTRING::new(),
            );
        }
    };
    let stream: Option<IStream> = SHCreateMemStream(Some(&bytes));
    env.CreateWebResourceResponse(
        stream.as_ref(),
        200,
        &HSTRING::from("OK"),
        &HSTRING::from(headers),
    )
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
    // On a runtime that has it, the filter also covers requests from every
    // source (workers included), not only the page's documents.
    match core.cast::<ICoreWebView2_22>() {
        Ok(core22) => core22.AddWebResourceRequestedFilterWithRequestSourceKinds(
            &HSTRING::from("*"),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
            COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
        ),
        Err(_) => core.AddWebResourceRequestedFilter(
            &HSTRING::from("*"),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
        ),
    }
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
            let served = super::answer_request(&uri, &page, &base_dir);
            trace(format_args!("request {uri} -> {served:?}"));
            if served == Some(Served::Refused) {
                super::log_refusal("request", &uri);
            }
            if let Some(served) = served {
                let response = respond(&filter_env, served, &page, &base_dir)?;
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
                super::log_refusal("navigation", &uri);
                args.SetCancel(true)?;
            } else {
                trace(format_args!("navigation {id} to {uri} allowed"));
            }
            Ok(())
        })),
        &mut token,
    )
    .map_err(hr("navigation handler"))?;

    // No frame ever navigates (the page's policy forbids frames too), and no
    // new window is ever opened: the request is marked handled without one.
    core.add_FrameNavigationStarting(
        &NavigationStartingEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut uri = PWSTR::null();
            args.Uri(&mut uri)?;
            let uri = take_pwstr(uri);
            trace(format_args!("frame navigation to {uri} cancelled"));
            super::log_refusal("frame navigation", &uri);
            args.SetCancel(true)
        })),
        &mut token,
    )
    .map_err(hr("frame navigation handler"))?;
    core.add_NewWindowRequested(
        &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut uri = PWSTR::null();
            args.Uri(&mut uri)?;
            let uri = take_pwstr(uri);
            trace(format_args!("new window for {uri} refused"));
            super::log_refusal("new window", &uri);
            args.SetHandled(true)
        })),
        &mut token,
    )
    .map_err(hr("new window handler"))?;

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

    /// Security note 9: a file is served only when the handle it was opened
    /// through still resolves inside the document folder. A junction inside
    /// the folder that points at its parent is refused on the handle.
    #[test]
    fn nfr_security_served_files_are_checked_on_their_handle() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().canonicalize().unwrap();
        let doc = parent.join("doc");
        std::fs::create_dir_all(&doc).unwrap();
        std::fs::write(doc.join("in.png"), b"inside").unwrap();
        std::fs::write(parent.join("secret.txt"), b"secret").unwrap();
        assert_eq!(
            read_contained(&doc.join("in.png"), &doc).unwrap(),
            b"inside"
        );
        assert!(read_contained(&parent.join("secret.txt"), &doc).is_err());
        // A junction (no privilege needed) inside the folder to its parent.
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(doc.join("up"))
            .arg(&parent)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        let through = doc.join("up").join("secret.txt");
        assert_eq!(std::fs::read(&through).unwrap(), b"secret");
        let err = read_contained(&through, &doc).unwrap_err();
        assert!(err.contains("outside the document folder"), "{err}");
        // ... and serve() never hands it out in the first place.
        let page = parent.join("stage").join("page.html");
        assert_eq!(
            super::super::serve("https://doc.pluryprint.example/up/secret.txt", &page, &doc),
            Served::Refused
        );
    }

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
