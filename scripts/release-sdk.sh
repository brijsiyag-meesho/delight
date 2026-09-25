#!/usr/bin/env bash
# Release the plugin SDK: build the app (which carries its SDK kit), keep a
# copy of the kit in dist/sdk/ — release-app.sh checks later app releases
# against it — and prove the kit builds plugins that app loads.
#
#   scripts/release-sdk.sh     # → dist/sdk/delight-sdk-<version>-<target>.tar.gz
#
# The kit is this workspace's source — the exact inputs of the app's
# libdelight_sdk.dylib (crates/sdk, Cargo.lock, rust-toolchain.toml,
# .cargo/config.toml, the release profile). A plugin built inside it (under
# plugins/, depending on `delight-sdk = "=<version>"`, which the workspace's
# [patch.crates-io] points at crates/sdk) gets the app's SDK identity.
#
# Bump crates/sdk's version (and crates/gpui-alias's) first whenever the SDK
# changed: its sources, GPUI or any dependency, the toolchain or the profile.
set -euo pipefail
cd "$(dirname "$0")/.."
version() { sed -n 's/^version = "\(.*\)"$/\1/p' "$1" | head -1; }
SDK=$(version crates/sdk/Cargo.toml)
ALIAS=$(version crates/gpui-alias/Cargo.toml)
[[ "$SDK" == "$ALIAS" ]] || { echo "crates/gpui-alias must have the SDK's version ($SDK), not $ALIAS" >&2; exit 1; }
TARGET=$(rustc -vV | sed -n 's/^host: //p')

./scripts/bundle.sh
KIT="dist/sdk/delight-sdk-$SDK-$TARGET.tar.gz"
mkdir -p dist/sdk
cp dist/Delight.app/Contents/Resources/sdk-kit.tar.gz "$KIT"
echo "Packaged $KIT ($(du -h "$KIT" | cut -f1)) — $(dist/Delight.app/Contents/MacOS/Delight --sdk-build-id)"

./scripts/check-kit.sh "$KIT"
