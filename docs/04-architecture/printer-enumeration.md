# Printer Enumeration & Detection

> **TL;DR** — How MaeclyPrint discovers printers and reads their capabilities on **Windows** (print spooler via the `windows` crate) and **macOS/Linux** (CUPS via its CLIs). Not every printer or OS reports every attribute, so the backend returns whatever the system gives and the UI degrades gracefully — absent fields are `null`/empty, never faked.

This is the source of truth for **how** `ipc::printers::{list_printers, default_printer, discover_printers}` are implemented. The command/return contract lives in [ipc-and-cli.md](./ipc-and-cli.md); the screen that consumes it is [printers-management.md](../05-features/printers-management.md); the Windows-first sequencing is [ADR-0007](../adr/0007-windows-first-mac-second.md).

## Strategy

| Platform | Backend (`src-tauri/src/platform/`) | Mechanism |
| --- | --- | --- |
| macOS, Linux | `cups.rs` (`#[cfg(unix)]`) | Shell out to `lpstat` / `lpoptions` / `ippfind`, parse **IPP-keyword** output |
| Windows | `windows.rs` (`#[cfg(windows)]`) | `EnumPrinters` + `DeviceCapabilities` + `GetDefaultPrinter` via the `windows` crate |

**Why CLI shell-out on Unix, not FFI:** the only CUPS FFI crate (`cups-sys`) is unmaintained, and the high-level `printers` crate exposes no capabilities. The CUPS CLIs are stable, dependency-free, and expose everything we need as machine-readable IPP keywords. The unified `Printer` shape is built per-platform and returned by all three commands.

## macOS / CUPS

**The locale trap.** macOS localizes `lpstat`'s human-readable prose *even under `LC_ALL=C`* (e.g. "la impresora … está inactiva"). We therefore **never parse `lpstat`'s sentences** — only locale-independent sources:

| Need | Source | Notes |
| --- | --- | --- |
| Queue names | `lpstat -e` | Bare names, one per line |
| Default | `lpstat -d` | Trailing token, trusted only if it's a known queue |
| Metadata | `lpoptions -p NAME` | IPP `key=value` line (quote-aware parse) |
| Capabilities | `lpoptions -p NAME -l` | `ColorModel`, `Duplex`, `PageSize`, `InputSlot`, `Resolution` |
| Supplies | `marker-colors` + `marker-levels` (from `lpoptions -p`) | CMYK by hex: `#000000`→black, `#00FFFF`→cyan, … (0–100 → 0–1) |
| Discovery | `ippfind _ipp._tcp _ipps._tcp -T 4` | mDNS/Bonjour URIs; already-installed devices filtered out |

`lpoptions -p NAME` yields a single line such as:

```
device-uri=dnssd://EPSON%20L15160%20Series._ipps._tcp.local./ printer-state=3
marker-colors=#000000,#00FFFF,#FF00FF,#FFFF00 marker-levels=38,52,51,55
printer-make-and-model='EPSON L15160 Series-AirPrint' printer-is-shared=false …
```

Values may be single-quoted (spaces) or bare (empty); `printer-state` is `3=idle→ready`, `4=printing→busy`, `5=stopped→error`, refined by `printer-state-reasons` (`offline`/`shutdown`→offline). A real device here reports **no** `Resolution` line → `max_dpi = null` (a concrete example of graceful degradation).

## Windows

`EnumPrinters` (level 2 → `PRINTER_INFO_2W`) gives name, `pPortName`, `pDriverName`, `pLocation`, `Attributes` (shared bit), `Status` bitmap, and `cJobs`. `DeviceCapabilities` adds the rest; `GetDefaultPrinter` resolves the default.

| Capability | `DeviceCapabilities` cap |
| --- | --- |
| Color | `DC_COLORDEVICE` |
| Duplex | `DC_DUPLEX` |
| Paper sizes | `DC_PAPERNAMES` (64-WCHAR entries) |
| Trays | `DC_BINNAMES` (24-WCHAR entries) |
| Max DPI | `DC_ENUMRESOLUTIONS` (LONG x,y pairs) |

**`windows` crate gotchas (0.58), discovered via `cargo check --target x86_64-pc-windows-msvc`:** `DeviceCapabilitiesW` and the `DC_*` constants live under `Win32::Storage::Xps` (not `Gdi`); `PRINTER_INFO_2W` needs the `Win32_Security` **and** `Win32_Graphics_Gdi` features; `PRINTER_STATUS_*` / `PRINTER_ATTRIBUTE_*` are plain `u32`; `GetDefaultPrinterW` returns `BOOL`. **Toner/supply levels are not exposed by Win32** (they require SNMP/WMI/IPP) and are left empty. Network **discovery** (WSD/mDNS) still returns an empty list on Windows — `platform::windows::discover_printers` is an explicit `Ok(Vec::new())`, not a stub that fails.

## Cross-platform capability matrix

