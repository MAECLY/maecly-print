//! Keep the Chromium-based web views this app starts (the app's own
//! WebView2, and the render worker's private one that prints HTML, Markdown
//! and text) from trying to log the user on with an empty password.
//!
//! On Windows, Chromium's password manager checks at startup whether the OS
//! account has a blank password (`CheckBlankPasswordWithPrefs` in
//! `password_manager_util_win.cc`): it calls `LogonUser(user, "", INTERACTIVE)`.
//! For every account that *has* a password that call fails, and Windows
//! records it as a failed logon (Security event 4625, status `0xC000006A`).
//! Windows 11 locks an account after 10 failed logons in 10 minutes by
//! default, so ten HTML prints, or a handful of app starts plus a few
//! conversions, locked the user out of their own machine (verified in a
//! Windows 11 24H2 VM: one 4625 per headless Edge run and one per app start,
//! then event 4740 "account locked out").
//!
//! The browser caches the answer in its `Local State` file:
//! `password_manager.os_password_last_changed` holds the time the password
//! was last changed when it last checked, and the check is skipped while
//! that time is not older than the account's real "password last set". The
//! browser rewrites the value with a freshly computed (and slightly jittered)
//! time on each run, so the cache never holds across launches by itself.
//!
//! [`suppress_blank_password_probe`] therefore stamps a far-future value into
//! the profile's `Local State` right before every launch. Measured in the VM
//! (Security log 4625 count before/after): headless Edge with a seeded
//! profile, 0 failures per conversion (1 without); the app's WebView2 with a
//! seeded `EBWebView`, 0 per start (1 without). (Headless Edge was the HTML
//! converter then; that browser fallback was removed in round 6, ADR-0012,
//! and the render worker's WebView2 is seeded the same way.) Nothing else in
//! the file is touched, and the profile is either private to this app (the
//! render worker's throwaway directory, the app's own `EBWebView`) — never
//! the user's browser.

use std::path::Path;

use serde_json::{Map, Value};

/// File name of a Chromium user-data directory's machine-wide prefs.
pub const LOCAL_STATE: &str = "Local State";

/// The `os_password_last_changed` value written: Chromium's internal time
/// (microseconds since 1601), as the string it stores int64 prefs as. It is
/// far enough in the future (year ~4770) that the account's password is
/// never "newer" than the cached check.
pub const FAR_FUTURE_LAST_CHANGED: &str = "99999999999999999";

/// `local_state` (the parsed file, or `None` when absent/unreadable) with the
/// blank-password probe disabled. Pure.
///
/// Only `password_manager.os_password_last_changed` is set; an existing
/// `os_password_blank` is kept (Chromium's default, `false`, otherwise). A
/// file that is not a JSON object is replaced by a minimal one: the browser
/// would discard it as corrupt anyway.
pub fn with_probe_suppressed(local_state: Option<Value>) -> Value {
    let mut root = match local_state {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    let pm = root
        .entry("password_manager")
        .or_insert_with(|| Value::Object(Map::new()));
    if !pm.is_object() {
        *pm = Value::Object(Map::new());
    }
    let pm = pm.as_object_mut().expect("object ensured above");
    pm.insert(
        "os_password_last_changed".into(),
        Value::String(FAR_FUTURE_LAST_CHANGED.into()),
    );
    pm.entry("os_password_blank").or_insert(Value::Bool(false));
    Value::Object(root)
}

/// Stamp the user-data directory `user_data_dir` so the next browser start
/// skips the blank-password logon probe (module docs). Creates the directory
/// and the file when missing. Writes through a temporary file and a rename,
/// so a browser starting concurrently never reads half a file.
pub fn suppress_blank_password_probe(user_data_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(user_data_dir)?;
    let path = user_data_dir.join(LOCAL_STATE);
    let current = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let updated = with_probe_suppressed(current);
    let tmp = user_data_dir.join(format!("{LOCAL_STATE}.pluryprint-tmp"));
    std::fs::write(&tmp, serde_json::to_vec(&updated)?)?;
    std::fs::rename(&tmp, &path)
}

/// The environment variable that points WebView2 at another user-data
/// folder than the one the app asks for.
pub const WEBVIEW2_USER_DATA_FOLDER_ENV: &str = "WEBVIEW2_USER_DATA_FOLDER";

/// The sub-folder of a WebView2 user-data folder its browser keeps the
/// Chromium profile (and `Local State`) in.
pub const EBWEBVIEW: &str = "EBWebView";

/// `args` split the way Chromium reads a Windows command line: on
/// whitespace outside double quotes, with the quotes removed. Pure.
fn split_command_line(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut started = false;
    for c in args.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                started = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if started {
                    out.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(current);
    }
    out
}

/// The user-data directories named by `--user-data-dir=` in `args` (a
/// WebView2 browser-argument string), in order. Chromium takes `--`, `-` or
/// `/` before a switch and its name in any case on Windows, and the value may
/// be quoted. Pure.
pub fn user_data_dirs_in_args(args: &str) -> Vec<std::path::PathBuf> {
    split_command_line(args)
        .into_iter()
        .filter_map(|token| {
            let name = token
                .strip_prefix("--")
                .or_else(|| token.strip_prefix('-'))
                .or_else(|| token.strip_prefix('/'))?;
            let (switch, value) = name.split_once('=')?;
            let value = value.trim();
            (switch.eq_ignore_ascii_case("user-data-dir") && !value.is_empty())
                .then(|| std::path::PathBuf::from(value))
        })
        .collect()
}

/// Every Chromium user-data directory a WebView2 started with these
/// settings may read its `Local State` from, besides the app's own
/// `EBWebView`, so each can be stamped before the web view starts (round 7:
/// the Edge WebDriver hands WebView2 its own `--user-data-dir` through
/// [`WEBVIEW2_ARGS_ENV`], whose profile was never stamped and recorded a
/// failed logon per test session).
///
/// - a `--user-data-dir=` in `browser_args`: that folder, and its
///   [`EBWEBVIEW`] sub-folder in case WebView2 treats it as a user-data
///   folder of its own;
/// - `user_data_folder` ([`WEBVIEW2_USER_DATA_FOLDER_ENV`]): its
///   [`EBWEBVIEW`] sub-folder, where WebView2 keeps the profile.
///
/// No duplicates. Pure.
pub fn extra_profiles_to_stamp(
    browser_args: Option<&str>,
    user_data_folder: Option<&std::ffi::OsStr>,
) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    let mut push = |p: std::path::PathBuf| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    for dir in browser_args.map(user_data_dirs_in_args).unwrap_or_default() {
        push(dir.join(EBWEBVIEW));
        push(dir);
    }
    if let Some(folder) = user_data_folder.filter(|f| !f.is_empty()) {
        push(std::path::Path::new(folder).join(EBWEBVIEW));
    }
    out
}

