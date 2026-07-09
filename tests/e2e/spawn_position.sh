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

set -uo pipefail

DISPLAY_NUM=":99"
PTM="${PTM_BIN:-/tmp/ptm-dev/release/ptm}"
HOME_DIR=$(mktemp -d -t ptm-e2e-home.XXXXXX)
SHOTS=$(mktemp -d -t ptm-e2e-shots.XXXXXX)
# Per-test isolated tmux server socket — `+ New tmux` autonames sessions
# (0, 1, 2…) and we don't want those to outlive the test or leak into
# the user's real tmux server.
export TMUX_TMPDIR=$(mktemp -d -t ptm-e2e-tmux.XXXXXX)
# Claude Code and dev shells often run INSIDE tmux. A leaked $TMUX makes
# every tmux call here target the user's real server (TMUX_TMPDIR is
# ignored when $TMUX is set) -- cleanup's kill-server would then nuke all
# of the user's sessions. Sever the link before the first tmux command.
unset TMUX

cleanup() {
    [[ -n "${PTM_PID:-}" ]] && kill "$PTM_PID" 2>/dev/null || true
    [[ -n "${WM_PID:-}" ]] && kill "$WM_PID" 2>/dev/null || true
    [[ -n "${XVFB_PID:-}" ]] && kill "$XVFB_PID" 2>/dev/null || true
    pkill -f "$DISPLAY_NUM.*xterm" 2>/dev/null || true
    [[ -n "${TMUX_TMPDIR:-}" && -d "$TMUX_TMPDIR" ]] && tmux kill-server 2>/dev/null || true
    rm -rf "$HOME_DIR" "$TMUX_TMPDIR"
    wait 2>/dev/null || true
}
trap cleanup EXIT

if [[ ! -x "$PTM" ]]; then
    echo "FAIL: ptm binary not found at $PTM"; exit 2
fi
for tool in Xvfb xdotool tmux xterm xdpyinfo scrot openbox; do
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

# ptm reads `_NET_CLIENT_LIST` to enumerate windows; that property is only
# maintained by an EWMH window manager. Run openbox so the property is
# real; openbox decorates so we use window-relative xdotool clicks
# (mousemove --window) to address ptm's content area regardless of the
# frame offset. snap_to_sidebar's move_window goes through the WM as a
# ConfigureRequest, which openbox honours.
openbox --sm-disable >/dev/null 2>&1 &
WM_PID=$!
sleep 0.4

# Use xterm so we don't depend on gnome-terminal/DBus and the test stays
# deterministic. ptm reads PTM_TERMINAL_CMD when launching the attached
# terminal for "+ New tmux" / "+ New terminal".
export PTM_TERMINAL_CMD=xterm

HOME="$HOME_DIR" "$PTM" >/tmp/ptm-e2e-spawn.log 2>&1 &
PTM_PID=$!

# Wait for ptm window.
WID=""
for i in {1..30}; do
    WID=$(xdotool search --name "^ptm$" 2>/dev/null | head -1 || true)
    [[ -n "$WID" ]] && break
    sleep 0.1
done
[[ -z "$WID" ]] && { echo "FAIL: ptm window did not appear"; exit 1; }
echo "[setup] ptm window id: $WID"

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
