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

`EnumPrinters` (level 2 → `PRINTER_INFO_2W`) gives name, `pPortName`, `pDriverName`, `pLocation`, `Attributes`, `Status` bitmap, `cJobs` and the default `pDevMode`. `DeviceCapabilities` adds the rest; `GetDefaultPrinter` resolves the default.

**Errors are not "no printers".** The sizing call is expected to fail with `ERROR_INSUFFICIENT_BUFFER`; any other failure is returned. A stopped Print Spooler (`RPC_S_SERVER_UNAVAILABLE`) is reported as "The Windows Print Spooler service is not running", never as an empty list, and the pair of calls is retried when a printer appears between them. The buffer is 8-byte aligned because the records are read in place.

**Status** combines `Status` with `Attributes`:

| State | Source |
| --- | --- |
| Offline | `PRINTER_STATUS_OFFLINE`, `NOT_AVAILABLE`, `SERVER_UNKNOWN`, `PAUSED`, or the `PRINTER_ATTRIBUTE_WORK_OFFLINE` attribute ("Use Printer Offline", often with `Status` = 0) |
| Error | `ERROR`, `PAPER_JAM`, `PAPER_OUT`, `PAPER_PROBLEM`, `NO_TONER`, `DOOR_OPEN`, `USER_INTERVENTION` |
| Busy | `PRINTING`, `PROCESSING`, `BUSY`, or jobs waiting |
| Ready | anything else |

A paused queue reads Offline: it accepts jobs but prints none.

| Capability | `DeviceCapabilities` cap |
| --- | --- |
| Color | `DC_COLORDEVICE` |
| Duplex | `DC_DUPLEX` |
| Paper sizes | `DC_PAPERNAMES` (64-WCHAR entries), ids in `DC_PAPERS` |
| Trays | `DC_BINNAMES` (24-WCHAR entries), ids in `DC_BINS` |
| Max DPI | `DC_ENUMRESOLUTIONS` (LONG x,y pairs) |
| Default paper / tray | `pDevMode` `dmPaperSize` / `dmDefaultSource` (when set in `dmFields`), named through the id arrays above |

Driver paper names are **localized** (Spanish Windows lists Letter as "Carta"), so anything that has to understand a paper — the test page — maps the name to its `DMPAPER_*` id through `DC_PAPERS` instead of matching the string.

**`windows` crate gotchas (0.58), discovered via `cargo check --target x86_64-pc-windows-msvc`:** `DeviceCapabilitiesW` and the `DC_*` constants live under `Win32::Storage::Xps` (not `Gdi`); `PRINTER_INFO_2W` needs the `Win32_Security` **and** `Win32_Graphics_Gdi` features; `PRINTER_STATUS_*` / `PRINTER_ATTRIBUTE_*` are plain `u32`; `GetDefaultPrinterW` returns `BOOL`. **Toner/supply levels are not exposed by Win32** (they require SNMP/WMI/IPP) and are left empty. Network **discovery** browses mDNS/DNS-SD for `_ipp._tcp` and `_ipps._tcp` for 4 s with the pure-Rust `mdns-sd` crate (Windows has no `ippfind`). Records become rows in `platform::dnssd` (URI `ipp[s]://<mDNS host>:<port>/<rp>`, kind from the `ty` TXT key), and devices whose queue name, port host or address match an installed printer are hidden. WSD-only devices are not found; Windows' own *Add device* dialog lists them.

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
| Print test page | ✅ branded PDF via `lp` | ✅ branded PDF via SumatraPDF (`printui.dll` only without the sidecar) |
| Open the OS printers pane | ✅ | ✅ |
| Network discovery (scan) | ✅ `ippfind` (mDNS/Bonjour) | ✅ mDNS (`mdns-sd`); WSD-only devices not listed |
| Pause / resume / delete an OS queue | — not offered | — not offered |
| **Add a printer (mutates the spooler)** | ✅ **implemented** — `lpadmin` | ✅ **implemented** — PowerShell, IPP/IPPS only |

**OS queue management is out of scope.** MaeclyPrint never pauses, resumes or deletes a spooler queue. *Remove* on the Printers screen is a MaeclyPrint-only soft-disable (the printer's id goes into `prefs.disabled`, see [printers-management.md](../05-features/printers-management.md#actions)), and anything that changes the OS queue itself is left to Windows or CUPS through *Open printer settings*. An earlier Windows-only set of `pause_printer` / `resume_printer` / `remove_printer` / `open_printer_queue` functions was never wired to any command or screen and has been removed.

**Add is shipped, not deferred.** `ipc::printers::add_printer` is wired end to end; the earlier "the commit step is a documented follow-up" note is stale.

- **macOS / Linux** (`platform::cups::add_printer`): the display name is sanitized into a legal CUPS queue id — spaces, `/` and `#` become `_`, the result is capped at 127 characters and falls back to `"printer"` if it sanitizes to nothing — then de-duplicated against the existing queue names. `lpadmin` is invoked with that queue id, the URI, and the user's original text kept as the description. The spooler's own stderr is surfaced verbatim on failure.
- **Windows** (`platform::windows::add_printer`): only `ipp://` and `ipps://` URIs are accepted; anything else is refused up front with "must be added via system settings". The URI becomes the IPP Class Driver's URL (`ipp://h[:p]/path` → `http://h:p/path`, `ipps` → `https`, port 631 when absent) and a PowerShell 5.1 script runs `Add-Printer -Name <queue> -IppURL <url>`, with `CREATE_NO_WINDOW`. The script is a constant. The queue name and URL reach it only as environment variables (`$env:MAECLYPRINT_PRINTER_NAME`, `$env:MAECLYPRINT_PRINTER_URL`) and are never spliced into PowerShell source, because both can come from the network: a DNS-SD instance name and a TXT `rp` path. PowerShell also treats the Unicode quotes `‘ ’ ‚ ‛` as string delimiters, so escaping ASCII `'` was not enough. The URL host must also be a plain DNS name, an IPv4 address or a bracketed IPv6 address, and every path byte outside RFC 3986's path characters is percent-encoded. The queue name is de-duplicated case-insensitively against the installed queues first (`Name`, `Name_2`, …), as on CUPS. The script switches its output to UTF-8 so localized error text arrives intact, and reports two cases **by exit code** rather than by (localized) message text: access denied (HRESULT `0x80070005` or the `PermissionDenied` category) becomes "may require administrator rights; use Open printer settings", and a Windows build whose `Add-Printer` has no `-IppURL` becomes a pointer to the system dialog. An earlier version pre-created a port with `Add-PrinterPort -Name 'IPP://…'`, which creates a *Local Port* named after the URL (jobs never reached the device) inside a `catch {}` that hid every failure.

Elevation is never requested by the app itself; a machine that requires admin to add a port gets the guidance message instead.

## Open questions for v1+

- Cache driver capabilities (slow network drivers) — for how long, invalidated how?
- Supply-level polling cadence — on focus, on demand, or a background interval?
- Windows supply levels — adopt SNMP or IPP (for IPP-Everywhere printers) as a best-effort source?
- Windows network discovery — add WSD next to mDNS?
- Should `add_printer` on Windows keep shelling out to PowerShell, or move to `AddPrinter`/`AddPortEx` through the `windows` crate?

These are tracked as design discussions, not commitments.
