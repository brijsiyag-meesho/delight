#!/usr/bin/env bash
# Build a plugin of this workspace and install it into Delight's plugins folder.
#
#   scripts/install-plugin.sh delight-plugin-ab            # for `cargo run` (debug)
#   scripts/install-plugin.sh delight-plugin-ab --release  # for the bundled app
#
# Plugins must be built here — in Delight's workspace, with the app's profile —
# to link against the exact SDK build the app loads. Restart Delight afterwards.
set -euo pipefail
cd "$(dirname "$0")/.."
PACKAGE=${1:?usage: $0 <plugin package> [--release]}
PROFILE=debug DIR=plugins-debug FLAGS=()
if [[ "${2:-}" == "--release" ]]; then PROFILE=release DIR=plugins FLAGS=(--release); fi
# `${FLAGS[@]+…}`: bash 3.2 (macOS) treats an empty array as unset under `set -u`.
cargo build ${FLAGS[@]+"${FLAGS[@]}"} -p "$PACKAGE"
LIB="target/$PROFILE/lib${PACKAGE//-/_}.dylib"
DEST="$HOME/Library/Application Support/Delight/$DIR"
mkdir -p "$DEST"
# Replace, never overwrite in place: macOS caches a loaded dylib's code signature
# per file, and a process that dlopens the rewritten file gets SIGKILLed
# ("Code Signature Invalid"). Removing it first gives the copy a new file.
rm -f "$DEST/${PACKAGE#delight-plugin-}.dylib"
cp "$LIB" "$DEST/${PACKAGE#delight-plugin-}.dylib"
echo "Installed $DEST/${PACKAGE#delight-plugin-}.dylib — restart Delight (menu bar → Restart Delight)."
