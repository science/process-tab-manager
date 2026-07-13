#!/bin/bash
# Geometry round-trip: the sidebar's on-screen position must survive a
# graceful quit + relaunch under a reparenting WM.
#
# Regression guard for the frame-relative-coords bug: PTM used to persist
# ConfigureNotify x/y verbatim, but real (non-synthetic) ConfigureNotify
# coordinates are relative to the WM FRAME, not the root window — the
# geometry file ended up containing the frame's interior child offset
# (literally "10 44"), and every restart re-parked the window wherever the
# WM dropped that request. Under muffin, (10,44) is also exactly the
# stale-seed trigger for the titlebar-burial re-adoption bug, so drifting
# onto those coordinates had real consequences. The fix persists the
# visible frame's root origin (client root pos minus _NET_FRAME_EXTENTS),
# which is what a NorthWest-gravity ConfigureRequest positions on restore.
#
# Flow: launch ptm under openbox → move it to a distinctive position →
# graceful close (wmctrl -ic → _NET_CLOSE_WINDOW → WM_DELETE → shutdown
# save) → relaunch with the same HOME → assert the client window's
# absolute position is unchanged.

source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
e2e_require xdotool openbox wmctrl
e2e_start_xvfb
e2e_start_wm
e2e_mktemp_dir HOME_DIR ptm-e2e-geom.XXXXXX

e2e_launch_ptm "$HOME_DIR" /tmp/ptm-e2e-geom-1.log
sleep 0.8   # let openbox finish framing before we move

# Distinctive position so we aren't accidentally asserting a WM default.
xdotool windowmove --sync "$WID" 300 180
sleep 0.5

client_abs_pos() {
    xwininfo -id "$WID" | awk '/Absolute upper-left X/{x=$4} /Absolute upper-left Y/{y=$4} END{print x, y}'
}

POS1=$(client_abs_pos)
echo "[obs ] pre-restart client abs pos: $POS1"

# Graceful close so PTM's shutdown geometry save runs (kill would skip it).
wmctrl -i -c "$WID"
for i in {1..30}; do
    kill -0 "$PTM_PID" 2>/dev/null || break
    sleep 0.1
done
if kill -0 "$PTM_PID" 2>/dev/null; then
    echo "FAIL: ptm did not exit after WM_DELETE"
    exit 1
fi

GEOM_FILE="$HOME_DIR/.config/ptm/profiles/default/geometry"
echo "[obs ] saved geometry: $(cat "$GEOM_FILE" 2>/dev/null || echo '<missing>')"

e2e_launch_ptm "$HOME_DIR" /tmp/ptm-e2e-geom-2.log
sleep 0.8   # restore + reframe settle

POS2=$(client_abs_pos)
echo "[obs ] post-restart client abs pos: $POS2"

if [[ "$POS1" != "$POS2" ]]; then
    echo "FAIL: window position did not survive restart (before: '$POS1', after: '$POS2')"
    exit 1
fi

echo "PASS: geometry round-trips across restart ($POS1)"
