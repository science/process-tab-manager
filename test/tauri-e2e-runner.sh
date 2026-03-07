#!/usr/bin/env bash
# In-VM E2E test runner for Tauri-based Process Tab Manager
# Run directly in VM: bash /mnt/host-dev/process-tab-manager/test/tauri-e2e-runner.sh [test_name]

set -uo pipefail

# Support both direct local use and /mnt/host-dev path
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARY="$PROJECT/target/release/process-tab-manager"
SCREENSHOT_DIR="$PROJECT/test/screenshots/tauri-e2e"
EVENT_LOG="/tmp/ptm-events.log"
STATE_FILE="/tmp/ptm-test-state.json"
STATE_JSON_PATH="$HOME/.config/process-tab-manager/state.json"

PASS=0
FAIL=0
SKIP=0
PTM_PID=""
PTM_WID=""

mkdir -p "$SCREENSHOT_DIR"

# ─── Helpers ─────────────────────────────────────────────────────

result() {
    local name="$1" status="$2" detail="${3:-}"
    if [[ "$status" == "PASS" ]]; then
        echo "  ✓ $name"
        ((PASS++))
    elif [[ "$status" == "FAIL" ]]; then
        echo "  ✗ $name — $detail"
        ((FAIL++))
    else
        echo "  - $name (skipped: $detail)"
        ((SKIP++))
    fi
}

wait_for() {
    local desc="$1" cmd="$2" timeout="${3:-5}"
    local attempts=$((timeout * 10))
    for i in $(seq 1 "$attempts"); do
        if eval "$cmd" 2>/dev/null; then return 0; fi
        sleep 0.1
    done
    return 1
}

wait_for_event() {
    local pattern="$1" timeout="${2:-3}"
    wait_for "event $pattern" "[[ -f '$EVENT_LOG' ]] && grep -q '$pattern' '$EVENT_LOG'" "$timeout"
}

screenshot() {
    DISPLAY=:0 import -window root "$SCREENSHOT_DIR/$1.png" 2>/dev/null
}

clean_logs() {
    rm -f "$EVENT_LOG" "$STATE_FILE"
}

start_ptm() {
    local keep_state="${1:-}"
    pkill -f "release/process-tab-manager" 2>/dev/null || true
    sleep 0.5
    clean_logs
    if [[ "$keep_state" != "keep_state" ]]; then
        rm -f "$STATE_JSON_PATH"  # Start fresh
    fi

    RUST_LOG=info DISPLAY=:0 "$BINARY" > /tmp/ptm-e2e.log 2>&1 &
    PTM_PID=$!

    # Wait for window
    if ! wait_for "PTM window" "DISPLAY=:0 xdotool search --name 'Process Tab Manager' 2>/dev/null | head -1 | grep -q ." 15; then
        echo "FATAL: PTM window not found"
        return 1
    fi
    PTM_WID=$(DISPLAY=:0 xdotool search --name "Process Tab Manager" 2>/dev/null | head -1)

    # Wait for event log (proves frontend + Tauri IPC working)
    if ! wait_for_event "init" 10; then
        echo "FATAL: Event log not working"
        return 1
    fi

    focus_ptm
}

stop_ptm() {
    # Try graceful close first (triggers Tauri CloseRequested → saves state)
    if [[ -n "$PTM_WID" ]]; then
        DISPLAY=:0 xdotool windowactivate "$PTM_WID" 2>/dev/null || true
        sleep 0.2
        DISPLAY=:0 xdotool windowclose "$PTM_WID" 2>/dev/null || true
    fi
    # Wait for graceful exit
    if [[ -n "$PTM_PID" ]]; then
        for i in $(seq 1 20); do
            kill -0 "$PTM_PID" 2>/dev/null || break
            sleep 0.1
        done
        # Force kill if still running
        kill "$PTM_PID" 2>/dev/null || true
        wait "$PTM_PID" 2>/dev/null || true
        PTM_PID=""
    fi
    PTM_WID=""
    pkill -f "release/process-tab-manager" 2>/dev/null || true
    sleep 0.3
}

