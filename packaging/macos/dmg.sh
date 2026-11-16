#!/usr/bin/env bash
# Makes build/Craftworks-<version>-<arch>.dmg from build/Craftworks.app: the app and a link to
# Applications to drag it onto. Signs (and, with CRAFTWORKS_NOTARY_PROFILE, notarises and
# staples) the disk image when CRAFTWORKS_SIGN_ID is set. Phase 8.5.
#
#   packaging/macos/bundle.sh release && packaging/macos/sign.sh && packaging/macos/dmg.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
app=build/Craftworks.app
version=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$app/Contents/Info.plist")
arch=$(lipo -archs "$app/Contents/MacOS/craftworks" | tr ' ' '+')
out="build/Craftworks-$version-$arch.dmg"
stage=$(mktemp -d)/Craftworks
mkdir -p "$stage"
ditto "$app" "$stage/Craftworks.app"
ln -s /Applications "$stage/Applications"
rm -f "$out"
hdiutil create -quiet -volname "Craftworks" -srcfolder "$stage" -fs APFS -format UDZO -ov "$out"
if [ -n "${CRAFTWORKS_SIGN_ID:-}" ]; then
  codesign --force --timestamp --sign "$CRAFTWORKS_SIGN_ID" "$out"
  if [ -n "${CRAFTWORKS_NOTARY_PROFILE:-}" ]; then
    xcrun notarytool submit "$out" --keychain-profile "$CRAFTWORKS_NOTARY_PROFILE" --wait
    xcrun stapler staple "$out"
  fi
fi
hdiutil verify -quiet "$out"
echo "$out"
