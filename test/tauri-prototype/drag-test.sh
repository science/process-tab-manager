#!/bin/bash
# Targeted drag-and-drop test
# Tests whether HTML5 DnD fires from synthetic events in WebKitGTK
# Also tests pointer-event-based DnD as fallback

set -uo pipefail

SCREENSHOT_DIR="/mnt/host-dev/process-tab-manager/test/screenshots/tauri"

screenshot() {
    DISPLAY=:0 import -window root "$SCREENSHOT_DIR/drag-$1.png" 2>/dev/null
}

# Find the window
WID=$(DISPLAY=:0 xdotool search --name "PTM Prototype" 2>/dev/null | head -1)
if [[ -z "$WID" ]]; then
    echo "FATAL: Start the prototype first"
    echo "  DISPLAY=:0 ./src-tauri/target/release/tauri-prototype &"
    exit 1
fi

GEO=$(DISPLAY=:0 xdotool getwindowgeometry "$WID")
WIN_X=$(echo "$GEO" | grep -oP 'Position: \K\d+')
WIN_Y=$(echo "$GEO" | grep -oP ',\K\d+(?= )')
echo "Window at: ${WIN_X},${WIN_Y}"

DISPLAY=:0 xdotool windowactivate "$WID"
sleep 0.5

# Item positions
ITEM_X=$((WIN_X + 100))
ITEM1_Y=$((WIN_Y + 55))
ITEM3_Y=$((WIN_Y + 125))

echo ""
echo "=== Test A: xdotool HTML5 DnD — slow drag with large distance ==="
screenshot "A0-before"

DISPLAY=:0 xdotool mousemove "$ITEM_X" "$ITEM1_Y"
sleep 0.5

# Press and hold
DISPLAY=:0 xdotool mousedown 1
sleep 0.3

# Move slowly over a large distance (120px, should be well above drag threshold)
for step in $(seq 1 20); do
    DISPLAY=:0 xdotool mousemove "$ITEM_X" "$((ITEM1_Y + step * 6))"
    sleep 0.05
done

sleep 0.5
DISPLAY=:0 xdotool mouseup 1
sleep 0.5
screenshot "A1-after-slow-drag"
echo "  Check drag-A1-after-slow-drag.png for list reorder + event log"

echo ""
echo "=== Test B: xdotool DnD — fast drag ==="
# Click first item to reset selection
DISPLAY=:0 xdotool mousemove "$ITEM_X" "$ITEM1_Y"
sleep 0.2
DISPLAY=:0 xdotool click 1
sleep 0.3

DISPLAY=:0 xdotool mousemove "$ITEM_X" "$ITEM1_Y"
sleep 0.2
DISPLAY=:0 xdotool mousedown 1
sleep 0.1

# Single fast jump
DISPLAY=:0 xdotool mousemove --sync "$ITEM_X" "$ITEM3_Y"
sleep 0.5

DISPLAY=:0 xdotool mouseup 1
sleep 0.5
screenshot "B1-after-fast-drag"
echo "  Check drag-B1-after-fast-drag.png"

echo ""
echo "=== Test C: ydotool DnD ==="
# Position with xdotool first
DISPLAY=:0 xdotool mousemove "$ITEM_X" "$ITEM1_Y"
sleep 0.3

# ydotool mousedown + relative movement + mouseup
sudo ydotool mousedown 1 2>/dev/null || { echo "  ydotool mousedown not available"; exit 0; }
sleep 0.3

# Move mouse down 100px relative
sudo ydotool mousemove -- 0 100 2>/dev/null || sudo ydotool mousemove 0 100 2>/dev/null || true
sleep 0.3

sudo ydotool mouseup 1 2>/dev/null || true
sleep 0.5
screenshot "C1-after-ydotool-drag"
echo "  Check drag-C1-after-ydotool-drag.png"

echo ""
echo "=== Test D: xdotool with xdotool mousemove --window ==="
# Try drag with window-relative coordinates
DISPLAY=:0 xdotool mousemove --window "$WID" 100 55
sleep 0.3
DISPLAY=:0 xdotool mousedown 1
sleep 0.3

for step in $(seq 1 15); do
    DISPLAY=:0 xdotool mousemove --window "$WID" 100 "$((55 + step * 7))"
    sleep 0.05
done
sleep 0.3
DISPLAY=:0 xdotool mouseup 1
sleep 0.5
screenshot "D1-after-window-relative-drag"
echo "  Check drag-D1-after-window-relative-drag.png"

echo ""
echo "=== Final screenshot ==="
screenshot "final-drag-test"
echo "Done. Check screenshots for dragstart/drop events in the event log."
echo ""
echo "If HTML5 DnD doesn't fire from synthetic events, alternatives:"
echo "  1. Use SortableJS (pointer-event based, not HTML5 DnD)"
echo "  2. Custom mousedown/mousemove/mouseup reorder (no HTML5 DnD API)"
echo "  3. Keyboard reorder (Alt+Up/Down) — always works"
