# settings

Loads and persists `config.toml` (atomic writes). Exposed via the IPC commands in `src-tauri/src/ipc/settings.rs` (`get_app_settings` / `set_app_settings`).

See `docs/04-architecture/data-model.md` §2 for the schema and migration policy.
