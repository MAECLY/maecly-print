# Printers Management

> **TL;DR** — The **Printers** screen: a master-detail view of every printer MaeclyPrint knows about, showing live status, driver capabilities, tray/paper defaults, supply levels, and file-type routing — plus flows to discover and add printers, set the default, and reconcile a printer a profile points to that has gone missing.

This document is the source of truth for the **content and behavior** of the Printers screen. The underlying enumeration and capability querying live in the [system architecture doc](../04-architecture/system-architecture.md) (`ipc::printers`) and the [IPC & CLI contract](../04-architecture/ipc-and-cli.md); the data shapes live in the [data model](../04-architecture/data-model.md). UX and screen structure live here.

The screen is the GUI counterpart to the `list_printers` / `default_printer` IPC commands. It is one of the seven primary navigation destinations, sitting between **Queue** and **Profiles** in the sidebar.

## Screen layout

A master-detail split, mirroring the Profiles screen: a fixed list of configured printers on the left, the selected printer's detail on the right.

```
┌─ Printers ────────────────────  6 configured · 5 ready   [⟳ Rescan] [+ Add printer] ┐
├──────────────────┬──────────────────────────────────────────────────────────────────┤
│ ▣ HP LaserJet  ★ │  ▣  HP LaserJet Pro M404dn   [✓ READY] [★ default]               │
│   ● Ready·Laser  │      Laser · USB001 · queue 2     [Test page] [Set default] [Remove]│
│                  │  ┌ Default printer managed by your organization via policy.json ─┐ │
│ ▣ Sharp MX-3071  │  └────────────────────────────────────────────────────────────────┘ │
│   ● Ready·MFP    │  CONNECTION                 │ CAPABILITIES        queried from driver │
│                  │  Driver   HP UPD PCL6 …     │ [✓ Duplex] [✗ Mono only]               │
│ ▣ Brother …      │  Port     USB001            │ [✓ 5 paper sizes] [✓ 3 trays]          │
│   ● Ready·Laser  │  Location Front office      │ A4 Letter Legal A5 Executive           │
│ …                │  Shared   No                │ SUPPLY LEVELS                          │
│                  │  Max res  1200 dpi          │ Black ▓▓▓▓▓▓░░ 62%                      │
│ + Add a printer  │  DEFAULTS  [Tray 1 ▾][A4 ▾] │ INPUT TRAYS                            │
│                  │  ROUTE FILE TYPES           │ Tray 1  A4         ▓▓▓▓▓▓░             │
│                  │  [.docx ✕][.xlsx ✕][+ rule] │ Tray 2  Letterhead ▓▓▓░░░░             │
└──────────────────┴──────────────────────────────────────────────────────────────────┘
```

- **Top bar** — title `Printers`, subtitle `{total} configured · {ready} ready`, and two actions: **Rescan** (re-run discovery) and **Add printer** (opens the modal).
- **Master list (264 px)** — one row per printer: type icon, name, a star for the system default, and a status dot with `{status} · {type}` beneath. A dashed **Add a printer** button closes the list.
- **Detail pane** — header (icon, name, status badge, default badge, `type · connection · queue N`, action buttons) followed by a two-column body.

## Printer data model

