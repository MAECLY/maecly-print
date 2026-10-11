//! The landing follows the published release on its own:
//!
//! - nothing in `site/` names a version, a release date, a download link or a
//!   file size; those places carry `{{release.*}}` markers;
//! - `scripts/site/apply_release.py` fills them from the GitHub API record of
//!   the latest release into a copy of `site/`, and refuses a release that
//!   lacks any of the four installers or SHA256SUMS (exercised here where a
//!   `python3` exists);
//! - `.github/workflows/pages.yml` runs it before uploading the Pages
//!   artifact, publishes that copy and not `site/`, and redeploys when a
//!   release is published.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read(rel: &str) -> String {
    let p = repo().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Every text file of `site/`, as (path relative to the repo, text).
fn site_texts(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| {
                ["html", "txt", "webmanifest", "xml", "json"].contains(&x.to_str().unwrap())
            }) {
                let rel = p
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, std::fs::read_to_string(&p).unwrap()));
            }
        }
    }
    out.sort();
    assert!(out.len() >= 6, "{out:?}");
    out
}

const LABELS: [(&str, &str); 4] = [
    ("windows_x64", "exe"),
    ("windows_arm64", "exe"),
    ("apple_silicon", "dmg"),
    ("apple_intel", "dmg"),
];

#[test]
fn site_sources_link_no_release_by_hand() {
    for (file, text) in site_texts(&repo().join("site")) {
        for hard in [
            "/releases/download/v",
            "/releases/tag/v",
            "Pluryprint_0.",
            "0.0.1",
            "0.0.2",
        ] {
            assert!(
                !text.contains(hard),
                "site/{file} hardcodes {hard:?}: use a {{{{release.*}}}} marker"
            );
        }
    }
    // Both landings and the agent files carry the markers that matter.
    for file in ["index.html", "en/index.html"] {
        let text = read(&format!("site/{file}"));
        for (label, _) in LABELS {
            assert!(
                text.contains(&format!("href=\"{{{{release.{label}.url}}}}\"")),
                "{file} {label}"
            );
            assert!(
                text.contains(&format!("{{{{release.{label}.mib_")),
                "{file} {label} size"
            );
        }
        for m in [
            "{{release.version}}",
            "{{release.notes_url}}",
            "{{release.sha256sums_url}}",
            "\"softwareVersion\":\"{{release.version}}\"",
            "\"datePublished\":\"{{release.date}}\"",
            "\"releaseNotes\":\"{{release.notes_url}}\"",
        ] {
            assert!(text.contains(m), "{file} lacks {m}");
        }
    }
    let llms = read("site/llms.txt");
    for (label, _) in LABELS {
        assert!(llms.contains(&format!(
            "{{{{release.{label}.url}}}} (about {{{{release.{label}.mib_en}}}} MiB)"
        )));
    }
    assert!(read("site/agents.txt").contains("Release: {{release.notes_url}}"));
}

#[test]
fn pages_workflow_fills_the_release_before_publishing() {
    let wf = read(".github/workflows/pages.yml");
    let on = wf
        .split_once("\non:\n")
        .unwrap()
        .1
        .split_once("\npermissions:")
        .unwrap()
        .0;
    assert!(on.contains("  release:\n    types: [published]\n"), "{on}");
    assert!(on.contains("  workflow_dispatch:"), "{on}");
    assert!(on.contains("      - 'scripts/site/**'"), "{on}");

    let fill = wf
        .find("python3 -I scripts/site/apply_release.py")
        .expect("pages.yml runs apply_release.py");
    let upload = wf.find("actions/upload-pages-artifact@").unwrap();
    assert!(
        fill < upload,
        "the site is filled before the artifact is uploaded"
    );
    assert!(wf.contains("gh api \"repos/$REPO/releases/latest\" > \"$RUNNER_TEMP/release.json\""));
    assert!(wf.contains("GH_TOKEN: ${{ github.token }}"));
    // The filled copy is what Pages gets, never site/ with its markers.
    assert!(wf.contains("--out \"$RUNNER_TEMP/site\""));
    assert!(wf.contains("          path: ${{ runner.temp }}/site\n"));
    assert!(!wf.contains("          path: site\n"));
    // Nothing turns the script's checks off.
    let step = &wf[fill..upload];
    for bypass in ["continue-on-error", "|| true", "--skip", "--allow"] {
        assert!(!wf.contains(bypass) && !step.contains(bypass), "{bypass}");
    }
    // Least privilege: reading the release needs no more than contents: read.
    assert!(wf.contains("\npermissions: {}\n"));
    assert!(!wf.contains("contents: write"));
}

