# Security Policy

## Supported versions

MaeclyPrint is in pre-alpha. Once the first stable release ships, this section will list which versions receive security fixes.

| Version | Status                                 |
| ------- | -------------------------------------- |
| 0.x     | Pre-alpha. No security guarantees yet. |

## Reporting a vulnerability

**Do not open a public GitHub issue for security reports.**

Use one of these private channels:

1. **GitHub Security Advisories** — preferred. Open a private advisory at:
   `https://github.com/<org>/maecly-print/security/advisories/new`
2. **Email** — `security@maecly.com` (PGP key fingerprint will be published once available).

Please include:

- A description of the issue and its impact.
- Steps to reproduce, ideally with a minimal proof of concept.
- The affected version, OS, and any relevant logs (with paths redacted).
- Whether you would like credit in the eventual disclosure.

## Response timeline

- **48 hours**: initial acknowledgement.
- **5 business days**: triage assessment with severity (CVSS-aligned).
- **30 days**: target for a fix in mainline. Some issues take longer; we will keep you informed.
- **90 days**: maximum embargo before coordinated public disclosure, unless mutually extended.

## What is in scope

- The Tauri application (Rust core + React frontend).
- The Windows shell extension DLL.
- The auto-updater pipeline (manifest signing, delta verification).
- The installer (Inno Setup script + Sparse MSIX).
- Any code shipped on `updates.maecly.com` infrastructure.

## What is out of scope

- Vulnerabilities in third-party dependencies that are already publicly disclosed (please report upstream).
- Self-XSS or social-engineering attacks that require the user to paste arbitrary code into a console.
- Issues that only affect unsupported pre-alpha builds older than 90 days.

## Acknowledgements

Reporters who follow this policy and act in good faith will be credited in the release notes (unless they opt out).
