//! Tests for the converters that shell out (FR-052..FR-058).
//!
//! Two layers:
//!
//! - **Fake binaries** (Unix only — they are `/bin/sh` scripts): pin the
//!   argument vectors actually spawned, the "exit 0 but no output" case, the
//!   timeout kill and the scratch-folder cleanup without needing any real
//!   converter installed.
//! - **Real tools**: convert genuine fixtures generated in-test (nothing binary
//!   is committed). Each test locates its tool the same way the engine does and
//!   **skips with a note** when it is absent, so a CI runner without
//!   LibreOffice stays green while a developer machine with the tools runs the
//!   real conversion.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::*;

fn pages_of(pdf: &Path) -> usize {
    lopdf::Document::load(pdf)
        .unwrap_or_else(|e| panic!("{} must parse as a PDF: {e}", pdf.display()))
        .get_pages()
        .len()
}

/// The tool, or `None` with a printed note (the test then returns early).
fn tool_or_skip(dep: Dependency, test: &str) -> Option<PathBuf> {
    match locate(dep) {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("SKIPPED {test}: {e}");
            None
        }
    }
}

fn no_progress() -> impl Fn(f32) + Send + Sync {
    |_p: f32| {}
}

// ---------------------------------------------------------------------------
// Fake binaries
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod fakes {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Write an executable `/bin/sh` script to `dir/name`.
    pub fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// A fake `soffice` that records its argv and writes `<outdir>/<stem>.pdf`.
    fn fake_soffice(dir: &Path, argv_log: &Path) -> PathBuf {
        script(
            dir,
            "soffice",
            &format!(
                r#"printf '%s\n' "$@" > '{log}'
out=""; prev=""; last=""
for a in "$@"; do
  if [ "$prev" = "--outdir" ]; then out="$a"; fi
  prev="$a"; last="$a"
done
base=$(basename "$last"); stem="${{base%.*}}"
printf '%%PDF-1.4\n%%fake\n' > "$out/$stem.pdf""#,
                log = argv_log.display()
            ),
        )
    }

    #[test]
    fn fr_053_office_spawns_soffice_with_a_private_profile_and_returns_its_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("argv");
        let soffice = fake_soffice(dir.path(), &log);
        let input = dir.path().join("Informe $(whoami) final.docx");
        std::fs::write(&input, b"PK fake docx").unwrap();
        let output = dir.path().join("out.pdf");
        let work = WorkDir::new("job").unwrap();
        let cancel = AtomicBool::new(false);

        office::convert_with(&soffice, &input, &output, &work, &cancel, None).unwrap();

        assert!(validate_pdf_header(&output).is_ok());
        let argv = std::fs::read_to_string(&log).unwrap();
        let argv: Vec<&str> = argv.lines().collect();
        assert!(
            argv[0].starts_with("-env:UserInstallation=file://")
                && argv[0].contains("pluryprint-convert-job-"),
            "{argv:?}"
        );
        assert!(argv.contains(&"--headless"));
        // The hostile-looking original name never reaches the argv: the
        // converter works on its own staged copy.
        assert!(argv.last().unwrap().ends_with("input.docx"), "{argv:?}");
        assert!(!argv.iter().any(|a| a.contains("whoami")));
    }

    #[test]
    fn fr_053_soffice_exiting_zero_without_output_is_a_conversion_failure() {
        let dir = tempfile::tempdir().unwrap();
        let soffice = script(
            dir.path(),
            "soffice",
            "echo 'Error: source file could not be loaded' >&2; exit 0",
        );
        let input = dir.path().join("broken.odt");
        std::fs::write(&input, b"nope").unwrap();
        let work = WorkDir::new("job").unwrap();
        let cancel = AtomicBool::new(false);
        let err = office::convert_with(
            &soffice,
            &input,
            &dir.path().join("o.pdf"),
            &work,
            &cancel,
            None,
        )
        .unwrap_err();
        assert!(
            matches!(&err, EngineError::ConversionFailed(m) if m.contains("could not be loaded")),
            "{err:?}"
        );
    }

    #[test]
    fn fr_053_a_hung_converter_is_killed_at_the_timeout_and_its_folder_removed() {
        let dir = tempfile::tempdir().unwrap();
        let soffice = script(dir.path(), "soffice", "sleep 30");
        let input = dir.path().join("slow.docx");
        std::fs::write(&input, b"x").unwrap();
        let work = WorkDir::new("job").unwrap();
        let scratch = work.path().to_path_buf();
        let cancel = AtomicBool::new(false);
        let started = std::time::Instant::now();
        let err = office::convert_with(
            &soffice,
            &input,
            &dir.path().join("o.pdf"),
            &work,
            &cancel,
            Some(Duration::from_millis(300)),
        )
        .unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(
            matches!(&err, EngineError::ConversionTimedOut(m) if m.contains("did not finish")),
            "{err:?}"
        );
        drop(work);
        assert!(
            !scratch.exists(),
            "the scratch folder must not outlive the job"
        );
    }

    /// The browser fallback (D15) prints the staged page, never the user's
    /// file, and returns its PDF.
    #[test]
    fn fr_055_html_fallback_spawns_the_browser_on_the_staged_page() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("argv");
        let browser = script(
            dir.path(),
            "chrome",
            &format!(
                r#"printf '%s\n' "$@" > '{log}'
for a in "$@"; do
  case "$a" in --print-to-pdf=*) printf '%%PDF-1.4\n' > "${{a#--print-to-pdf=}}";; esac
done"#,
                log = log.display()
            ),
        );
        let page = dir.path().join("my page.html");
        std::fs::write(&page, "<h1>hi</h1>").unwrap();
        let output = dir.path().join("out.pdf");
        let work = WorkDir::new("job").unwrap();
        let cancel = AtomicBool::new(false);

        html::convert_with(&browser, &page, &output, &work, &cancel, None).unwrap();

        assert!(validate_pdf_header(&output).is_ok());
        let argv = std::fs::read_to_string(&log).unwrap();
        assert!(argv.contains("--headless\n"));
        assert!(argv.contains("--no-pdf-header-footer\n"));
        assert!(argv.trim_end().ends_with(&file_url(&page)), "{argv}");
        assert!(file_url(&page).contains("my%20page.html"));
    }

    /// The browser starts on a profile whose `Local State` already disables
    /// the blank-password logon probe (one failed Windows logon per run
    /// otherwise; ten lock the account).
    #[test]
    fn fr_055_the_browser_profile_is_seeded_before_launch() {
        let dir = tempfile::tempdir().unwrap();
        let seen = dir.path().join("local-state");
        let browser = script(
            dir.path(),
            "chrome",
            &format!(
                r#"for a in "$@"; do
  case "$a" in
    --user-data-dir=*) cat "${{a#--user-data-dir=}}/Local State" > '{seen}';;
    --print-to-pdf=*) printf '%%PDF-1.4\n%%%%EOF\n' > "${{a#--print-to-pdf=}}";;
  esac
done"#,
                seen = seen.display()
            ),
        );
        let page = dir.path().join("p.html");
        std::fs::write(&page, "<p>x</p>").unwrap();
        let work = WorkDir::new("job").unwrap();
        html::convert_with(
            &browser,
            &page,
            &dir.path().join("out.pdf"),
            &work,
            &AtomicBool::new(false),
            None,
        )
        .unwrap();
        let state: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&seen).unwrap()).unwrap();
        assert_eq!(
            state["password_manager"]["os_password_last_changed"],
            crate::util::chromium::FAR_FUTURE_LAST_CHANGED
        );
    }

    /// A fake render worker: records its argv and the staged page it was
    /// given, and writes a PDF at `--out`.
    fn fake_worker(dir: &Path) -> PathBuf {
        let argv = dir.join("worker-argv");
        let page = dir.join("worker-page");
        script(
            dir,
            "pluryprint",
            &format!(
                r#"printf '%s\n' "$@" > '{argv}'
prev=""
for a in "$@"; do
  case "$prev" in
    --page) cp "$a" '{page}';;
    --out) printf '%%PDF-1.4\n%%%%EOF\n' > "$a";;
  esac
  prev="$a"
done"#,
                argv = argv.display(),
                page = page.display()
            ),
        )
    }

    /// FR-055: the default renderer is the hidden worker of this executable,
    /// given a staged page (CSP, base at the document folder, paper) and the
    /// paper in inches; no browser and no Pandoc are looked up.
    #[test]
    fn fr_055_html_renders_without_browser_through_the_worker() {
        let dir = tempfile::tempdir().unwrap();
        let worker = fake_worker(dir.path());
        let docs = dir.path().join("my docs");
        std::fs::create_dir_all(&docs).unwrap();
        let output = dir.path().join("out.pdf");
        let work = WorkDir::new("job").unwrap();
        let a4 = PageSetup {
            css_size: Some("210mm 297mm".into()),
        };
        html::render_system_with(
            &worker,
            b"<html><head><title>t</title></head><body><script>x()</script></body></html>",
            &docs,
            &output,
            &work,
            &AtomicBool::new(false),
            None,
            &a4,
        )
        .unwrap();
        assert!(validate_pdf_header(&output).is_ok());

        let argv: Vec<String> = std::fs::read_to_string(dir.path().join("worker-argv"))
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(argv[0], "--worker");
        assert_eq!(argv[1], "render-pdf");
        let value = |flag: &str| {
            let at = argv.iter().position(|a| a == flag).unwrap();
            argv[at + 1].clone()
        };
        assert_eq!(value("--base"), docs.to_string_lossy());
        assert!(value("--page").ends_with("page.html"));
        assert!(value("--data").starts_with(&*work.path().to_string_lossy()));
        let (w, h) = value("--paper-in")
            .split_once('x')
            .map(|(w, h)| (w.parse::<f64>().unwrap(), h.parse::<f64>().unwrap()))
            .unwrap();
        assert!(
            (w - 8.27).abs() < 0.01 && (h - 11.69).abs() < 0.01,
            "{w}x{h}"
        );

        let staged = std::fs::read_to_string(dir.path().join("worker-page")).unwrap();
        assert!(staged.contains("script-src 'none'"), "{staged}");
        assert!(
            staged.contains(&format!(
                "<base href=\"{}\">",
                crate::render::base_href(&docs)
            )),
            "{staged}"
        );
        assert!(staged.contains("@page { size: 210mm 297mm; }"), "{staged}");
    }

    /// A worker that fails reports `render_failed` with its stderr as the
    /// detail, and leaves no output behind.
    #[test]
    fn fr_055_a_failing_worker_is_render_failed() {
        let dir = tempfile::tempdir().unwrap();
        let worker = script(
            dir.path(),
            "pluryprint",
            "echo 'pluryprint worker render-pdf: the page did not finish loading' >&2; exit 1",
        );
        let output = dir.path().join("out.pdf");
        let work = WorkDir::new("job").unwrap();
        let err = html::render_system_with(
            &worker,
            b"<p>x",
            dir.path(),
            &output,
            &work,
            &AtomicBool::new(false),
            None,
            &PageSetup::default(),
        )
        .unwrap_err();
        match &err {
            EngineError::ConversionFailed(f) => {
                assert_eq!(f.reason, Some("render_failed"));
                assert!(f.detail.contains("did not finish loading"), "{f:?}");
            }
            other => panic!("{other:?}"),
        }
        assert!(!output.exists());
    }

    /// A hung worker is killed at the timeout like any converter.
    #[test]
    fn fr_055_a_hung_worker_is_killed_at_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let worker = script(dir.path(), "pluryprint", "sleep 30");
        let work = WorkDir::new("job").unwrap();
        let started = std::time::Instant::now();
        let err = html::render_system_with(
            &worker,
            b"<p>x",
            dir.path(),
            &dir.path().join("out.pdf"),
            &work,
            &AtomicBool::new(false),
            Some(Duration::from_millis(300)),
            &PageSetup::default(),
        )
        .unwrap_err();
        assert!(matches!(err, EngineError::ConversionTimedOut(_)), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    /// FR-056: Markdown goes through the same worker as HTML: no Pandoc.
    #[test]
    fn fr_056_markdown_is_rendered_by_the_worker_not_pandoc() {
        let dir = tempfile::tempdir().unwrap();
        let worker = fake_worker(dir.path());
        let html = markdown::to_html("# Hi\n\n<script>x()</script>\n", "notes");
        let work = WorkDir::new("job").unwrap();
        html::render_system_with(
            &worker,
            html.as_bytes(),
            dir.path(),
            &dir.path().join("o.pdf"),
            &work,
            &AtomicBool::new(false),
            None,
            &PageSetup::default(),
        )
        .unwrap();
        let staged = std::fs::read_to_string(dir.path().join("worker-page")).unwrap();
        assert!(staged.contains("<h1>Hi</h1>"), "{staged}");
        assert!(!staged.contains("x()"), "{staged}");
    }

    #[test]
    fn fr_052_sips_path_turns_the_png_it_writes_into_a_one_page_pdf() {
        let dir = tempfile::tempdir().unwrap();
        // A fake `sips` that "decodes" by copying a real PNG to --out.
        let png = dir.path().join("real.png");
        ::image::RgbImage::from_pixel(4, 3, ::image::Rgb([200, 10, 10]))
            .save_with_format(&png, ::image::ImageFormat::Png)
            .unwrap();
        let sips = script(
            dir.path(),
            "sips",
            &format!(
                r#"prev=""
for a in "$@"; do
  if [ "$prev" = "--out" ]; then cp '{png}' "$a"; fi
  prev="$a"
done"#,
                png = png.display()
            ),
        );
        let heic = dir.path().join("IMG_0001.HEIC");
        std::fs::write(&heic, b"\0\0\0\x18ftypheic").unwrap();
        let output = dir.path().join("o.pdf");
        let work = WorkDir::new("job").unwrap();
        let cancel = AtomicBool::new(false);
        heif::convert_with_sips(&sips, &heic, &output, &work, &cancel, None, &no_progress())
            .unwrap();
        assert_eq!(pages_of(&output), 1);
    }
}

