# Windows Context Menu Limitations (and How MaeclyPrint Sidesteps Them)

> **TL;DR** — Windows imposes several structural and behavioral limits on the right-click "Print" experience: a hard 15-file cap on the legacy verb, multiple `ShellExecute` quirks with the `printto` verb, application-specific bugs with Adobe Reader and Acrobat DC, and a Windows 11 modern-menu requirement that shell extensions ship with package identity (MSIX or Sparse). MaeclyPrint avoids each of these by registering as a Sparse MSIX with an `IExplorerCommand` handler that consumes the full `IShellItemArray`, owning its own format pipeline, and never delegating the print job to a third-party application like Adobe.

## Why This Matters for the Project

The single biggest claim MaeclyPrint makes is that **right-click batch printing should just work**. To make that claim credibly, we have to be precise about what Windows actually does, what it intentionally does not do, and where the application-layer bugs live. This document is the reference: when a future contributor asks "why are we doing it that way?" the answer should be in here.

## The 15-File Cap

### The behavior

Selecting more than **15 files** in Windows Explorer causes the operating system to **silently remove** the "Print" verb from the right-click context menu. The selection itself is not blocked; the menu entry simply disappears. Other verbs (Open, Cut, Copy, Delete, Send to) remain available. The same hidden cap applies to several other "bulk action" verbs (Open, Edit, Print, Encrypt) according to long-standing reports from sysadmins and Microsoft support staff.

### Why it exists

The cap is **deliberate UX policy**, not a technical limit. It is documented anecdotally by Microsoft support across forum responses dating back to the Windows Vista era and has been the subject of dozens of "please remove this limit" requests on the Microsoft Answers community over the past 15 years. The stated rationale, paraphrased from official responses:

> The limit exists to prevent users from accidentally launching dozens of applications at once when they shift-click a folder of files.

There is **no command-line length problem** behind the cap. `cmd.exe` accepts up to **8,191 characters** per invocation, which is enough room for several hundred typical file paths even before quoting. The implementations of `ShellExecuteEx`, the `printto` verb, and the print spooler can all handle hundreds of jobs in sequence; Windows simply chooses not to expose the verb past 15 selections.

### Where the cap lives in the stack

The cap is enforced by the **legacy context menu host** in Explorer when it composes the menu for a multi-selection. It is not enforced inside `ShellExecuteEx` (which will happily print a single file when invoked programmatically), and it is not enforced by the print spooler. This matters because it means an application that registers its *own* shell extension can choose not to enforce the cap.

### Implications

- A user who needs to print 16+ files via right-click in stock Windows is stuck.
- Programmatically iterating with `ShellExecuteEx` works but inherits all the per-application quirks below.
- A custom shell extension that consumes the **full `IShellItemArray`** (which has no documented item-count limit) bypasses the cap entirely.

This last bullet is the foundation of MaeclyPrint's shell integration design.

## The `printto` Verb

### What it does

`printto` is a registered shell verb that tells Windows: "open the application registered for this file type, hand it the file, and direct it to print the file to a specified printer."

The verb's command line, registered in HKEY_CLASSES_ROOT under `\shell\printto\command` for each file extension, supports four substitution parameters:

| Parameter | Meaning |
| --- | --- |
| `%1` | The full path of the file to print. |
| `%2` | The name of the target printer. |
| `%3` | The driver name associated with the printer. |
| `%4` | The port name associated with the printer (e.g., `LPT1`, `USB001`, an IP address). |

A typical registration for Microsoft Word looks something like:

```text
"C:\Program Files\Microsoft Office\root\Office16\WINWORD.EXE" /pt "%1" "%2" "%3" "%4"
```

A `ShellExecuteEx` call can invoke this verb against an arbitrary file:

```cpp
SHELLEXECUTEINFO sei = { sizeof(sei) };
sei.lpVerb = L"printto";
sei.lpFile = L"C:\\Invoices\\report.docx";
sei.lpParameters = L"\"HP M404 Office\"";
sei.fMask = SEE_MASK_NOCLOSEPROCESS;
ShellExecuteEx(&sei);
```

### Quirks

The verb works, but with several behaviors that bite real-world batch printers:

1. **The default printer is not changed automatically.** If the registered application ignores the `printto` parameter set and falls back to "default printer" behavior — and many do — the file goes to whatever printer is currently the user's default. The robust workaround is to *temporarily change* the system default printer before invocation and restore it after, which is a race condition waiting to happen on a multi-user system.
2. **Print order is unpredictable when multiple applications are launched.** A 30-file mixed batch (PDF + DOCX + JPG) launches the PDF reader, Word, and the image viewer in parallel. Each spools jobs to the printer asynchronously. The order in which spool entries land on the printer queue is not guaranteed to match the order of `ShellExecuteEx` calls.
3. **Format support is uneven.** Many applications register `printto` only nominally and silently ignore some parameters. Image viewers in particular often print to the user's default printer regardless of `%2`.
4. **DDE (Dynamic Data Exchange) issues.** Older registrations — especially for Adobe Reader — use DDE to talk to a long-running application instance instead of launching a fresh process per file. Reinstalling Adobe Reader can break the DDE Server registration, after which `printto` invocations against PDFs hang indefinitely until the DDE handshake times out (typically 30 seconds), then fail.
5. **The application stays open.** `printto` does not require the helper application to exit when the print job completes. Word, Excel, and Adobe all remain in the foreground, requiring the calling code to track and close the spawned processes — itself a fragile dance, since some of those applications coalesce multiple invocations into a single process.

