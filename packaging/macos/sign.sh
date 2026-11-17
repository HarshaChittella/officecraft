#!/usr/bin/env bash
# Signs build/OfficeCraft.app with the hardened runtime, and (with your Apple account) notarises
# and staples it. Phase 8.3.
#
#   OFFICECRAFT_SIGN_ID="Developer ID Application: Your Name (TEAMID)" \
#   OFFICECRAFT_NOTARY_PROFILE=officecraft-notary \
#   packaging/macos/sign.sh [build/OfficeCraft.app]
#
# - OFFICECRAFT_SIGN_ID: a Developer ID Application identity in your keychain
#   (`security find-identity -v -p codesigning` lists them). Unset: ad-hoc signing with the
#   hardened runtime, for testing on this Mac only.
# - OFFICECRAFT_NOTARY_PROFILE: a notarytool keychain profile, made once with
#   `xcrun notarytool store-credentials officecraft-notary --apple-id … --team-id …`
#   (it asks for an app-specific password). Unset: no notarisation.
#
# The entitlements file is empty on purpose: the hardened runtime with no exceptions. OfficeCraft
# needs no JIT, no unsigned executable memory, no library-validation exemption and no DYLD
# variables (the hardened build passes the full --check). It isn't sandboxed: it opens files
# across folders and its AI connector talks to the window over a Unix socket. (macOS's
# entitlements parser refuses XML comments, so this note lives here.)
set -euo pipefail
cd "$(dirname "$0")/../.."
app="${1:-build/OfficeCraft.app}"
id="${OFFICECRAFT_SIGN_ID:--}"
ents=packaging/macos/entitlements.plist

if [ "$id" = "-" ]; then
  echo "Signing ad hoc (hardened runtime): this Mac only. Set OFFICECRAFT_SIGN_ID for a Developer ID."
  codesign --force --options runtime --entitlements "$ents" --sign - "$app"
else
  codesign --force --options runtime --timestamp --entitlements "$ents" --sign "$id" "$app"
fi
codesign --verify --strict --verbose=2 "$app"

if [ -n "${OFFICECRAFT_NOTARY_PROFILE:-}" ]; then
  [ "$id" != "-" ] || { echo "Notarisation needs a Developer ID (OFFICECRAFT_SIGN_ID)." >&2; exit 2; }
  zip=$(mktemp -d)/OfficeCraft.zip
  ditto -c -k --keepParent "$app" "$zip"
  xcrun notarytool submit "$zip" --keychain-profile "$OFFICECRAFT_NOTARY_PROFILE" --wait
  xcrun stapler staple "$app"
  spctl --assess --type execute --verbose "$app"
fi
echo "$app"
