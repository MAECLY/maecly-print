/**
 * Typed IPC wrappers.
 *
 * For this scaffold we hand-write the wrappers around `@tauri-apps/api`'s
 * `invoke()`. In M1 the Rust side will emit `src/ipc/bindings.ts` via
 * `tauri-specta` and this file will simply re-export those typed commands.
 */

import { invoke } from '@tauri-apps/api/core';

/**
 * Shape returned by the Rust `app_info` command.
 * Mirrors `src-tauri/src/ipc/diag.rs::AppInfo`.
 */
export interface AppInfoLike {
  version: string;
  build_date: string;
  config_dir: string;
  data_dir: string;
  log_dir: string;
}

function isTauri(): boolean {
  // Tauri injects __TAURI_INTERNALS__ when running inside the host.
  if (typeof window === 'undefined') return false;
  return '__TAURI_INTERNALS__' in window || '__TAURI__' in window;
}

/** Returns the live AppInfo from Rust, or rejects in non-Tauri contexts. */
export async function fetchAppInfo(): Promise<AppInfoLike> {
  if (!isTauri()) {
    throw new Error('Not running inside Tauri (standalone Vite preview)');
  }
  return invoke<AppInfoLike>('app_info');
}