The cumulative result is that `ShellExecute(printto, ...)`-based batch print scripts are reliable for single files, mostly reliable for small batches of one format, and increasingly fragile as the batch grows or mixes formats.

## Application-Specific Bugs

### Adobe Acrobat DC: "Print" disappears entirely

When Adobe Acrobat DC is the registered default handler for PDFs, the **"Print" verb does not appear** in the right-click menu for PDFs at all. Adobe's installer registers Acrobat DC under a verb namespace that does not include a usable `print` verb, so Explorer has nothing to expose. This is a well-known, multi-year issue with Acrobat DC; Adobe has not shipped a fix.

The standard "fixes" users circulate are all unsatisfying:

- Set Adobe Reader (not Acrobat DC) as the default PDF handler — defeats the purpose if the user paid for Acrobat.
- Install a separate lightweight PDF reader (SumatraPDF, Foxit) and set it as the default — adds a second application for one verb.
- Open Acrobat DC and use File → Print on each file — what users were trying to avoid.

### Adobe Reader: silent partial-batch failures

When Adobe Reader is the default PDF handler and a user selects multiple PDFs and uses the right-click "Print" verb (within the 15-file cap), Reader will frequently print only a **subset of the selected files** — for example, 5 of 8 — without surfacing any error message. The pattern is intermittent: the same selection re-attempted minutes later may succeed.

The community-documented partial workaround is to **disable "Protected Mode at startup"** in Reader's preferences (`Edit → Preferences → Security (Enhanced) → uncheck Enable Protected Mode at startup`). This raises the hit rate of complete batches but is itself a security regression — Protected Mode is a sandbox that mitigates exploits in malicious PDFs.

There is no clean fix from the user's side. The Reader behavior is documented in countless forum threads dating back to Reader X (released 2010).

### Microsoft Word: DDE timeouts and per-process serialization

Word's `printto` registration drives a single instance of `WINWORD.EXE` via DDE. Submitting many files in quick succession can exhaust the DDE handshake or cause Word to coalesce several jobs into one document, with unpredictable results.

Word also exhibits dialog-popping behavior when documents have unresolved data sources (mail merge), missing fonts, or modified-but-unsaved global templates. Each dialog blocks the print operation until dismissed.

### Microsoft Excel: print-area gotchas

Excel's `printto` invocation respects the workbook's *currently saved* print area. Workbooks where the user expected to print "the whole sheet" but the print area was set to a small range will silently print just that range. There is no per-invocation override via `printto`.

### Implication

Any batch printer that delegates to Word, Excel, or Adobe via `printto` inherits the bugs and quirks of each of those applications. The implication for MaeclyPrint is unambiguous: **own the rendering pipeline**. We render PDFs with `pdfium`/`mupdf`. We render Office files with headless LibreOffice (or Office Interop only when explicitly available and selected by the user). We render images, HTML, and text with Rust crates we control. We never invoke `ShellExecute(printto, ...)` against a third-party application as a primary code path.

## Windows 11 Modern Context Menu: Package Identity Required

### The new menu

Windows 11 introduced a redesigned context menu — the "modern" menu — that surfaces a curated set of common verbs at the top, with a **Show more options** entry that opens the legacy menu for everything else. Shell extensions registered the old way (COM CLSID under `HKEY_CLASSES_ROOT\*\shellex\ContextMenuHandlers`) appear only in the legacy ("Show more options") view by default.

### The package identity requirement

To appear in the **modern** menu (top level, no "Show more options" detour), a shell extension must:

1. Implement `IExplorerCommand` (and optionally `IExplorerCommandState` for dynamic enable/disable).
2. Be registered through an **MSIX package** (a regular MSIX or a **Sparse MSIX** that retains an external EXE installer) so that the extension has **package or application identity** in the modern Windows app model.
3. Declare the extension via an `<uap3:Extension Category="windows.fileExplorerContextMenus">` entry in the package manifest.

A shell extension installed by a classic MSI without package identity can still be registered, but **it will not appear in the modern menu** — only under "Show more options."

This is a structural change. No amount of registry hacking around the legacy CLSIDs will surface an entry in the modern menu without package identity.

### Sparse MSIX as the practical choice

Full MSIX packaging means accepting the MSIX execution model: the app runs in a container, file system writes are virtualized, and certain Win32 behaviors (registry writes, service installation, kernel driver loading) are restricted. For a desktop print utility this is more constraint than we need.

**Sparse MSIX** is the compromise. The application is installed by a traditional MSI/EXE installer and runs as a regular Win32 process. A separate, signed `.msix` file is registered alongside the install to grant the application **package identity** without forcing it into the MSIX runtime container. The Sparse package's manifest declares the `IExplorerCommand` extension, and Windows treats the entry as a first-class modern menu participant.