// ---------------------------------------------------------------------------
// Real tools
// ---------------------------------------------------------------------------

/// Generous: LibreOffice's first start with a fresh profile is slow, and
/// these tests run in parallel with the rest of the suite.
const REAL_TIMEOUT: Option<Duration> = Some(Duration::from_secs(180));

fn convert_real(input: &Path, cache: &Path, job: &str) -> PathBuf {
    convert_real_on(input, cache, job, &PageSetup::default())
}

fn convert_real_on(input: &Path, cache: &Path, job: &str, page: &PageSetup) -> PathBuf {
    let cancel = AtomicBool::new(false);
    convert_to_pdf(
        input,
        cache,
        job,
        &cancel,
        &no_progress(),
        REAL_TIMEOUT,
        page,
        None,
    )
    .unwrap_or_else(|e| panic!("convert {}: {e}", input.display()))
    .expect("a converted document yields a cache PDF")
}

/// Width and height of the first page's MediaBox, in points, rounded.
fn first_page_size(pdf: &Path) -> (i64, i64) {
    let doc = lopdf::Document::load(pdf).expect("a parseable PDF");
    let (_, page_id) = doc.get_pages().into_iter().next().expect("one page");
    let page = doc.get_dictionary(page_id).unwrap();
    let mb = page
        .get(b"MediaBox")
        .and_then(|o| o.as_array())
        .expect("a MediaBox");
    let n = |o: &lopdf::Object| {
        o.as_float()
            .map(f64::from)
            .or_else(|_| o.as_i64().map(|v| v as f64))
            .unwrap()
    };
    (
        (n(&mb[2]) - n(&mb[0])).round() as i64,
        (n(&mb[3]) - n(&mb[1])).round() as i64,
    )
}

