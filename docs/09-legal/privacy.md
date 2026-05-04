# Privacy Policy

> **TL;DR.** MaeclyPrint does not collect telemetry, does not phone home, does not transmit your file contents, and does not have user accounts. The only network call we make by default is to check for updates against `updates.maecly.com`. An opt-in crash reporter exists for users who want to help us debug; it is off by default. Logs stay on your machine.

This document describes what data MaeclyPrint collects, transmits, stores, and shares — which is nearly nothing. It is current as of **2026-05-03** and applies to all installed versions of MaeclyPrint on Windows and macOS.

## Our principles

1. **Minimum necessary collection.** We do not collect data we do not need.
2. **No telemetry by default.** Anything we do collect is opt-in.
3. **File contents never leave your machine.** Period.
4. **Local-first.** Logs, profiles, and settings stay on your device.
5. **No user accounts.** We don't know who you are. We don't want to know.

## What we do **not** collect

The list of things we explicitly do not do:

- We do not have analytics. No Google Analytics, no Mixpanel, no Amplitude, no self-hosted equivalent.
- We do not track installs. We don't ping a server when you install or uninstall.
- We do not generate per-install IDs or fingerprints.
- We do not track which features you use.
- We do not track which printers you have.
- We do not track which file types you print.
- We do not transmit file contents anywhere, ever.
- We do not transmit file names.
- We do not transmit profile names or contents.
- We do not collect crash data unless you opt in (see below).
- We do not have user accounts. There is no sign-in, no email collection.
- We do not use cookies (the app is not a website; the webview only loads our local bundle).
- We do not embed third-party tracking scripts in the app.
- We do not transmit information to third-party advertisers, brokers, or analytics providers.
- We do not sell or rent any data because we do not collect any data to sell or rent.

## Network calls we **do** make (by default)

There is exactly one outbound network call MaeclyPrint makes by default:

### Update check

- **Endpoint:** `https://updates.maecly.com/maeclyprint/<channel>.json` where `<channel>` is `stable`, `beta`, or `nightly`.
- **Frequency:** every 24 hours after launch (and on demand if the user clicks "Check for updates").
- **Request payload:** none. It's a plain HTTPS GET.
- **Request headers:** standard HTTP headers including a `User-Agent` of the form `MaeclyPrint/<version> (<os>)` (e.g. `MaeclyPrint/1.2.3 (Windows NT 10.0)`). This is technically necessary for HTTP and does not contain personally identifying information.
- **What our server does with the request:** serves a JSON manifest. It logs the request in standard CDN/web-server access logs (timestamp, source IP, user agent, requested URL, HTTP status). We do not link these logs to anything else, do not analyze them for marketing, and do not retain them beyond 30 days.
- **What you can disable:** the periodic update check can be disabled in Settings → Updates. The on-demand button still works when needed.

If you also choose to install an update, that triggers an additional download from a URL like `https://updates.maecly.com/maeclyprint/<version>/setup.exe` — same logging properties.

### What `updates.maecly.com` is hosted on

The endpoint is served by Cloudflare R2 with Cloudflare CDN. Cloudflare is processing the requests on our behalf. Cloudflare's privacy policy applies to that infrastructure layer. We do not pass any user-identifying data to them; they see only the standard HTTP request data described above.

## Optional features that involve network calls

These are **off by default** and require explicit user consent.

### Opt-in crash reporter

- **What it does:** if MaeclyPrint crashes, an anonymized crash report (stack trace, OS version, MaeclyPrint version, last few log lines with file paths redacted) is sent to a self-hosted Sentry instance.
- **What is not collected:** file contents, file names, profile names, printer names, IP address (Sentry config strips IPs), user identifiers.
- **What is collected:** stack trace (function names + line numbers from our own code), OS version string, MaeclyPrint version, the last 50 log lines from our `tracing` output (which never include file contents but may include error messages — we redact file paths automatically before sending).
- **How to enable:** Settings → Privacy → Send anonymous crash reports. Off by default.
- **How to disable:** the same setting. Disabling stops all crash reporting immediately.
- **Where reports go:** a self-hosted Sentry instance under our control. Not Sentry's hosted SaaS.
- **Retention:** crash reports retained for 90 days, then deleted.

We may add other opt-in features in the future (e.g. usage stats, feature voting). They will all be:

- Off by default.
- Clearly disclosed in this document.
- Toggleable in Settings.
- Anonymous and aggregated where possible.

We will **never** flip a default from off to on without a major version bump and clear communication.

## Data we store on your machine

Locally, the app maintains:

