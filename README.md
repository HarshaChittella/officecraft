# OfficeCraft

Documents, sheets, presentations and PDFs in tabs of one window: WordCraft, GridCraft,
DeckCraft and PdfCraft hosted together, with one Home page, one tab strip, projects, and an AI
connector.

OfficeCraft is not made by ArtCraft. It builds the four Craft apps
([storytold](https://github.com/storytold)) from their source, with a few small changes kept
on `officecraft/hosted` branches of this account's forks (`docs/upstream-patches.md`).

## Build

You need Rust (see `rust-version` in `Cargo.toml`) and the submodules:

```bash
git clone --recurse-submodules https://github.com/HarshaChittella/officecraft.git
```

- **macOS:** `packaging/macos/bundle.sh release` makes `build/OfficeCraft.app`
  (`sign.sh` and `dmg.sh` sign and package it). `target/measure/officecraft --check DIR` runs
  the acceptance check.
- **Windows:** build natively with `cargo build --release -p officecraft`, or cross-build from a
  Mac with `packaging/windows/package.sh`.
- **Every push to `main`** builds both on GitHub Actions (`.github/workflows`); the Windows zip
  is attached to the run.

## Docs

- `docs/PLAN.md`: phases and status
- `docs/design.md`: the look and behaviour
- `docs/decisions.md`: decisions and why
- `docs/updates.md`: following the Craft apps' releases
- `docs/ai.md`: the AI connector

## Licence

MIT OR Apache-2.0, at your option (`LICENSE-MIT`, `LICENSE-APACHE`), like the Craft apps.
Third-party licences are listed in `docs/licences.md` and shipped with each build.
