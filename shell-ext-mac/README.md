# shell-ext-mac — why this directory is (almost) empty

The macOS Finder integration ships **inside the app bundle**, not as a separate
extension. There is no Xcode project here, no `.appex`, and no build step of its
own, because the mechanism we chose does not need one.

Where the code actually lives:

| Piece                             | Path                                                                                         |
| --------------------------------- | -------------------------------------------------------------------------------------------- |
| Service declaration               | [`src-tauri/Info.plist`](../src-tauri/Info.plist) (`NSServices`)                             |
| Service provider (Rust / `objc2`) | [`src-tauri/src/platform/macos/services.rs`](../src-tauri/src/platform/macos/services.rs)    |
| Handoff file writer               | [`src-tauri/src/platform/macos/handoff.rs`](../src-tauri/src/platform/macos/handoff.rs)      |
| Handoff file reader + validation  | [`src-tauri/src/cli/handoff.rs`](../src-tauri/src/cli/handoff.rs)                            |
| Architecture write-up             | [`docs/04-architecture/shell-extensions.md`](../docs/04-architecture/shell-extensions.md) §3 |

## The decision: NSServices, not a Finder Sync extension

macOS offers four ways to put a command on a Finder selection. Only one of them
costs nothing to ship:

|                                            | **NSServices** (chosen)     | Action Extension (`.appex`)  | Finder Sync (`.appex`)                               | Automator Quick Action       |
| ------------------------------------------ | --------------------------- | ---------------------------- | ---------------------------------------------------- | ---------------------------- |
| Appears in                                 | Context menu → **Services** | Context menu → Quick Actions | Context menu **top level**                           | Context menu → Quick Actions |
| Receives a multi-file selection            | yes (`NSPasteboard`)        | yes                          | yes                                                  | yes                          |
| Separate bundle to build                   | **no**                      | yes                          | yes                                                  | yes                          |
| Must be sandboxed                          | **no**                      | yes                          | yes (mandatory)                                      | no                           |
| Needs an App Group                         | **no**                      | recommended                  | yes                                                  | no                           |
| Separate signature + re-signing the `.app` | **no**                      | yes                          | yes                                                  | no                           |
| User must switch it on                     | rarely                      | yes                          | yes                                                  | yes                          |
| Covers the whole filesystem                | **yes**                     | yes                          | **no** — only the directories the extension declares | yes                          |
| Survives an MDM extension policy           | **yes**                     | no                           | no                                                   | n/a                          |
| Leaves residue on uninstall                | **no**                      | no                           | no                                                   | yes (`~/Library/Services`)   |

Why the others lost, concretely:

1. **Tauri cannot bundle an `.appex` without breaking signing.** Tauri's
   bundler signs the `.app` as one unit; an extension has to be inserted
   _before_ the container's final signature (inside-out signing) and
   `codesign --deep` has been deprecated since macOS 13. That means a
   post-bundle script that re-signs by hand, and a notarization step that
   silently produces a broken app if the order is ever wrong. NSServices needs
   no post-processing at all — the plist is merged by the bundler and signed
   with everything else.
2. **Finder Sync is the wrong tool by Apple's own description.** Its model is
   _monitored directories_: an extension declares which folders it watches.
   Covering every disk means declaring `/`, which is abusing the API, and any
   folder outside the declared set gets no menu entry at all. Apple states
   plainly that "Finder Sync is not intended as a general tool for modifying
   the Finder's user interface."
3. **Extensions are switchable off by someone who is not the user.** macOS 15
   removed the Finder-extension management UI from System Settings and only
   partially restored it in 15.2, and a corporate MDM profile can block
   ExtensionKit outright. Pluryprint's target users are offices — exactly the
   population running MDM. An NSService is part of the app, so there is nothing
   for a policy to disable.
4. **Automator Quick Actions install into `~/Library/Services`**, i.e. outside
   the app bundle, so an uninstall leaves a broken menu entry behind.

## The cost we accepted

The entry lands in the **Services** submenu rather than at the top level of the
context menu. That is one extra hover compared with Windows 11. We judged that
cheaper than a second signed bundle, a sandbox, an App Group and an activation
step the user has to discover — and unlike the alternatives, it works on every
folder on every volume with no setup.

Users who want it faster can bind a keyboard shortcut in
**System Settings → Keyboard → Keyboard Shortcuts → Services**, which works for
NSServices and not for the other three mechanisms.

## If we ever do want the `.appex`

The only thing a Finder Sync extension buys us that NSServices cannot is
**badges** — an overlay icon on files that are queued or printing — plus a
top-level menu entry. If that becomes a requirement, this directory is where the
extension would live, and the work is roughly:

1. An Xcode project (or a `swiftc` invocation driven from a script) producing
   `PluryprintFinder.appex`, implementing `FIFinderSync`:
   `menu(for:)` for the context menu, `beginObservingDirectory(at:)` /
   `requestBadgeIdentifier(for:)` for the badges.
2. An App Group (`group.com.maecly.pluryprint`) shared by app and extension —
   mandatory, because a sandboxed extension cannot reach
   `~/Library/Application Support/Pluryprint`. The handoff directory would have
   to move into the group container, which changes
   [`crate::util::paths::data_dir`](../src-tauri/src/util/paths.rs) for macOS
   and therefore the handoff contract.
3. Insertion into `Contents/PlugIns/` **before** the container is signed —
   either via `bundle.macOS.files` in `tauri.conf.json` or a post-bundle script
   — followed by inside-out `codesign` of the extension and then the `.app`, and
   notarization of the result. Both app and extension need the same Team ID.
4. A user-facing "enable the extension" onboarding step, plus a fallback for
   when an MDM policy refuses.

That is a v1.1+ item at best, and only if badges are actually asked for. The
NSService above stays either way: it is the path that works when the extension
is switched off.

## Testing it by hand

`cargo test` covers everything that does not need the Finder (payload writing,
pasteboard decoding, the action vocabulary). Making the menu entry actually
appear requires a real bundle registered with Launch Services:

```sh
pnpm tauri build --bundles app
LSREG=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

# 1. Confirm the declaration survived the bundler.
plutil -p src-tauri/target/release/bundle/macos/Pluryprint.app/Contents/Info.plist | grep -A5 NSServices

# 2. Tell Launch Services and the pasteboard server about this build.
"$LSREG" -f src-tauri/target/release/bundle/macos/Pluryprint.app
/System/Library/CoreServices/pbs -flush

# 3. Confirm the system *discovered* the service (this is the real assertion).
/System/Library/CoreServices/pbs -dump_pboard | grep -i "Print with Pluryprint"

# 4. Restart the Finder so its menu is rebuilt.
killall Finder
```

Notes:

- The app is most reliably discovered from `/Applications`; a bundle sitting in
  `target/release/bundle/macos` works once `lsregister -f` has seen it, but the
  Services cache is the flakiest part of this integration — if the entry does
  not appear, `pbs -flush` and a Finder restart before anything else.
- If the entry is present but greyed out, the selection did not match
  `NSSendFileTypes`; check the plist, not the Rust.
- The user can hide or rename the entry in
  **System Settings → Keyboard → Keyboard Shortcuts → Services**; a service the
  user unticked there will not appear, which is worth knowing before debugging
  the code.

These commands modify the _developer's_ Launch Services database. They are
documented here rather than run by any script in the repo.
