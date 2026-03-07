#!/usr/bin/env bash
# Install PTM binary, icon, and .desktop file
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"

install -Dm755 target/release/process-tab-manager "$PREFIX/bin/process-tab-manager"
install -Dm644 assets/ptm.svg "$PREFIX/share/icons/hicolor/scalable/apps/process-tab-manager.svg"
install -Dm644 assets/process-tab-manager.desktop "$PREFIX/share/applications/process-tab-manager.desktop"
update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true

echo "Installed to $PREFIX. Ensure $PREFIX/bin is in PATH."