This is the path MaeclyPrint will take.

### Per-user vs per-machine registration

Shell extensions registered under `HKEY_CURRENT_USER` are **silently ignored** by Explorer for security reasons (a malicious application running as the user could otherwise inject menu entries). They must be registered under `HKEY_LOCAL_MACHINE` to be activated.

For Sparse MSIX, the equivalent constraint surfaces in the package signing and installation scope: per-machine package registration requires admin elevation; per-user package registration is permitted but Explorer's enforcement of the per-machine rule for shell extensions still applies, so the modern-menu entry only reliably appears for per-machine installs.

The MaeclyPrint installer will offer both modes:

- **Per-machine install (default).** Requires elevation. Modern context menu entry available to all users.
- **Per-user install (`/CurrentUser` switch).** No elevation. App functions normally; modern context menu entry not registered (the user can still launch MaeclyPrint and add files manually).

## Putting It Together: How MaeclyPrint Sidesteps Each Limit

| Limitation | MaeclyPrint approach |
| --- | --- |
| 15-file cap on legacy verb | Register our own `IExplorerCommand` handler. Consume the full `IShellItemArray`. Process unlimited selections. |
| Modern Win11 menu requires package identity | Ship as **Sparse MSIX** alongside the MSI installer. Declare `windows.fileExplorerContextMenus` extension in the package manifest. |
| Legacy menu still needed for Win10 and "Show more options" | Register a parallel `IShellExtInit` / `IContextMenu` handler that exposes the same verb. Same backend command processing on both code paths. |
| `printto` quirks (default printer race, ordering, DDE) | Do not use `printto`. Render every supported format with libraries we own. Submit jobs to the spooler with our own DEVMODE. |
| Adobe Acrobat DC hides "Print" verb | Irrelevant to us — we never depend on Adobe being the default handler. We open the PDF with `pdfium`/`mupdf` ourselves. |
| Adobe Reader silent partial failures | Same — we never invoke Reader. The user can keep Reader as their default viewer; MaeclyPrint does not ride on top of it. |
| Per-user shell extension registrations are ignored | Default installer is per-machine and elevates. A per-user mode skips shell registration but still gives a working GUI/CLI. |
| Print order races across multiple application launches | A single `IExplorerCommand` invocation hands the entire selection to a single MaeclyPrint process. We render and submit jobs serially (or, optionally, as a single spool job — the "Single Print Job Mode" idea borrowed from 2Printer). |
| Helper applications (Word/Excel/Adobe) staying open after print | We do not launch helper applications. |

## Pseudocode of the Shell Integration Path

The integration path the user actually exercises looks like this in shorthand. The full implementation lives in the `crates/shell-ext/` directory of the source repository (see `../04-architecture/`).

```text
1. User selects N files in Explorer (N may be > 15).
2. User right-clicks. Explorer composes the modern context menu.
3. Explorer queries our IExplorerCommand for icon, label ("Print all with MaeclyPrint"), and enabled state.
4. We answer "enabled" if at least one file extension in the selection is in our supported list.
5. User clicks the entry.
6. Explorer calls IExplorerCommand::Invoke with an IShellItemArray containing all N items.
7. We enumerate the IShellItemArray (no item-count limit).
8. We serialize the file paths into a job manifest.
9. We launch (or send IPC to a running) maeclyprint.exe with the manifest.
10. The Tauri window opens with the file list pre-populated.
11. The user confirms options and clicks Print.
12. The Rust core renders each file via the appropriate backend (pdfium, lo-headless, image-rs, etc.).
13. The core submits print jobs to the spooler via OpenPrinter / StartDocPrinter / WritePrinter / EndDocPrinter.
14. Progress and results are reflected in the UI.
15. On completion, the window closes (or stays open showing the results log, depending on user preference).
```

No `ShellExecute(printto, ...)`. No third-party application launches. No 15-file cap. Modern menu compatible. Legacy menu compatible. Per-user or per-machine. Aware of every limitation enumerated above and routed around each.

## References (Conceptual, Not Linked)

- Microsoft documentation on `IExplorerCommand`, `IShellItemArray`, and modern Windows 11 context menu extensions.
- Microsoft documentation on Sparse Packages and package identity.
- Microsoft documentation on `ShellExecuteEx` and registered shell verbs (`open`, `print`, `printto`).
- The long-standing Microsoft Answers and TechNet threads on the 15-file cap (none of which Microsoft has acted on; the cap is "by design").
- Adobe community forum discussions of Reader's silent partial-batch failures and Acrobat DC's missing context-menu Print verb.
- The fCoder, Traction Software, and Peter Lerup product documentation, which collectively describe how each commercial competitor avoided shell integration entirely (a tacit admission of how thorny it is).

## Related Documents

- [Competitor Analysis](./competitor-analysis.md)
- [Feature Matrix](./feature-matrix.md)
- [Market Gap](./market-gap.md)
- [Product Vision](../01-vision/product-vision.md)
- [Value Proposition](../01-vision/value-proposition.md)
