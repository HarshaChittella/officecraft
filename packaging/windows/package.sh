#!/usr/bin/env bash
# Builds Craftworks for Windows (x64) from this Mac and packs it as
# build/Craftworks-<version>-windows-x64.zip: Craftworks.exe, the licences, and a note.
#
# Cross-built with cargo-xwin (Microsoft's SDK and C runtime, fetched on first use; running
# this accepts Microsoft's licence terms for them) and Homebrew's llvm and lld. Unsigned:
# Windows SmartScreen warns on first run until it is signed with a code-signing certificate.
#
# Usage: packaging/windows/package.sh [cargo profile, default: release]
set -euo pipefail
cd "$(dirname "$0")/../.."
profile="${1:-release}"
export PATH="$(brew --prefix llvm)/bin:$(brew --prefix lld)/bin:$HOME/.cargo/bin:$PATH" XWIN_ACCEPT_LICENSE=1
target=x86_64-pc-windows-msvc
python3 scripts/licences.py
cargo xwin build --profile "$profile" --target "$target" -p craftworks
version=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')
name="Craftworks-$version-windows-x64"
stage="build/windows/$name"
rm -rf "build/windows"
mkdir -p "$stage/Licenses"
cp "target/$target/$profile/craftworks.exe" "$stage/Craftworks.exe"
cp docs/licences.md "$stage/Licenses/"
for repo in wordcraft gridcraft deckcraft pdfcraft craft-fonts craft-libs; do
  for f in vendor/$repo/ATTRIBUTION.* vendor/$repo/NOTICE vendor/$repo/LICENSES.toml; do
    [ -f "$f" ] && cp "$f" "$stage/Licenses/$repo-$(basename "$f")"
  done
done
cat > "$stage/README.txt" <<TXT
Craftworks $version for Windows (x64)

Documents, sheets, presentations and PDFs in tabs of one window.

Run Craftworks.exe. It isn't signed yet, so Windows may say it protected your PC: choose
"More info", then "Run anyway". Settings are kept in %APPDATA%\Craftworks.

This build was made on a Mac and has not been tested on Windows yet.
TXT
( cd build/windows && rm -f "../$name.zip" && zip -qr "../$name.zip" "$name" )
echo "build/$name.zip"
