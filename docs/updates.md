# Updates: following the Craft apps' releases

Status: milestone 1 (local sync and the local rebuild mode) built 2026-11-17; milestones 2–4 not yet.

**Using it now:** `scripts/upstream.py status` lists each app's newest release against `vendor/versions.json`; `scripts/upstream.py sync` takes the newer ones (see its `--help`). In the app, OfficeCraft ▸ Check for Updates… (and a check at most once every 20 hours) shows **Update available** in the title row; **Update OfficeCraft** runs the sync here and, when it passes, **Restart to update**. This needs the checkout this build came from (`~/src/officecraft`), Rust and Python 3; without them the feature stays hidden.

**How releases relate to what we build:** upstream's release tags merge a tested `main` into the `release` branch and add nothing of their own, so a release *is* that tested `main` commit (`release_source` in the script). On 2026-11-17 our vendored copies were already 75–143 commits past the latest published releases (WordCraft 0.18.1, GridCraft 0.19.1, DeckCraft 0.18.1, PdfCraft 0.21.1), so `vendor/versions.json` records those as included and the next published release is the first one a sync will take.

Goal: when WordCraft, GridCraft, DeckCraft or PdfCraft ship a release, OfficeCraft picks it up, rebuilds, checks itself, and offers you **one** update: "OfficeCraft 0.3.0 is available (WordCraft 0.19.0, PdfCraft 0.22.0)".

## How the Craft apps update today

All four use `craft-update` (vendor/craft-libs/crates/update):

1. **Check.** At start, `GET api.github.com/repos/storytold/<app>/releases?per_page=30`. The newest stable, non-draft release is compared with the app's `CARGO_PKG_VERSION`.
2. **Download.** It picks the asset for this Mac by name, `<app>-<version>-macos-<universal|arm64|aarch64|x86_64>.dmg`, and checks its SHA-256, taken from GitHub's digest or from `SHA256SUMS.txt`.
3. **Install.** It mounts the DMG, runs `codesign --verify --strict` on the bundle and copies it over the running app. The copy only happens when the running bundle's `CFBundleIdentifier` is that app's (`ai.storyteller.<id>`, or `bundle_id_override`). Any other copy, including a Cargo build, gets a download link.
4. **Cadence.** Releases are cut from upstream's `release` branch by `release.yml` (nightly cron plus manual runs). Each release commit names its source: "Release: WordCraft v0.19.0 (main 323cf78232)". There were 12–15 releases per app in the month to 2026-11-17.

Inside OfficeCraft none of this runs. Each app turns its startup check on only in its own `main.rs`, which OfficeCraft doesn't use. The apps' **Help ▸ Check for Updates…** can still reach OfficeCraft's menu bar. It can't touch OfficeCraft (the bundle ID differs), but it would offer to download the standalone app, so it is hidden (see stage 6).

## Why OfficeCraft rebuilds rather than downloads

OfficeCraft is one binary built from the four apps' source. They share one egui fork and one craft-libs revision (`[patch]` in Cargo.toml), and carry our patches: H1, H5–H7 and T1 in `upstream-patches.md`. A WordCraft DMG is a separate app with nothing that can be swapped in. Taking an upstream release therefore means building OfficeCraft from that release's source.

## The pipeline

```
 watch ──▶ sync ──▶ align ──▶ build + check ──▶ sign, notarise, package ──▶ publish ──▶ in-app update
 (API)    (rebase   (craft-libs,  (tests, --check      (Developer ID,             (GitHub      (craft-update,
           patches)  egui)         light + dark)        notarytool, DMG)            release)     "Update available")
   any failure ──▶ stop, keep the last good release, report what failed
```

### 1. Watch

`scripts/upstream.py status` asks GitHub for each app's latest stable release, the same call `craft-update` makes, sending a token when one is set. It compares the result with `vendor/versions.json`, the release each submodule is built from. If nothing is newer it stops. Several new releases together make one OfficeCraft build.

### 2. Sync

For each app with a newer release:

- **Base:** the release tag `vX.Y.Z` on upstream's `release` branch, which is what upstream built and shipped, not `main`.
- **Rebase:** `git rebase --onto vX.Y.Z <old base> officecraft/hosted`, done on a new branch `officecraft/hosted-X.Y.Z` in that app's fork. Our patches are small (2, 1, 1 and 3 commits), so most syncs replay cleanly.
- **On conflict:** stop and report the app, the patch and the files. Nothing is pushed.
- **Then:** move the submodule pointer and update `vendor/versions.json`.

The first sync is the big one. We are on upstream `main` from 2026-11-12/14, 43–85 commits behind `main`, and between releases rather than on one.

### 3. Align

Each app pins its own craft-libs and egui revisions; OfficeCraft forces one of each.

- **craft-libs:** move `vendor/craft-libs` to the newest revision any of the four pins. All four pin commits on craft-libs `main`, so the newest is the one that descends from the others.
- **egui:** move the egui fork `rev` in `[patch]` the same way.
- **Lock file:** `cargo update -p` the crates that changed, then `scripts/licences.py`, which fails on a licence outside the allowlist.
- **If two apps need revisions that don't build together:** stop and report. This is the most likely failure, and it needs a hand fix or a wait for upstream to converge.

### 4. Build and check

All of these must pass:

