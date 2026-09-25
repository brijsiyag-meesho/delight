#!/usr/bin/env bash
# Installs (or updates) Delight and its `delight` plugin CLI:
#
#   curl -fsSL https://github.com/brijsiyag-meesho/delight/releases/latest/download/install.sh | bash
#
# Puts Delight.app in /Applications (~/Applications if that isn't writable) and
# links the CLI into /usr/local/bin (~/.local/bin if that isn't writable).
# DELIGHT_VERSION=0.1.0 installs that release instead of the latest.
set -euo pipefail

REPO=brijsiyag-meesho/delight
die() { echo "delight install: $*" >&2; exit 1; }
say() { echo "delight install: $*" >&2; }

[[ $(uname -s) == Darwin ]] || die "Delight is a macOS app"
[[ $(uname -m) == arm64 ]] || die "this release is for Apple silicon Macs only"

if [[ -n ${DELIGHT_VERSION:-} ]]; then
    BASE="https://github.com/$REPO/releases/download/v$DELIGHT_VERSION"
else
    BASE="https://github.com/$REPO/releases/latest/download"
fi

WORK=$(mktemp -d "${TMPDIR:-/tmp}/delight-install.XXXXXX")
MOUNT="$WORK/mount"
cleanup() {
    hdiutil detach -quiet "$MOUNT" 2>/dev/null || true
    rm -rf "$WORK"
}
trap cleanup EXIT

say "downloading Delight${DELIGHT_VERSION:+ $DELIGHT_VERSION}…"
curl -fL --progress-bar -o "$WORK/Delight.dmg" "$BASE/Delight-arm64.dmg"
mkdir -p "$MOUNT"
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$MOUNT" "$WORK/Delight.dmg"
[[ -d $MOUNT/Delight.app ]] || die "the download has no Delight.app"

APPS=/Applications
[[ -w $APPS ]] || { APPS=$HOME/Applications; mkdir -p "$APPS"; }

# Quit a running Delight before replacing it.
if pgrep -xq Delight; then
    say "quitting the running Delight"
    osascript -e 'quit app id "dev.delight.app"' >/dev/null 2>&1 || true
    for _ in $(seq 50); do pgrep -xq Delight || break; sleep 0.1; done
    pgrep -xq Delight && pkill -x Delight || true
fi
rm -rf "$APPS/Delight.app"
ditto "$MOUNT/Delight.app" "$APPS/Delight.app"
say "installed $APPS/Delight.app"

BIN=/usr/local/bin
if [[ ! -w $BIN ]]; then
    BIN=$HOME/.local/bin
    mkdir -p "$BIN"
fi
ln -sfn "$APPS/Delight.app/Contents/Resources/bin/delight" "$BIN/delight"
say "linked the CLI: $BIN/delight"
case ":$PATH:" in
    *":$BIN:"*) ;;
    *) say "add $BIN to your PATH to run \`delight\` (e.g. echo 'export PATH=\"$BIN:\$PATH\"' >> ~/.zshrc)" ;;
esac

open "$APPS/Delight.app"
say "done — press ⌘⇧Space. Build a plugin: delight new my-tool && cd my-tool && delight install --restart"
