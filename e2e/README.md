# End-to-end tests

WebdriverIO specs that drive the **real built MaeclyPrint binary** through
[`tauri-driver`](https://v2.tauri.app/develop/tests/webdriver/) — the WebDriver
bridge Tauri ships. These cover flows, not features (the feature-level coverage
lives in the Rust and Vitest unit tests); see
[`docs/08-development/testing-strategy.md`](../docs/08-development/testing-strategy.md).

## Layout

```
e2e/
├── wdio.conf.ts         # runner config; spawns/kills tauri-driver, screenshots on failure
├── tsconfig.json        # standalone TS config (not part of `pnpm typecheck`)
├── artifacts/           # failure screenshots (gitignored)
└── specs/
    ├── helpers.ts       # dismiss onboarding, navigate the sidebar
    ├── profiles.e2e.ts  # create a custom profile, make it default
    ├── settings.e2e.ts  # switch theme to dark, assert the dark class
    └── about.e2e.ts     # About shows a semver version + license/release links
```

Specs are named `*.e2e.ts` (not `*.spec.ts`) so vitest's default glob never
picks them up — `pnpm test` stays unit-only and `pnpm test:e2e` is the sole
E2E entry point.

## Platform support

`tauri-driver` supports **Windows and Linux only** — WKWebView on macOS exposes
no WebDriver endpoint. The CI e2e job runs on Windows and is **non-blocking**
(`continue-on-error`) because a real GUI driver is inherently flake-prone.

## Running locally (Windows)

```powershell
# 1. Install the WebDriver bridge (once).
cargo install tauri-driver --locked

# 2. Build the app so there is a binary to drive.
pnpm tauri build --debug

# 3. Point tauri-driver at the Edge WebDriver that matches your WebView2, then
#    run the suite. msedgedriver.exe ships with the `edgedriver` npm package
#    (a WebdriverIO dependency) or with the Selenium tools on CI runners.
$env:NATIVE_DRIVER = "C:\path\to\msedgedriver.exe"
pnpm test:e2e
```

Environment variables:

| Var              | Purpose                                                                        |
| ---------------- | ------------------------------------------------------------------------------ |
| `TAURI_APP_PATH` | Override the binary path (default `src-tauri/target/debug/maeclyprint[.exe]`). |
| `NATIVE_DRIVER`  | Path to `msedgedriver.exe` (Windows) passed to `tauri-driver`.                 |
