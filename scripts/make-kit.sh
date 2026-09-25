#!/usr/bin/env bash
# Packages the SDK kit: this workspace's source — the exact inputs of the
# app's libdelight_sdk.dylib — for building plugins that match it.
#
#   scripts/make-kit.sh <sdk build id> <out.tar.gz>
#
# Used by bundle.sh (the kit ships inside Delight.app for the `delight` CLI)
# and release-sdk.sh.
set -euo pipefail
cd "$(dirname "$0")/.."
BUILD_ID=${1:?usage: $0 <sdk build id> <out.tar.gz>}
OUT=${2:?usage: $0 <sdk build id> <out.tar.gz>}
SDK=$(sed -n 's/^version = "\(.*\)"$/\1/p' crates/sdk/Cargo.toml | head -1)
TARGET=$(rustc -vV | sed -n 's/^host: //p')

STAGE=$(mktemp -d "${TMPDIR:-/tmp}/delight-sdk-kit.XXXXXX")
trap 'rm -rf "$STAGE"' EXIT
rsync -a --exclude /target --exclude /dist --exclude '/plugins/*' --exclude .git --exclude .DS_Store ./ "$STAGE/"
mkdir -p "$STAGE/plugins" && touch "$STAGE/plugins/.keep"
cat > "$STAGE/KIT.toml" <<EOF
# Delight SDK kit: the workspace that built this SDK. Build plugins inside it
# (plugins/<name>) so they match the app's libdelight_sdk.dylib — the
# \`delight\` CLI does this for you.
sdk_version = "$SDK"
build_id = "$BUILD_ID"
target = "$TARGET"
EOF
mkdir -p "$(dirname "$OUT")"
tar -czf "$OUT" -C "$STAGE" .