open_xterms() {
    local count="${1:-3}"
    for i in $(seq 1 "$count"); do
        DISPLAY=:0 xterm -title "TestXterm$i" &
    done
    sleep 1
}

close_xterms() {
    pkill -f "xterm.*TestXterm" 2>/dev/null || true
    pkill xterm 2>/dev/null || true
    sleep 0.5
}

get_window_geometry() {
    DISPLAY=:0 xdotool getwindowgeometry "$1" 2>/dev/null
}


# Ensure PTM window is raised, active, and has DOM focus.
# Clicking an item in PTM activates that window, stealing X11 focus.
# This helper restores focus to PTM and clicks empty space to ensure
# WebKitGTK routes keyboard events to the DOM.
focus_ptm() {
    DISPLAY=:0 xdotool windowraise "$PTM_WID" 2>/dev/null || true
    DISPLAY=:0 xdotool windowactivate --sync "$PTM_WID" 2>/dev/null || true
    DISPLAY=:0 xdotool windowfocus --sync "$PTM_WID" 2>/dev/null || true
    sleep 0.1
    # Click near the bottom of PTM (empty area below items) to establish
    # DOM keyboard focus without changing the selection.
    local geo; geo=$(DISPLAY=:0 xdotool getwindowgeometry "$PTM_WID" 2>/dev/null)
    if [[ -n "$geo" ]]; then
        local wx; wx=$(echo "$geo" | grep -oP 'Position: \K\d+')
        local wy; wy=$(echo "$geo" | grep -oP ',\K\d+(?= )')
        local wh; wh=$(echo "$geo" | grep -oP 'x\K\d+$')
        # Click in lower portion of content area — below any item rows but
        # well above the resize handle at the bottom edge
        DISPLAY=:0 xdotool mousemove "$((wx + 50))" "$((wy + 400))" 2>/dev/null
        sleep 0.1
        DISPLAY=:0 xdotool click 1
    fi
    sleep 0.3
    # Verify PTM is still the active window; if not, try once more
    local active; active=$(DISPLAY=:0 xdotool getactivewindow 2>/dev/null) || active=""
    if [[ "$active" != "$PTM_WID" ]]; then
        DISPLAY=:0 xdotool windowactivate --sync "$PTM_WID" 2>/dev/null || true
        sleep 0.2
    fi
}

# Send a key to PTM, ensuring it has focus first.
# Usage: send_key F2   or   send_key ctrl+shift+Down
send_key() {
    DISPLAY=:0 xdotool windowactivate --sync "$PTM_WID" 2>/dev/null || true
    DISPLAY=:0 xdotool windowfocus --sync "$PTM_WID" 2>/dev/null || true
    sleep 0.1
    DISPLAY=:0 xdotool key --clearmodifiers "$1"
}

# ─── Tests ───────────────────────────────────────────────────────

