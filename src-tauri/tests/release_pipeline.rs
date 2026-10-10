//! The release pipeline's contract (FR-080..086, NFR-006, NFR-022, NFR-027,
//! NFR-070), checked from the files themselves so a reviewer does not have to
//! run a release to see it broken:
//!
//! - `.github/workflows/release.yml` builds the four targets into a draft,
//!   refuses to run without the updater signing secret, names every asset for
//!   the computer it is for, verifies signatures, writes SHA256SUMS and
//!   attaches the Corresponding Source;
//! - `.github/workflows/release-channels.yml` moves the beta manifest forward
//!   only;
//! - `scripts/release/release_tools.py` implements those rules (exercised
//!   here where a `python3` exists);
//! - `scripts/bump-version.sh` keeps the version sources in step.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read(rel: &str) -> String {
    let p = repo().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn release_yml() -> String {
    read(".github/workflows/release.yml")
}

/// Whether `line` opens a job: a key at two spaces of indentation.
fn is_job_key(line: &str) -> bool {
    line.strip_prefix("  ").is_some_and(|key| {
        key.ends_with(':')
            && key
                .trim_end_matches(':')
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && !key.starts_with(' ')
            && key.len() > 1
    })
}

/// The text of the job named `name`, from its key to the next job's.
fn job(workflow: &str, name: &str) -> String {
    let jobs = workflow
        .split_once("\njobs:\n")
        .unwrap_or_else(|| panic!("no jobs"))
        .1;
    let key = format!("  {name}:");
    let mut lines = jobs.lines().skip_while(|l| *l != key);
    let first = lines.next().unwrap_or_else(|| panic!("no job {name}"));
    let mut out = vec![first];
    out.extend(lines.take_while(|l| !is_job_key(l)));
    out.join("\n")
}

/// NFR-022: a tag build stops before building anything when the updater
/// signing key is not configured, and every build waits for that check.
#[test]
fn nfr_022_release_requires_signature_secret() {
    let wf = release_yml();
    let guard = job(&wf, "guard");
    assert!(
        guard.contains("secrets.TAURI_SIGNING_PRIVATE_KEY"),
        "{guard}"
    );
    assert!(
        guard.contains(r#"if [ -z "$SIGNING_KEY" ]; then"#),
        "{guard}"
    );
    assert!(guard.contains("exit 1"), "{guard}");
    let build = job(&wf, "build");
    assert!(build.contains("needs: [guard]"), "{build}");
    assert!(
        build.contains("needs.guard.result != 'failure'"),
        "a failed guard must stop the builds: {build}"
    );
    // The key reaches tauri-action, which signs the update artifacts.
    assert!(build.contains("TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}"));
    assert!(build.contains(
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}"
    ));
    // The signatures are verified against the key the app carries, and a
    // tampered artifact must fail.
    let finalise = job(&wf, "finalise");
    assert!(
        finalise.contains("minisign -V -p updater.pub"),
        "{finalise}"
    );
    assert!(
        finalise.contains("['plugins']['updater']['pubkey']"),
        "{finalise}"
    );
    assert!(
        finalise.contains("a tampered artifact verified"),
        "{finalise}"
    );
    // Each platform is paired with its file through the asset listing, and
    // the workings stay out of assets/, which SHA256SUMS covers.
    assert!(finalise.contains("release_tools.py extract-signatures"));
    assert!(finalise.contains("--out sigcheck"));
}

