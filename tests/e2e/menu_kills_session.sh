#!/usr/bin/env bash
# E2E control test: right-click -> Kill Session on an orphan tmux session row
# DOES kill the session (this is the path the user confirmed works manually).
# Companion to x_button_kills_session.sh which exercises the broken popup
# path. Both should pass once the popup-accept ordering bug is fixed; today
# this one passes and the [x]-button one fails.
#
# Tools required: Xvfb, xdotool, tmux, scrot.

set -uo pipefail

DISPLAY_NUM=":99"
SESSION="ptm_e2e_menu_$$_ghost"
PTM="${PTM_BIN:-/tmp/ptm-dev/release/ptm}"
HOME_DIR=$(mktemp -d -t ptm-e2e-home.XXXXXX)
SHOTS=$(mktemp -d -t ptm-e2e-shots.XXXXXX)
# Per-test isolated tmux server socket so leftover sessions from other
# scripts (or earlier failed runs) can't contaminate row layout.
export TMUX_TMPDIR=$(mktemp -d -t ptm-e2e-tmux.XXXXXX)

cleanup() {
    [[ -n "${PTM_PID:-}" ]] && kill "$PTM_PID" 2>/dev/null || true
    [[ -n "${XVFB_PID:-}" ]] && kill "$XVFB_PID" 2>/dev/null || true
    tmux kill-server 2>/dev/null || true
    rm -rf "$HOME_DIR" "$TMUX_TMPDIR"
    wait 2>/dev/null || true
}
trap cleanup EXIT

if [[ ! -x "$PTM" ]]; then
    echo "FAIL: ptm binary not found at $PTM"; exit 2
fi
for tool in Xvfb xdotool tmux xdpyinfo scrot; do
    command -v "$tool" >/dev/null || { echo "FAIL: missing $tool"; exit 2; }
done

Xvfb "$DISPLAY_NUM" -screen 0 1024x768x24 >/dev/null 2>&1 &
XVFB_PID=$!
for i in {1..20}; do
    DISPLAY="$DISPLAY_NUM" xdpyinfo >/dev/null 2>&1 && break
    sleep 0.1
done
DISPLAY="$DISPLAY_NUM" xdpyinfo >/dev/null 2>&1 || { echo "FAIL: Xvfb did not become responsive"; exit 2; }
export DISPLAY="$DISPLAY_NUM"

tmux kill-session -t "$SESSION" 2>/dev/null || true
tmux new-session -d -s "$SESSION"
echo "[setup] staged tmux session: $SESSION"

HOME="$HOME_DIR" "$PTM" >/tmp/ptm-e2e-menu.log 2>&1 &
PTM_PID=$!

WID=""
for i in {1..30}; do
    WID=$(xdotool search --name "^ptm$" 2>/dev/null | head -1 || true)
    [[ -n "$WID" ]] && break
    sleep 0.1
done
[[ -z "$WID" ]] && { echo "FAIL: ptm window did not appear"; exit 1; }
echo "[setup] ptm window id: $WID"

sleep 1.0
eval "$(xdotool getwindowgeometry --shell "$WID")"
echo "[setup] ptm geometry: X=$X Y=$Y W=$WIDTH H=$HEIGHT"

scrot "$SHOTS/01-fresh-start.png"

# Layout: same as x_button_kills_session.sh.
# Step 1: expand TmuxSystem group (defaults collapsed).
xdotool mousemove $((X + 125)) $((Y + 52)); sleep 0.1
xdotool click 1; sleep 0.4
scrot "$SHOTS/02-after-expand.png"

# Step 2: right-click in the middle of the session row (row 1) to open the
# context menu. Use x=125 (label area, well left of the [x] band at 226-242).
RC_X=$((X + 125))
RC_Y=$((Y + 82))
echo "[act ] right-click session row at ${RC_X},${RC_Y}"
xdotool mousemove "$RC_X" "$RC_Y"; sleep 0.1
xdotool click 3; sleep 0.4
scrot "$SHOTS/03-after-rightclick.png"

echo "[diag] windows after right-click:"
xdotool search "" 2>/dev/null | while read -r w; do
    n=$(xdotool getwindowname "$w" 2>/dev/null || echo "?")
    g=$(xdotool getwindowgeometry --shell "$w" 2>/dev/null | grep -E '^(X|Y|WIDTH|HEIGHT)=' | tr '\n' ' ')
    echo "        wid=$w name='$n' $g"
done

# Step 3: click "Kill Session" — third menu item for an orphan session row.
# Menu layout (constants from src/main.rs:19-21): MENU_ITEM_H=24, MENU_PADDING=4,
# MENU_MIN_W=180. Items render as: padding(4) + N*item_h(24) + item_h/2(12)
# vertical centre. Width = MENU_MIN_W = 180 (entries fit). Menu top-left
# anchored at the right-click root_xy (clamped to screen).
# Kill Session is item index 2 -> centre y = 4 + 2*24 + 12 = 64.
KILL_X=$((RC_X + 90))
KILL_Y=$((RC_Y + 64))
echo "[act ] click 'Kill Session' at ${KILL_X},${KILL_Y}"
xdotool mousemove "$KILL_X" "$KILL_Y"; sleep 0.1
xdotool click 1; sleep 0.6
scrot "$SHOTS/04-after-kill.png"

if tmux has-session -t "$SESSION" 2>/dev/null; then
    echo "FAIL: session $SESSION still exists after right-click -> Kill Session"
    echo "tmux ls:"; tmux ls 2>&1 | sed 's/^/  /'
    echo "screenshots: $SHOTS"
    exit 1
fi
echo "PASS: session $SESSION killed via right-click menu"
exit 0