fn python() -> Option<&'static str> {
    ["python3", "python"].into_iter().find(|p| {
        Command::new(p)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    })
}

fn fixture() -> Value {
    let base = "https://github.com/MAECLY/pluryprint/releases";
    let mut assets: Vec<Value> = LABELS
        .iter()
        .enumerate()
        .map(|(i, (label, ext))| {
            let name = format!("Pluryprint_9.8.7_{label}.{ext}");
            json!({
                "name": name,
                "state": "uploaded",
                // 10.0, 11.0, 12.0 and 13.0 MiB plus a little.
                "size": (10 + i as u64) * 1024 * 1024 + 60_000,
                "browser_download_url": format!("{base}/download/v9.8.7/{name}"),
            })
        })
        .collect();
    for name in [
        "SHA256SUMS",
        "latest.json",
        "Pluryprint_9.8.7_windows_x64.exe.sig",
    ] {
        assets.push(json!({
            "name": name,
            "state": "uploaded",
            "size": 1000,
            "browser_download_url": format!("{base}/download/v9.8.7/{name}"),
        }));
    }
    json!({
        "tag_name": "v9.8.7",
        "name": "Pluryprint v9.8.7",
        "draft": false,
        "prerelease": false,
        "published_at": "2027-03-05T23:59:00Z",
        "html_url": format!("{base}/tag/v9.8.7"),
        "assets": assets,
    })
}

/// Runs the script on `release`; returns its output and the filled folder.
fn apply(release: &Value) -> Option<(std::process::Output, tempfile::TempDir)> {
    let py = python()?;
    let dir = tempfile::tempdir().unwrap();
    let json = dir.path().join("release.json");
    std::fs::write(&json, serde_json::to_vec(release).unwrap()).unwrap();
    let out = Command::new(py)
        .arg("-I")
        .arg(repo().join("scripts/site/apply_release.py"))
        .arg("--release-json")
        .arg(&json)
        .args(["--repo", "MAECLY/pluryprint", "--site"])
        .arg(repo().join("site"))
        .arg("--out")
        .arg(dir.path().join("out"))
        .output()
        .expect("run apply_release.py");
    Some((out, dir))
}

#[test]
fn apply_release_fills_every_marker_from_the_release() {
    let Some((out, dir)) = apply(&fixture()) else {
        eprintln!("no python on this machine; apply_release.py not exercised");
        return;
    };
    assert!(out.status.success(), "{out:?}");
    let filled = dir.path().join("out");
    let base = "https://github.com/MAECLY/pluryprint/releases";
    for (file, text) in site_texts(&filled) {
        assert!(!text.contains("{{"), "{file} keeps a marker");
        assert!(!text.contains("0.0.1"), "{file}");
    }
    // Files without markers are copied as they are.
    assert_eq!(
        std::fs::read(filled.join("CNAME")).unwrap(),
        std::fs::read(repo().join("site/CNAME")).unwrap()
    );
    assert!(filled.join("assets").is_dir());

    let es = std::fs::read_to_string(filled.join("index.html")).unwrap();
    let en = std::fs::read_to_string(filled.join("en/index.html")).unwrap();
    for (i, (label, ext)) in LABELS.iter().enumerate() {
        let href = format!("href=\"{base}/download/v9.8.7/Pluryprint_9.8.7_{label}.{ext}\"");
        assert!(es.contains(&href) && en.contains(&href), "{href}");
        assert!(es.contains(&format!("{},1&nbsp;MiB", 10 + i)), "es {label}");
        assert!(en.contains(&format!("{}.1&nbsp;MiB", 10 + i)), "en {label}");
    }
    for page in [&es, &en] {
        assert!(page.contains(&format!("href=\"{base}/download/v9.8.7/SHA256SUMS\"")));
        assert!(page.contains(&format!("href=\"{base}/tag/v9.8.7\"")));
        assert!(page.contains("v9.8.7 preview"));
        // The JSON-LD still parses and names the release.
        let ld = page
            .split_once("<script type=\"application/ld+json\">")
            .unwrap()
            .1
            .split_once("</script>")
            .unwrap()
            .0;
        let ld: Value = serde_json::from_str(ld).unwrap();
        let app = ld["@graph"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["@type"] == "SoftwareApplication")
            .unwrap();
        assert_eq!(app["softwareVersion"], "9.8.7");
        assert_eq!(app["datePublished"], "2027-03-05");
        assert_eq!(app["releaseNotes"], format!("{base}/tag/v9.8.7"));
        assert_eq!(app["downloadUrl"].as_array().unwrap().len(), 4);
    }
    assert!(es.contains("5 de marzo de 2027"));
    assert!(en.contains("5 March 2027"));

    let llms = std::fs::read_to_string(filled.join("llms.txt")).unwrap();
    assert!(llms.contains(&format!(
        "{base}/download/v9.8.7/Pluryprint_9.8.7_apple_intel.dmg (about 13.1 MiB)"
    )));
    let agents = std::fs::read_to_string(filled.join("agents.txt")).unwrap();
    assert!(agents.contains("version 9.8.7 preview, published 2027-03-05"));
    for manifest in ["site.webmanifest", "en/site.webmanifest"] {
        let m: Value =
            serde_json::from_str(&std::fs::read_to_string(filled.join(manifest)).unwrap()).unwrap();
        assert!(
            m["description"].as_str().unwrap().contains("9.8.7"),
            "{manifest}"
        );
    }
    // site/ itself is left alone.
    assert!(read("site/index.html").contains("{{release.windows_x64.url}}"));
}

