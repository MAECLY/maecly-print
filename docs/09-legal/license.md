# License

> **TL;DR.** Pluryprint is licensed under GPLv3. You can use it for anything (including commercial), modify it, and redistribute it, but if you distribute a derivative work, you must release the source code under GPLv3 as well. We do not require copyright assignment; contributors retain their copyright and license to the project via DCO sign-off.

This document is a plain-language explanation of what GPLv3 means for users and contributors of Pluryprint. **It is not legal advice.** The canonical, legally binding text is in `LICENSE` at the repository root, which is the official Free Software Foundation GPL-3.0-or-later text.

## What is GPLv3?

The **GNU General Public License version 3** is the most widely used copyleft free-software license. It guarantees four freedoms to users:

1. **Freedom to run** the software for any purpose.
2. **Freedom to study** how the software works (access to source code).
3. **Freedom to redistribute** copies.
4. **Freedom to modify and distribute** modified versions.

The "copyleft" part means: any derivative work that you distribute must also be released under GPLv3, with source code made available. This ensures the freedoms propagate downstream and prevents proprietary forks.

Why we chose it: see [adr/](../adr/) for the original decision record.

## What you (the user) can do

| Activity | Allowed? | Notes |
|---|---|---|
| Use Pluryprint at home | Yes | No restrictions on personal use. |
| Use Pluryprint at work | Yes | No restrictions on commercial use. |
| Install on as many machines as you want | Yes | No license keys, no per-seat fees. |
| Distribute Pluryprint to friends/colleagues | Yes | Provide them the source or a written offer to obtain source. |
| Sell copies of Pluryprint | Yes | You can charge for distribution; you cannot prevent recipients from also distributing. |
| Use Pluryprint inside a service you offer | Yes | Internal use is not "distribution" under GPLv3, so the source-availability requirement doesn't trigger. |
| Embed Pluryprint in a closed-source commercial product | **No** | GPLv3 is "viral" with respect to derivative works. See FAQ below. |
| Modify Pluryprint and use the modification privately | Yes | Modifications you don't distribute don't trigger any obligation. |
| Modify Pluryprint and distribute the modification | Yes, but you must release the source under GPLv3 | This is the copyleft requirement. |
| Use the Pluryprint name and logo to advertise your modified version | **No** without permission | See [trademark.md](./trademark.md). |

## What you (the contributor) commit to

When you contribute code, documentation, or any other content to Pluryprint:

