# Target Users

> **TL;DR** — Pluryprint is built for people who print **batches** of mixed-format files from Windows Explorer as part of their day. This document profiles five representative users — an office administrator, a print-shop operator, a sysadmin, a software engineer, and a teaching assistant — and shows what current tooling forces them to do, and what Pluryprint replaces.

## How to read this document

The personas below are fictional, but they are composites of real workflows reported in Print Conductor reviews, AskWoody and Stack Overflow threads about the 15-file context-menu limit, and the user feedback collected on tools like Batch & Print Pro. Each persona answers four questions:

1. **What they do** — role and printing context.
2. **How often they print in batch** — to gauge the cost of friction.
3. **Their current workaround** — the manual or paid path they use today.
4. **What Pluryprint solves** — the specific tax we lift.

These personas drive feature prioritization. If a proposed feature does not visibly help at least one of them, it should be challenged.

---

## Persona 1: Carla, Office Administrator at a Mid-Sized Law Firm

- **Role.** Office administrator at a 40-attorney commercial litigation firm.
- **Tools she lives in.** Windows 11 on a managed laptop, Outlook, Word, Excel, Adobe Acrobat Reader, the firm's document management system (NetDocuments), and a Sharp MX-Series multifunction copier on the LAN.
- **Frequency of batch printing.** Three to seven times per day. Court filings, contract bundles, exhibit packs, and signed copies for client mailings.
- **Typical job shape.** 20–80 mixed PDFs and Word documents per batch. Often duplex, frequently with a separator sheet between cases.

### Current workaround

Carla has memorized that Windows hides "Print" past 15 files. So she:

1. Selects 15 files at a time, right-clicks, picks Print, and watches Acrobat Reader pop up briefly.
2. Repeats four or five times for an 80-file bundle.
3. Walks to the copier and re-orders the output by hand because the print order across batches is not guaranteed.
4. When Acrobat Reader silently skips files (a known intermittent bug — it printed 5 of 8 last Tuesday), she reprints the missing ones individually after walking back to her desk.

Her IT team priced **Print Conductor at $149 per seat** and declined the purchase because it "isn't standard software."

### What Pluryprint solves

- Selecting 80 files and right-clicking yields a single **Print all with Pluryprint** entry, no batching by 15.
- A consolidated job list with a stable, user-controlled print order eliminates the at-the-copier resort step.
- Built-in separator pages (text or blank) eliminate the manual divider routine.
- One-click duplex toggle persists per printer.
- It is free, so IT approval reduces to "is it on our software allowlist?" rather than a procurement cycle.

---

## Persona 2: Devon, Production Operator at a Small Print Shop

- **Role.** Operator at a 4-person print shop running two Konica Minolta production presses, a Roland wide-format, and a Zebra label printer.
- **Tools.** Windows 10 LTSC workstation tied to Fiery RIPs, Adobe Acrobat Pro, plus a folder of customer-supplied files arriving by email and Dropbox throughout the day.
- **Frequency of batch printing.** Continuously. Twenty to fifty discrete jobs per shift, each containing anywhere from 1 to 300 files.
- **Typical job shape.** Customer drops a ZIP. Devon extracts. He needs to send all PDFs to Press A, all PNGs to the Roland, and a Zebra ZPL file to the label printer — without launching Photoshop, Acrobat, and the Zebra utility separately.

### Current workaround

Devon's shop bought **Print Conductor** years ago and uses it daily. It is the workhorse. But:

- The license is single-user, tied to his workstation. When his colleague needs to run a job from her PC, she walks to his desk.
- The shop also evaluated **2Printer** for the CLI side ($149) but never bought a second seat.
- Print Conductor does not integrate into the Explorer context menu. Devon drags files into the Print Conductor window, which means leaving the shell and back.
- For one-off label runs, he uses a custom `.bat` that wraps `ShellExecute` with the `printto` verb. It works most days. It misorders files on the days it doesn't.

### What Pluryprint solves

- Free across all four shop workstations on day one — no license-shuffling.
- Right-click in Explorer, batch through, no detour to a separate launcher window.
- Per-printer profiles: one named "Press A Duplex Long Edge," one named "Roland Glossy," one named "Zebra Labels." Each is selectable directly from the context menu submenu.
- A CLI surface (`pluryprint --src ... --printer ... --profile ...`) so Devon can keep automating jobs from his existing shell scripts without paying for 2Printer.

---

## Persona 3: Priya, IT Systems Administrator at a Regional Hospital

- **Role.** Sysadmin responsible for clinical printing across three hospital sites (200+ printers, 1,400 endpoints).
- **Tools.** Windows Server 2022 print server, Active Directory, Group Policy, PowerShell, Microsoft Endpoint Configuration Manager, the EHR's print API.
- **Frequency of batch printing personally.** Low, but she **deploys** batch printing tools to nursing stations and admin desks daily.
- **Typical job shape.** Per-shift handoff packets, monthly compliance binders, training manuals — printed by clinical staff, not by Priya, but supported by Priya.

### Current workaround

Priya has tried to standardize on a tool. The institution's options:

- **Print Conductor** — at $59/user for 50+ seats, would still cost roughly $40,000 across the org. Not happening.
- **Batch & Print Pro** at $79.95–$109.95 — cheaper, but still a line item, and the licensing math gets messy across departments.
- **Custom PowerShell** wrapping `Out-Printer` and `ShellExecute` — what she actually uses. Brittle. Each new format breaks it.

