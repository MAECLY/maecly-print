# License

> **TL;DR.** Pluryprint is licensed under the GNU Affero General Public License, version 3 only (`AGPL-3.0-only`). You can use it for anything (including commercial use), modify it, and redistribute it; if you distribute a modified version, or let people use a modified version over a network, you must offer them its source under the same license. Contributions come in under the [Contributor License Agreement](../../CLA.md): contributors keep their copyright and give the owner a license that lets him also offer Pluryprint on other terms. The name and the mark are not covered by the license ([`TRADEMARKS.md`](../../TRADEMARKS.md)).

This document is a plain-language explanation of what the AGPL-3.0 means for users and contributors of Pluryprint. **It is not legal advice.** The canonical, legally binding text is in `LICENSE` at the repository root, which is the Free Software Foundation's AGPL-3.0 text, verbatim. A copy ships inside every Pluryprint bundle, next to [`THIRD-PARTY-NOTICES.md`](../../THIRD-PARTY-NOTICES.md).

Pluryprint was licensed under GPL-3.0-only until 2026-10-09. Why it moved, and what that changed: [ADR-0010](../adr/0010-relicense-to-agpl-3.0.md).

## What is the AGPL-3.0?

The **GNU Affero General Public License version 3** is the GNU GPL version 3 with one added section (section 13). Like the GPL it guarantees four freedoms to users:

1. **Freedom to run** the software for any purpose.
2. **Freedom to study** how the software works (access to source code).
3. **Freedom to redistribute** copies.
4. **Freedom to modify and distribute** modified versions.

The "copyleft" part means: any modified version that you distribute must also be released under the AGPL-3.0, with source code made available. Section 13 adds that if you **modify** the program and let people **interact with it remotely over a network**, you must offer those people the source of your modified version too. For a desktop app that rarely matters; it closes the gap that lets a modified copy be offered as a service without its source.

## The additional term about names and marks

`TRADEMARKS.md` states one additional term under section 7(e) of the AGPL: the license grants no rights under trademark law to the Pluryprint name, wordmark and Abanico mark beyond reasonable and customary use in describing where the work came from, for example "a fork of Pluryprint". A fork is free to exist; it has to carry its own name. See [trademark.md](./trademark.md).

## What you (the user) can do

| Activity | Allowed? | Notes |
|---|---|---|
| Use Pluryprint at home | Yes | No restrictions on personal use. |
| Use Pluryprint at work | Yes | No restrictions on commercial use. |
| Install on as many machines as you want | Yes | No license keys, no per-seat fees. |
| Distribute Pluryprint to friends/colleagues | Yes | Provide them the source or a written offer to obtain source, and keep `LICENSE` and the notices with it. |
| Sell copies of Pluryprint | Yes | You can charge for distribution; you cannot prevent recipients from also distributing. |
| Run an unmodified Pluryprint as part of a service you offer | Yes | Section 13 applies only to modified versions. |
| Embed Pluryprint in a closed-source commercial product | **No** | A combined work must be AGPL-3.0 as a whole. See FAQ below, and the commercial-license question. |
| Modify Pluryprint and use the modification privately | Yes | Modifications you neither distribute nor offer over a network don't trigger any obligation. |
| Modify Pluryprint and distribute the modification | Yes, but you must release the source under the AGPL-3.0 | This is the copyleft requirement. |
| Modify Pluryprint and let others use it over a network | Yes, but you must offer those users the source | This is section 13. |
| Use the Pluryprint name and logo to advertise your modified version | **No** | See [trademark.md](./trademark.md) and the 7(e) term above. |

## What you (the contributor) commit to

When you contribute code, documentation, translations or any other content to Pluryprint, you do it under the [Contributor License Agreement](../../CLA.md) (version 1.0, 2026-10-09). In short:

1. **You keep the copyright in your contribution.** There is no copyright assignment.
2. **You give the project's owner a broad license to it**, including the right to publish it under other terms: the AGPL-3.0 today, and another open-source license or a commercial license later if he chooses to.
3. **You give a patent license** for patent claims you can license that your contribution necessarily infringes.
4. **You promise the contribution is yours to give**, or say whose it is and under which license.

