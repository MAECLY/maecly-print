//! FR-055/056/057 with the real render worker: the freshly built `pluryprint`
//! binary prints HTML, Markdown, text (and RTF on macOS) through the system
//! web view, with no Edge, Chrome, Pandoc or LibreOffice.
//!
//! Runs on Windows and macOS (the platforms with a system web view the
//! worker drives); elsewhere every test returns early.

#![cfg(any(windows, target_os = "macos"))]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Once;
use std::time::{Duration, Instant};

use pluryprint_lib::engine::convert::{convert_to_pdf, PageSetup};
use pluryprint_lib::engine::EngineError;

/// Point the engine at the binary cargo built for this test run, and pin the
/// renderer to the system web view whatever the machine's config says.
fn setup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::env::set_var("PLURYPRINT_WORKER_EXE", env!("CARGO_BIN_EXE_pluryprint"));
        std::env::set_var("PLURYPRINT_HTML_RENDERER", "system");
    });
}

const TIMEOUT: Option<Duration> = Some(Duration::from_secs(120));

fn a4() -> PageSetup {
    PageSetup {
        css_size: Some("210mm 297mm".into()),
    }
}

fn convert(input: &Path, page: &PageSetup) -> Result<PathBuf, EngineError> {
    setup();
    let cache = input.parent().unwrap().join("cache");
    let job = format!(
        "job-{}",
        input
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .replace(' ', "-")
    );
    convert_to_pdf(
        input,
        &cache,
        &job,
        &AtomicBool::new(false),
        &|_| {},
        TIMEOUT,
        page,
        None,
    )
    .map(|out| out.expect("a converted document yields a cache PDF"))
}

fn pages(pdf: &Path) -> usize {
    lopdf::Document::load(pdf).unwrap().get_pages().len()
}

fn first_page_size(pdf: &Path) -> (i64, i64) {
    let doc = lopdf::Document::load(pdf).unwrap();
    let (_, id) = doc.get_pages().into_iter().next().unwrap();
    let page = doc.get_dictionary(id).unwrap();
    let mb = page
        .get(b"MediaBox")
        .or_else(|_| {
            // Inherited from the page tree.
            let parent = page.get(b"Parent").unwrap().as_reference().unwrap();
            doc.get_dictionary(parent).unwrap().get(b"MediaBox")
        })
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
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

/// Count image XObjects in the document.
fn images(pdf: &Path) -> usize {
    let doc = lopdf::Document::load(pdf).unwrap();
    doc.objects
        .values()
        .filter(|o| {
            o.as_stream()
                .ok()
                .and_then(|s| s.dict.get(b"Subtype").ok())
                .and_then(|v| v.as_name().ok())
                == Some(b"Image".as_slice())
        })
        .count()
}

fn red_png(path: &Path) {
    let img = image::RgbImage::from_pixel(40, 40, image::Rgb([220, 20, 20]));
    img.save_with_format(path, image::ImageFormat::Png).unwrap();
}

/// A listener that must never see a connection.
struct Tripwire(std::net::TcpListener);

impl Tripwire {
    fn new() -> Self {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.set_nonblocking(true).unwrap();
        Self(l)
    }
    fn port(&self) -> u16 {
        self.0.local_addr().unwrap().port()
    }
    fn assert_untouched(&self) {
        match self.0.accept() {
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            other => panic!("the page reached the network: {other:?}"),
        }
    }
}

#[test]
fn fr_055_html_renders_without_browser() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("img")).unwrap();
    red_png(&dir.path().join("img").join("red dot.png"));
    std::fs::write(dir.path().join("style.css"), "h1 { color: #036; }").unwrap();
    let paragraphs: String = (1..=200)
        .map(|i| format!("<p>Paragraph {i} of a long page.</p>"))
        .collect();
    let html = dir.path().join("página larga.html");
    std::fs::write(
        &html,
        format!(
            "<!doctype html><html><head><meta charset=utf-8><title>t</title>\
             <link rel=stylesheet href=style.css></head><body><h1>Título</h1>\
             <img src=\"img/red%20dot.png\" alt=dot>{paragraphs}</body></html>"
        ),
    )
    .unwrap();
    let pdf = convert(&html, &a4()).unwrap();
    assert!(
        pages(&pdf) > 1,
        "200 paragraphs span pages: {}",
        pages(&pdf)
    );
    assert_eq!(first_page_size(&pdf), (595, 842), "A4 from the job");
    assert!(images(&pdf) >= 1, "the relative image loads");
}