/// The job's paper for the page renderers: A4 portrait, the default.
fn a4() -> PageSetup {
    PageSetup::for_settings(&crate::queue::job::JobSettings::default())
}

/// RTF goes to LibreOffice where the OS has no converter of its own (on
/// macOS it is `textutil`, tested with the render worker in
/// `tests/render_worker.rs`).
#[cfg(not(target_os = "macos"))]
#[test]
fn fr_057_real_rtf_converts_through_libreoffice() {
    if tool_or_skip(Dependency::LibreOffice, "rtf").is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let rtf = dir.path().join("memo.rtf");
    std::fs::write(
        &rtf,
        r"{\rtf1\ansi{\fonttbl\f0 Helvetica;}\f0\b Hello\b0  RTF.\par}",
    )
    .unwrap();
    let pdf = convert_real(&rtf, &dir.path().join("cache"), "real-rtf");
    assert_eq!(pages_of(&pdf), 1);
}

#[test]
fn fr_053_real_docx_made_by_libreoffice_converts_back_to_pdf() {
    let Some(soffice) = tool_or_skip(Dependency::LibreOffice, "docx") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let txt = dir.path().join("source.txt");
    std::fs::write(&txt, "A Word document generated for the test.\n").unwrap();

    // Build the .docx fixture with LibreOffice itself (txt → docx).
    let work = WorkDir::new("fixture").unwrap();
    let profile = work.path().join("profile");
    let mut cmd = std::process::Command::new(&soffice);
    cmd.args([
        format!("-env:UserInstallation={}", file_url(&profile)),
        "--headless".into(),
        "--norestore".into(),
        "--convert-to".into(),
        "docx:MS Word 2007 XML".into(),
        "--outdir".into(),
        dir.path().to_string_lossy().into_owned(),
        txt.to_string_lossy().into_owned(),
    ]);
    let cancel = AtomicBool::new(false);
    process::run_bounded(
        cmd,
        "LibreOffice",
        &cancel,
        REAL_TIMEOUT,
        &work.path().join("log"),
    )
    .unwrap();
    let docx = dir.path().join("source.docx");
    assert!(docx.is_file(), "fixture docx was not produced");

    let pdf = convert_real(&docx, &dir.path().join("cache"), "real-docx");
    assert_eq!(pages_of(&pdf), 1);
}

