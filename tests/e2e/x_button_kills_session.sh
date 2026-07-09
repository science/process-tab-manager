#!/usr/bin/env bash
# E2E reproducer: clicking [x] on an orphan tmux session row should kill the
# session. Drives the actual ptm binary under Xvfb via xdotool, then asks the
# tmux server whether the session still exists.
#
# Pass: session is gone after popup-accept.
# Fail: session is still alive (the bug under investigation).
#
# Tools required: Xvfb, xdotool, tmux, scrot.

source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

SESSION="ptm_e2e_$$_ghost"
e2e_mktemp_dir HOME_DIR ptm-e2e-home.XXXXXX
# Plain mktemp (not e2e_mktemp_dir): screenshots survive for debugging.
SHOTS=$(mktemp -d -t ptm-e2e-shots.XXXXXX)

e2e_require xdotool tmux scrot
e2e_start_xvfb

# Stage: orphan tmux session on the isolated server (ptm reads from the
# same socket via the inherited TMUX_TMPDIR).
tmux kill-session -t "$SESSION" 2>/dev/null || true
tmux new-session -d -s "$SESSION"
tmux has-session -t "$SESSION" || { echo "FAIL: could not stage tmux session"; exit 2; }
echo "[setup] staged tmux session: $SESSION"

# Run ptm with a clean HOME so the user's saved groups state (collapse
# states, named groups) doesn't interfere with row positions.
e2e_launch_ptm "$HOME_DIR" /tmp/ptm-e2e.log

# Allow startup ensure_tmux_system_group + first paint to finish.
sleep 1.0

eval "$(xdotool getwindowgeometry --shell "$WID")"
echo "[setup] ptm geometry: X=$X Y=$Y W=$WIDTH H=$HEIGHT"

scrot "$SHOTS/01-fresh-start.png"
echo "[diag] fresh-start screenshot: $SHOTS/01-fresh-start.png"

# Layout constants from src/main.rs:10-31:
#   ITEM_MARGIN=8, ITEM_H=28, ITEM_SPACING=2, HEADER_H=28
#   ITEM_Y_START = HEADER_H + ITEM_SPACING + 8 = 38
#   GROUP_INDENT=16, SESSION_CLOSE_BAND_WIDTH=16, WIN_W=250
# Row N top = 38 + N*30; row N centre = 52 + N*30.
# Group header (row 0) is hit anywhere on its row.
# Session row in group: row_left = 8+16 = 24, row_w = (250-16)-16 = 218.
# Close band: local_x in [202,218); midpoint local_x=210; absolute window x=234.

# Step 1: expand the TmuxSystem group (it defaults to collapsed per
# ensure_tmux_system_group at src/main.rs:2213).
EXPAND_X=$((X + 125))
EXPAND_Y=$((Y + 52))
echo "[act ] click group header at ${EXPAND_X},${EXPAND_Y} to expand"
xdotool mousemove "$EXPAND_X" "$EXPAND_Y"
sleep 0.1
xdotool click 1
sleep 0.4

scrot "$SHOTS/02-after-expand.png"
echo "[diag] after-expand screenshot: $SHOTS/02-after-expand.png"

# Step 2: click the [x] glyph on row 1 (the session row).
CLICK_X=$((X + 234))
CLICK_Y=$((Y + 82))
echo "[act ] click [x] at window-relative 234,82 -> screen ${CLICK_X},${CLICK_Y}"

xdotool mousemove "$CLICK_X" "$CLICK_Y"
sleep 0.1
xdotool click 1
sleep 0.4

scrot "$SHOTS/03-after-x-click.png"
echo "[diag] after-x-click screenshot: $SHOTS/03-after-x-click.png"

# Did a popup appear? Count windows beyond root + ptm.
WIN_COUNT=$(xdotool search "" 2>/dev/null | wc -l)
echo "[diag] visible window count: $WIN_COUNT (expected >=3 if popup opened)"
xdotool search "" 2>/dev/null | while read -r w; do
    n=$(xdotool getwindowname "$w" 2>/dev/null || echo "?")
    g=$(xdotool getwindowgeometry --shell "$w" 2>/dev/null | grep -E '^(X|Y|WIDTH|HEIGHT)=' | tr '\n' ' ')
    echo "        wid=$w name='$n' $g"
done

# Accept popup. Popup grabs keyboard so the keypress reaches it.
xdotool key Return
sleep 0.6

scrot "$SHOTS/04-after-enter.png"
echo "[diag] after-enter screenshot: $SHOTS/04-after-enter.png"

# Verdict.
if tmux has-session -t "$SESSION" 2>/dev/null; then
    echo "FAIL: session $SESSION still exists after [x] -> popup -> Enter"
    echo "tmux ls:"; tmux ls 2>&1 | sed 's/^/  /'
    echo "ptm log tail:"; tail -20 /tmp/ptm-e2e.log 2>&1 | sed 's/^/  /'
    exit 1
fi
echo "PASS: session $SESSION killed"
exit 0