/// The version a release is tagged with is the version every source carries:
/// Cargo.toml (the only one the app reads), package.json (kept equal) and no
/// `version` in tauri.conf.json.
#[test]
fn release_version_sources_agree() {
    let cargo = read("src-tauri/Cargo.toml");
    let cargo_version = cargo
        .lines()
        .find(|l| l.starts_with("version = "))
        .and_then(|l| l.split('"').nth(1))
        .expect("Cargo.toml [package] version");
    assert_eq!(cargo_version, env!("CARGO_PKG_VERSION"));

    let pkg: serde_json::Value = serde_json::from_str(&read("package.json")).unwrap();
    assert_eq!(
        pkg["version"], cargo_version,
        "package.json and Cargo.toml disagree"
    );

    let conf: serde_json::Value = serde_json::from_str(&read("src-tauri/tauri.conf.json")).unwrap();
    assert!(conf.get("version").is_none());

    // The guard compares the tag with all of them.
    let guard = job(&release_yml(), "guard");
    for needle in [
        "src-tauri/Cargo.toml",
        "package.json",
        "src-tauri/tauri.conf.json",
    ] {
        assert!(guard.contains(needle), "guard does not read {needle}");
    }
}

/// NFR-070 and decision D2: four builds, each named for the computer it is
/// for (Versorium's `assetPattern`), Windows as NSIS only.
#[test]
fn nfr_070_release_builds_every_platform_with_its_own_asset_names() {
    let build = job(&release_yml(), "build");
    for (args, pattern) in [
        (
            "--target aarch64-apple-darwin --bundles app,dmg",
            "[name]_[version]_apple_silicon[ext]",
        ),
        (
            "--target x86_64-apple-darwin --bundles app,dmg",
            "[name]_[version]_apple_intel[ext]",
        ),
        (
            "--target x86_64-pc-windows-msvc --bundles nsis",
            "[name]_[version]_windows_x64[ext]",
        ),
        (
            "--target aarch64-pc-windows-msvc --bundles nsis",
            "[name]_[version]_windows_arm64[ext]",
        ),
    ] {
        assert!(build.contains(&format!("args: '{args}'")), "missing {args}");
        assert!(
            build.contains(&format!("assetPattern: '{pattern}'")),
            "missing {pattern}"
        );
    }
    assert!(build.contains("releaseAssetNamePattern: ${{ matrix.assetPattern }}"));
    assert!(build.contains("updaterJsonPreferNsis: true"));

    // tauri-action is pinned to a commit, not a moving tag.
    assert!(build.contains(
        "uses: tauri-apps/tauri-action@1deb371b0cd8bd54025b384f1cd735e725c4060f # action-v1.0.0"
    ));
    // A draft, never published by the workflow.
    assert!(build.contains("releaseDraft: true"));
    assert!(!release_yml().contains("--draft=false"));
}

/// Every name a build uploads is distinct across the release: the same
/// extension in two builds must not render to one file name, or one build
/// silently replaces the other's file (tauri-action deletes a same-named
/// asset before uploading).
#[test]
fn release_asset_names_never_collide() {
    let build = job(&release_yml(), "build");
    let patterns: Vec<&str> = build
        .lines()
        .filter_map(|l| l.trim().strip_prefix("assetPattern: '"))
        .map(|l| l.trim_end_matches('\''))
        .collect();
    assert_eq!(patterns.len(), 4, "{patterns:?}");
    let exts = [".dmg", ".app.tar.gz", ".app.tar.gz.sig", ".exe", ".exe.sig"];
    let mut seen = std::collections::HashSet::new();
    for p in &patterns {
        for ext in exts {
            let name = p
                .replace("[name]", "Pluryprint")
                .replace("[version]", "1.2.3")
                .replace("[ext]", ext);
            assert!(seen.insert(name.clone()), "two builds would upload {name}");
        }
    }
}

/// Decision D2: no certificates, so the release notes say how to get past
/// Gatekeeper and SmartScreen, in English and Spanish.
#[test]
fn d2_release_notes_explain_gatekeeper_and_smartscreen_in_en_and_es() {
    let wf = release_yml();
    for needle in [
        "xattr -rd com.apple.quarantine /Applications/Pluryprint.app",
        "**More info**, then **Run anyway**",
        "**Más información** y después **Ejecutar de todas formas**",
        "## English",
        "## Español",
        "apple_silicon",
        "apple_intel",
        "windows_x64",
        "windows_arm64",
    ] {
        assert!(wf.contains(needle), "release notes lack {needle:?}");
    }
}

