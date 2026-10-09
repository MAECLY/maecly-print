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
                && argv[0].contains("maeclyprint-convert-job-"),
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

    #[test]
    fn fr_055_html_spawns_the_browser_on_the_file_url_and_returns_its_pdf() {
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

        html::convert_with(
            &browser,
            &page,
            &output,
            &work,
            &cancel,
            None,
            &PageSetup::default(),
        )
        .unwrap();

        assert!(validate_pdf_header(&output).is_ok());
        let argv = std::fs::read_to_string(&log).unwrap();
        assert!(argv.contains("--headless\n"));
        assert!(argv.contains("--no-pdf-header-footer\n"));
        assert!(argv.trim_end().ends_with(&file_url(&page)), "{argv}");
        assert!(file_url(&page).contains("my%20page.html"));
    }

    #[test]
    fn fr_056_markdown_runs_pandoc_then_the_browser_on_the_generated_page() {
        let dir = tempfile::tempdir().unwrap();
        let pandoc = script(
            dir.path(),
            "pandoc",
            r#"for a in "$@"; do
  case "$a" in --output=*) printf '<html><body>md</body></html>' > "${a#--output=}";; esac
done"#,
        );
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
        let md = dir.path().join("notes.md");
        std::fs::write(&md, "# Hi").unwrap();
        let output = dir.path().join("out.pdf");
        let work = WorkDir::new("job").unwrap();
        let cancel = AtomicBool::new(false);

        markdown::convert_with(
            &pandoc,
            &browser,
            &md,
            &output,
            &work,
            &cancel,
            None,
            &PageSetup::default(),
        )
        .unwrap();

        assert!(validate_pdf_header(&output).is_ok());
        let argv = std::fs::read_to_string(&log).unwrap();
        assert!(
            argv.trim_end().ends_with("/page.html"),
            "the browser prints Pandoc's page, not the .md: {argv}"
        );
    }

    #[test]
    fn fr_056_a_pandoc_failure_stops_before_the_browser() {
        let dir = tempfile::tempdir().unwrap();
        let pandoc = script(
            dir.path(),
            "pandoc",
            "echo 'pandoc: bad input' >&2; exit 64",
        );
        let marker = dir.path().join("browser-ran");
        let browser = script(
            dir.path(),
            "chrome",
            &format!("touch '{}'", marker.display()),
        );
        let md = dir.path().join("x.md");
        std::fs::write(&md, "x").unwrap();
        let work = WorkDir::new("job").unwrap();
        let cancel = AtomicBool::new(false);
        let err = markdown::convert_with(
            &pandoc,
            &browser,
            &md,
            &dir.path().join("o.pdf"),
            &work,
            &cancel,
            None,
            &PageSetup::default(),
        )
        .unwrap_err();
        assert!(
            matches!(&err, EngineError::ConversionFailed(m) if m.contains("bad input")),
            "{err:?}"
        );
        assert!(!marker.exists());
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

/// The job's paper for the browser converters: A4 portrait, the default.
fn a4() -> PageSetup {
    PageSetup::for_settings(&crate::queue::job::JobSettings::default())
}

#[test]
fn fr_057_real_txt_converts_through_libreoffice_with_its_page_count() {
    if tool_or_skip(Dependency::LibreOffice, "txt").is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let txt = dir.path().join("notas año.txt");
    // ~300 lines: more than one page, and non-ASCII text that the UTF-8
    // import filter must keep intact.
    let body: String = (1..=300)
        .map(|i| format!("Línea {i}: ñandú, acción, café\n"))
        .collect();
    std::fs::write(&txt, body).unwrap();
    let pdf = convert_real(&txt, &dir.path().join("cache"), "real-txt");
    let pages = pages_of(&pdf);
    assert!(pages > 1, "300 lines should span pages, got {pages}");
    let bytes = std::fs::metadata(&pdf).unwrap().len();
    assert_eq!(
        crate::queue::probe_pages(&pdf, "pdf", bytes),
        Some(pages as u32),
        "the queue's page probe reads the converted PDF"
    );
}

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

#[test]
fn fr_055_real_html_prints_through_the_browser() {
    if tool_or_skip(Dependency::Chromium, "html").is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("style.css"), "h1 { color: #036; }").unwrap();
    let html = dir.path().join("página.html");
    let sections: String = (1..=80).map(|i| format!("<p>Paragraph {i}</p>")).collect();
    std::fs::write(
        &html,
        format!(
            "<!doctype html><html><head><meta charset=utf-8>\
             <link rel=stylesheet href=style.css></head>\
             <body><h1>Título</h1>{sections}</body></html>"
        ),
    )
    .unwrap();
    let pdf = convert_real_on(&html, &dir.path().join("cache"), "real-html", &a4());
    assert!(pages_of(&pdf) >= 1);
    // An A4 job gets A4 pages, not the browser's US Letter default.
    assert_eq!(first_page_size(&pdf), (595, 842));
}