/// Security finding: a document LibreOffice converts must not fetch what it
/// links to (a linked image over HTTP, here; a `\\host\share` one on
/// Windows takes the same path). The hardened private profile
/// (`office::HARDENING_XCU`) blocks it: the listener sees no connection.
#[test]
fn fr_053_real_libreoffice_fetches_no_linked_resource() {
    let Some(soffice) = tool_or_skip(Dependency::LibreOffice, "office-links") else {
        return;
    };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let dir = tempfile::tempdir().unwrap();
    // A flat ODF text document with one linked (not embedded) picture.
    let fodt = dir.path().join("linked.fodt");
    std::fs::write(
        &fodt,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
 xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
 xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"
 xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"
 xmlns:xlink="http://www.w3.org/1999/xlink" office:version="1.3"
 office:mimetype="application/vnd.oasis.opendocument.text">
 <office:body><office:text>
  <text:p>Linked picture:
   <draw:frame svg:width="2cm" svg:height="2cm" text:anchor-type="as-char">
    <draw:image xlink:href="http://127.0.0.1:{port}/linked.png" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>
   </draw:frame>
  </text:p>
 </office:text></office:body>
</office:document>"#
        ),
    )
    .unwrap();
    let work = WorkDir::new("office-links").unwrap();
    let output = dir.path().join("linked.pdf");
    office::convert_with(
        &soffice,
        &fodt,
        &output,
        &work,
        &AtomicBool::new(false),
        REAL_TIMEOUT,
    )
    .unwrap();
    assert_eq!(pages_of(&output), 1);
    match listener.accept() {
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
        other => panic!("LibreOffice fetched the linked picture: {other:?}"),
    }
}

