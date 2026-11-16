# Phase 8 checks: polish and ship

What's verified automatically on every build, and what needs a person, another Mac or your Apple account. Status as of 2026-11-16.

## Automatic

| Check | How | Status |
|---|---|---|
| The window's accessibility tree has the shell's controls and the shown app's tree under "Tab content" | `--check` Accessibility stage (D28) | ✅ |
| Reduce motion stops animation in the chrome and in every app | `--check` Display stage (with the setting forced on) | ✅ |
| Increase contrast switches the chrome to its higher-contrast colours (muted text ≥ 7:1 on every surface) | `--check` Display stage; `tokens::tests` | ✅ |
| View ▸ Interface Text Size scales the chrome's text (Default, Large, Larger; kept between launches) | `--check` Display stage; `tokens::tests` | ✅ |
| A panic writes a crash report to `~/Library/Logs/Craftworks`; the next launch offers to show it | `--check` Crash stage; `crash::tests` | ✅ |
| No crate under a licence outside the allowlist | `scripts/licences.py` (run by `bundle.sh`; fails the build) | ✅ 935 crates |
| The hardened runtime needs no exceptions | `--check` of the ad-hoc, hardened build from the disk image | ✅ |

## By hand

| Check | How | Status |
|---|---|---|
| VoiceOver: tab strip (each tab read with its name and "selected"), "+" button, sidebar, search field, group labels, AI pill and review panel | ⌘F5, then VO-arrows through the title row and sidebar | ⏳ |
| VoiceOver inside each kind of tab: text in a document, a cell's address and value in a sheet, a slide's shapes, a PDF's text | Same, in each app's tab | ⏳ |
| System Settings ▸ Accessibility ▸ Display: Reduce motion and Increase contrast followed live, without restarting | Toggle each while Craftworks is open | ⏳ |

## Needs your Apple account or other Macs

| Check | Needs | Status |
|---|---|---|
| Developer ID signature with the hardened runtime and a secure timestamp | A Developer ID Application certificate in your keychain (`packaging/macos/sign.sh`) | ⏳ |
| Notarised and stapled app and disk image | A notarytool keychain profile (`xcrun notarytool store-credentials`) | ⏳ |
| Intel build (universal binary) | The `x86_64-apple-darwin` Rust target (a download) | ⏳ |
| The `.dmg` installs and runs on a clean Mac, Apple Silicon and Intel | Two Macs (or VMs) without developer tools | ⏳ |
| Updates | Where releases will be published (`craft-update` checks a release feed) | ⏳ |