| Data | Location (Windows) | Location (macOS) | Contents |
|---|---|---|---|
| Logs | `%LOCALAPPDATA%\MaeclyPrint\logs\` | `~/Library/Logs/MaeclyPrint/` | JSON-lines tracing output. Includes file paths of jobs you've printed (we use these for debugging). Rotates daily, retained 30 days. |
| Profiles | `%APPDATA%\MaeclyPrint\profiles\` | `~/Library/Application Support/MaeclyPrint/profiles/` | TOML files with your printer presets. |
| Settings | `%APPDATA%\MaeclyPrint\settings.toml` | `~/Library/Application Support/MaeclyPrint/settings.toml` | Channel choice, language, opt-in flags. |
| Update cache | `%LOCALAPPDATA%\MaeclyPrint\update-cache\` | `~/Library/Caches/MaeclyPrint/update/` | Temp files during update download. Auto-cleaned after install. |

This data stays on your device. We never transmit it. You can inspect or delete any of it manually at any time. Uninstalling the app prompts you to delete this data; the default is to preserve it (in case you reinstall).

## File contents

When MaeclyPrint prints a file:

- The file is read from disk by MaeclyPrint.
- It is passed to a print adapter (SumatraPDF, LibreOffice, etc.).
- The adapter renders it.
- The rendered output is sent to your printer (via the OS print spooler).

At no point is the file content transmitted over the network. The print spooler may keep the rendered output in a temp folder briefly (this is OS behavior, not ours), then deletes it after the job completes.

For network printers: the rendered output is sent **to the printer**, over your local network or via the printer's own network connection. This is necessary for printing to work. We do not also send it to anyone else.

## Children's privacy

MaeclyPrint is a desktop utility for printing files. We do not knowingly collect data from children under 13 because we do not knowingly collect data from anyone. If you are a parent and have concerns about your child using MaeclyPrint, the answer is: there is no data flow that reaches us under default settings.

## GDPR posture

The General Data Protection Regulation (GDPR) governs the processing of personal data of individuals in the European Union. MaeclyPrint:

- Does not process personal data under default settings, because we do not collect any.
- Does not require user accounts or identifiers.
- Does not transfer personal data outside the EU because we don't have any to transfer.
- Does not use personal data for profiling, automated decision-making, or marketing.

If you opt in to crash reporting, the data sent (stack trace, OS version, app version, redacted log excerpts) is generally not personal data under GDPR (no name, no email, no IP), but in some interpretations a stack trace plus IP address could constitute pseudonymous data. We strip IPs in our Sentry config to remove this ambiguity.

If you have a GDPR data subject request and you've opted in to crash reporting, contact the email in `SECURITY.md`. Because we store crash reports without user identifiers, the most we can do is delete crash reports matching certain criteria (e.g. all reports from a specific OS version + app version + IP range). We will respond to legitimate requests within 30 days.

## CCPA / state-level US privacy laws

The California Consumer Privacy Act (CCPA) and similar state laws (Virginia VCDPA, Colorado CPA, Connecticut CTDPA, Utah UCPA) grant rights regarding personal information. Same posture as GDPR: under default settings we collect no personal information, so the rights to access, delete, opt out of sale, etc. all have nothing to act on. We do not sell data; we don't have data to sell.

## Third-party services

We use these services in the operation of the project. None of them receive user data from MaeclyPrint runtime by default; they are part of our infrastructure or development pipeline.

| Service | What for | Data they see |
|---|---|---|
| GitHub | Source hosting, CI/CD, releases | Source code, CI logs (no user data). When you download a release from GitHub Releases, GitHub sees the standard HTTP request including IP. |
| Cloudflare R2 + CDN | Update manifest hosting, binary distribution | Standard HTTP request data when your app or browser fetches manifests/binaries. |
| Sentry (self-hosted, optional) | Crash reports (opt-in only) | Sanitized crash reports if you opt in. |
| Apple (during macOS use) | Notarization checks (Gatekeeper phones home on first launch) | Apple sees the app bundle hash + your IP. This is OS behavior, not ours. Stapled tickets allow offline verification once notarized. |
| Microsoft (during Windows use) | SmartScreen reputation lookup at install time | Microsoft sees the installer hash + your IP. This is OS behavior, not ours. |

We do **not** use Google Analytics, Facebook Pixel, ad networks, marketing platforms, or any other consumer-tracking services.

## Security

Although unrelated to privacy directly, security is the foundation of any privacy claim. Brief notes:

- We sign all binaries (see [code-signing.md](../07-build-and-release/code-signing.md)) so you can verify they came from us, unmodified.
- We sign updater payloads with Ed25519 (see [auto-update-flow.md](../07-build-and-release/auto-update-flow.md)) so updates can be verified offline.
- We disclose vulnerabilities responsibly per `SECURITY.md`.
- We do not bundle remote-code-execution tools or scripting engines that would let an update silently change app behavior beyond what the new version does.

## Logging on our infrastructure

For our website (`maecly.com`) and update endpoint (`updates.maecly.com`):

- Cloudflare keeps standard access logs (timestamp, source IP, user agent, requested URL, HTTP status, response time, country code derived from IP).
- These logs are retained for **30 days** and used only for security monitoring (DDoS detection, anomalous traffic patterns) and basic operational debugging.
- We do not analyze these logs for marketing, audience profiling, or product analytics.
- We do not share these logs with third parties (other than Cloudflare itself, which is the host).

## Children and accessibility

Privacy considerations for accessibility tools: if you use a screen reader, eye tracker, or other assistive technology with MaeclyPrint, that interaction is between you and the assistive technology. MaeclyPrint does not log, transmit, or store information about the use of assistive technology.

## Changes to this policy

If we change this policy in any way that adds collection or sharing, we will:

1. Announce the change in the release notes for the version that introduces it.
2. Bump to a new MAJOR version (per SemVer rules; this is a behavioral change to a documented promise).
3. Show a clear in-app notification on first launch of the new version asking the user to acknowledge.
4. Update this document with a "Changes" section dating the previous version.

We will **never** make a privacy-affecting change quietly. The principle: if you're surprised by something MaeclyPrint did with your data, we failed.

For changes that *reduce* collection (e.g. removing an opt-in feature, reducing log retention), we may make the change in a normal MINOR or PATCH release without ceremony.

## Contact

For privacy questions or concerns:

- Open an issue on GitHub tagged `privacy` for general questions.
- Email the security address in `SECURITY.md` for sensitive matters or formal requests (GDPR, CCPA, etc.).

## Version history of this policy

| Version | Date | Changes |
|---|---|---|
| 1.0 | 2026-05-03 | Initial policy. |

## See also

- [license.md](./license.md)
- [third-party-licenses.md](./third-party-licenses.md)
- [trademark.md](./trademark.md)
- [../07-build-and-release/auto-update-flow.md](../07-build-and-release/auto-update-flow.md)