Her staff routinely calls the help desk because they selected "too many files and the print option disappeared." Priya has written an internal KB article titled *"Why does the Print option vanish when I select more than 15 files?"*. It has been viewed 11,000 times.

### What Pluryprint solves

- Free GPLv3 — no procurement, no per-seat math.
- Silent install via MSI/MSIX, deployable through Configuration Manager.
- Group Policy-friendly: registry-based default printer overrides, a documented JSON config for org-wide defaults (`%PROGRAMDATA%\Pluryprint\policy.json`).
- A documented CLI for unattended use in nursing-station kiosks.
- An audit log (opt-in) to a configurable file path, satisfying the records-retention people without sending data anywhere off-network.
- The KB article gets shorter: "Right-click → Print all with Pluryprint."

---

## Persona 4: Sam, Senior Software Engineer at a Hardware Startup

- **Role.** Embedded firmware engineer at a 90-person robotics startup. Reviews PCB schematics, mechanical drawings (DWG/DXF/STEP), and design specs (PDFs, Markdown rendered to PDF).
- **Tools.** Windows 11 workstation, KiCad, Fusion 360, VS Code, Git, a Brother HL-L8360CDW color laser, an HP DesignJet plotter for E-size prints.
- **Frequency of batch printing.** A few times per week. Design review prep typically needs a packet of 30–60 mixed PDFs and DXFs.
- **Typical job shape.** Pre-meeting: print all the schematics in a directory, all PDF specs, and one DXF on the plotter — ideally with the right printer chosen per file type, automatically.

### Current workaround

Sam wrote a Python script that wraps `subprocess.run` with the SumatraPDF CLI for PDFs and a Fusion 360 batch macro for DXFs. It works on his machine. It crashes on his teammates' machines because they have different PATH layouts. He has rewritten it twice.

He has no interest in paying $149 for Print Conductor for what he considers a utility-belt problem. He has starred the repos for `clawPDF`, `Batch PDF Printer`, and `DarkStorm's BPH` over the years and bounced off all three because none of them fits.

### What Pluryprint solves

- A real, batteries-included tool he can install via `winget install Pluryprint` and recommend to his teammates without a setup ritual.
- File-type-based printer routing as a built-in feature: `*.pdf → Brother`, `*.dxf → DesignJet`, `*.docx → Brother`. Configured once, lives in his profile.
- A scriptable CLI he can call from `make print-review-packet`.
- Open source: when something breaks, he can read the code, file an issue with a stack trace, and submit a fix. That is the unlock that keeps him from rolling another Python script.

---

## Persona 5: Mr. Alvarez, High School Teacher

- **Role.** Tenth-grade biology teacher. Prints quizzes, lab handouts, and parent-conference packets weekly.
- **Tools.** A school-issued Windows 10 laptop, Office 365, a shared Ricoh copier in the staff workroom, occasional access to a personal Brother home printer.
- **Frequency of batch printing.** Twice a week. Monday morning he prints handouts for the week's labs.
- **Typical job shape.** 25–40 PDFs (worksheets) plus 4–8 DOCXs (lesson plans), ideally duplex, stapled, sorted by class period.

### Current workaround

He selects 15 files, right-clicks, prints. Then 15 more. Then the rest. If Adobe Reader is the default, the second batch sometimes fails silently. He has emailed the IT coordinator about it twice. Nothing changed.

He has never heard of Print Conductor and would not have a budget for it if he had. He uses what is on the laptop.

### What Pluryprint solves

- Removes the 15-file cap entirely. Monday morning printing becomes a single right-click.
- Format-agnostic: he does not need to know that Word and PDF go through different code paths. He selects everything and prints.
- Free, lightweight (~10 MB installer), no telemetry, no account. The IT coordinator approves it without a review meeting.
- The persistence of a default profile ("Ricoh Duplex Stapled") means he picks his printer once and never thinks about it again.

---

## Cross-Persona Themes

Looking across these five personas, a few patterns dominate:

| Theme | Personas affected | Pluryprint response |
| --- | --- | --- |
| The 15-file cap is a daily papercut | Carla, Devon, Sam, Alvarez | Remove it via a custom `IExplorerCommand` that processes the full `IShellItemArray`. |
| Commercial alternatives are excellent but cost-prohibitive at scale | Priya, Devon, Sam | Free GPLv3, no seat licensing. |
| Per-file-type routing is a real ask | Devon, Sam | Built-in routing rules in profiles. |
| Adobe Reader's silent partial failures are a trust problem | Carla, Alvarez | Use our own renderer (`pdfium`/`mupdf`) so we never depend on Reader behavior. |
| Sysadmins need silent, scriptable, telemetry-free deployment | Priya | MSI/MSIX, JSON policy file, opt-in logging only. |
| Power users want a CLI they can call from scripts and CI | Devon, Sam, Priya | First-class CLI parity with the GUI. |

## Anti-Personas

People Pluryprint is **not** built for, and should resist trying to serve:

- **Casual users who print one file every other month.** Windows' built-in Print verb already serves them. We do not want to add a context-menu entry to their machines for no reason; the installer offers an "advanced users only" toggle for context-menu integration.
- **Print shops needing job-tracking, billing, and customer accounts.** Use a print MIS (PrintSmith, EFI Pace, etc.).
- **Users who want PDFs generated from arbitrary print jobs.** Use clawPDF or Microsoft Print to PDF.
- **Mobile users.** Out of scope — there is no mobile version planned.

## Related Documents

- [Product Vision](./product-vision.md)
- [Value Proposition](./value-proposition.md)
- [Feature Matrix](../02-research/feature-matrix.md)
