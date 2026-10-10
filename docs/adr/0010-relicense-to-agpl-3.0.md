# ADR-0010: Relicense to AGPL-3.0

## Status

Accepted. Supersedes [ADR-0002](./0002-license-gplv3.md).

## Date

2026-10-09

## Context

[ADR-0002](./0002-license-gplv3.md) chose the GPLv3 and a DCO sign-off instead of a contributor licence agreement. It said, about the CLA: "This means we cannot unilaterally relicense the project later — every contributor would have to re-sign", and it listed "Dual: GPLv3 + commercial" as something to adopt "later by adding a CLA" if there was demand. The manifests ended up declaring `GPL-3.0-only`, not the `GPL-3.0-or-later` the ADR described.

The owner, Miguel Angel Esparza Calero, who publishes as MAECLY, also publishes **Versorium**. On 2026-10-04 Versorium moved to a licensing model he wants for all his apps:

- the code under the **GNU Affero General Public License v3.0** (`AGPL-3.0-only`);
- contributions under a **Contributor License Agreement** (`CLA.md`): contributors keep their copyright and give the owner a licence that lets him publish the work under other terms, commercial ones included;
- the name and the mark kept out of the licence, with an **additional term under AGPL section 7(e)** stated in `TRADEMARKS.md`;
- the third-party notices in **`THIRD-PARTY-NOTICES.md`** at the repository root, shipped inside the application bundle.

Pluryprint should be handled the same way, so that one set of rules covers both products and one person can keep them straight.

Relicensing is possible now because every commit in this repository was written by the owner. There are no outside contributions whose authors would have to agree. The DCO sign-offs on those commits are the owner's own. After the first outside contribution under the old terms, the same change would need every such contributor's consent.

Two facts constrain the choice of licence:

- Pluryprint ships **SumatraPDF** (GPL-3.0) on Windows, and SumatraPDF contains **MuPDF**, which is AGPL-3.0. SumatraPDF runs as a separate process, so the two programs are an aggregate, and section 13 of both the GPL-3.0 and the AGPL-3.0 allows combining code under one with code under the other in any case. ADR-0002's argument that the GPLv3 was needed "to match SumatraPDF" holds equally for the AGPL-3.0.
- Every other shipped dependency (369 Rust crates on the four release targets, the bundled npm packages, JetBrains Mono, the WebView2 loader) is under a licence compatible with the AGPL-3.0. None is GPL-2.0-only. The inventory is in `THIRD-PARTY-NOTICES.md`.

## Decision

We relicense Pluryprint under the **GNU Affero General Public License, version 3 only**, and take contributions under a **Contributor License Agreement**, exactly as Versorium does.

- `LICENSE` is the FSF's AGPL-3.0 text, verbatim. The GPL-3.0 text moves to `LICENSES/GPL-3.0.txt`, where it remains as SumatraPDF's licence.
- `package.json`, `src-tauri/Cargo.toml` and `shell-ext-win/Cargo.toml` declare `AGPL-3.0-only`; so do the shell extension's version resource and the bundle's copyright line.
- `CLA.md` is Versorium's agreement with the product name changed, version 1.0, dated 2026-10-09, with the same owner and terms. `CONTRIBUTING.md` requires it and the pull request template asks contributors to confirm it.
- The DCO sign-off check (`.github/workflows/dco.yml`) is removed. Versorium has no CLA bot and no sign-off gate; the template checkbox is the record. A `Signed-off-by:` line stays welcome but is no longer required.
- `TRADEMARKS.md` states the 7(e) term in Versorium's words: the licence grants no rights under trademark law to the Pluryprint names and marks beyond reasonable and customary use in describing where the work came from.
- `THIRD-PARTY-NOTICES.md` is built the way Versorium's is (`cargo tree` per release target, the native code the crates compile or link, a production `vite build`), and ships with `LICENSE` and `LICENSES/` through `bundle.resources`. `src-tauri/tests/licence_bundle.rs` checks that.
- The About screen says AGPL-3.0 and links the licence, the CLA and the third-party notices.

## Alternatives Considered

| Option | Why not |
|---|---|
| **Keep GPL-3.0-only with the DCO** | Works, but the owner's two apps would follow different rules, and the DCO leaves every future contributor with a veto over any later change of terms. |
| **GPL-3.0-only with a CLA** | Gets the relicensing right, but not the network clause, and still differs from Versorium for no reason that applies to Pluryprint. |
| **GPL-3.0-or-later / AGPL-3.0-or-later** | Hands the choice of a future licence version to the FSF. With a CLA the owner can adopt a later version himself if one is published, so "only" loses nothing. |
| **A permissive licence (MIT, Apache-2.0)** | Lets a fork close the source; the owner wants modified versions to stay open. It would also make a commercial licence pointless. |
| **A CLA bot (CLA Assistant and similar)** | Versorium does not use one. A third-party service holding signatures adds an account, a dependency and a privacy question for a project that has no outside contributors yet. The template checkbox can be replaced by a bot later without changing the agreement. |

## Consequences

### Positive

- One licensing model across the owner's apps: the same licence, agreement, trademark term and notice file.
- The owner can offer Pluryprint under other terms, commercially included, without asking every contributor again.
- Modified versions offered over a network must share their source (AGPL section 13).
- The notices obligation is met in the bundle itself, not only in the repository: `LICENSE`, `THIRD-PARTY-NOTICES.md` and `LICENSES/` ship with every build.
- `docs/09-legal/third-party-licenses.md` was reconciled on the way: the WebView2 loader is statically linked (not a shipped DLL) under Microsoft's BSD-style licence, and `tauri-plugin-updater` is listed as used.

### Negative

- Some organisations refuse AGPL software by policy, even for a desktop tool whose network clause they would never trigger. A commercial licence is the answer for them.
- Contributors have to accept a CLA, which some people decline on principle; that may cost contributions.
- Pre-relicense builds and the source published before 2026-10-09 stay available under GPL-3.0-only. Anyone who took that code keeps those rights.

### Risks / Open Questions

- `THIRD-PARTY-NOTICES.md` is written by hand and is incomplete: it names the MIT-only crates but does not reproduce their notices. Generating it (for example with `cargo about`) is open work, as in Versorium.
- SumatraPDF's checksum pin is empty. Until a release names the exact SumatraPDF version it ships, its "corresponding source" is the pinned 3.5.2 tag.
- The CLA is written to be read by people, not drafted by a lawyer (it says so). A company contributing would need to sign separately, as `CLA.md` says.

## Related

- [ADR-0002](./0002-license-gplv3.md) — the GPLv3 decision this replaces.
- [ADR-0009](./0009-rename-to-pluryprint.md) — the rename, which added `TRADEMARKS.md`.
- [`docs/09-legal/license.md`](../09-legal/license.md), [`third-party-licenses.md`](../09-legal/third-party-licenses.md), [`trademark.md`](../09-legal/trademark.md).
