#!/usr/bin/env bash
# Makes build/OfficeCraft-<version>-<arch>.dmg from build/OfficeCraft.app: the app and a link to
# Applications to drag it onto. Signs (and, with OFFICECRAFT_NOTARY_PROFILE, notarises and
# staples) the disk image when OFFICECRAFT_SIGN_ID is set. Phase 8.5.
#
#   packaging/macos/bundle.sh release && packaging/macos/sign.sh && packaging/macos/dmg.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
app=build/OfficeCraft.app
version=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$app/Contents/Info.plist")
arch=$(lipo -archs "$app/Contents/MacOS/officecraft" | tr ' ' '+')
out="build/OfficeCraft-$version-$arch.dmg"
stage=$(mktemp -d)/OfficeCraft
mkdir -p "$stage"
ditto "$app" "$stage/OfficeCraft.app"
ln -s /Applications "$stage/Applications"
rm -f "$out"
hdiutil create -quiet -volname "OfficeCraft" -srcfolder "$stage" -fs APFS -format UDZO -ov "$out"
if [ -n "${OFFICECRAFT_SIGN_ID:-}" ]; then
  codesign --force --timestamp --sign "$OFFICECRAFT_SIGN_ID" "$out"
  if [ -n "${OFFICECRAFT_NOTARY_PROFILE:-}" ]; then
    xcrun notarytool submit "$out" --keychain-profile "$OFFICECRAFT_NOTARY_PROFILE" --wait
    xcrun stapler staple "$out"
  fi
fi
hdiutil verify -quiet "$out"
echo "$out"
