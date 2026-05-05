#!/bin/bash
# Build and install Process Tab Manager into ~/.local/ via symlinks.
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

# Install icon into the freedesktop hicolor theme so Icon=ptm in the
# .desktop file resolves by name (the standard pattern). We also keep the
# legacy ~/.local/share/icons/ptm.svg path for any tools that bypass the
# theme.
THEME_ICON_DIR="$HOME/.local/share/icons/hicolor/scalable/apps"
mkdir -p "$THEME_ICON_DIR"
src="$SCRIPT_DIR/ptm.svg"
dst="$THEME_ICON_DIR/ptm.svg"
if [[ -L "$dst" && "$(readlink "$dst")" == "$src" ]]; then
    echo "Icon (theme) symlink already correct"
else
    ln -sf "$src" "$dst"
    echo "Linked $dst -> $src"
fi
LEGACY_ICONS_DIR="$HOME/.local/share/icons"
dst="$LEGACY_ICONS_DIR/ptm.svg"
if [[ -L "$dst" && "$(readlink "$dst")" == "$src" ]]; then
    echo "Icon (legacy) symlink already correct"
else
    ln -sf "$src" "$dst"
    echo "Linked $dst -> $src"
fi

# Refresh the icon cache so DEs that consult it (GTK-based menus) pick up
# the new ptm icon without needing a full re-login.
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    # -t = update mtime even if no changes; -f = force; -q = quiet on success.
    gtk-update-icon-cache -t -f -q "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
fi

# Generate desktop entry with the bin path resolved. (Icon stays as the
# bare name "ptm" — themed lookup; INSTALL_DIR is interpolated for Exec.)
APPS_DIR="$HOME/.local/share/applications"
mkdir -p "$APPS_DIR"
dst="$APPS_DIR/ptm.desktop"
rm -f "$dst"
sed -e "s|%INSTALL_DIR%|$INSTALL_DIR|g" \
    "$SCRIPT_DIR/ptm.desktop" > "$dst"
# Some launchers (Nautilus, GNOME Files) require executable bit on
# .desktop files in user dirs. Cinnamon/MATE generally don't, but it
# doesn't hurt and aligns with the freedesktop convention.
chmod +x "$dst"
echo "Installed $dst"

# Tell the freedesktop database about the new entry. Without this, some
# panel launchers' "Add to panel..." dialogs won't list the app even
# though the menu does.
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APPS_DIR" 2>/dev/null || true
fi

# Optional: validate the installed entry. Non-fatal — just warn so the
# user gets early feedback if something looks off.
if command -v desktop-file-validate >/dev/null 2>&1; then
    if ! desktop-file-validate "$dst"; then
        echo "warning: desktop-file-validate flagged issues with $dst"
    fi
fi

echo
echo "Done."
echo
echo "PTM should now appear in your application menu under 'Utility' and"
echo "be searchable as 'Process Tab Manager' or 'ptm'."
echo
echo "If your desktop environment is Cinnamon and PTM doesn't appear yet,"
echo "the panel menu sometimes caches its app list. Either:"
echo "  - Right-click the menu icon → 'Reload menu', OR"
echo "  - Log out and back in."
echo
echo "On GNOME the menu picks up new entries automatically."
