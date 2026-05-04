fn main() {
    // Embed the build date so the About screen can display it.
    let build_date = chrono_like_now();
    println!("cargo:rustc-env=BUILD_DATE={build_date}");

    // Re-run if these change
    println!("cargo:rerun-if-changed=tauri.conf.json");
    println!("cargo:rerun-if-changed=Cargo.toml");

    tauri_build::build();
}

/// Tiny inline helper to avoid pulling `chrono` only for build metadata.
/// Returns YYYY-MM-DD UTC.
fn chrono_like_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before 1970")
        .as_secs();

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
