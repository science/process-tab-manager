#!/usr/bin/env bash
# E2E reproducer: right-clicking a Normal group header → "New terminal"
# should spawn an xterm AND snap it to the sidebar anchor (immediately
# right of ptm), exactly the way "+ New tmux" does for tmux-attached
# spawns.
#
# Today the bare-terminal-in-group path lands at the WM's default
# position (typically overlapping the sidebar): RED.
#
# Pass: spawned xterm x ≥ sidebar's right edge (within a tiny slack).
# Fail: spawned xterm overlaps ptm.
#
# Tools required: Xvfb, xdotool, tmux, xterm, openbox, xdpyinfo, scrot.

set -uo pipefail

DISPLAY_NUM=":99"
PTM="${PTM_BIN:-/tmp/ptm-dev/release/ptm}"
HOME_DIR=$(mktemp -d -t ptm-e2e-grp-term.XXXXXX)
SHOTS=$(mktemp -d -t ptm-e2e-grp-term-shots.XXXXXX)
# Per-test isolated tmux server socket so leftover sessions can't
# contaminate row layout via the auto TmuxSystem group.
export TMUX_TMPDIR=$(mktemp -d -t ptm-e2e-grp-term-tmux.XXXXXX)
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

[[ -x "$PTM" ]] || { echo "FAIL: ptm binary not found at $PTM"; exit 2; }
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

# openbox maintains _NET_CLIENT_LIST and honours snap_to_sidebar's
# ConfigureRequest move — same setup as spawn_position.sh.
openbox --sm-disable >/dev/null 2>&1 &
WM_PID=$!
sleep 0.4

# Pre-seed a Normal group named "spawn-test" so we can right-click its
# header without first creating it via the UI. Restoration order is:
# pre-seeded groups (in file order) first, then ensure_tmux_system_group
# appends the auto TmuxSystem group, so spawn-test ends up at row 0.
mkdir -p "$HOME_DIR/.config/ptm/profiles/default"
GROUPS_FILE="$HOME_DIR/.config/ptm/profiles/default/groups"
{
    printf 'v2\n'
    printf 'GROUP\tspawn-test\t0\tnormal\n'
} > "$GROUPS_FILE"
echo "[setup] pre-seeded groups file:"
sed 's/^/    /' "$GROUPS_FILE"

export PTM_TERMINAL_CMD=xterm

HOME="$HOME_DIR" "$PTM" >/tmp/ptm-e2e-grp-term.log 2>&1 &
PTM_PID=$!

WID=""
for i in {1..30}; do
    WID=$(xdotool search --name "^ptm$" 2>/dev/null | head -1 || true)
    [[ -n "$WID" ]] && break
    sleep 0.1
done
[[ -z "$WID" ]] && {
    echo "FAIL: ptm window did not appear"
    tail -20 /tmp/ptm-e2e-grp-term.log 2>&1 | sed 's/^/  /'
    exit 1
}
echo "[setup] ptm window id: $WID"

sleep 0.8
eval "$(xdotool getwindowgeometry --shell "$WID")"
PTM_X=$X PTM_Y=$Y PTM_W=$WIDTH PTM_H=$HEIGHT
SIDEBAR_RIGHT=$((PTM_X + PTM_W))
echo "[setup] ptm geometry: X=$PTM_X Y=$PTM_Y W=$PTM_W H=$PTM_H sidebar_right=$SIDEBAR_RIGHT"

scrot "$SHOTS/01-before.png"

# Right-click the spawn-test header (row 0) using WINDOW-RELATIVE coords
# so the openbox title-bar offset doesn't shift our click into the wrong
# row (same technique spawn_position.sh uses). Row layout: ITEM_H=30,
# top buttons band ~22px, so row 0 centre is at window-y 52.
echo "[act ] right-click spawn-test header at window-relative 125,52"
xdotool mousemove --window "$WID" --sync 125 52; sleep 0.1
# Capture the resulting SCREEN coords; the context menu pops up at the
# right-click's root_xy, so we need real screen offsets for the menu-item
# click below.
eval "$(xdotool getmouselocation --shell)"
RC_X=$X RC_Y=$Y
echo "[diag] right-click landed at screen $RC_X,$RC_Y"
xdotool click 3; sleep 0.4
scrot "$SHOTS/02-menu-open.png"

# Menu layout for a Normal group header (4 entries): New terminal (index 0),
# New tmux (1), Rename Group (2), Delete Group (3). Per constants in
# src/main.rs (MENU_ITEM_H=24, MENU_PADDING=4, MENU_MIN_W=180), item index N
# centre is at: y = MENU_PADDING + N*MENU_ITEM_H + MENU_ITEM_H/2.
# Index 0 centre y = 4 + 0*24 + 12 = 16. Menu top-left anchored at the
# right-click root_xy (clamped to screen). Centre x = MENU_MIN_W/2 = 90.
NT_X=$((RC_X + 90))
NT_Y=$((RC_Y + 16))
echo "[act ] click 'New terminal' at $NT_X,$NT_Y"
xdotool mousemove "$NT_X" "$NT_Y"; sleep 0.1
xdotool click 1; sleep 0.3
scrot "$SHOTS/03-after-click.png"

# Wait for the spawned xterm to appear in _NET_CLIENT_LIST.
TERM_WID=""
for i in {1..40}; do
    TERM_WID=$(xdotool search --class "xterm" 2>/dev/null | head -1 || true)
    [[ -n "$TERM_WID" ]] && break
    sleep 0.1
done
if [[ -z "$TERM_WID" ]]; then
    echo "FAIL: no xterm window appeared after 'New terminal' click within 4s"
    echo "ptm log tail:"; tail -20 /tmp/ptm-e2e-grp-term.log 2>&1 | sed 's/^/  /'
    echo "windows now:"
    xdotool search "" 2>/dev/null | while read -r w; do
        n=$(xdotool getwindowname "$w" 2>/dev/null || echo "?")
        echo "  wid=$w name='$n'"
    done
    echo "screenshots: $SHOTS"
    exit 1
fi
echo "[diag] xterm appeared as wid=$TERM_WID"

# Let refresh_items run: it claims the pending attach, runs add_to_group
# for the target_group_id, and snaps the wid if claim.snap is true.
sleep 0.8
scrot "$SHOTS/04-after-spawn.png"

eval "$(xdotool getwindowgeometry --shell "$TERM_WID")"
TERM_X=$X TERM_Y=$Y TERM_W=$WIDTH TERM_H=$HEIGHT
echo "[diag] xterm geometry: X=$TERM_X Y=$TERM_Y W=$TERM_W H=$TERM_H"

# Verdict: same as spawn_position.sh — the spawned terminal must sit at
# the sidebar anchor (left edge ≥ sidebar right edge, with 4px slack
# for any off-by-one in frame extents).
if (( TERM_X < SIDEBAR_RIGHT - 4 )); then
    echo "FAIL: spawned xterm at x=$TERM_X overlaps the sidebar (right edge $SIDEBAR_RIGHT)"
    echo "      expected x >= $SIDEBAR_RIGHT (sidebar-anchored, same as '+ New tmux')"
    echo "      screenshots: $SHOTS"
    exit 1
fi
echo "PASS: spawned xterm at x=$TERM_X is at the sidebar anchor"
exit 0
