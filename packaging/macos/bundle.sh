#!/usr/bin/env bash
# Builds Craftworks.app (macOS) into build/: the binary, Info.plist with the document types it
# opens, an icon drawn by make_icon.py, and an ad-hoc signature. Developer ID signing and
# notarisation are Phase 8.
#
# Usage: packaging/macos/bundle.sh [cargo profile, default: measure]
set -euo pipefail
cd "$(dirname "$0")/../.."
profile="${1:-measure}"
cargo build -p craftworks --profile "$profile"
app=build/Craftworks.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/$profile/craftworks" "$app/Contents/MacOS/craftworks"

# The icon: our own artwork, drawn in code.
iconset=$(mktemp -d)/Craftworks.iconset
python3 packaging/macos/make_icon.py "$iconset"
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Craftworks.icns"

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
  <key>CFBundleName</key><string>Craftworks</string>
  <key>CFBundleDisplayName</key><string>Craftworks</string>
  <key>CFBundleIdentifier</key><string>dev.craftworks.Craftworks</string>
  <key>CFBundleExecutable</key><string>craftworks</string>
  <key>CFBundleIconFile</key><string>Craftworks</string>
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
# Office formats: Craftworks edits them (Alternate: it never takes over your default app).
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
