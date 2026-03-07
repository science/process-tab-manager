#!/usr/bin/env bash
# Remove PTM binary, icon, and .desktop file
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"

rm -f "$PREFIX/bin/process-tab-manager"
rm -f "$PREFIX/share/icons/hicolor/scalable/apps/process-tab-manager.svg"
rm -f "$PREFIX/share/applications/process-tab-manager.desktop"
update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true

echo "Uninstalled from $PREFIX."