| Field | Windows | macOS/CUPS | Reliable? |
| --- | --- | --- | --- |
| name, driver, status, default, shared, queued jobs | `EnumPrinters` / `GetDefaultPrinter` | `lpstat` / `lpoptions` | ✅ |
| connection | `pPortName` | `device-uri` | ✅ |
| location | `pLocation` | `printer-location` | ⚠️ often blank |
| color, duplex | `DeviceCapabilities` | `ColorModel` / `Duplex` | ✅ |
| paper sizes, trays | `DC_PAPERNAMES` / `DC_BINNAMES` | `PageSize` / `InputSlot` | ✅ (names only; per-tray paper/level rarely available) |
| max DPI | `DC_ENUMRESOLUTIONS` | `printer-resolution` / `Resolution` | ⚠️ many drivers omit |
| toner/supply levels | ❌ (SNMP/WMI/IPP only) | `marker-levels` | ⚠️ macOS best-effort, Windows none |

This is the user-visible reason fields can be missing: **we depend on the system, and printers vary.** Every optional field is `Option`/`null` end-to-end (`Tray.paper`, `Tray.level`, `max_dpi`, `Supplies.*`, `default_tray`, `default_paper`); the UI hides the supply section, the resolution row, etc. when absent.

## Data shape

Rust `Printer` (snake_case, `src-tauri/src/ipc/printers.rs`) → UI `UiPrinter` (camelCase) is mapped in `src/lib/ipc.ts` (`toUiPrinter`). Key renames: `kind→type`, `is_default→isDefault`, `connection→conn`, `queued_jobs→queue`, `supplies→levels`.

`Printer` is the **only** printer shape on the wire. There is no `PrinterInfo` type — earlier drafts described `Printer` as "extending" a spooler-reliable `PrinterInfo` subset, and no such type exists in the code or in `src/ipc/bindings.ts`. Likewise `PrinterStatus` has exactly **four** variants — `ready` | `offline` | `error` | `busy` — not the seven-value set (`paused`, `out_of_paper`, `out_of_toner`, `jammed`, `needs_attention`) some documents still show; the per-platform status bitmaps are normalized down to those four. The full field list is in [ipc-and-cli.md §2.2](./ipc-and-cli.md#22-type-definitions).

## Testing

- **Rust unit** (`cups.rs` / `windows.rs`): pure parsers tested against verbatim real output (quote-aware kv, CMYK supplies, caps, status, jobs, URI prettifying).
- **Rust smoke**: `list_printers()` against the host's real spooler — asserts shape and normalized levels, tolerating **zero** printers (headless CI).
- **Windows-without-Windows**: the FFI is type-checked on macOS via `cargo check --target x86_64-pc-windows-msvc` (no linker needed); runtime is exercised on the `windows-latest` CI job.
- **Frontend smoke** (`PrintersScreen.test.tsx`): `@tauri-apps/api/mocks` `mockIPC` feeds fixtures → asserts rendering, the **degradation** case (mono printer hides supplies), the unavailable-outside-Tauri state, discovery, and the empty state.
- **CI lanes**: `windows-latest` (spooler path) + `macos-latest` (CUPS path + frontend). True WebDriver e2e (`tauri-driver`) is **not supported on macOS** by Apple's WKWebView, so it's reserved for the Windows/Linux CI as a follow-up.

## Add-printer & discovery — status

| Capability | macOS / Linux | Windows |
| --- | --- | --- |
| Enumerate installed | ✅ | ✅ |
| Set OS default | ✅ `lpoptions -d` | ✅ `SetDefaultPrinterW` |
| Print test page | ✅ branded PDF via `lp` | ✅ `printui.dll` |
| Open the OS printers pane | ✅ | ✅ |
| Network discovery (scan) | ✅ `ippfind` (mDNS/Bonjour) | ⛔ WSD/mDNS still deferred — returns an empty list |
| **Add a printer (mutates the spooler)** | ✅ **implemented** — `lpadmin` | ✅ **implemented** — PowerShell, IPP/IPPS only |

**Add is shipped, not deferred.** `ipc::printers::add_printer` is wired end to end; the earlier "the commit step is a documented follow-up" note is stale.

- **macOS / Linux** (`platform::cups::add_printer`): the display name is sanitized into a legal CUPS queue id — spaces, `/` and `#` become `_`, the result is capped at 127 characters and falls back to `"printer"` if it sanitizes to nothing — then de-duplicated against the existing queue names. `lpadmin` is invoked with that queue id, the URI, and the user's original text kept as the description. The spooler's own stderr is surfaced verbatim on failure.
- **Windows** (`platform::windows::add_printer`): only `ipp://` and `ipps://` URIs are accepted; anything else is refused up front with "must be added via system settings". A port is derived from the URI and a PowerShell 5.1 one-liner creates the port and the printer against the Microsoft IPP Class Driver, run with `CREATE_NO_WINDOW`. An access-denied outcome is detected **by HRESULT `0x80070005`** as well as by message text — because that text is localized on non-English Windows — and is turned into actionable guidance ("may require administrator rights; use Open printer settings") rather than a retry, since creating the port needs the same rights.

Elevation is never requested by the app itself; a machine that requires admin to add a port gets the guidance message instead.

## Open questions for v1+

- Cache driver capabilities (slow network drivers) — for how long, invalidated how?
- Supply-level polling cadence — on focus, on demand, or a background interval?
- Windows supply levels — adopt SNMP or IPP (for IPP-Everywhere printers) as a best-effort source?
- Windows network discovery — WSD, mDNS, or both?
- Should `add_printer` on Windows keep shelling out to PowerShell, or move to `AddPrinter`/`AddPortEx` through the `windows` crate?

These are tracked as design discussions, not commitments.