/// Security finding: nothing on the page reaches the network or a file
/// share — sub-resources, a protocol-relative reference, a refresh and a
/// frame all point at a listener that must stay untouched.
#[test]
fn fr_055_remote_resources_blocked() {
    let wire = Tripwire::new();
    let port = wire.port();
    let dir = tempfile::tempdir().unwrap();
    let html = dir.path().join("hostile.html");
    std::fs::write(
        &html,
        format!(
            "<!doctype html><html><head><meta charset=utf-8>\
             <meta http-equiv=refresh content=\"0;url=http://127.0.0.1:{port}/refresh\">\
             <link rel=stylesheet href=\"http://localhost:{port}/x.css\">\
             <style>@import url(http://127.0.0.1:{port}/import.css); \
             body {{ background: url(//127.0.0.1:{port}/bg.png); }}</style></head>\
             <body><p>static text</p>\
             <img src=\"http://127.0.0.1:{port}/beacon.png\">\
             <img src=\"//127.0.0.1:{port}/proto.png\">\
             <iframe src=\"http://127.0.0.1:{port}/frame\"></iframe>\
             <object data=\"http://127.0.0.1:{port}/obj\"></object>\
             <script>fetch('http://127.0.0.1:{port}/fetch');</script>\
             </body></html>"
        ),
    )
    .unwrap();
    let pdf = convert(&html, &PageSetup::default()).unwrap();
    assert_eq!(pages(&pdf), 1);
    // Give a late request (the refresh) the chance to show up.
    std::thread::sleep(Duration::from_millis(300));
    wire.assert_untouched();
}

#[test]
fn fr_055_page_size_from_pagesetup() {
    let dir = tempfile::tempdir().unwrap();
    let html = dir.path().join("letter.html");
    std::fs::write(&html, "<p>letter landscape</p>").unwrap();
    let letter_landscape = PageSetup {
        css_size: Some("11in 8.5in".into()),
    };
    let pdf = convert(&html, &letter_landscape).unwrap();
    assert_eq!(first_page_size(&pdf), (792, 612));
}

#[test]
fn fr_056_markdown_no_pandoc_real_render() {
    let dir = tempfile::tempdir().unwrap();
    red_png(&dir.path().join("pic.png"));
    let md = dir.path().join("README.md");
    std::fs::write(
        &md,
        "# Título\n\nSome *emphasis* and a table:\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n\
         ![local](pic.png)\n\n<script>document.write('ran')</script>\n",
    )
    .unwrap();
    let pdf = convert(&md, &a4()).unwrap();
    assert_eq!(pages(&pdf), 1);
    assert_eq!(first_page_size(&pdf), (595, 842));
    assert!(images(&pdf) >= 1, "the local picture is printed");
}

#[test]
fn fr_057_txt_real_render_utf16_and_long() {
    let dir = tempfile::tempdir().unwrap();
    let txt = dir.path().join("notas año.txt");
    let body: String = (1..=300)
        .map(|i| format!("Línea {i}: ñandú, acción, café\r\n"))
        .collect();
    let mut utf16 = vec![0xFF, 0xFE];
    for unit in body.encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(&txt, utf16).unwrap();
    let pdf = convert(&txt, &PageSetup::default()).unwrap();
    assert!(pages(&pdf) > 1, "300 lines span pages");
    assert_eq!(first_page_size(&pdf), (612, 792), "Letter when auto");
}

#[cfg(target_os = "macos")]
#[test]
fn fr_057_rtf_macos_textutil() {
    let dir = tempfile::tempdir().unwrap();
    let rtf = dir.path().join("memo.rtf");
    std::fs::write(
        &rtf,
        r"{\rtf1\ansi{\fonttbl\f0 Helvetica;}\f0\b Hello\b0  RTF \u241?and\u250?.\par}",
    )
    .unwrap();
    let pdf = convert(&rtf, &a4()).unwrap();
    assert_eq!(pages(&pdf), 1);
    assert_eq!(first_page_size(&pdf), (595, 842));
}

/// D10: the worker runs as its own process and exits; it never forwards to
/// a running instance or waits on one, and a second one in parallel works.
#[test]
fn nfr_worker_not_single_instance_and_no_ipc() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.html");
    let b = dir.path().join("b.html");
    std::fs::write(&a, "<p>a</p>").unwrap();
    std::fs::write(&b, "<p>b</p>").unwrap();
    let (ra, rb) = std::thread::scope(|s| {
        let ta = s.spawn(|| convert(&a, &PageSetup::default()));
        let tb = s.spawn(|| convert(&b, &PageSetup::default()));
        (ta.join().unwrap(), tb.join().unwrap())
    });
    assert_eq!(pages(&ra.unwrap()), 1);
    assert_eq!(pages(&rb.unwrap()), 1);
}

/// Informative: how long one render takes end to end (worker start, load,
/// print). Printed, and only bounded loosely.
#[test]
fn nfr_001_worker_startup_time_is_measured() {
    let dir = tempfile::tempdir().unwrap();
    let html = dir.path().join("tiny.html");
    std::fs::write(&html, "<p>tiny</p>").unwrap();
    let started = Instant::now();
    convert(&html, &PageSetup::default()).unwrap();
    let took = started.elapsed();
    eprintln!("NFR-001 render worker, tiny page: {took:?}");
    assert!(took < Duration::from_secs(60), "{took:?}");
}