/// The browser fallback (D15) still prints a page on the job's paper.
#[test]
fn fr_055_real_html_prints_through_the_browser_fallback() {
    if tool_or_skip(Dependency::Chromium, "html").is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("style.css"), "h1 { color: #036; }").unwrap();
    let sections: String = (1..=80).map(|i| format!("<p>Paragraph {i}</p>")).collect();
    let html = format!(
        "<!doctype html><html><head><meta charset=utf-8>\
         <link rel=stylesheet href=style.css></head>\
         <body><h1>Título</h1>{sections}</body></html>"
    );
    let work = WorkDir::new("real-html-browser").unwrap();
    let pdf = dir.path().join("out.pdf");
    html::render_document(
        html.as_bytes(),
        dir.path(),
        &pdf,
        &work,
        &AtomicBool::new(false),
        REAL_TIMEOUT,
        &a4(),
        crate::settings::HtmlRenderer::Browser,
    )
    .unwrap();
    assert!(pages_of(&pdf) >= 1);
    // An A4 job gets A4 pages, not the browser's US Letter default.
    assert_eq!(first_page_size(&pdf), (595, 842));
}

/// The browser fallback reaches no network, loopback included: its
/// stylesheet, its image, a script `fetch` and a WebSocket all point at a
/// local listener that must never see a connection.
#[test]
fn fr_055_real_html_fallback_reaches_no_network() {
    if tool_or_skip(Dependency::Chromium, "html-sandbox").is_none() {
        return;
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();

    let dir = tempfile::tempdir().unwrap();
    let html = format!(
        "<!doctype html><html><head><meta charset=utf-8>\
         <link rel=stylesheet href=\"http://localhost:{port}/x.css\"></head>\
         <body><p>static</p>\
         <img src=\"http://127.0.0.1:{port}/beacon.png\">\
         <script>fetch('http://127.0.0.1:{port}/fetch').catch(() => {{}});\
         try {{ new WebSocket('ws://127.0.0.1:{port}/ws'); }} catch (e) {{}}</script>\
         </body></html>"
    );
    let work = WorkDir::new("real-html-browser-sandbox").unwrap();
    let pdf = dir.path().join("out.pdf");
    html::render_document(
        html.as_bytes(),
        dir.path(),
        &pdf,
        &work,
        &AtomicBool::new(false),
        REAL_TIMEOUT,
        &PageSetup::default(),
        crate::settings::HtmlRenderer::Browser,
    )
    .unwrap();
    assert_eq!(pages_of(&pdf), 1);
    match listener.accept() {
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
        other => panic!("the page reached the network: {other:?}"),
    }
}

#[cfg(target_os = "macos")]
#[test]
fn fr_052_real_heic_made_by_sips_converts_to_one_page() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("photo.png");
    let mut img = ::image::RgbImage::new(64, 48);
    for (x, y, px) in img.enumerate_pixels_mut() {
        *px = ::image::Rgb([(x * 4) as u8, (y * 5) as u8, 90]);
    }
    img.save_with_format(&png, ::image::ImageFormat::Png)
        .unwrap();
    let heic = dir.path().join("photo.HEIC");
    let status = std::process::Command::new("/usr/bin/sips")
        .args(["-s", "format", "heic"])
        .arg(&png)
        .arg("--out")
        .arg(&heic)
        .stdout(std::process::Stdio::null())
        .status()
        .expect("sips is part of macOS");
    if !status.success() || !heic.is_file() {
        eprintln!("SKIPPED heic: this macOS cannot encode HEIC");
        return;
    }
    let pdf = convert_real(&heic, &dir.path().join("cache"), "real-heic");
    assert_eq!(pages_of(&pdf), 1);
}

#[cfg(target_os = "macos")]
#[test]
fn fr_052_corrupt_heic_is_a_conversion_failure_not_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let heic = dir.path().join("broken.heic");
    std::fs::write(&heic, b"\0\0\0\x18ftypheic-not-really").unwrap();
    let cancel = AtomicBool::new(false);
    let err = convert_to_pdf(
        &heic,
        &dir.path().join("cache"),
        "bad-heic",
        &cancel,
        &no_progress(),
        REAL_TIMEOUT,
        &PageSetup::default(),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, EngineError::ConversionFailed(_)), "{err:?}");
    assert!(
        !dir.path().join("cache").join("bad-heic.pdf").exists(),
        "no partial cache PDF"
    );
}