/// FR-085: the manifest is complete for the four platforms the app updates
/// on before anyone can publish, with URLs the app can download, and
/// checksums of every file.
#[test]
fn fr_085_finalise_requires_every_platform_and_writes_sha256sums() {
    let finalise = job(&release_yml(), "finalise");
    for key in [
        "darwin-aarch64",
        "darwin-x86_64",
        "windows-x86_64",
        "windows-aarch64",
    ] {
        assert!(
            finalise.contains(&format!("--require {key}")),
            "finalise does not require {key}"
        );
    }
    assert!(finalise.contains("needs.build.result == 'success'"));
    assert!(finalise.contains("sha256sum -- * > ../SHA256SUMS"));
    assert!(finalise.contains("gh release upload \"$GITHUB_REF_NAME\" SHA256SUMS latest.json"));
}

/// FR-085/086: publishing a release moves the beta manifest, forward only.
#[test]
fn fr_086_beta_pointer_moves_forward_only() {
    let wf = read(".github/workflows/release-channels.yml");
    assert!(wf.contains("types: [published]"));
    assert!(wf.contains("BETA_TAG: channel-beta"));
    assert!(wf.contains("release_tools.py semver-gt \"$new_version\" \"$current_version\""));
    assert!(wf.contains("gh release upload \"$BETA_TAG\" new/latest.json"));
    // The tag reaches the shell through the environment, never spliced in.
    assert!(wf.contains("TAG: ${{ github.event.release.tag_name }}"));
    assert!(!wf.contains("run: ${{"));
}

/// AGPL-3.0 and GPL-3.0 section 6: every release carries the source of
/// Pluryprint and of the SumatraPDF build its installers contain.
#[test]
fn nfr_027_releases_attach_the_corresponding_source() {
    let finalise = job(&release_yml(), "finalise");
    assert!(finalise.contains("git archive --format=tar.gz"));
    assert!(finalise.contains("pluryprint-$version-source.tar.gz"));
    assert!(finalise.contains("SumatraPDF-$sumatra_version-source.tar.gz"));
    // The commit comes from the fetch script, so the two cannot drift.
    assert!(finalise.contains("SOURCE_COMMIT"));
    assert!(finalise.contains("scripts/fetch-sumatrapdf.sh"));
    // And the Windows builds really fetch SumatraPDF and the DLLs.
    let build = job(&release_yml(), "build");
    assert!(build.contains("./scripts/fetch-sumatrapdf.ps1 -Arch ${{ matrix.arch }}"));
    assert!(build.contains("./scripts/build-shell-ext.ps1 -Arch all"));
}

/// NFR-006: the installer size is measured on every Windows build.
#[test]
fn nfr_006_installer_size_is_measured_against_the_budget() {
    let build = job(&release_yml(), "build");
    assert!(build.contains("Installer size budget (NFR-006)"));
    assert!(build.contains("$budget = 15MB"));
    let ci = read(".github/workflows/ci.yml");
    assert!(
        ci.contains("NFR-006"),
        "ci.yml does not measure the installer"
    );
}

fn python() -> Option<&'static str> {
    ["python3", "python"].into_iter().find(|p| {
        Command::new(p)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    })
}

fn tools(args: &[&str]) -> Option<std::process::Output> {
    let py = python()?;
    Some(
        Command::new(py)
            .arg("-I")
            .arg(repo().join("scripts/release/release_tools.py"))
            .args(args)
            .output()
            .expect("run release_tools.py"),
    )
}