test_launch_and_list() {
    echo "--- test_launch_and_list ---"
    close_xterms
    open_xterms 3
    start_ptm || return 1

    # Wait for sidebar to populate (safety timer polls every 1s, then emit + state write)
    if ! wait_for "sidebar items" "[[ -f '$STATE_FILE' ]] && python3 -c \"import json; s=json.load(open('$STATE_FILE')); exit(0 if len([i for i in s['items'] if i.get('kind')=='window']) >= 3 else 1)\"" 15; then
        true  # Fall through to check
    fi

    # Check state file has items
    if [[ -f "$STATE_FILE" ]]; then
        local count
        count=$(python3 -c "
import json
with open('$STATE_FILE') as f:
    s = json.load(f)
print(len([i for i in s['items'] if i.get('kind') == 'window']))
" 2>/dev/null) || count=0
        if [[ "$count" -ge 3 ]]; then
            result "PTM lists xterm windows ($count found)" "PASS"
        else
            result "PTM lists xterm windows" "FAIL" "only $count windows in state"
        fi
    else
        result "PTM lists xterm windows" "FAIL" "state file not created"
    fi

    screenshot "launch-and-list"
    stop_ptm
    close_xterms
}

test_click_focus() {
    echo "--- test_click_focus ---"
    close_xterms
    open_xterms 2
    start_ptm || return 1

    # Wait for items to appear
    wait_for "sidebar items" "[[ -f '$STATE_FILE' ]] && python3 -c \"import json; s=json.load(open('$STATE_FILE')); exit(0 if len([i for i in s['items'] if i.get('kind')=='window']) >= 2 else 1)\"" 15 || true

    clean_logs

    # Get PTM geometry for clicking
    local geo
    geo=$(get_window_geometry "$PTM_WID")
    local win_x; win_x=$(echo "$geo" | grep -oP 'Position: \K\d+')
    local win_y; win_y=$(echo "$geo" | grep -oP ',\K\d+(?= )')

    # Click first item
    local item_x=$((win_x + 100))
    local item_y=$((win_y + 15))

    focus_ptm
    DISPLAY=:0 xdotool mousemove "$item_x" "$item_y"
    sleep 0.2
    DISPLAY=:0 xdotool click 1
    sleep 0.5

    # Single-click should select + dispatch activate_window (both in same handler)
    if wait_for_event "click" 3; then
        result "Single-click received (activates window)" "PASS"
    else
        result "Single-click received (activates window)" "FAIL" "no click in event log"
    fi

    # Verify selection was set in state (proves the handler executed fully)
    if [[ -f "$STATE_FILE" ]]; then
        local sel_wid
        sel_wid=$(python3 -c "
import json
with open('$STATE_FILE') as f:
    s = json.load(f)
print(s.get('selectedWid', ''))
" 2>/dev/null) || sel_wid=""
        if [[ -n "$sel_wid" && "$sel_wid" != "None" ]]; then
            result "Click sets selectedWid ($sel_wid)" "PASS"
        else
            result "Click sets selectedWid" "FAIL" "selectedWid is empty"
        fi
    else
        result "Click sets selectedWid" "FAIL" "no state file"
    fi

    screenshot "click-focus"
    stop_ptm
    close_xterms
}

test_self_filter() {
    echo "--- test_self_filter ---"
    close_xterms
    open_xterms 2
    start_ptm || return 1

    # Wait for items to appear
    wait_for "sidebar items" "[[ -f '$STATE_FILE' ]] && python3 -c \"import json; s=json.load(open('$STATE_FILE')); exit(0 if len([i for i in s['items'] if i.get('kind')=='window']) >= 2 else 1)\"" 15 || true

    # Check that PTM's own window is not in the list
    if [[ -f "$STATE_FILE" ]]; then
        local has_ptm
        has_ptm=$(python3 -c "
import json
with open('$STATE_FILE') as f:
    s = json.load(f)
ptm = [i for i in s['items'] if 'Process Tab Manager' in i.get('title', '')]
print(len(ptm))
" 2>/dev/null) || has_ptm=0
        if [[ "$has_ptm" -eq 0 ]]; then
            result "PTM not in own list" "PASS"
        else
            result "PTM not in own list" "FAIL" "found PTM in sidebar items"
        fi
    else
        result "PTM not in own list" "FAIL" "no state file"
    fi

    stop_ptm
    close_xterms
}

test_f2_rename() {
    echo "--- test_f2_rename ---"
    close_xterms
    open_xterms 1
    start_ptm || return 1

    wait_for "sidebar items" "[[ -f '$STATE_FILE' ]] && python3 -c \"import json; s=json.load(open('$STATE_FILE')); exit(0 if len([i for i in s['items'] if i.get('kind')=='window']) >= 1 else 1)\"" 15 || true

    local geo; geo=$(get_window_geometry "$PTM_WID")
    local win_x; win_x=$(echo "$geo" | grep -oP 'Position: \K\d+')
    local win_y; win_y=$(echo "$geo" | grep -oP ',\K\d+(?= )')
    local item_x=$((win_x + 100))
    local item_y=$((win_y + 15))

    # Select first item (click activates the target window, stealing focus)
    focus_ptm
    DISPLAY=:0 xdotool mousemove "$item_x" "$item_y"
    sleep 0.2
    DISPLAY=:0 xdotool click 1
    sleep 0.5

    # Verify item was selected before continuing
    if ! wait_for "item selected" "[[ -f '$STATE_FILE' ]] && python3 -c \"import json; s=json.load(open('$STATE_FILE')); exit(0 if s.get('selectedWid') else 1)\"" 5; then
        # Retry click if selection didn't register
        focus_ptm
        DISPLAY=:0 xdotool mousemove "$item_x" "$item_y"
        sleep 0.2
        DISPLAY=:0 xdotool click 1
        sleep 0.5
    fi

    clean_logs
    focus_ptm

    # Press F2 to rename
    send_key F2
    sleep 0.5

    if wait_for_event "f2-rename" 3; then
        result "F2 rename triggered" "PASS"
    else
        result "F2 rename triggered" "FAIL" "no f2-rename event"
    fi

    # Type new name and press Enter
    DISPLAY=:0 xdotool type --clearmodifiers "MyTerminal"
    sleep 0.3
    send_key Return
    sleep 1

    if wait_for_event "rename" 3; then
        result "Rename committed" "PASS"
    else
        result "Rename committed" "FAIL" "no rename event"
    fi

    screenshot "f2-rename"
    stop_ptm
    close_xterms
}

test_right_click_menu() {
    echo "--- test_right_click_menu ---"
    close_xterms
    open_xterms 1
    start_ptm || return 1

    wait_for "sidebar items" "[[ -f '$STATE_FILE' ]] && python3 -c \"import json; s=json.load(open('$STATE_FILE')); exit(0 if len([i for i in s['items'] if i.get('kind')=='window']) >= 1 else 1)\"" 15 || true

    local geo; geo=$(get_window_geometry "$PTM_WID")
    local win_x; win_x=$(echo "$geo" | grep -oP 'Position: \K\d+')
    local win_y; win_y=$(echo "$geo" | grep -oP ',\K\d+(?= )')
    local item_x=$((win_x + 100))
    local item_y=$((win_y + 15))

    focus_ptm
    # First left-click to ensure the webview is active and an item is selected
    DISPLAY=:0 xdotool mousemove "$item_x" "$item_y"
    sleep 0.2
    DISPLAY=:0 xdotool click 1
    sleep 0.3
    # Re-focus PTM (click may have activated the target window)
    focus_ptm
    DISPLAY=:0 xdotool mousemove "$item_x" "$item_y"
    sleep 0.2

    clean_logs

    # Right-click
    DISPLAY=:0 xdotool click 3
    sleep 0.5

    if wait_for_event "contextmenu" 3; then
        result "Right-click context menu (xdotool)" "PASS"
    else
        result "Right-click context menu (xdotool)" "FAIL" "no contextmenu event"
    fi

    screenshot "right-click-menu"

    # Dismiss menu
    DISPLAY=:0 xdotool mousemove "$((win_x + 5))" "$((win_y + 5))"
    sleep 0.2
    DISPLAY=:0 xdotool click 1
    sleep 0.3

    stop_ptm
    close_xterms
}

test_dark_theme() {
    echo "--- test_dark_theme ---"
    close_xterms
    open_xterms 1
    start_ptm || return 1

    wait_for "sidebar items" "[[ -f '$STATE_FILE' ]] && python3 -c \"import json; s=json.load(open('$STATE_FILE')); exit(0 if len([i for i in s['items'] if i.get('kind')=='window']) >= 1 else 1)\"" 15 || true

    screenshot "dark-theme"

    local geo; geo=$(get_window_geometry "$PTM_WID")
    local win_x; win_x=$(echo "$geo" | grep -oP 'Position: \K\d+')
    local win_y; win_y=$(echo "$geo" | grep -oP ',\K\d+(?= )')

    # Sample background pixel
    local lum
    lum=$(DISPLAY=:0 import -window root -crop "1x1+$((win_x + 50))+$((win_y + 300))" -format '%[fx:luminance]' info: 2>/dev/null) || lum="1"
    if python3 -c "import sys; sys.exit(0 if float('$lum') < 0.3 else 1)" 2>/dev/null; then
        result "Dark theme (luminance=$lum < 0.3)" "PASS"
    else
        result "Dark theme (luminance=$lum)" "FAIL" "background too bright"
    fi

    stop_ptm
    close_xterms
}

test_save_on_exit() {
    echo "--- test_save_on_exit ---"
    close_xterms
    open_xterms 2
    rm -f "$STATE_JSON_PATH"
    start_ptm || return 1

    wait_for "sidebar items" "[[ -f '$STATE_FILE' ]] && python3 -c \"import json; s=json.load(open('$STATE_FILE')); exit(0 if len([i for i in s['items'] if i.get('kind')=='window']) >= 1 else 1)\"" 15 || true

    # Rename a window so we have something to persist
    local geo; geo=$(get_window_geometry "$PTM_WID")
    local win_x; win_x=$(echo "$geo" | grep -oP 'Position: \K\d+')
    local win_y; win_y=$(echo "$geo" | grep -oP ',\K\d+(?= )')
    focus_ptm
    DISPLAY=:0 xdotool mousemove "$((win_x + 100))" "$((win_y + 15))"
    sleep 0.2
    DISPLAY=:0 xdotool click 1
    sleep 0.3
    focus_ptm
    send_key F2
    sleep 0.5
    DISPLAY=:0 xdotool type --clearmodifiers "SaveTest"
    sleep 0.2
    send_key Return
    sleep 3  # Wait for debounced save

    # Stop PTM (triggers save)
    stop_ptm

    # Check state.json exists and is valid
    if [[ -f "$STATE_JSON_PATH" ]]; then
        if python3 -c "import json; json.load(open('$STATE_JSON_PATH'))" 2>/dev/null; then
            result "State saved on exit (valid JSON)" "PASS"
        else
            result "State saved on exit" "FAIL" "invalid JSON"
        fi
    else
        result "State saved on exit" "FAIL" "state.json not found"
    fi

    close_xterms
}

test_state_persistence() {
    echo "--- test_state_persistence ---"
    close_xterms
    open_xterms 2
    rm -f "$STATE_JSON_PATH"
    start_ptm || return 1

    # Wait for sidebar items
    wait_for "sidebar items" "[[ -f '$STATE_FILE' ]] && python3 -c \"import json; s=json.load(open('$STATE_FILE')); exit(0 if len([i for i in s['items'] if i.get('kind')=='window']) >= 2 else 1)\"" 15 || true

    # Rename a window
    local geo; geo=$(get_window_geometry "$PTM_WID")
    local win_x; win_x=$(echo "$geo" | grep -oP 'Position: \K\d+')
    local win_y; win_y=$(echo "$geo" | grep -oP ',\K\d+(?= )')
    focus_ptm
    DISPLAY=:0 xdotool mousemove "$((win_x + 100))" "$((win_y + 15))"
    sleep 0.2
    DISPLAY=:0 xdotool click 1
    sleep 0.3
    focus_ptm
    send_key F2
    sleep 0.5
    DISPLAY=:0 xdotool type --clearmodifiers "PersistTest"
    sleep 0.2
    send_key Return
    sleep 3  # Wait for debounced save (2s debounce + margin)

    stop_ptm
    sleep 1

    # Restart PTM — renamed window should survive (keep state.json!)
    start_ptm keep_state || return 1

    # Wait for sidebar to populate after restart
    if ! wait_for "rename persisted" "[[ -f '$STATE_FILE' ]] && python3 -c \"import json; s=json.load(open('$STATE_FILE')); exit(0 if any(i.get('title')=='PersistTest' for i in s['items']) else 1)\"" 15; then
        true  # Fall through to check
    fi

    if [[ -f "$STATE_FILE" ]]; then
        local has_rename
        has_rename=$(python3 -c "
import json
with open('$STATE_FILE') as f:
    s = json.load(f)
found = [i for i in s['items'] if i.get('title') == 'PersistTest']
print(len(found))
" 2>/dev/null) || has_rename=0
        if [[ "$has_rename" -ge 1 ]]; then
            result "Rename survives restart" "PASS"
        else
            result "Rename survives restart" "FAIL" "PersistTest not in items"
        fi
    else
        result "Rename survives restart" "FAIL" "no state file"
    fi

    screenshot "state-persistence"
    stop_ptm
    close_xterms
}

test_dynamic_list() {
    echo "--- test_dynamic_list ---"
    close_xterms
    start_ptm || return 1

    # Wait for PTM init
    wait_for "PTM init" "[[ -f '$STATE_FILE' ]]" 10 || true

    # Open windows one by one
    DISPLAY=:0 xterm -title "TestXterm1" &
    DISPLAY=:0 xterm -title "TestXterm2" &
    DISPLAY=:0 xterm -title "TestXterm3" &

    # Wait for all 3 to appear in sidebar
    if ! wait_for "3 windows" "[[ -f '$STATE_FILE' ]] && python3 -c \"import json; s=json.load(open('$STATE_FILE')); exit(0 if len([i for i in s['items'] if i.get('kind')=='window']) >= 3 else 1)\"" 15; then
        true  # Fall through to check
    fi

    local count=0
    if [[ -f "$STATE_FILE" ]]; then
        count=$(python3 -c "
import json
with open('$STATE_FILE') as f:
    s = json.load(f)
print(len([i for i in s['items'] if i.get('kind') == 'window']))
" 2>/dev/null) || count=0
        if [[ "$count" -ge 3 ]]; then
            result "Dynamic window additions ($count windows)" "PASS"
        else
            result "Dynamic window additions" "FAIL" "only $count windows"
        fi
    else
        result "Dynamic window additions" "FAIL" "no state file"
    fi

    # Close one window and wait for count to decrease
    pkill -f "TestXterm1" 2>/dev/null || true

    if ! wait_for "window removed" "[[ -f '$STATE_FILE' ]] && python3 -c \"import json; s=json.load(open('$STATE_FILE')); exit(0 if len([i for i in s['items'] if i.get('kind')=='window']) < $count else 1)\"" 15; then
        true  # Fall through to check
    fi

    if [[ -f "$STATE_FILE" ]]; then
        local new_count
        new_count=$(python3 -c "
import json
with open('$STATE_FILE') as f:
    s = json.load(f)
print(len([i for i in s['items'] if i.get('kind') == 'window']))
" 2>/dev/null) || new_count=0
        if [[ "$new_count" -lt "$count" ]]; then
            result "Window removal detected ($new_count after close)" "PASS"
        else
            result "Window removal detected" "FAIL" "count didn't decrease (still $new_count)"
        fi
    fi

    stop_ptm
    close_xterms
}

test_keyboard_nav() {
    echo "--- test_keyboard_nav ---"
    close_xterms
    open_xterms 3
    start_ptm || return 1

    # Wait for sidebar items
    wait_for "sidebar items" "[[ -f '$STATE_FILE' ]] && python3 -c \"import json; s=json.load(open('$STATE_FILE')); exit(0 if len([i for i in s['items'] if i.get('kind')=='window']) >= 3 else 1)\"" 15 || true

    # Click inside PTM to establish DOM focus (X11 windowfocus alone isn't enough for WebKitGTK)
    local geo; geo=$(get_window_geometry "$PTM_WID")
    local win_x; win_x=$(echo "$geo" | grep -oP 'Position: \K\d+')
    local win_y; win_y=$(echo "$geo" | grep -oP ',\K\d+(?= )')
    focus_ptm
    DISPLAY=:0 xdotool mousemove "$((win_x + 100))" "$((win_y + 15))"
    sleep 0.2
    DISPLAY=:0 xdotool click 1
    sleep 0.3

    clean_logs
    focus_ptm

    # Arrow down to navigate
    send_key Down
    sleep 0.3
    send_key Down
    sleep 0.3

    if wait_for_event "keydown.*key=ArrowDown" 3; then
        result "Keyboard navigation (ArrowDown)" "PASS"
    else
        result "Keyboard navigation (ArrowDown)" "FAIL" "no ArrowDown event"
    fi

    # Enter to activate
    clean_logs
    focus_ptm
    send_key Return
    sleep 0.5

    if wait_for_event "enter-activate" 3; then
        result "Enter key activates window" "PASS"
    else
        result "Enter key activates window" "FAIL" "no enter-activate event"
    fi

    stop_ptm
    close_xterms
}

test_create_group() {
    echo "--- test_create_group ---"
    close_xterms
    open_xterms 2
    start_ptm || return 1

    wait_for "sidebar items" "[[ -f '$STATE_FILE' ]] && python3 -c \"import json; s=json.load(open('$STATE_FILE')); exit(0 if len([i for i in s['items'] if i.get('kind')=='window']) >= 2 else 1)\"" 15 || true

    local geo; geo=$(get_window_geometry "$PTM_WID")
    local win_x; win_x=$(echo "$geo" | grep -oP 'Position: \K\d+')
    local win_y; win_y=$(echo "$geo" | grep -oP ',\K\d+(?= )')
    local item_x=$((win_x + 100))
    local item_y=$((win_y + 15))

    # Right-click first item to get context menu
    focus_ptm
    DISPLAY=:0 xdotool mousemove "$item_x" "$item_y"
    sleep 0.2
    DISPLAY=:0 xdotool click 3
    sleep 0.5

    clean_logs
    # Context menu CSS top ≈ clientY (15px). Screen: win_y + 15.
    # Menu items: Rename(~24px), sep(~9px), Close(~24px), Remove(~24px), sep(~9px), Create Group(~24px)
    # Create Group center ≈ offset 105 from menu top (empirically verified)
    local menu_screen_top=$((win_y + 15))
    local menu_x=$((win_x + 120))
    DISPLAY=:0 xdotool mousemove "$menu_x" "$((menu_screen_top + 105))"
    sleep 0.2
    DISPLAY=:0 xdotool click 1
    sleep 1

    if wait_for_event "create-group" 3; then
        result "Create Group from context menu" "PASS"
    else
        result "Create Group from context menu" "FAIL" "no create-group event"
    fi

    # Verify group appears in state
    if [[ -f "$STATE_FILE" ]]; then
        local has_group
        has_group=$(python3 -c "
import json
with open('$STATE_FILE') as f:
    s = json.load(f)
groups = [i for i in s['items'] if i.get('kind') == 'group']
print(len(groups))
" 2>/dev/null) || has_group=0
        if [[ "$has_group" -ge 1 ]]; then
            result "Group visible in sidebar" "PASS"
        else
            result "Group visible in sidebar" "FAIL" "no group in state file"
        fi
    else
        result "Group visible in sidebar" "FAIL" "no state file"
    fi

    screenshot "create-group"
    stop_ptm
    close_xterms
}

test_keyboard_reorder() {
    echo "--- test_keyboard_reorder ---"
    close_xterms
    open_xterms 3
    start_ptm || return 1

    wait_for "sidebar items" "[[ -f '$STATE_FILE' ]] && python3 -c \"import json; s=json.load(open('$STATE_FILE')); exit(0 if len([i for i in s['items'] if i.get('kind')=='window']) >= 3 else 1)\"" 15 || true

    # Get initial order
    local initial_first
    initial_first=$(python3 -c "
import json
with open('$STATE_FILE') as f:
    s = json.load(f)
windows = [i for i in s['items'] if i.get('kind') == 'window']
print(windows[0]['title'] if windows else '')
" 2>/dev/null) || initial_first=""

    # Click first item to select it
    local geo; geo=$(get_window_geometry "$PTM_WID")
    local win_x; win_x=$(echo "$geo" | grep -oP 'Position: \K\d+')
    local win_y; win_y=$(echo "$geo" | grep -oP ',\K\d+(?= )')
    focus_ptm
    DISPLAY=:0 xdotool mousemove "$((win_x + 100))" "$((win_y + 15))"
    sleep 0.2
    DISPLAY=:0 xdotool click 1
    sleep 0.3

    clean_logs
    focus_ptm

    # Ctrl+Shift+Down to move selected item down
    send_key ctrl+shift+Down
    sleep 0.5

    if wait_for_event "keyboard-reorder" 3; then
        result "Ctrl+Shift+Down keyboard reorder" "PASS"
    else
        result "Ctrl+Shift+Down keyboard reorder" "FAIL" "no keyboard-reorder event"
    fi

    # Verify order changed — first item should now be different
    sleep 0.5
    if [[ -f "$STATE_FILE" ]]; then
        local new_first
        new_first=$(python3 -c "
import json
with open('$STATE_FILE') as f:
    s = json.load(f)
windows = [i for i in s['items'] if i.get('kind') == 'window']
print(windows[0]['title'] if windows else '')
" 2>/dev/null) || new_first=""
        if [[ -n "$initial_first" && "$new_first" != "$initial_first" ]]; then
            result "Reorder changed item order" "PASS"
        else
            result "Reorder changed item order" "FAIL" "first item unchanged: '$new_first'"
        fi
    fi

    stop_ptm
    close_xterms
}

# ─── Test runner ─────────────────────────────────────────────────

ALL_TESTS=(
    test_launch_and_list
    test_click_focus
    test_self_filter
    test_f2_rename
    test_right_click_menu
    test_dark_theme
    test_save_on_exit
    test_state_persistence
    test_dynamic_list
    test_keyboard_nav
    test_create_group
    test_keyboard_reorder
)

FILTER="${1:-}"

echo "=== Tauri PTM E2E Tests ==="
echo ""

# Clean stale WM state (Cinnamon2d doesn't always update _NET_CLIENT_LIST on destroy)
echo "Restarting Cinnamon for clean WM state..."
DISPLAY=:0 nohup cinnamon --replace > /dev/null 2>&1 &
sleep 3
echo ""

for test in "${ALL_TESTS[@]}"; do
    if [[ -n "$FILTER" ]] && [[ "$test" != *"$FILTER"* ]]; then
        continue
    fi

    # Run test; if it fails, retry once (focus flakiness workaround)
    old_pass=$PASS old_fail=$FAIL old_skip=$SKIP
    $test
    if [[ $FAIL -gt $old_fail ]]; then
        echo "  (retrying...)"
        PASS=$old_pass FAIL=$old_fail SKIP=$old_skip
        sleep 2  # Extra settle time before retry
        $test
    fi

    sleep 1  # Let WM settle between tests
    echo ""
done

# ─── Summary ─────────────────────────────────────────────────────

echo "=== Summary ==="
echo "  Passed: $PASS   Failed: $FAIL   Skipped: $SKIP"
echo ""

if [[ $FAIL -eq 0 ]]; then
    echo "ALL TESTS PASSED"
    exit 0
else
    echo "SOME TESTS FAILED"
    exit 1
fi
