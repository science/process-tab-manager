#!/bin/bash
# Launch PTM for interactive use (local X11 desktop).
# Handles: prerequisite check, build, launch, and opens test xterms.
#
# Idempotent — safe to run repeatedly. Cleans up stale PTM/xterm processes
# and avoids accumulating duplicate xterms.
#
# Usage: ./run.sh

set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT="$SCRIPT_DIR"
BINARY="$PROJECT/target/release/process-tab-manager"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BOLD='\033[1m'
NC='\033[0m'

step() { echo -e "\n${BOLD}==> $1${NC}"; }
ok()   { echo -e "    ${GREEN}$1${NC}"; }
warn() { echo -e "    ${YELLOW}$1${NC}"; }
fail() { echo -e "    ${RED}$1${NC}" >&2; }

# ── 1. Check display ──

step "Checking display..."
if DISPLAY=:0 xdpyinfo >/dev/null 2>&1; then
    ok "X11 display :0 is available"
else
    fail "No X11 display available (DISPLAY=:0)"
    exit 1
fi

# ── 2. Check prerequisites ──

step "Checking prerequisites..."
source "$HOME/.cargo/env" 2>/dev/null || true

if ! command -v cargo >/dev/null 2>&1; then
    fail "Rust/Cargo not found. Install with: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi
ok "Rust $(rustc --version | awk '{print $2}')"

MISSING=""
for pkg in libwebkit2gtk-4.1-dev xdotool xterm imagemagick; do
    if ! dpkg -l "$pkg" 2>/dev/null | grep -q ^ii; then
        MISSING="$MISSING $pkg"
    fi
done
if [[ -n "$MISSING" ]]; then
    warn "Installing missing packages:$MISSING"
    sudo apt-get install -y $MISSING
fi
ok "System packages OK"

# ── 3. Build ──

step "Building PTM..."
cd "$PROJECT" && cargo build -p process-tab-manager --release 2>&1
ok "Build complete"

# ── 4. Kill existing PTM/xterms, launch fresh ──

step "Launching PTM..."
pkill -f 'release/process-tab-manager' 2>/dev/null || true
pkill xterm 2>/dev/null || true
sleep 0.5

DISPLAY=:0 RUST_LOG=info nohup "$BINARY" > /tmp/ptm.log 2>&1 &
sleep 2

if DISPLAY=:0 xdotool search --name 'Process Tab Manager' 2>/dev/null | grep -q .; then
    ok "PTM is running"
else
    warn "PTM window not detected (may still be starting)"
fi

# ── 5. Open test xterms (only if none exist) ──

step "Opening test xterms..."
existing=$(pgrep -c xterm 2>/dev/null || echo "0")
if [[ "$existing" -ge 3 ]]; then
    ok "$existing xterms already running — skipping"
else
    for i in 1 2 3; do
        DISPLAY=:0 xterm -title "xterm-$i" &
    done
    sleep 1
    ok "3 xterms opened"
fi

echo ""
echo -e "${GREEN}${BOLD}Ready!${NC} PTM is running with test xterm windows."
echo ""
echo "Things to try:"
echo "  - Click a row to activate that window"
echo "  - F2 to rename, Delete to hide"
echo "  - Right-click a row for context menu"
echo "  - Drag rows to reorder"
echo "  - Ctrl+Shift+Up/Down to reorder via keyboard"
echo "  - Open/close xterms and watch the list update"
echo ""
echo "Logs: cat /tmp/ptm.log"
