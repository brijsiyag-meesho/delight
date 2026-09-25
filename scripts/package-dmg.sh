#!/usr/bin/env bash
# Package Delight for distribution: dist/Delight-<version>-sdk<sdk>-<arch>.dmg
# (the app and an Applications link to drag it to).
#
#   scripts/package-dmg.sh                    # ad-hoc signed: for your own Macs
#
#   DELIGHT_SIGN_IDENTITY="Developer ID Application: Name (TEAMID)" \
#   DELIGHT_NOTARY_PROFILE=delight \
#   scripts/package-dmg.sh                    # signed, notarized, stapled: for anyone
#
# Notarization needs an Apple Developer account and a notarytool profile,
# made once with:
#   xcrun notarytool store-credentials delight --apple-id <you> --team-id <TEAMID>
#
# Builds the app first (scripts/bundle.sh; set DELIGHT_BUNDLE_PLUGINS=0 to
# leave out your local plugins). It's an app release: run release-app.sh or
# release-sdk.sh before, so plugins built with the SDK kit are known to load.
set -euo pipefail
cd "$(dirname "$0")/.."
IDENTITY=${DELIGHT_SIGN_IDENTITY:--}
PROFILE=${DELIGHT_NOTARY_PROFILE:-}
[[ -z $PROFILE || $IDENTITY != - ]] || { echo "notarization needs DELIGHT_SIGN_IDENTITY (a Developer ID)" >&2; exit 1; }

./scripts/bundle.sh
APP=dist/Delight.app
VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
SDK=$(sed -n 's/^version = "\(.*\)"$/\1/p' crates/sdk/Cargo.toml | head -1)
DMG="dist/Delight-$VERSION-sdk$SDK-$(uname -m).dmg"

STAGE=$(mktemp -d "${TMPDIR:-/tmp}/delight-dmg.XXXXXX")
trap 'rm -rf "$STAGE"' EXIT
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
hdiutil create -quiet -volname "Delight $VERSION" -srcfolder "$STAGE" -fs HFS+ -format UDZO "$DMG"
[[ $IDENTITY == - ]] || codesign --force --sign "$IDENTITY" --timestamp "$DMG"

if [[ -n $PROFILE ]]; then
    echo "Notarizing $DMG (a few minutes)…"
    xcrun notarytool submit "$DMG" --keychain-profile "$PROFILE" --wait
    xcrun stapler staple "$DMG"
    spctl --assess --type open --context context:primary-signature --verbose "$DMG"
fi
echo "Packaged $DMG ($(du -h "$DMG" | cut -f1))$([[ $IDENTITY == - ]] && echo " — ad-hoc signed: other Macs need right-click → Open")"
