fn main() {
    // Embed the build date so the About screen (and the prefilled issue
    // report) can display it.
    //
    // Reproducible/release builds pin it through the standard
    // `SOURCE_DATE_EPOCH` (seconds since the epoch); everything else uses the
    // wall clock at the moment this script runs.
    let epoch = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok());
    let build_date = civil_date(epoch.unwrap_or_else(now_secs));
    println!("cargo:rustc-env=BUILD_DATE={build_date}");

    // Once a build script prints any `rerun-if-*` line, Cargo stops re-running
    // it on every package change and only watches what is listed. Watching just
    // the manifests meant a source-only rebuild weeks later still reported the
    // first build's date, so the source tree is watched too: any change that
    // produces a new binary also produces a new date.
    println!("cargo:rerun-if-changed=tauri.conf.json");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    println!("cargo:rerun-if-changed=windows-app-manifest.xml");

    // Windows: the Common-Controls v6 manifest must reach *every* linked
    // artifact, not only the app binary. `tauri_build::build()` embeds it as a
    // resource that is linked into `[[bin]]` targets alone, so the lib's test
    // harness (`maeclyprint_lib-*.exe`) imports comctl32 v6 entry points
    // (TaskDialogIndirect, pulled in by tao/rfd) without the manifest that
    // makes the loader resolve them, and dies with STATUS_ENTRYPOINT_NOT_FOUND
    // (0xc0000139) before running a single test. Tauri's own guidance is to
    // drop its manifest resource and hand the same manifest to the linker for
    // all targets instead (`rustc-link-arg` covers tests, benches and bins).
    // Doing both would embed two manifests and fail the link (CVT1100).
    // `windows-app-manifest.xml` is a verbatim copy of tauri-build's default.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        // A missing manifest would only show up at run time on Windows, as
        // the same entry-point crash, so fail the build here instead.
        assert!(manifest.is_file(), "missing {}", manifest.display());
        let attrs = tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        tauri_build::try_build(attrs).expect("tauri-build failed");
    } else {
        tauri_build::build();
    }
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before 1970")
        .as_secs()
}

/// Tiny inline helper to avoid pulling `chrono` only for build metadata.
/// Returns the UTC `YYYY-MM-DD` of `secs` since the Unix epoch.
fn civil_date(secs: u64) -> String {
    // Days since epoch
    let days = (secs / 86_400) as i64;

    // Civil-from-days algorithm by Howard Hinnant (public domain).
    // https://howardhinnant.github.io/date_algorithms.html
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}")
}