/// FR-086: the beta pointer's comparison is SemVer precedence.
#[test]
fn fr_086_semver_precedence_of_the_release_tools() {
    let gt = |a: &str, b: &str| tools(&["semver-gt", a, b]).map(|o| o.status.code() == Some(0));
    let Some(_) = gt("1.0.0", "0.9.0") else {
        eprintln!("no python on this machine; release_tools.py not exercised");
        return;
    };
    for (a, b, want) in [
        ("0.2.0", "0.2.0-beta.3", true),
        ("0.2.0-beta.3", "0.2.0", false),
        ("0.2.0-beta.10", "0.2.0-beta.9", true),
        ("0.2.0-beta.2", "0.2.0-alpha.9", true),
        ("0.2.0-rc.1", "0.2.0-beta.9", true),
        ("0.1.5", "0.2.0-beta.3", false),
        ("v0.3.0", "0.2.9", true),
        ("0.2.0", "0.2.0", false),
        ("0.2.0+build.2", "0.2.0", false),
        ("0.2.0-beta", "0.2.0-beta.1", false),
        ("0.2.0-1", "0.2.0-beta", false),
    ] {
        assert_eq!(gt(a, b), Some(want), "{a} > {b}");
    }
    let bad = tools(&["semver-gt", "nightly", "1.0.0"]).unwrap();
    assert_eq!(bad.status.code(), Some(2));
}

/// FR-085: the finalise rewrite points every platform at an asset of this
/// release in the API form, refuses a manifest that lost a platform, and
/// carries the release body as the update notes.
#[test]
fn fr_085_finalise_manifest_rewrites_urls_and_refuses_a_lost_platform() {
    if python().is_none() {
        eprintln!("no python on this machine; release_tools.py not exercised");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let p = dir.path().join(name);
        std::fs::write(&p, text).unwrap();
        p.to_string_lossy().into_owned()
    };
    let manifest = write(
        "latest.json",
        r#"{"version":"0.2.0","notes":"x","pub_date":"2026-10-10T00:00:00Z","platforms":{
            "darwin-aarch64":{"signature":"c2lnMQ==","url":"https://github.com/MAECLY/maecly-print/releases/download/v0.2.0/Pluryprint_0.2.0_apple_silicon.app.tar.gz"},
            "darwin-x86_64":{"signature":"c2lnMg==","url":"https://api.github.com/repos/MAECLY/maecly-print/releases/assets/12"},
            "windows-x86_64":{"signature":"c2lnMw==","url":"https://github.com/MAECLY/maecly-print/releases/download/v0.2.0/Pluryprint_0.2.0_windows_x64.exe"}
        }}"#,
    );
    let assets = write(
        "assets.json",
        r#"[{"name":"Pluryprint_0.2.0_apple_silicon.app.tar.gz","id":11},
            {"name":"Pluryprint_0.2.0_apple_intel.app.tar.gz","id":12},
            {"name":"Pluryprint_0.2.0_windows_x64.exe","id":13}]"#,
    );
    let body = write("body.md", "  What changed  \n");
    let out = dir.path().join("out.json");
    let out_s = out.to_string_lossy().into_owned();

    let base = [
        "finalise-manifest",
        "--manifest",
        &manifest,
        "--assets",
        &assets,
        "--repo",
        "MAECLY/maecly-print",
        "--body-file",
        &body,
        "--out",
        &out_s,
    ];
    let ok = tools(
        &[
            &base[..],
            &["--require", "darwin-aarch64", "--require", "windows-x86_64"],
        ]
        .concat(),
    )
    .unwrap();
    assert!(ok.status.success(), "{ok:?}");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    let api = "https://api.github.com/repos/MAECLY/maecly-print/releases/assets/";
    assert_eq!(v["platforms"]["darwin-aarch64"]["url"], format!("{api}11"));
    assert_eq!(v["platforms"]["darwin-x86_64"]["url"], format!("{api}12"));
    assert_eq!(v["platforms"]["windows-x86_64"]["url"], format!("{api}13"));
    assert_eq!(v["notes"], "What changed");

    // windows-aarch64 is missing: the build for it failed or raced.
    let lost = tools(&[&base[..], &["--require", "windows-aarch64"]].concat()).unwrap();
    assert_eq!(lost.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&lost.stderr).contains("windows-aarch64"));

    // A URL naming a file this release does not have.
    let foreign = write(
        "foreign.json",
        r#"{"version":"0.2.0","platforms":{"darwin-aarch64":{"signature":"s","url":"https://example.com/evil.tar.gz"}}}"#,
    );
    let bad = tools(&[
        "finalise-manifest",
        "--manifest",
        &foreign,
        "--assets",
        &assets,
        "--repo",
        "MAECLY/maecly-print",
    ])
    .unwrap();
    assert_eq!(bad.status.code(), Some(1));

    // check-manifest: the version must be the tag's.
    let wrong = tools(&["check-manifest", "--manifest", &out_s, "--version", "0.3.0"]).unwrap();
    assert_eq!(wrong.status.code(), Some(1));
    let right = tools(&[
        "check-manifest",
        "--manifest",
        &out_s,
        "--version",
        "0.2.0",
        "--require",
        "darwin-aarch64",
    ])
    .unwrap();
    assert!(right.status.success(), "{right:?}");

    // NFR-022: the signature of each platform is decoded next to, not into,
    // the assets, and paired with the file its API URL names by id.
    let sigs = dir.path().join("sigcheck");
    let sigs_s = sigs.to_string_lossy().into_owned();
    let extracted = tools(&[
        "extract-signatures",
        "--manifest",
        &out_s,
        "--assets",
        &assets,
        "--out",
        &sigs_s,
        "--require",
        "darwin-aarch64",
        "--require",
        "windows-x86_64",
    ])
    .unwrap();
    assert!(extracted.status.success(), "{extracted:?}");
    assert_eq!(
        String::from_utf8_lossy(&extracted.stdout),
        "darwin-aarch64 Pluryprint_0.2.0_apple_silicon.app.tar.gz\n\
         windows-x86_64 Pluryprint_0.2.0_windows_x64.exe\n"
    );
    // The manifest carries the signature in base64; minisign reads the
    // decoded bytes.
    assert_eq!(
        std::fs::read(sigs.join("darwin-aarch64.minisig")).unwrap(),
        b"sig1"
    );
    let missing = tools(&[
        "extract-signatures",
        "--manifest",
        &out_s,
        "--assets",
        &assets,
        "--out",
        &sigs_s,
        "--require",
        "windows-aarch64",
    ])
    .unwrap();
    assert_eq!(missing.status.code(), Some(1));
}