1. **You retain copyright on your contribution.** We do not require copyright assignment.
2. **You license your contribution to the project under GPLv3** (specifically, GPLv3-or-later, matching the project license).
3. **You sign off the [Developer Certificate of Origin](https://developercertificate.org/)** by adding `Signed-off-by: Your Name <your@email>` to every commit (`git commit -s` does this automatically). This is your assertion that:
   - You wrote the contribution, or
   - The contribution is based on a previous work that was contributed under an appropriate open source license, or
   - You received the contribution under such a license.

We chose DCO over a CLA (Contributor License Agreement) because:

- DCO is a per-commit attestation, easy to audit.
- It does not require contributors to assign copyright.
- It does not require legal review of every contribution.
- It works for community contributions at scale.

## What "distribution" means

GPLv3's key trigger is **distribution** (called "conveying" in the formal text). Distribution happens when you give a copy of the software to someone else. It includes:

- Letting users download a binary from your server.
- Selling installer DVDs.
- Bundling Pluryprint with your product.

Distribution does **not** include:

- Internal use within an organization (employees of the same company are not "distributing" to each other).
- SaaS-like deployments where users interact with the software but never get a copy (this is the "ASP loophole" GPLv3 deliberately preserves; AGPL closes it, but we use GPLv3, not AGPL).
- Loading the software in your own browser or running it on your own machine.

## Source code availability

If you distribute Pluryprint (or a modified version), you must also make the corresponding source code available. Per GPLv3 section 6, you can do this by:

1. **Accompanying the binary with source code** on the same medium (CD, USB, download bundle).
2. **Providing a written offer** valid for at least 3 years, allowing the recipient to request the source for the cost of distribution.
3. **Linking to a public repository** containing the source.

For binaries we distribute (`maecly.com`, GitHub Releases), option 3 applies: the source is available at the GitHub repository, and our binary release notes link to it.

## License compatibility

GPLv3 is compatible with these licenses (you can include code under them in a GPLv3 project):

- GPLv3 (obviously)
- LGPLv3
- MIT
- BSD 2-Clause / 3-Clause
- Apache 2.0 (with caveats; see GPLv3 section 7 about additional permissions)
- ISC
- Zlib

GPLv3 is **incompatible** with these (cannot include their code in a GPLv3 project):

- GPLv2-only (without "or later" clause). GPLv3 is also v2-incompatible from the other direction; the FSF wrote GPLv3 specifically to address Tivoization that GPLv2 didn't cover.
- CDDL (Common Development and Distribution License)
- EPL (Eclipse Public License)
- MPL 1.x (MPL 2.0 is compatible with GPLv3 if explicitly licensed as MPL-2.0-or-GPL)
- Any proprietary license

For the third-party components we ship, see [third-party-licenses.md](./third-party-licenses.md) for compatibility analysis.

## FAQ

### Can I use Pluryprint in my company?

Yes. GPLv3 puts no restriction on use, only on distribution. You can install Pluryprint on every employee's machine, no license fees, no audits.

### Can I modify Pluryprint and sell it?

Yes. GPLv3 explicitly permits selling. You can:

- Charge for the binary distribution.
- Charge for support contracts.
- Charge for custom modifications.

What you **cannot** do is prevent the people you sold it to from also redistributing it. Once they have the binary, they have all the same rights you gave them, including the right to give it away for free.

### Can I embed Pluryprint inside my closed-source product?

**No, not as a derivative work**. If your product links to Pluryprint code, calls into Pluryprint internals as a library, or otherwise creates a combined work, your product becomes a derivative and must be GPLv3 too.

You can:

- **Invoke Pluryprint as a separate process** via its CLI. The "subprocess invocation" pattern is generally not considered creating a derivative work (this is the same logic that lets us use LibreOffice via subprocess without our code becoming MPL-licensed). See the FSF's position: ["mere aggregation"](https://www.gnu.org/licenses/gpl-faq.en.html#MereAggregation).
- **Distribute Pluryprint alongside your product** as a separate, optional component. As long as it's clear they're separate works, this is "mere aggregation" and doesn't trigger copyleft on your product.

### Can I use parts of Pluryprint's source code in my closed-source project?

No. Copying any non-trivial amount of Pluryprint source into a closed-source project would violate GPLv3.

If you want to use Pluryprint's code in a closed-source product, your options are:

1. Refactor the relevant logic from scratch (clean-room implementation).
2. Negotiate a commercial license with the copyright holders. This is non-trivial because copyright is held by all contributors, and any individual contributor can refuse.

We don't offer commercial relicensing as a project. The community of contributors collectively owns the copyright and is unlikely to grant blanket relicensing.

### Can I fork Pluryprint?

Yes, that's an explicit GPLv3 freedom. You must:

- Maintain GPLv3 on your fork.
- Make your fork's source code available.
- Mark modifications clearly so the original is not blamed for your changes.
- Not use the Pluryprint name/logo to imply official endorsement (see [trademark.md](./trademark.md)).

### Can I run Pluryprint on my company's servers and let users connect?

Yes. SaaS-style use does not trigger GPLv3's distribution requirement, so you don't need to publish your modifications. (Note: AGPLv3 is the variant designed to close this "loophole"; we use GPLv3, not AGPL, so SaaS use is unrestricted.)

### What if I find a bug or vulnerability?

For security issues, see `SECURITY.md` at the repo root. For other bugs, file an issue on GitHub.

GPLv3 disclaims warranty (see sections 15 and 16). The software is provided "as is". This is standard and doesn't reduce our motivation to fix bugs; it just means you can't sue us if something breaks.

### Does GPLv3 affect documents I print with Pluryprint?

**No.** Output of the program is not derivative of the program. Documents you print remain entirely your own property under whatever license you put on them.

### Does GPLv3 affect printer drivers I use with Pluryprint?

**No.** We invoke printer drivers via OS APIs (Win32, CUPS), not by linking. Printer drivers (often proprietary) remain under their own licenses.

### Can I use Pluryprint's icon or name in my project?

That's a trademark question, not a copyright question. See [trademark.md](./trademark.md). Short answer: linking to Pluryprint and saying "uses Pluryprint" is fine; using our name/logo to imply official endorsement is not.

## "Or later" clause

Our `LICENSE` file is GPL-3.0-or-later. This means:

- The code is licensed under GPLv3 today.
- Recipients can choose to use it under GPLv3 or any later GPL version (when published by FSF).

This forward-compatibility lets the project benefit from future GPL improvements without re-licensing. It also means a future GPLv4 (if FSF publishes one) automatically becomes an option for users.

## How to comply if you redistribute

A short checklist:

- [ ] Include the full GPLv3 text (`LICENSE` file).
- [ ] Include a copyright notice in each substantial source file (e.g. at the top: `Copyright (C) 2026 Pluryprint contributors`).
- [ ] Make the corresponding source code available (link to repo, or bundle source with binary, or provide a written offer).
- [ ] Mark modifications clearly so the original isn't held responsible for your changes.
- [ ] Don't add restrictions beyond GPLv3 (you can't, e.g., add a "no commercial use" clause; that would conflict with GPLv3).

If you're distributing as part of a larger product:

- [ ] Audit the product for incidental use of Pluryprint code that might create a derivative work.
- [ ] Consider an OSS Compliance review (a service many companies offer).

## Where to read the actual license

- The legally binding GPLv3 text: [https://www.gnu.org/licenses/gpl-3.0.html](https://www.gnu.org/licenses/gpl-3.0.html).
- A FSF-curated FAQ: [https://www.gnu.org/licenses/gpl-faq.html](https://www.gnu.org/licenses/gpl-faq.html).
- A Tl;DR overview: [https://choosealicense.com/licenses/gpl-3.0/](https://choosealicense.com/licenses/gpl-3.0/).
- Our `LICENSE` file at the repository root (the operative copy).

## Disclaimer

This document is a summary written by project maintainers, not a lawyer. For specific legal questions about how GPLv3 applies to your situation, consult a qualified attorney. The Software Freedom Conservancy ([sfconservancy.org](https://sfconservancy.org/)) and the Free Software Foundation ([fsf.org](https://www.fsf.org/)) maintain compliance resources we recommend.

## See also

- [third-party-licenses.md](./third-party-licenses.md)
- [trademark.md](./trademark.md)
- [privacy.md](./privacy.md)
- [../08-development/contributing.md](../08-development/contributing.md)
