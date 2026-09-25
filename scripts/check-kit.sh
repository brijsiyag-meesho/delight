#!/usr/bin/env bash
# Proves an SDK kit builds plugins the app accepts. Extracts the kit into its
# own directory, builds a standalone plugin inside it — the hello example, with
# the SDK as a plain crates.io dependency — and loads it with the app's loader.
#
#   scripts/check-kit.sh <kit.tar.gz> [app binary]   # default: dist/Delight.app
#
# The kit is built under target/kit-check/ (reused, so later runs are
# incremental): a different workspace location than this repo, like an
# author's machine.
set -euo pipefail
cd "$(dirname "$0")/.."
KIT=${1:?usage: $0 <kit.tar.gz> [app binary]}
APP=${2:-dist/Delight.app/Contents/MacOS/Delight}
SDK=$(tar -xzOf "$KIT" ./KIT.toml | sed -n 's/^sdk_version = "\(.*\)"$/\1/p')
WORK="$PWD/target/kit-check/$SDK"

# A fresh copy of the kit; keep its target/ for incremental builds.
mkdir -p "$WORK/kit"
find "$WORK/kit" -mindepth 1 -maxdepth 1 ! -name target -exec rm -rf {} +
tar -xzf "$KIT" -C "$WORK/kit"

# A standalone plugin project, outside the kit.
rm -rf "$WORK/plugin" && mkdir -p "$WORK/plugin/src"
cp examples/plugins/hello/src/lib.rs "$WORK/plugin/src/"
cat > "$WORK/plugin/Cargo.toml" <<EOF
[package]
name = "delight-plugin-kit-check"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["dylib"]

[dependencies]
delight-sdk = "=$SDK"
gpui = { package = "delight-gpui", version = "=$SDK" }
EOF
ln -sfn "$WORK/plugin" "$WORK/kit/plugins/kit-check"

echo "Building a standalone plugin with SDK kit ${SDK}…"
(cd "$WORK/kit" && cargo build --release -p delight-plugin-kit-check)
"$APP" --check-plugin "$WORK/kit/target/release/libdelight_plugin_kit_check.dylib"
echo "Kit $SDK builds plugins that $APP loads."