- `cargo build --release -p officecraft`
- `cargo test -p officecraft-shell -p officecraft-core -p officecraft`, and `cargo clippy` with the workspace lints
- `--check` in light and dark: every stage, plus the screenshots kept as build artefacts

Our glue code (`crates/officecraft-core/src/engines/*.rs`) reaches into each app's internals, so an upstream refactor can break the build here. That is caught, and the fix is made by hand on the sync branch.

### 5. Sign, notarise, package

- `packaging/macos/bundle.sh release`, then `sign.sh` with `OFFICECRAFT_SIGN_ID` (Developer ID Application) and `OFFICECRAFT_NOTARY_PROFILE`, which notarises and staples.
- `dmg.sh` signs, notarises and staples the disk image.
- **Asset name:** `craft-update`'s convention, `officecraft-<version>-macos-arm64.dmg`, plus `SHA256SUMS.txt`. `dmg.sh` changes to write that name (today it writes `OfficeCraft-0.1.0-arm64.dmg`).
- **Universal build:** `macos-universal` needs `rustup target add x86_64-apple-darwin` and roughly doubles build time. It waits until Intel Macs matter.

### 6. Publish, and update in the app

**Publish:**
- A GitHub release on the OfficeCraft releases repo, tagged `vX.Y.Z`, with the DMG and `SHA256SUMS.txt`.
- The notes are generated from `vendor/versions.json` and the upstream release notes, for example "WordCraft 0.18.1 → 0.19.0: …".
- `main` gets the sync merge and the tag.

**In the app** (`apps/officecraft/src/updates.rs`):

- **Updater:** `craft-update`'s `Updater` with `App { id: "officecraft", name: "OfficeCraft", repo: "<you>/officecraft-releases", bundle_id_override: Some("dev.officecraft.OfficeCraft"), .. }`. The download, checksum, signature check, swap and relaunch are the library's, already used by four apps; no new dependency.
- **When it checks:** at most once a day at launch, plus **OfficeCraft ▸ Check for Updates…**. A setting, **Check for updates automatically**, defaults on.
- **What it shows:**
  - an **Update available** pill in the title row, in the same family as the AI pill;
  - a banner on Home listing the apps that changed;
  - **Restart to Update**. With unsaved work, the library's save flow asks first (`show_with_save`).
- **What it hides:** each app's own **Check for Updates…**, from the menu bar the front tab supplies. Their updaters stay off, as they are today.
- **About OfficeCraft** lists the four apps' versions, from the embedded `versions.json`.

## Versions

- **OfficeCraft has its own semver.** Minor goes up when any app's minor or major goes up; patch goes up otherwise.
- **`vendor/versions.json`** records each app's upstream tag and release commit, plus the craft-libs and egui revisions. It is embedded in the binary, and the release notes and About come from it.

## Where it runs

1. **Now: on this Mac** with `scripts/upstream.py sync`, by hand or from a nightly `launchd` job. The toolchain, build cache and the Developer ID in the keychain are already here, and an incremental release build takes about 3 minutes.
2. **Then: GitHub Actions** on a **self-hosted runner on this Mac**, with `schedule` and `workflow_dispatch` triggers only (never `pull_request`, so no outside code runs here). That gives logs, artefacts and failure e-mails, with the signing key staying in this Mac's keychain.
3. **Later, optionally: GitHub-hosted runners.** The standard macOS arm64 runner (3 cores, 7 GB RAM, 14 GB disk) is likely too small: `target/release` alone is 5.3 GB, before the five vendored repos and the registry. A larger runner costs money, and the certificate would go in as an encrypted `.p12` secret plus an App Store Connect API key.

## Failure handling

- **Every stage stops on failure.** Nothing is published, the last good release stays current, and the report names the stage, the app and the log (a GitHub issue, once on Actions).
- **Upstream keeps shipping:** the next run tries again with whatever is newest. An app that keeps failing can be pinned in `vendor/versions.json` while the other three move on.
- **One release, one revert:** a bad OfficeCraft release is withdrawn by publishing a higher version built from the previous `versions.json`.

## What the owner provides

- **Developer ID Application certificate** in this Mac's keychain, and a **notarytool profile** (`officecraft-notary`).
- **A public repo for releases.** The source can stay private in a separate repo: `craft-update` reads releases without signing in, so the releases repo must be public.
- ~~Forks for GridCraft and DeckCraft~~: done; all four apps come from the HarshaChittella forks.
- **A licence for OfficeCraft**, before anything is distributed (open in PLAN Phase 8).

## Milestones

1. **Local sync.** `scripts/upstream.py status|sync`, `vendor/versions.json`, and stages 1–4 on this Mac. Done when the first catch-up to the current releases passes `--check` and is installed here.
2. **Signed releases.** Stage 5 with your Developer ID, the asset naming and `SHA256SUMS.txt`, and a first public GitHub release. Done when a downloaded DMG opens on a Mac without Gatekeeper warnings.
3. **In-app updates.** Stage 6: the pill, the Home banner, Check for Updates…, the apps' own update items hidden, and versions in About. Done when an installed 0.x.0 updates itself to 0.x.1 and relaunches.
4. **Nightly.** GitHub Actions on the self-hosted runner, with failure issues. Done when an upstream release turns into a published OfficeCraft release with no hands on it.