#[test]
fn fr_056_real_markdown_goes_through_pandoc_and_the_browser() {
    if tool_or_skip(Dependency::Pandoc, "md").is_none()
        || tool_or_skip(Dependency::Chromium, "md").is_none()
    {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let md = dir.path().join("README.md");
    std::fs::write(
        &md,
        "# Título\n\nSome *emphasis*, a list:\n\n- one\n- two\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n",
    )
    .unwrap();
    let pdf = convert_real_on(&md, &dir.path().join("cache"), "real-md", &a4());
    assert_eq!(pages_of(&pdf), 1);
    assert_eq!(first_page_size(&pdf), (595, 842));
}

/// A printed page reaches no network, loopback included: its stylesheet, its
/// image, a script `fetch` and a WebSocket all point at a local listener that
/// must never see a connection.
#[test]
fn fr_055_real_html_reaches_no_network() {
    if tool_or_skip(Dependency::Chromium, "html-sandbox").is_none() {
        return;
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();

    let dir = tempfile::tempdir().unwrap();
    let html = dir.path().join("hostile.html");
    std::fs::write(
        &html,
        format!(
            "<!doctype html><html><head><meta charset=utf-8>\
             <link rel=stylesheet href=\"http://localhost:{port}/x.css\"></head>\
             <body><p>static</p>\
             <img src=\"http://127.0.0.1:{port}/beacon.png\">\
             <script>fetch('http://127.0.0.1:{port}/fetch').catch(() => {{}});\
             try {{ new WebSocket('ws://127.0.0.1:{port}/ws'); }} catch (e) {{}}</script>\
             </body></html>"
        ),
    )
    .unwrap();
    let pdf = convert_real(&html, &dir.path().join("cache"), "real-html-sandbox");
    assert_eq!(pages_of(&pdf), 1);
    match listener.accept() {
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
        other => panic!("the page reached the network: {other:?}"),
    }
}

/// Raw HTML in a Markdown file is printed as text, never run or loaded, and
/// only images on the local disk survive; remote and UNC ones become their
/// alt text.
#[test]
fn fr_056_real_markdown_drops_raw_html_and_non_local_images() {
    let Some(pandoc) = tool_or_skip(Dependency::Pandoc, "md-sandbox") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let md = dir.path().join("README.md");
    std::fs::write(
        &md,
        "# Title\n\n\
         <script>document.write('ran')</script>\n\n\
         <iframe src=\"file:///etc/passwd\"></iframe>\n\n\
         `<b>raw</b>`{=html}\n\n\
         ![keep-rel](pic.png) ![keep-abs](file:///tmp/x.png) \
         ![keep-data](data:image/png;base64,AA==)\n\n\
         ![drop-http](http://example.com/x.png) ![drop-proto](//attacker/s/x.png)\n\n\
         ![drop-unc](\\\\\\\\attacker\\\\s\\\\x.png) ![drop-host](file://attacker/x.png)\n\n\
         ![drop-four](file:////attacker/x.png)\n",
    )
    .unwrap();
    let work = WorkDir::new("md-sandbox").unwrap();
    let header = work.path().join("head.html");
    std::fs::write(&header, markdown::head_fragment(dir.path())).unwrap();
    let filter = work.path().join("local-images.lua");
    std::fs::write(&filter, markdown::LOCAL_IMAGES_LUA).unwrap();
    let page = work.path().join("page.html");
    let mut cmd = std::process::Command::new(&pandoc);
    cmd.args(markdown::pandoc_args(
        &md, &header, &filter, &page, "README",
    ));
    let cancel = AtomicBool::new(false);
    process::run_bounded(
        cmd,
        "Pandoc",
        &cancel,
        REAL_TIMEOUT,
        &work.path().join("log"),
    )
    .unwrap();
    let html = std::fs::read_to_string(&page).unwrap();
    let body = &html[html.find("<body").expect("standalone page")..];

    assert!(!body.contains("<script"), "{body}");
    assert!(!body.contains("<iframe"), "{body}");
    assert!(!body.contains("<b>raw"), "{body}");
    assert!(
        body.contains("&lt;script&gt;"),
        "raw HTML stays visible as text: {body}"
    );
    // The injected header (CSP, base URL, print CSS) still reaches the page.
    assert!(html.contains("<base href="), "{html}");
    assert!(html.contains("script-src 'none'"), "{html}");
    for keep in ["keep-rel", "keep-abs", "keep-data"] {
        assert!(body.contains(&format!("alt=\"{keep}\"")), "{keep}: {body}");
    }
    for drop in [
        "drop-http",
        "drop-proto",
        "drop-unc",
        "drop-host",
        "drop-four",
    ] {
        assert!(!body.contains(&format!("alt=\"{drop}\"")), "{drop}: {body}");
        assert!(body.contains(drop), "{drop} keeps its alt text: {body}");
    }
    assert!(!body.contains("attacker"), "{body}");
    assert!(!body.contains("example.com"), "{body}");
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
    )
    .unwrap_err();
    assert!(matches!(err, EngineError::ConversionFailed(_)), "{err:?}");
    assert!(
        !dir.path().join("cache").join("bad-heic.pdf").exists(),
        "no partial cache PDF"
    );
}