/// [`extra_profiles_to_stamp`] for this process' environment
/// ([`WEBVIEW2_ARGS_ENV`], [`WEBVIEW2_USER_DATA_FOLDER_ENV`]).
pub fn extra_profiles_from_env() -> Vec<std::path::PathBuf> {
    let args = std::env::var(WEBVIEW2_ARGS_ENV).ok();
    let folder = std::env::var_os(WEBVIEW2_USER_DATA_FOLDER_ENV);
    extra_profiles_to_stamp(args.as_deref(), folder.as_deref())
}

/// The browser arguments wry passes to WebView2 when a window sets none:
/// no "mini menu" and no SmartScreen ping, plus Tauri's autoplay policy.
pub const WRY_DEFAULT_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
--autoplay-policy=no-user-gesture-required";

/// The environment variable WebView2 reads extra browser arguments from.
pub const WEBVIEW2_ARGS_ENV: &str = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";

/// `defaults` plus `extra`, as one WebView2 argument string. Pure.
///
/// wry always hands WebView2 an explicit argument list, and WebView2 then
/// ignores `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`. That variable is how the
/// Edge WebDriver (`msedgedriver`, driven by `tauri-driver`) asks for
/// `--remote-debugging-port`, so without this merge no WebDriver session could
/// start (verified in a Windows 11 VM: `DevToolsActivePort file doesn't
/// exist`). Chromium honours only the **last** `--disable-features` /
/// `--enable-features` switch, so their lists are combined into one switch
/// each instead of letting `extra` silently cancel the defaults.
pub fn merge_browser_args(defaults: &str, extra: &str) -> String {
    let mut disable: Vec<String> = Vec::new();
    let mut enable: Vec<String> = Vec::new();
    let mut rest: Vec<String> = Vec::new();
    for arg in defaults.split_whitespace().chain(extra.split_whitespace()) {
        let list = if let Some(v) = arg.strip_prefix("--disable-features=") {
            Some((&mut disable, v))
        } else {
            arg.strip_prefix("--enable-features=")
                .map(|v| (&mut enable, v))
        };
        match list {
            Some((list, values)) => {
                for v in values.split(',').filter(|v| !v.is_empty()) {
                    if !list.iter().any(|x| x == v) {
                        list.push(v.to_string());
                    }
                }
            }
            None => {
                if !rest.iter().any(|x| x == arg) {
                    rest.push(arg.to_string());
                }
            }
        }
    }
    let mut out = Vec::new();
    if !disable.is_empty() {
        out.push(format!("--disable-features={}", disable.join(",")));
    }
    if !enable.is_empty() {
        out.push(format!("--enable-features={}", enable.join(",")));
    }
    out.extend(rest);
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The Edge WebDriver's arguments reach WebView2, and the defaults
    /// survive: one `--disable-features` switch carrying both lists.
    #[test]
    fn webdriver_args_are_merged_with_the_defaults() {
        let merged = merge_browser_args(
            WRY_DEFAULT_ARGS,
            "--remote-debugging-port=0 --disable-features=IgnoreDuplicateNavs,Prewarm --disable-sync",
        );
        assert_eq!(
            merged,
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,IgnoreDuplicateNavs,Prewarm \
--autoplay-policy=no-user-gesture-required --remote-debugging-port=0 --disable-sync"
        );
        assert_eq!(merged.matches("--disable-features").count(), 1);
        assert_eq!(merge_browser_args(WRY_DEFAULT_ARGS, ""), WRY_DEFAULT_ARGS);
        assert_eq!(
            merge_browser_args("--a", "--enable-features=X --a --enable-features=Y,X"),
            "--enable-features=X,Y --a"
        );
    }

    /// Round 7: the profile the Edge WebDriver names in the WebView2
    /// arguments (and a `WEBVIEW2_USER_DATA_FOLDER`) is stamped too.
    #[test]
    fn nfr_security_webdriver_profiles_are_found_in_the_browser_args() {
        use std::path::PathBuf;
        let args = r#"--remote-debugging-port=0 --user-data-dir=C:\Users\me\AppData\Local\Temp\scoped_dir1_2 --disable-sync"#;
        assert_eq!(
            user_data_dirs_in_args(args),
            [PathBuf::from(
                r"C:\Users\me\AppData\Local\Temp\scoped_dir1_2"
            )]
        );
        // Quoted values with spaces, other prefixes and any case.
        assert_eq!(
            user_data_dirs_in_args(
                r#"--x "--user-data-dir=C:\My Temp\a" -USER-DATA-DIR=D:\b /user-data-dir="E:\c d""#
            ),
            [
                PathBuf::from(r"C:\My Temp\a"),
                PathBuf::from(r"D:\b"),
                PathBuf::from(r"E:\c d")
            ]
        );
        // Not the switch, or no value.
        for none in [
            "",
            "--user-data-dir",
            "--user-data-dir=",
            "--user-data-dir-x=C:\\a",
            "--profile-directory=C:\\a",
            "user-data-dir=C:\\a",
        ] {
            assert!(user_data_dirs_in_args(none).is_empty(), "{none}");
        }

        let dirs = extra_profiles_to_stamp(
            Some("--user-data-dir=C:\\t\\s --user-data-dir=C:\\t\\s"),
            Some(std::ffi::OsStr::new("D:\\wv")),
        );
        assert_eq!(
            dirs,
            [
                PathBuf::from("C:\\t\\s").join(EBWEBVIEW),
                PathBuf::from("C:\\t\\s"),
                PathBuf::from("D:\\wv").join(EBWEBVIEW),
            ]
        );
        assert!(extra_profiles_to_stamp(None, None).is_empty());
        assert!(extra_profiles_to_stamp(Some("--a"), Some(std::ffi::OsStr::new(""))).is_empty());
    }

    #[test]
    fn fr_055_a_missing_local_state_gets_a_minimal_one() {
        let v = with_probe_suppressed(None);
        assert_eq!(
            v,
            json!({"password_manager": {
                "os_password_last_changed": FAR_FUTURE_LAST_CHANGED,
                "os_password_blank": false
            }})
        );
    }

    #[test]
    fn fr_055_an_existing_local_state_keeps_everything_else() {
        let v = with_probe_suppressed(Some(json!({
            "browser": {"x": 1},
            "password_manager": {
                "os_password_blank": true,
                "os_password_last_changed": "13436067249815884",
                "other": 2
            }
        })));
        assert_eq!(v["browser"], json!({"x": 1}));
        assert_eq!(v["password_manager"]["other"], json!(2));
        assert_eq!(v["password_manager"]["os_password_blank"], json!(true));
        assert_eq!(
            v["password_manager"]["os_password_last_changed"],
            json!(FAR_FUTURE_LAST_CHANGED)
        );
    }

    #[test]
    fn fr_055_garbage_is_replaced_not_propagated() {
        let v = with_probe_suppressed(Some(json!([1, 2])));
        assert!(v["password_manager"].is_object());
        let v = with_probe_suppressed(Some(json!({"password_manager": 7})));
        assert_eq!(
            v["password_manager"]["os_password_last_changed"],
            json!(FAR_FUTURE_LAST_CHANGED)
        );
    }

    #[test]
    fn fr_055_suppression_writes_the_file_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let udd = dir.path().join("browser");
        suppress_blank_password_probe(&udd).unwrap();
        suppress_blank_password_probe(&udd).unwrap();
        let v: Value =
            serde_json::from_slice(&std::fs::read(udd.join(LOCAL_STATE)).unwrap()).unwrap();
        assert_eq!(
            v["password_manager"]["os_password_last_changed"],
            json!(FAR_FUTURE_LAST_CHANGED)
        );
        // No temporary file is left behind.
        assert_eq!(std::fs::read_dir(&udd).unwrap().count(), 1);
    }
}