You agree to it by opening a pull request; the pull request template asks you to confirm it. A `Signed-off-by:` line is welcome but is not what the project relies on.

Why a CLA rather than a DCO sign-off: with a DCO, every contributor keeps an equal say over the license, so the project can never be relicensed or offered commercially without asking every one of them again. The owner wants to be able to offer Pluryprint on other terms later while it stays open source, as with his other app, Versorium. The CLA makes that possible without taking contributors' copyright away, and the published versions stay available under the license they were published with.

## What "distribution" means

The AGPL's main trigger, like the GPL's, is **distribution** (called "conveying" in the formal text). Distribution happens when you give a copy of the software to someone else. It includes:

- Letting users download a binary from your server.
- Selling installer media.
- Bundling Pluryprint with your product.

Distribution does **not** include:

- Internal use within an organization (employees of the same company are not "distributing" to each other).
- Loading the software in your own browser or running it on your own machine.

The AGPL adds one more trigger: offering a **modified** version to users who interact with it remotely over a network (section 13).

## Source code availability

If you distribute Pluryprint (or a modified version), you must also make the corresponding source code available. Per section 6, you can do this by:

1. **Accompanying the binary with source code** on the same medium (USB, download bundle).
2. **Providing a written offer** valid for at least 3 years, allowing the recipient to request the source for the cost of distribution.
3. **Offering access to the source from a network server** at no further charge, for example a public repository.

For binaries we distribute (`maecly.com`, GitHub Releases), option 3 applies: the source is available at the GitHub repository, and our binary release notes link to it. Every bundle also carries `LICENSE`, `THIRD-PARTY-NOTICES.md` and the license texts in `LICENSES/`.

## License compatibility

The AGPL-3.0 can include code under these licenses:

- AGPL-3.0, and GPL-3.0 (section 13 of each license allows combining with the other)
- GPLv2-or-later, LGPLv2.1-or-later, LGPLv3 (through their "or later" clause or their own terms)
- MIT, ISC, Zlib, BSD 2-Clause / 3-Clause, Unicode-3.0
- Apache 2.0
- MPL 2.0, unless a file is marked "Incompatible With Secondary Licenses"

It is **incompatible** with these (their code cannot be included in Pluryprint):

- GPLv2-only (without the "or later" clause)
- CDDL (Common Development and Distribution License)
- EPL (Eclipse Public License)
- MPL 1.x, and MPL 2.0 code marked "Incompatible With Secondary Licenses"
- Any proprietary license

Every component we ship was checked against this list; see [third-party-licenses.md](./third-party-licenses.md) and [`THIRD-PARTY-NOTICES.md`](../../THIRD-PARTY-NOTICES.md).

## FAQ

### Can I use Pluryprint in my company?

Yes. The AGPL-3.0 puts no restriction on use. You can install Pluryprint on every employee's machine, no license fees, no audits.

### Can I modify Pluryprint and sell it?

Yes. The AGPL-3.0 permits selling. You can:

- Charge for the binary distribution.
- Charge for support contracts.
- Charge for custom modifications.

What you **cannot** do is prevent the people you sold it to from also redistributing it, or withhold the source of your modifications from them.

### Can I embed Pluryprint inside my closed-source product?

**Not under the AGPL.** If your product links to Pluryprint code, calls into Pluryprint internals as a library, or otherwise creates a combined work, the combined work must be AGPL-3.0 too.

You can:

- **Invoke Pluryprint as a separate process** via its CLI. The "subprocess invocation" pattern is generally not considered creating a combined work (this is the same logic that lets us use LibreOffice via subprocess without our code becoming MPL-licensed). See the FSF's position: ["mere aggregation"](https://www.gnu.org/licenses/gpl-faq.en.html#MereAggregation).
- **Distribute Pluryprint alongside your product** as a separate, optional component. As long as it's clear they're separate works, this is "mere aggregation" and doesn't trigger copyleft on your product.
- **Ask for a commercial license.** Because contributions come in under the CLA, the owner can license Pluryprint on other terms. Write to hola@maecly.com.

