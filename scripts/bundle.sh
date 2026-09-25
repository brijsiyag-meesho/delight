#!/usr/bin/env bash
# Build a release Delight.app (menu-bar only: LSUIElement) into ./dist.
#
# It carries:
# * Contents/PlugIns — the plugins in plugins/ (your local, first-party ones),
#   built with the app so they always match it. DELIGHT_BUNDLE_PLUGINS=0 skips
#   them (e.g. for a public build).
# * Contents/Resources — the plugin tooling: the SDK kit and the `delight` CLI.
#
# Signed with the hardened runtime: ad-hoc by default, or with
# DELIGHT_SIGN_IDENTITY="Developer ID Application: …" for distribution
# (scripts/package-dmg.sh then notarizes).
set -euo pipefail
shopt -s nullglob
cd "$(dirname "$0")/.."
APP=dist/Delight.app
APP_VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
IDENTITY=${DELIGHT_SIGN_IDENTITY:--}

PLUGINS=()
if [[ ${DELIGHT_BUNDLE_PLUGINS:-1} != 0 ]]; then
    for manifest in plugins/*/Cargo.toml; do
        PLUGINS+=("$(awk '/^\[package\]/ {p=1; next} /^\[/ {p=0} p && $1 == "name" { sub(/^[^"]*"/, ""); sub(/".*/, ""); print; exit }' "$manifest")")
    done
fi
PACKAGES=(-p delight)
for pkg in ${PLUGINS[@]+"${PLUGINS[@]}"}; do PACKAGES+=(-p "$pkg"); done
cargo build --release "${PACKAGES[@]}"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks" "$APP/Contents/PlugIns" "$APP/Contents/Resources/bin"
cp target/release/delight "$APP/Contents/MacOS/Delight"
# The shared SDK (GPUI) and Rust's std, found via @executable_path/../Frameworks.
cp target/release/libdelight_sdk.dylib "$APP/Contents/Frameworks/"
cp "$(rustc --print target-libdir)"/libstd-*.dylib "$APP/Contents/Frameworks/"
for pkg in ${PLUGINS[@]+"${PLUGINS[@]}"}; do
    cp "target/release/lib${pkg//-/_}.dylib" "$APP/Contents/PlugIns/${pkg#delight-plugin-}.dylib"
done
# Plugin tooling: the kit this app's SDK was built from, and the CLI that
# builds plugins with it.
./scripts/make-kit.sh "$("$APP/Contents/MacOS/Delight" --sdk-build-id)" "$APP/Contents/Resources/sdk-kit.tar.gz"
cp scripts/delight "$APP/Contents/Resources/bin/delight"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Delight</string>
  <key>CFBundleDisplayName</key><string>Delight</string>
  <key>CFBundleIdentifier</key><string>dev.delight.app</string>
  <key>CFBundleExecutable</key><string>Delight</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$APP_VERSION</string>
  <key>CFBundleVersion</key><string>$APP_VERSION</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST

# Inside-out: nested code first, then the app with the hardened runtime.
SIGN=(codesign --force --sign "$IDENTITY")
[[ $IDENTITY == - ]] || SIGN+=(--timestamp)
for lib in "$APP"/Contents/Frameworks/*.dylib "$APP"/Contents/PlugIns/*.dylib; do
    "${SIGN[@]}" "$lib"
done
"${SIGN[@]}" --options runtime --entitlements scripts/Delight.entitlements "$APP"
codesign --verify --strict --deep "$APP"

# The bundled plugins load in the finished (signed, hardened) app.
for lib in "$APP"/Contents/PlugIns/*.dylib; do
    "$APP/Contents/MacOS/Delight" --check-plugin "$lib" >/dev/null || { echo "bundled plugin $lib doesn't load" >&2; exit 1; }
done
echo "Built $APP ($APP_VERSION, SDK $("$APP/Contents/MacOS/Delight" --sdk-build-id | cut -d' ' -f3), ${#PLUGINS[@]} bundled plugins, signed: $IDENTITY)"
