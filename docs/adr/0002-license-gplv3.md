# ADR-0002: License — GPLv3

> Superseded on 2026-10-09 by [ADR-0010](./0010-relicense-to-agpl-3.0.md): Pluryprint is now AGPL-3.0-only, with contributions under a CLA.

## Status

Superseded by ADR-0010

## Date

2026-05-03

## Context

MaeclyPrint is a community-oriented open-source desktop utility. The project must pick a license that:

1. Lets users freely run, study, modify, and redistribute the software.
2. Protects against proprietary forks that take the work without contributing back.
3. Is compatible with the licenses of our dependencies — most importantly **SumatraPDF**, which is GPLv3.
4. Is widely understood by the OSS community and accepted by Linux distros, package repositories, and corporate users' legal departments.
5. Does not impose obligations that are inappropriate for a *desktop* application (we are not a SaaS).
6. Allows future commercial-but-open business models if we want them (paid hosted updates, paid support, optional pro features behind a separate licensed bundle, etc.) without re-licensing the core.

The major options are MIT, Apache 2.0, GPLv2, GPLv3, AGPLv3, MPL 2.0, and dual licensing.

We bundle SumatraPDF as the Windows printing back-end. SumatraPDF is licensed **GPLv3** (with permitted optional libraries used under their respective licenses). Our use of SumatraPDF as a separate process invoked via `ShellExecute` arguably stays at arm's length, but the more conservative reading is that distributing SumatraPDF *bundled in our installer* triggers GPL combination semantics. The cleanest path is for MaeclyPrint itself to also be GPLv3.

LibreOffice (Mozilla Public License 2.0), ImageMagick (custom Apache-like), Pandoc (GPLv2-or-later), and Chromium (BSD-like) are all compatible with GPLv3 when invoked as separate processes.

## Decision

We will release **all original MaeclyPrint code under the GNU General Public License, version 3 (GPLv3)**.

Concretely:

- The repository's `LICENSE` file is the GPLv3 text verbatim.
- Every source file carries an SPDX identifier header: `SPDX-License-Identifier: GPL-3.0-or-later`.
- The `Cargo.toml` and `package.json` declare `"GPL-3.0-or-later"`.
- The `README.md` and `About` dialog state the license clearly and link to the LICENSE file.
- Bundled third-party binaries (SumatraPDF, etc.) keep their own licenses, included in `THIRD-PARTY-NOTICES.md`.
- Contributors agree (via a CLA-free DCO sign-off in commits) that their contributions are GPL-3.0-or-later.

We choose **`GPL-3.0-or-later`** rather than `GPL-3.0-only` so that future GPLv4 (if it ever exists) can be adopted without re-licensing the entire codebase.

## Alternatives Considered

| Option            | Why rejected                                                                                              |
| ----------------- | --------------------------------------------------------------------------------------------------------- |
| **MIT**           | Permissive — anyone can take the code, close it, and ship it without contributions back. Also, packaging SumatraPDF (GPLv3) with MIT-licensed source raises real combination-license questions; the resulting distribution is GPL-effectively but the source labeling lies. We prefer honest labeling. |
| **Apache 2.0**    | Patent grants are appealing, but the "permissive forking" objection above still applies. Same combination concern with SumatraPDF. |
| **GPLv2**         | Compatible with many things but not with GPLv3-only code (which SumatraPDF is). Choosing GPLv2 would force us to drop SumatraPDF. |
| **AGPLv3**        | The "network use is distribution" trigger is designed for SaaS. MaeclyPrint is a desktop app; AGPL is overkill and creates unnecessary friction for corporate users who fear the AGPL clause. |
| **MPL 2.0**       | File-level copyleft. Compatible with proprietary code on a file-by-file basis. Doesn't solve the SumatraPDF GPLv3 combination question and weakens the "improvements come back" guarantee. |
| **Dual: GPLv3 + commercial** | Possible later — we'd write a CLA, accept code under both, and sell commercial relicensed copies. But this is something to introduce only if there is demand. We can adopt it later by adding a CLA path; we cannot easily *remove* a CLA later. Default is no CLA, plain GPLv3. |
| **No license** (source-available without an OSI license) | Not actually open source. Would block distros and discourage contributions. Not considered seriously. |

## Consequences

### Positive

- **Compatible with SumatraPDF (GPLv3) as a bundled dependency.** No license-incompatibility headache when we ship the installer.
- **Improvements come back to the project.** Forks that distribute binaries must release source. This is the central reason for picking copyleft.
- **Widely understood.** GPLv3 is recognized everywhere; corporate legal teams know how to evaluate it; distros know how to package it.
- **Honest signaling.** Anyone reading the license knows the intent: this is for the community, not for proprietary embedding.
- **Compatible with most of our other deps.** Pandoc (GPLv2+), LibreOffice (MPL 2.0), ImageMagick (Apache-like) all combine cleanly via the *separate process* boundary.

### Negative

- **Some corporate users will refuse to embed MaeclyPrint** in their proprietary internal pipelines, because they fear GPL obligations. For a *desktop utility* this is mostly fine — they can use the binary as a tool. They cannot, however, fork it into a closed-source product, which is the whole point.
- **Cannot directly link** GPLv3 code from non-GPLv3 code. We must maintain the separate-process boundary with SumatraPDF, LibreOffice, etc., which we already plan to do. This is not a regression; it's how we already designed the engine.
- **License compatibility analysis is required for every new dependency.** We add a `cargo deny` policy that whitelists compatible licenses (`MIT`, `Apache-2.0`, `BSD-*`, `MPL-2.0`, `Unicode-DFS-2016`, `GPL-3.0-or-later`) and rejects others by default.

### Risks / Open Questions

- **AGPL conversion.** If we ever build a hosted variant (e.g., a network printer broker), we may need to revisit whether AGPL is more appropriate for *that component*. We can do this without re-licensing the desktop app: the hosted component would be a separate sub-project.
- **Contributor License Agreement.** We deliberately do *not* require a CLA, only a DCO sign-off (`git commit -s`). This means we cannot unilaterally relicense the project later — every contributor would have to re-sign. That is a feature, not a bug: it commits us to GPLv3.
- **Apple App Store.** GPLv3 has long-standing compatibility issues with the App Store's distribution terms. We do not plan to distribute via the Mac App Store (we use direct download + Sparkle-style updates), so this is not a constraint for us.

## Related

- ADR-0001 (stack) — informs the licenses of bundled toolchain pieces (Tauri MIT/Apache, Rust crates).
- [`../09-legal/`](../09-legal/) — the full legal notes, third-party attributions, and CLA decisions.