Each printer is described by the following fields (`UiPrinter` in `src/types/domain.ts`), populated from `list_printers` plus the per-printer capability query (see [IPC mapping](#ipc-mapping)).

| Field        | Type                                  | Example                              | Source (M1)            |
| ------------ | ------------------------------------- | ------------------------------------ | ---------------------- |
| `id`         | string                                | `hp404`                              | spooler key — **the only value ever spooled to** |
| `name`       | string                                | `HP LaserJet Pro M404dn`             | `PrinterInfo.name` (display only; accepted as input and resolved to `id` by `ipc::printers::match_installed`) |
| `type`       | `laser\|mfp\|wide\|label\|virtual`    | `laser`                              | inferred from driver   |
| `color`      | boolean                               | `false`                              | driver capability      |
| `duplex`     | boolean                               | `true`                               | driver capability      |
| `status`     | `ready\|offline\|error\|busy`         | `ready`                              | spooler state          |
| `isDefault`  | boolean                               | `true`                               | `PrinterInfo.is_default`|
| `location`   | string                                | `Front office`                       | spooler metadata       |
| `conn`       | string                                | `USB001`, `10.0.0.41 (IPP)`          | `PrinterInfo.port`     |
| `driver`     | string                                | `HP Universal Print Driver PCL6 …`   | `PrinterInfo.driver`   |
| `shared`     | boolean                               | `false`                              | spooler metadata       |
| `queue`      | number                                | `2`                                  | live queue count       |
| `maxDpi`     | number                                | `1200`                               | driver capability      |
| `papers`     | string[]                              | `['A4','Letter',…]`                  | driver capability      |
| `trays`      | `{ name, paper, level }[]`            | `[{name:'Tray 1',paper:'A4',level:.82}]` | driver + status    |
| `levels`     | `{ cyan?,magenta?,yellow?,black? }`   | `{ black: 0.62 }`                    | SNMP / driver          |
| `defTray`    | string                                | `Tray 1`                             | profile/spooler default|
| `defPaper`   | string                                | `A4`                                 | profile/spooler default|

The IPC `PrinterInfo` ([data-model](../04-architecture/data-model.md)) is the **subset** that the spooler reliably returns (`name`, `port`, `driver`, `is_default`, `is_local`, `status`); the richer fields (`papers`, `trays`, `levels`, `maxDpi`) come from a follow-up capability query and may be absent for some drivers — the UI degrades gracefully (e.g. **Supply levels** is hidden when `levels` is empty).

### Statuses and types

| `status`  | Badge tone | Dot       | Meaning                          |
| --------- | ---------- | --------- | -------------------------------- |
| `ready`   | success    | done      | Online, accepting jobs           |
| `offline` | neutral    | cancelled | Not reachable / powered off, set to "Use Printer Offline", or a queue paused in the OS (CUPS `paused`, a paused Windows queue) — jobs wait, nothing prints |
| `error`   | error      | failed    | Jam, door open, out of paper/toner, needs a person, hard fault (any CUPS `*-error` state reason, or a stopped queue that is not merely paused) |
| `busy`    | accent     | printing  | Currently printing               |

| `type`    | Label         | Icon    |
| --------- | ------------- | ------- |
| `laser`   | Laser         | printer |
| `mfp`     | Multifunction | printer |
| `wide`    | Wide format   | image   |
| `label`   | Label         | page    |
| `virtual` | Virtual       | file    |

## Detail sections

- **Connection** — driver, port/address, location, shared state, max resolution.
- **Defaults for this printer** — default tray and default paper selects (options from `trays`/`papers`). Real jobs use them wherever the job leaves the value open (no tray chosen, paper **Printer default**); a tray or paper the job or its profile names explicitly always wins. Applied identically by the queue, the CLI and the honesty preview.
- **Route file types here** — extension pills (e.g. `.pdf`, `.jpg`) that default to this printer when no profile overrides. An **add rule** affordance appends a new extension. Cross-references the [context-menu routing](./context-menu-actions.md).
- **Capabilities** — pills for Duplex, Color/Monochrome, paper-size count, tray count, labelled *queried from driver*. When `duplex` is false, a warning says in plain words that **jobs asking for two-sided printing will pause on this printer** until they are switched to one-sided or sent elsewhere — consistent with the queue's paused-not-failed policy (the job pauses with `unsupported_setting`). The header meta line reads `Type · connection · N jobs in its queue`, dropping the connection when it only repeats the type (a virtual printer).
- **Supply levels** — per-supply bars (CMYK or just black), turning red below 20 %. Hidden when the device reports no supplies. A level the device reports as unknown (IPP `marker-levels` of `-1`/`-2`/`-3`) is omitted rather than drawn as an empty bar.
- **Input trays** — per-tray rows with loaded media and a fill bar (red below 20 %). Hidden when the driver reports no trays.

## Actions

| Action          | Where            | Behavior                                                                    |
| --------------- | ---------------- | --------------------------------------------------------------------------- |
| **Rescan**      | top bar          | Re-runs discovery; refreshes status/levels.                                 |
| **Add printer** | top bar / list   | Opens the add-printer modal (below).                                        |
| **Test page**   | detail header    | Sends a built-in test page to the selected printer.                         |
| **Set default** | detail header    | Marks the selected printer as the system default (disabled if already default, and overridden by org policy — see [edge states](#edge-states)). |
| **Remove**      | detail header    | Stops managing the printer in MaeclyPrint. **Hidden for `virtual` printers** (e.g. Microsoft Print to PDF). |

## Add-printer flows

A modal (`AddPrinterModal` in `PrintersScreen.tsx`) with three tabs, plus an **Open printer settings** button in the footer that always defers to the OS pane (`open_printer_settings`) for transports MaeclyPrint can't install itself.

1. **Scan network** — mDNS/Bonjour discovery (`discover_printers`; `ippfind` on macOS/Linux, an mDNS browse of `_ipp._tcp`/`_ipps._tcp` through the OS DNS-SD client on Windows; WSD-only devices are not listed). Results are keyed by the printer's advertised **UUID** (else its host without the port): a printer already installed — matched by UUID, Bonjour service name or host against each queue's `device-uri` — is not offered again, and the `ipp` and `ipps` adverts of one device collapse into a single row on `ipps`, named after the Bonjour service. Each result has an **Add** button that really installs the queue via `add_printer`, with a per-row inline error and toast on failure.
2. **By IP address** — manual entry: host/IP, protocol, port, optional queue path, display name. Host, port (an integer 1–65535) and name are validated client-side (`isValidHost` / `isValidPort` / `isValidDisplayName` / `buildPrinterUri` in `src/lib/printerAddress.ts`) before the URI is composed and handed to `add_printer`. The protocol list is **platform-dependent**: macOS/Linux offer IPP, IPPS, Raw (JetDirect 9100) and LPD/LPR; **Windows offers IPP/IPPS only** (a note steers raw/LPD/USB to *Open printer settings*).
3. **From system** — every queue the OS already reports, with a checkbox per printer. Unticking one **soft-disables** it (hides it from MaeclyPrint by adding its id to `prefs.disabled`); the underlying OS print queue is never modified. This is a MaeclyPrint-only preference, not an `lpadmin`/spooler mutation. The print path enforces it: a file-type route or app default naming a disabled printer is skipped as if unset, and a job that still resolves to one (an explicit printer in its profile or request, or a disabled OS default) is **paused** with `printer_disabled` and a message naming the printer — never sent. The CLI refuses it the same way (exit code `Printer`). Enabling the printer again and resuming sends the job there. Resuming while it is still disabled is refused (`resume_printer_disabled`); the job is never moved to another printer behind your back.

`add_printer` selects the install method per platform (see [Per-platform differences](#per-platform-differences)); on failure it surfaces the spooler's own diagnostic (e.g. `lpadmin`'s "Unable to connect to …") verbatim in the toast. It never touches MaeclyPrint preferences — only the OS spooler — and never silently overwrites an existing queue (the derived queue id is de-duplicated first, **ignoring case**, because CUPS resolves queue names case-insensitively). On CUPS the queue id is derived from the display name the way `lpadmin` requires: every character that is not printable ASCII, and each of space and `/ \ ? ' " # @`, becomes `_`, capped at 127 bytes; the display name itself is kept verbatim as the queue description.

## Edge states

- **Missing-printer reconciliation** — MaeclyPrint preferences can reference a printer id that the spooler no longer enumerates: a file-type route target, a per-printer tray/paper override, or the app default. When that happens, a warning banner names the missing id, explains that jobs using it are **paused, not failed** (mirroring the queue's failure semantics), and offers three actions:
  - **Update references (reassign)** — repoints the missing printer's routes at a chosen ready printer and remaps the app default to it. The missing printer's tray/paper overrides are **dropped, not carried over** — bin and media names are model-specific and don't transfer between devices.
  - **Forget** — removes every reference (routes, overrides, default) to the missing printer without reassigning.
  - **Keep paused** — dismisses the banner for this session only, without touching prefs; it reappears on the next **Rescan**/reload. If several printers are missing, the banner handles them one at a time and notes how many more remain.
- **Org-managed default** — when `policy.json` pins `default_printer`, a lock note appears and **Set default** is disabled with an explanatory tooltip. The effective default follows the precedence `policy.default_printer ?? prefs.default_printer ?? OS default`, and that is enforced in the **print path** too, not just on screen: the queue, the CLI and the command preview fold the policy into the app prefs (`ipc::printers::with_policy`) before resolving, so a job with no explicit printer and no file-type route goes to the policy printer. See the enterprise policy notes in the [security model](../04-architecture/security-model.md#43-enterprise-policy-policyjson).

## Per-platform differences

| Concern            | Windows                                  | macOS (M5+)                              |
| ------------------ | ---------------------------------------- | ---------------------------------------- |
| Enumeration source | Print Spooler (`EnumPrinters`)           | CUPS (`cupsGetDests`)                    |
| "From system" tab  | Spooler queues (XPS Writer, Fax, …)      | CUPS destinations                        |
| Virtual default    | Microsoft Print to PDF                   | Save as PDF                              |
| Discovery          | mDNS via `DnsServiceBrowse`, IPP/IPPS; WSD not yet | mDNS/Bonjour via `ippfind`               |
| Add printer        | `Add-Printer -IppURL` + **Microsoft IPP Class Driver**; **IPP/IPPS only** (raw/LPD/USB → OS pane); queue name de-duplicated; needs-admin surfaced as a friendly message | `lpadmin`: **IPP Everywhere** (`-m everywhere`) for ipp/ipps/dnssd, **raw** queue for socket/lpd |
| Set default        | `SetDefaultPrinterW`                     | `lpoptions -d` (writes `~/.cups/lpoptions`) |
| Test page          | branded MaeclyPrint PDF via SumatraPDF (paper/tray overrides applied; `printui.dll` fallback) | branded MaeclyPrint PDF via `lp`         |
| Supply levels      | Not exposed by Win32 (SNMP/WMI/IPP TODO) | `marker-levels` via `lpoptions`          |

macOS support follows [ADR-0007 (Windows first, Mac second)](../adr/0007-windows-first-mac-second.md); the screen is platform-neutral and reads whatever `list_printers` returns. Both add-printer paths avoid flashing a console window and surface the OS tool's own error text in the toast.

## IPC mapping

| UI surface              | IPC command                               | Notes                              |
| ----------------------- | ----------------------------------------- | ---------------------------------- |
| Master list + status + capabilities | `list_printers() => Printer[]`   | rich `Printer` (extends `PrinterInfo`) incl. color/duplex/papers/trays/supplies |
| Default star            | `default_printer() => string \| null`     | OS-default id                      |
| Rescan                  | re-invoke `list_printers`                 | —                                  |
| Scan network (add modal)| `discover_printers() => DiscoveredPrinter[]` | mDNS on both platforms (Windows: `DnsServiceBrowse`) |
| Add printer (network row / IP) | `add_printer(name, uri) => void`   | Installs a real spooler queue; spooler error surfaced verbatim |
| From-system checkboxes / tray-paper / routes | `get_printer_prefs()` / `set_printer_prefs(prefs)` | MaeclyPrint-only prefs: `disabled`, `overrides`, `routes`, `default_printer` |
| Set default             | `set_default_printer(name) => void`       | `lpoptions -d` / `SetDefaultPrinterW`; no elevation |
| Test page               | `print_test_page(name) => void`           | Branded PDF via `lp` / SumatraPDF (`printui.dll` fallback) |
| Open printer settings   | `open_printer_settings() => void`         | Defers to the OS Printers pane     |
| Org default lock        | `get_printer_policy() => PrinterPolicy`    | Reads `policy.json`; default when absent/malformed |

How the data is sourced per OS — and why fields may be absent — is documented in [Printer Enumeration](../04-architecture/printer-enumeration.md). See also [IPC & CLI](../04-architecture/ipc-and-cli.md) (§ printers).

## Implementation status

- **M0** — front-end only, mock-data faithful port (superseded).
- **M1 (current)** — **the full Printers surface is real, no mock data.** Rust: `src-tauri/src/ipc/printers.rs` + `src-tauri/src/platform/{cups,windows}.rs` + `src-tauri/src/settings/{mod,policy}.rs`. Frontend: the wrappers in `src/lib/ipc.ts`, consumed by `PrintersScreen.tsx` (loading / empty / unavailable states) and the Print Job printer dropdown. Implemented end-to-end:
  - Enumeration + capabilities (macOS supplies/papers/trays via CUPS; Windows via `DeviceCapabilities`) and OS default detection.
  - **Set default** (`lpoptions -d` / `SetDefaultPrinterW`) and **Test page** (branded PDF via `lp` / SumatraPDF on Windows, `printui.dll` only without the sidecar) — both are live spooler actions, not local stubs.
  - **Remove / Restore** as a MaeclyPrint-level soft-disable (OS queue untouched), **hidden for virtual printers**.
  - Preferences persistence (`get_printer_prefs` / `set_printer_prefs`): file-type routes, per-printer tray/paper overrides, soft-disabled set, app default.
  - **Add printer** by network discovery and by IP (`add_printer`: `lpadmin` IPP Everywhere/raw on macOS/Linux; Microsoft IPP Class Driver, IPP/IPPS-only on Windows), and **From system** spooler management.
  - **Missing-printer reconciliation** (reassign / forget / keep-paused) and **org-policy default lock** (`get_printer_policy` reading `policy.json`).
- **Follow-up** — Windows WSD discovery, Windows supply levels (SNMP/WMI/IPP), and profile-level reconciliation once profiles are wired.

## Open questions for v1+

- Should driver capability results (`papers`, `trays`, `levels`) be cached locally, and for how long, to avoid re-querying slow network drivers on every screen open?
- What cadence should supply-level polling use — on demand, on focus, or a background interval?
- Should file-type routing rules live per-printer (here) or be centralized with profiles to avoid conflicting defaults?
- How should reconciliation behave when *multiple* profiles reference the same missing printer — batch reassign?

These are tracked as design discussions, not commitments.
