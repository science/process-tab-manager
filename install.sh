#!/bin/bash
# Build and install Process Tab Manager into ~/.local/bin via symlinks.
# Idempotent: skips if symlink already correct.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_DIR="$HOME/.local/bin"

# Runtime dependency: tmux is used to mark windows that are attached to a
# tmux session (green dot on the row). PTM runs without it — the marker is
# just inactive — but this machine will usually want it installed.
if ! command -v tmux >/dev/null 2>&1; then
    echo "tmux not found (used for session-marker detection)."
    if command -v apt-get >/dev/null 2>&1; then
        echo "Installing tmux via apt..."
        if ! sudo apt-get install -y tmux; then
            echo "warning: tmux install failed — session detection will be disabled"
        fi
    else
        echo "warning: no apt-get on PATH; install tmux manually to enable session detection"
    fi
fi

# Build release binary
source "$HOME/.cargo/env"
echo "Building ptm..."
CARGO_TARGET_DIR=/tmp/ptm-target cargo build --release --manifest-path "$SCRIPT_DIR/Cargo.toml"

# Symlink binary
mkdir -p "$INSTALL_DIR"
src="/tmp/ptm-target/release/ptm"
dst="$INSTALL_DIR/ptm"
if [[ -L "$dst" && "$(readlink "$dst")" == "$src" ]]; then
    echo "Binary symlink already correct"
else
    ln -sf "$src" "$dst"
    echo "Linked $dst -> $src"
fi

# Install icon
ICONS_DIR="$HOME/.local/share/icons"
mkdir -p "$ICONS_DIR"
src="$SCRIPT_DIR/ptm.svg"
dst="$ICONS_DIR/ptm.svg"
if [[ -L "$dst" && "$(readlink "$dst")" == "$src" ]]; then
    echo "Icon symlink already correct"
else
    ln -sf "$src" "$dst"
    echo "Linked $dst -> $src"
fi

# Generate desktop entry with resolved paths
APPS_DIR="$HOME/.local/share/applications"
mkdir -p "$APPS_DIR"
dst="$APPS_DIR/ptm.desktop"
rm -f "$dst"
sed -e "s|%INSTALL_DIR%|$INSTALL_DIR|g" -e "s|%ICONS_DIR%|$ICONS_DIR|g" \
    "$SCRIPT_DIR/ptm.desktop" > "$dst"
echo "Installed $dst"

echo "Done. Launch 'Process Tab Manager' from the app menu."