/// `scripts/bump-version.sh X.Y.Z[-pre]` writes the version into Cargo.toml,
/// Cargo.lock and package.json together, and refuses a non-SemVer one.
#[cfg(unix)]
#[test]
fn release_bump_version_script_keeps_the_sources_in_step() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src-tauri")).unwrap();
    for f in [
        "src-tauri/Cargo.toml",
        "src-tauri/Cargo.lock",
        "package.json",
    ] {
        std::fs::copy(repo().join(f), root.join(f)).unwrap();
    }
    let run = |version: &str| {
        Command::new("bash")
            .arg(repo().join("scripts/bump-version.sh"))
            .arg("--root")
            .arg(root)
            .arg("--no-cargo")
            .arg(version)
            .output()
            .expect("run bump-version.sh")
    };
    let out = run("9.8.7-beta.1");
    assert!(out.status.success(), "{out:?}");

    let cargo = std::fs::read_to_string(root.join("src-tauri/Cargo.toml")).unwrap();
    assert!(cargo.contains("\nversion = \"9.8.7-beta.1\"\n"), "{cargo}");
    // Only the package's own version moved, not a dependency's.
    assert_eq!(cargo.matches("9.8.7-beta.1").count(), 1);
    let lock = std::fs::read_to_string(root.join("src-tauri/Cargo.lock")).unwrap();
    assert!(lock.contains("name = \"pluryprint\"\nversion = \"9.8.7-beta.1\""));
    let pkg: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("package.json")).unwrap()).unwrap();
    assert_eq!(pkg["version"], "9.8.7-beta.1");

    for bad in ["1.2", "v1.2.3", "1.2.3-", "01.2.3"] {
        let out = run(bad);
        assert!(!out.status.success(), "{bad} was accepted");
    }
    assert!(Path::new(&root.join("package.json")).exists());
}