### Can I use parts of Pluryprint's source code in my closed-source project?

Not under the AGPL. Copying any non-trivial amount of Pluryprint source into a closed-source project would violate it. Your options are to rewrite the logic yourself (clean-room) or to ask for a commercial license (hola@maecly.com).

### Can I fork Pluryprint?

Yes, that's an explicit AGPL freedom. You must:

- Keep the AGPL-3.0 on your fork.
- Make your fork's source code available to the people you give it to, and to the people who use a modified version over a network.
- Mark modifications clearly so the original is not blamed for your changes.
- Give the fork its own name and mark (the 7(e) term above; see [trademark.md](./trademark.md)).

### Can I run Pluryprint on my company's servers and let users connect?

If you run it unmodified, yes, with no obligation. If you modify it and users interact with your modified version over a network, section 13 requires you to offer them its source.

### What if I find a bug or vulnerability?

For security issues, see `SECURITY.md` at the repo root. For other bugs, file an issue on GitHub.

The AGPL-3.0 disclaims warranty (see sections 15 and 16). The software is provided "as is". This is standard and doesn't reduce our motivation to fix bugs; it just means you can't sue us if something breaks.

### Does the AGPL affect documents I print with Pluryprint?

**No.** Output of the program is not derivative of the program. Documents you print remain entirely your own property under whatever license you put on them.

### Does the AGPL affect printer drivers I use with Pluryprint?

**No.** We invoke printer drivers via OS APIs (Win32, CUPS), not by linking. Printer drivers (often proprietary) remain under their own licenses.

### Can I use Pluryprint's icon or name in my project?

That's a trademark question, not a copyright question. See [trademark.md](./trademark.md). Short answer: linking to Pluryprint and saying "uses Pluryprint" is fine; using our name/logo to imply official endorsement is not.

## "Only", not "or later"

The SPDX id is `AGPL-3.0-only`, in `package.json`, `src-tauri/Cargo.toml` and `shell-ext-win/Cargo.toml`. A future AGPL version does not apply automatically; because contributions come in under the CLA, the owner can adopt one if it is ever published, without asking every contributor again.

## How to comply if you redistribute

A short checklist:

- [ ] Include the full AGPL-3.0 text (`LICENSE`), the third-party notices (`THIRD-PARTY-NOTICES.md`) and the texts in `LICENSES/`. The official bundles already do.
- [ ] Keep the copyright and license notices that are there.
- [ ] Make the corresponding source code available (link to repo, or bundle source with binary, or provide a written offer).
- [ ] If you modified it and offer it over a network, offer those users the source.
- [ ] Mark modifications clearly so the original isn't held responsible for your changes.
- [ ] Change the name and the mark if you distribute a modified version.
- [ ] Don't add restrictions beyond the AGPL-3.0 (you can't, e.g., add a "no commercial use" clause).

## Where to read the actual license

- The legally binding AGPL-3.0 text: [https://www.gnu.org/licenses/agpl-3.0.html](https://www.gnu.org/licenses/agpl-3.0.html).
- An FSF-curated FAQ: [https://www.gnu.org/licenses/gpl-faq.html](https://www.gnu.org/licenses/gpl-faq.html).
- A short overview: [https://choosealicense.com/licenses/agpl-3.0/](https://choosealicense.com/licenses/agpl-3.0/).
- Our `LICENSE` file at the repository root (the operative copy).

## Disclaimer

This document is a summary written by the project's owner, not a lawyer. For specific legal questions about how the AGPL-3.0 applies to your situation, consult a qualified attorney. The Software Freedom Conservancy ([sfconservancy.org](https://sfconservancy.org/)) and the Free Software Foundation ([fsf.org](https://www.fsf.org/)) maintain compliance resources we recommend.

## See also

- [`CLA.md`](../../CLA.md)
- [`TRADEMARKS.md`](../../TRADEMARKS.md)
- [third-party-licenses.md](./third-party-licenses.md)
- [trademark.md](./trademark.md)
- [privacy.md](./privacy.md)
- [../08-development/contributing.md](../08-development/contributing.md)
