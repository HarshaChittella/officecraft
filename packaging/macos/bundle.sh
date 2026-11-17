#!/usr/bin/env bash
# Builds OfficeCraft.app (macOS) into build/: the binary, Info.plist with the document types it
# opens, an icon drawn by make_icon.py, and an ad-hoc signature. Developer ID signing and
# notarisation are Phase 8.
#
# Usage: packaging/macos/bundle.sh [cargo profile, default: measure]
set -euo pipefail
cd "$(dirname "$0")/../.."
profile="${1:-measure}"
# No licence outside the allowlist gets into a build (docs/licences.md, Phase 8.4).
python3 scripts/licences.py
cargo build -p officecraft --profile "$profile"
# OFFICECRAFT_APP builds the bundle somewhere else (say, while build/OfficeCraft.app is running).
app="${OFFICECRAFT_APP:-build/OfficeCraft.app}"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$profile/officecraft" "$app/Contents/MacOS/officecraft"

# Licences: the report, and each Craft repo's own credits for what it bundles.
mkdir -p "$app/Contents/Resources/Licenses"
cp docs/licences.md "$app/Contents/Resources/Licenses/"
for repo in wordcraft gridcraft deckcraft pdfcraft craft-fonts craft-libs; do
  for f in vendor/$repo/ATTRIBUTION.* vendor/$repo/NOTICE vendor/$repo/LICENSES.toml; do
    [ -f "$f" ] && cp "$f" "$app/Contents/Resources/Licenses/$repo-$(basename "$f")"
  done
done

# The icon: our own artwork, drawn in code.
iconset=$(mktemp -d)/OfficeCraft.iconset
python3 packaging/macos/make_icon.py "$iconset"
iconutil -c icns "$iconset" -o "$app/Contents/Resources/OfficeCraft.icns"

version=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')
doc_type() { # name, role, rank, UTIs…
  local name=$1 role=$2 rank=$3; shift 3
  printf '    <dict>\n      <key>CFBundleTypeName</key><string>%s</string>\n      <key>CFBundleTypeRole</key><string>%s</string>\n      <key>LSHandlerRank</key><string>%s</string>\n      <key>LSItemContentTypes</key>\n      <array>\n' "$name" "$role" "$rank"
  for uti in "$@"; do printf '        <string>%s</string>\n' "$uti"; done
  printf '      </array>\n    </dict>\n'
}
{
cat <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>OfficeCraft</string>
  <key>CFBundleDisplayName</key><string>OfficeCraft</string>
  <key>CFBundleIdentifier</key><string>dev.officecraft.OfficeCraft</string>
  <key>CFBundleExecutable</key><string>officecraft</string>
  <key>CFBundleIconFile</key><string>OfficeCraft</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>CFBundleDocumentTypes</key>
  <array>
PLIST
# Office formats: OfficeCraft edits them (Alternate: it never takes over your default app).
doc_type "Document" Editor Alternate org.openxmlformats.wordprocessingml.document org.oasis-open.opendocument.text public.rtf
doc_type "Legacy document (.doc)" Viewer Alternate com.microsoft.word.doc
doc_type "Text, Markdown, HTML and TeX" Editor Alternate net.daringfireball.markdown public.html public.plain-text org.tug.tex
doc_type "Spreadsheet" Editor Alternate org.openxmlformats.spreadsheetml.sheet public.comma-separated-values-text public.tab-separated-values-text
doc_type "Presentation" Editor Alternate org.openxmlformats.presentationml.presentation
doc_type "PDF" Editor Alternate com.adobe.pdf
cat <<PLIST
  </array>
</dict>
</plist>
PLIST
} > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null
codesign --force --sign - "$app" >/dev/null 2>&1 || echo "warning: ad-hoc signing failed" >&2
echo "$app"
