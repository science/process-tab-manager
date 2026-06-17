#!/bin/bash
# Build Process Tab Manager.
#
#   ./build.sh            # same as "dev"
#   ./build.sh dev        # build for local iteration; run in place, nothing installed
#   ./build.sh release    # build, then install the binary onto persistent disk
#
# Why builds always land in /tmp (and never in this folder):
#   ~/dev is a virtiofs mount with noexec — a binary compiled into the repo
#   fails at runtime with "Bad address (os error 14)" (see fix-virtiofs-exec.md).
#   So cargo always targets /tmp (ext4, exec-capable), never the repo tree.
#   This is a constraint, not a mode — callers never choose it.
#
# Why "release" copies instead of symlinks:
#   /tmp is wiped on every reboot, so a symlink ~/.local/bin/ptm -> /tmp/...
#   dangles after a restart and PTM looks uninstalled. Copying the binary onto
#   ~/.local/bin (which is /dev/sda2 ext4 — persistent AND exec-capable) makes
#   the install survive reboots with no rebuild needed.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODE="${1:-dev}"

source "$HOME/.cargo/env"

build() {
    local target_dir="$1"
    echo "Building ptm (release profile) into $target_dir ..."
    CARGO_TARGET_DIR="$target_dir" cargo build --release \
        --manifest-path "$SCRIPT_DIR/Cargo.toml"
}

case "$MODE" in
    dev)
        # Separate target dir from release so a dev rebuild never clobbers the
        # artifact a concurrent `release` build copies from.
        TARGET_DIR=/tmp/ptm-dev
        build "$TARGET_DIR"
        echo
        echo "Dev binary: $TARGET_DIR/release/ptm"
        echo "Run it with: DISPLAY=:0 $TARGET_DIR/release/ptm"
        ;;
    release|prod|production|install)
        TARGET_DIR=/tmp/ptm-target
        DEST="$HOME/.local/bin/ptm"
        build "$TARGET_DIR"
        mkdir -p "$(dirname "$DEST")"
        # Drop any prior install first. Older installs left $DEST as a symlink
        # into /tmp; writing through that symlink would recreate the file in
        # /tmp and keep the (reboot-fragile) link. rm guarantees a fresh copy.
        rm -f "$DEST"
        # install(1): copy + chmod 755 atomically. A real file on persistent
        # ext4 — NOT a symlink into the soon-to-be-wiped /tmp.
        install -m 755 "$TARGET_DIR/release/ptm" "$DEST"
        echo
        echo "Installed binary: $DEST"
        echo "  (copied from $TARGET_DIR/release/ptm; survives reboot)"
        ;;
    -h|--help|help)
        sed -n '2,6p' "$0" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *)
        echo "error: unknown mode '$MODE'" >&2
        echo "usage: $0 [dev|release]" >&2
        exit 2
        ;;
esac
