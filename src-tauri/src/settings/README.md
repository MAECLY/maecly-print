# settings

Loads and persists `config.toml` (atomic writes). Exposed via the IPC commands in `src-tauri/src/ipc/settings.rs` (`get_app_settings` / `set_app_settings`).

See `docs/04-architecture/data-model.md` §2 for the schema and migration policy.

## Engine limits — one source, three consumers

`ENGINE_LIMITS` (a `const EngineLimits` of three `Bounds`) is the only place the
numeric ranges are written down. Everything else derives from it:

- `EngineCfg::default()` — the shipped values (`Bounds::default`).
- `EngineCfg::sanitized()` — clamps a hand-edited or downgrade-corrupted file
  through `Bounds::sanitize`, honouring each knob's `ZeroPolicy` (`0` disables
  the timeout, falls back to the default size ceiling, and clamps elsewhere).
- `ipc::settings::validate_bounds` — rejects an out-of-range write through
  `Bounds::accepts`, with a message phrased from the same numbers.
- The Settings screen — the very same values ride to the frontend in
  `AppSettings.meta.limits`, so the inline bounds check and its error text are
  generated from the server's numbers instead of a hand-copied second set.

Changing a limit is therefore a one-line edit here.

## Reading the config

| Function                 | Corrupt file behavior                             | Use it when                                                                    |
| ------------------------ | ------------------------------------------------- | ------------------------------------------------------------------------------ |
| `Config::load_reporting` | Degrades to defaults **and reports** `unreadable` | The caller can tell the user (FR-101) — `get_app_settings` does.               |
| `Config::load_from`      | Degrades to defaults silently                     | The caller cannot act on it (queue boot, `launch_minimized`).                  |
| `Config::load_strict`    | Errors (a _missing_ file is still `Ok(default)`)  | Every mutator, so a broken file is never rewritten as "defaults + this patch". |

`AppSettings.meta` carries `config_unreadable` + `config_path` alongside the
limits. It is server-computed on every response and **ignored on write** — the
client only ever echoes back what a read gave it.

## Startup

`launch_minimized()` reads `[app].launch_minimized` straight from disk: the
window has to be minimized before it is shown, long before the first IPC
round-trip. It is called from `run_with`'s `setup` in `src-tauri/src/lib.rs`.