#[test]
fn apply_release_refuses_a_release_the_landing_cannot_link() {
    if python().is_none() {
        eprintln!("no python on this machine; apply_release.py not exercised");
        return;
    }
    let refused = |mutate: &dyn Fn(&mut Value), why: &str| {
        let mut release = fixture();
        mutate(&mut release);
        let (out, dir) = apply(&release).unwrap();
        assert!(!out.status.success(), "{why}: {out:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.starts_with("apply_release: "), "{why}: {err}");
        drop(dir);
        err.into_owned()
    };
    let drop_asset = |name: String| {
        move |r: &mut Value| {
            r["assets"]
                .as_array_mut()
                .unwrap()
                .retain(|a| a["name"] != name.as_str());
        }
    };
    for (label, ext) in LABELS {
        let name = format!("Pluryprint_9.8.7_{label}.{ext}");
        let err = refused(&drop_asset(name.clone()), &name);
        assert!(err.contains(&name), "{err}");
    }
    refused(&drop_asset("SHA256SUMS".into()), "no SHA256SUMS");
    refused(&|r| r["assets"][0]["size"] = json!(0), "empty installer");
    refused(
        &|r| r["assets"][1]["state"] = json!("starter"),
        "upload not finished",
    );
    refused(
        &|r| r["assets"][2]["browser_download_url"] = json!("https://example.com/x.dmg"),
        "download elsewhere",
    );
    refused(&|r| r["prerelease"] = json!(true), "pre-release");
    refused(&|r| r["draft"] = json!(true), "draft");
    refused(&|r| r["tag_name"] = json!("nightly"), "not a version tag");
    refused(
        &|r| r["html_url"] = json!("https://example.com/"),
        "notes elsewhere",
    );
    refused(&|r| r["published_at"] = json!("yesterday"), "no date");
}

/// A marker the script does not know, a malformed one, or a missing download
/// fails the deploy instead of reaching the page.
#[test]
fn apply_release_refuses_unknown_or_missing_markers() {
    let Some(py) = python() else {
        eprintln!("no python on this machine; apply_release.py not exercised");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let json_path = dir.path().join("release.json");
    std::fs::write(&json_path, serde_json::to_vec(&fixture()).unwrap()).unwrap();
    let all_links: String = LABELS
        .iter()
        .map(|(l, _)| format!("{{{{release.{l}.url}}}}\n"))
        .collect();
    let run = |page: &str, n: usize| {
        let site = dir.path().join(format!("site{n}"));
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(site.join("index.html"), page).unwrap();
        Command::new(py)
            .arg("-I")
            .arg(repo().join("scripts/site/apply_release.py"))
            .arg("--release-json")
            .arg(&json_path)
            .args(["--repo", "MAECLY/pluryprint", "--site"])
            .arg(&site)
            .arg("--out")
            .arg(dir.path().join(format!("out{n}")))
            .output()
            .unwrap()
    };
    assert!(run(&all_links, 0).status.success());
    let unknown = run(&format!("{all_links}{{{{release.windows_x86.url}}}}"), 1);
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown marker"));
    let malformed = run(&format!("{all_links}{{{{release.version}}"), 2);
    assert!(!malformed.status.success());
    let missing = run("{{release.windows_x64.url}}", 3);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("never links"));
    let bad_ld = run(
        &format!("{all_links}<script type=\"application/ld+json\">{{\"v\":{{{{release.version}}}}}}</script>"),
        4,
    );
    assert!(!bad_ld.status.success());
    assert!(String::from_utf8_lossy(&bad_ld.stderr).contains("valid JSON"));
}
