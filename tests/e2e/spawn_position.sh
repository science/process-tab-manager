#!/usr/bin/env bash
# E2E reproducer: clicking "+ New tmux" should spawn the attached terminal
# at the sidebar anchor (immediately right of ptm's sidebar window), not
# at the WM/X server's default position. Same expectation applies to
# "+ New terminal".
#
# Pass: spawned terminal x position is at-or-right-of the sidebar's right edge.
# Fail: terminal is positioned elsewhere (typically overlapping ptm).
#
# Tools required: Xvfb, xdotool, tmux, xterm, xdpyinfo, scrot.

source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

e2e_mktemp_dir HOME_DIR ptm-e2e-home.XXXXXX
# Plain mktemp (not e2e_mktemp_dir): screenshots survive for debugging.
SHOTS=$(mktemp -d -t ptm-e2e-shots.XXXXXX)

e2e_require xdotool tmux xterm scrot openbox
e2e_start_xvfb
# ptm reads `_NET_CLIENT_LIST` to enumerate windows; that property is only
# maintained by an EWMH window manager — see e2e_start_wm's doc.
# snap_to_sidebar's move_window goes through the WM as a ConfigureRequest,
# which openbox honours.
e2e_start_wm

# Use xterm so we don't depend on gnome-terminal/DBus and the test stays
# deterministic. ptm reads PTM_TERMINAL_CMD when launching the attached
# terminal for "+ New tmux" / "+ New terminal".
export PTM_TERMINAL_CMD=xterm

e2e_launch_ptm "$HOME_DIR" /tmp/ptm-e2e-spawn.log

sleep 0.6  # let initial paint finish
eval "$(xdotool getwindowgeometry --shell "$WID")"
PTM_X=$X PTM_Y=$Y PTM_W=$WIDTH PTM_H=$HEIGHT
echo "[setup] ptm geometry: X=$PTM_X Y=$PTM_Y W=$PTM_W H=$PTM_H"
SIDEBAR_RIGHT=$((PTM_X + PTM_W))
echo "[setup] sidebar right edge: $SIDEBAR_RIGHT"

scrot "$SHOTS/01-before-click.png"

# Snapshot windows BEFORE the click so we can subtract to identify the new one.
INITIAL_WIDS=$(xdotool search "" 2>/dev/null | sort -u)
echo "[diag] windows before click: $(echo "$INITIAL_WIDS" | wc -l)"

# Click "+ New tmux" — right half of the top header row, addressed
# via window-relative mousemove so the openbox frame offset doesn't
# matter. Layout (constants from src/main.rs:10-26):
#   WIN_W=250, ITEM_MARGIN=8, TOP_BUTTON_GAP=4
#   total_w = 250 - 16 = 234; half = (234 - 4)/2 = 115
#   right_button_x = 8 + 115 + 4 = 127, right_button_w = 115
#   centre x = 127 + 115/2 = 184; centre y = 4 + ITEM_H/2 = 4 + 14 = 18
echo "[act ] click '+ New tmux' at window-relative 184,18"
xdotool mousemove --window "$WID" --sync 184 18; sleep 0.1
xdotool click 1

# Wait for an xterm to appear (give tmux + xterm spawn time).
TERM_WID=""
for i in {1..40}; do
    TERM_WID=$(xdotool search --class "xterm" 2>/dev/null | head -1 || true)
    [[ -n "$TERM_WID" ]] && break
    sleep 0.1
done
if [[ -z "$TERM_WID" ]]; then
    echo "FAIL: no xterm window appeared after '+ New tmux' click within 4s"
    echo "ptm log tail:"; tail -20 /tmp/ptm-e2e-spawn.log 2>&1 | sed 's/^/  /'
    echo "windows now:"
    xdotool search "" 2>/dev/null | while read -r w; do
        n=$(xdotool getwindowname "$w" 2>/dev/null || echo "?")
        echo "  wid=$w name='$n'"
    done
    exit 1
fi
echo "[diag] xterm appeared as wid=$TERM_WID"

# openbox maintains _NET_CLIENT_LIST when xterm maps; ptm reacts via its
# PropertyNotify subscription on root, runs refresh_items, claims the
# new wid via pending_spawn, and snaps it.
sleep 0.8
scrot "$SHOTS/02-after-spawn.png"

eval "$(xdotool getwindowgeometry --shell "$TERM_WID")"
TERM_X=$X TERM_Y=$Y TERM_W=$WIDTH TERM_H=$HEIGHT
echo "[diag] xterm geometry: X=$TERM_X Y=$TERM_Y W=$TERM_W H=$TERM_H"

# Verdict: the spawned terminal should sit at the sidebar anchor — i.e. its
# left edge should be at-or-right-of the sidebar's right edge. Allow a tiny
# slack for off-by-one rounding.
if (( TERM_X < SIDEBAR_RIGHT - 4 )); then
    echo "FAIL: spawned xterm at x=$TERM_X overlaps the sidebar (right edge $SIDEBAR_RIGHT)"
    echo "      expected x >= $SIDEBAR_RIGHT (sidebar-anchored)"
    echo "      screenshots: $SHOTS"
    exit 1
fi
echo "PASS: spawned xterm at x=$TERM_X is at the sidebar anchor"
exit 0
