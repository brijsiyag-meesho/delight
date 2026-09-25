#!/usr/bin/env bash
# Release the app without changing the plugin SDK: builds dist/Delight.app and
# proves a plugin built with this SDK version's kit still loads in it — so
# every installed plugin keeps working.
#
#   scripts/release-app.sh
#
# If the check fails, the release changed the SDK (its sources, GPUI or
# another dependency, the toolchain or the profile): bump crates/sdk's
# version and run scripts/release-sdk.sh instead.
set -euo pipefail
cd "$(dirname "$0")/.."
SDK=$(sed -n 's/^version = "\(.*\)"$/\1/p' crates/sdk/Cargo.toml | head -1)
TARGET=$(rustc -vV | sed -n 's/^host: //p')
KIT="dist/sdk/delight-sdk-$SDK-$TARGET.tar.gz"
[[ -f "$KIT" ]] || { echo "No kit for SDK $SDK ($KIT) — run scripts/release-sdk.sh first." >&2; exit 1; }

./scripts/bundle.sh
if ! ./scripts/check-kit.sh "$KIT"; then
    echo "This release changes the plugin SDK: bump crates/sdk's version and run scripts/release-sdk.sh." >&2
    exit 1
fi
