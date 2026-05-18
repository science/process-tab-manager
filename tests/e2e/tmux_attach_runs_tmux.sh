#!/usr/bin/env bash
# E2E: clicking "+ New tmux" must produce a terminal that is ACTUALLY a
# tmux client, AND PTM must bind that terminal's row to its tmux session
# so the sidebar's green session marker renders.
#
# The attachment probe is a keystroke round-trip: focus the new xterm,
# type a unique marker via xdotool, and grep `tmux capture-pane` output
# for it. If the marker shows up in the pane capture, the xterm is
# unambiguously attached to that tmux session — no process-tree inference
# needed.
#
# Pass: (a) marker round-trips through tmux, AND (b) PTM's SIGUSR1 dump
#       contains a Tmux binding line naming the new session inside the
#       newly-spawned xterm's block.
# Fail: (a) marker missing — the xterm isn't a tmux client (e.g. the
#       spawn-attach path is broken; this is symptom #1 from the user
#       report). OR (b) no session binding — item.session is still None
#       (e.g. claim/carry was removed; this is symptom #2).
#
# Tools required: Xvfb, xdotool, tmux, xterm, openbox, xdpyinfo, scrot.

set -uo pipefail

DISPLAY_NUM=":99"
PTM="${PTM_BIN:-/tmp/ptm-dev/release/ptm}"
HOME_DIR=$(mktemp -d -t ptm-e2e-attach.XXXXXX)
SHOTS=$(mktemp -d -t ptm-e2e-attach-shots.XXXXXX)
export TMUX_TMPDIR=$(mktemp -d -t ptm-e2e-attach-tmux.XXXXXX)
MARKER="PTM_E2E_ATTACH_MARKER_$$_$(date +%s%N)"

cleanup() {
    [[ -n "${PTM_PID:-}" ]] && kill "$PTM_PID" 2>/dev/null || true
    [[ -n "${WM_PID:-}" ]] && kill "$WM_PID" 2>/dev/null || true
    [[ -n "${XVFB_PID:-}" ]] && kill "$XVFB_PID" 2>/dev/null || true
    pkill -f "$DISPLAY_NUM.*xterm" 2>/dev/null || true
    tmux kill-server 2>/dev/null || true
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

openbox --sm-disable >/dev/null 2>&1 &
WM_PID=$!
sleep 0.4

# PTM_TERMINAL_CMD=xterm keeps the spawn deterministic and avoids the
# DBus dance gnome-terminal needs. Under xterm there's no PID collision,
# so the walk tier of bind_sessions is the path exercised here. The
# claim tier handles the gnome-terminal case in production; that's
# covered by Tier-1 unit tests.
export PTM_TERMINAL_CMD=xterm

HOME="$HOME_DIR" "$PTM" >/tmp/ptm-e2e-attach.log 2>&1 &
PTM_PID=$!

WID=""
for i in {1..30}; do
    WID=$(xdotool search --name "^ptm$" 2>/dev/null | head -1 || true)
    [[ -n "$WID" ]] && break
    sleep 0.1
done
[[ -z "$WID" ]] && { echo "FAIL: ptm window did not appear"; exit 1; }
echo "[setup] ptm wid=$WID pid=$PTM_PID"

sleep 0.6  # initial paint

scrot "$SHOTS/01-before-click.png"

# Snapshot the tmux session list BEFORE the click so we can identify the
# session "+ New tmux" creates. With an empty isolated server this will
# be empty; the click produces session "0".
BEFORE_SESSIONS=$(tmux ls -F '#{session_name}' 2>/dev/null | sort -u || true)
echo "[setup] sessions before: ${BEFORE_SESSIONS:-<none>}"

# Click "+ New tmux" via window-relative coords (same recipe as
# spawn_position.sh) so the openbox frame doesn't matter.
echo "[act ] click '+ New tmux' at window-relative 184,18"
xdotool mousemove --window "$WID" --sync 184 18; sleep 0.1
xdotool click 1

# Wait for the new xterm AND the new tmux session.
TERM_WID=""
for i in {1..40}; do
    TERM_WID=$(xdotool search --class "xterm" 2>/dev/null | head -1 || true)
    [[ -n "$TERM_WID" ]] && break
    sleep 0.1
done
if [[ -z "$TERM_WID" ]]; then
    echo "FAIL: no xterm appeared within 4s of clicking '+ New tmux'"
    echo "ptm log tail:"; tail -20 /tmp/ptm-e2e-attach.log 2>&1 | sed 's/^/  /'
    exit 1
fi
echo "[diag] xterm wid=$TERM_WID"

# Identify the new session: whatever's in `tmux ls` now minus before.
SESSION=""
for i in {1..30}; do
    AFTER=$(tmux ls -F '#{session_name}' 2>/dev/null | sort -u || true)
    SESSION=$(comm -13 <(printf '%s\n' "$BEFORE_SESSIONS") <(printf '%s\n' "$AFTER") | head -1)
    [[ -n "$SESSION" ]] && break
    sleep 0.1
done
if [[ -z "$SESSION" ]]; then
    echo "FAIL: '+ New tmux' did not create a new tmux session"
    echo "tmux ls:"; tmux ls 2>&1 | sed 's/^/  /'
    exit 1
fi
echo "[diag] new tmux session: $SESSION"

# Let the xterm finish painting + tmux finish attaching before we type.
sleep 0.6
scrot "$SHOTS/02-after-spawn.png"

# ── Probe (a): keystroke round-trip proves attachment ──
#
# Focus the new xterm, type the marker, hit Enter. If the xterm is a
# tmux client, the keystrokes flow through tmux's pane and `echo`
# writes the marker to the pane's visible buffer. If the xterm is a
# plain shell (no tmux — symptom #1), `tmux capture-pane` on $SESSION
# captures the empty pane that nobody ever typed into.
echo "[act ] type marker into xterm: $MARKER"
xdotool windowfocus --sync "$TERM_WID"
sleep 0.1
xdotool type --delay 20 "echo $MARKER"
xdotool key Return
sleep 0.6  # tmux propagates input → shell echoes → pane buffer updates
scrot "$SHOTS/03-after-type.png"

PANE_CAPTURE=$(tmux capture-pane -t "$SESSION" -p 2>/dev/null || true)
if ! printf '%s' "$PANE_CAPTURE" | grep -qF "$MARKER"; then
    echo "FAIL: marker '$MARKER' did not round-trip through tmux session $SESSION"
    echo "      → the spawned xterm is NOT a tmux client (symptom #1)"
    echo "pane capture:"; printf '%s\n' "$PANE_CAPTURE" | sed 's/^/  /'
    echo "ptm log tail:"; tail -20 /tmp/ptm-e2e-attach.log | sed 's/^/  /'
    exit 1
fi
echo "[ok  ] (a) marker round-tripped — xterm IS attached to tmux session $SESSION"

# ── Probe (b): PTM bound item.session for the new xterm row ──
#
# SIGUSR1 makes PTM write a markdown dump of its current state per row,
# including a Tmux binding line when item.session is Some. We scope the
# grep to the xterm's block (matched by its $TERM_WID) so a binding on
# an unrelated row can't pass the test. The dump's per-window block
# starts with a "## N — …" header; the row's wid appears in a "wid:
# 0xNNN" line within it.
echo "[act ] SIGUSR1 to ptm pid=$PTM_PID"
kill -USR1 "$PTM_PID"

DUMP_FILE="$HOME_DIR/.cache/ptm/recipes-snapshot.md"
for i in {1..30}; do
    [[ -f "$DUMP_FILE" ]] && break
    sleep 0.1
done
[[ -f "$DUMP_FILE" ]] || { echo "FAIL: dump file not written at $DUMP_FILE"; exit 1; }
sleep 0.3  # let the dump fully flush

TERM_WID_HEX=$(printf '0x%08x' "$TERM_WID")
echo "[diag] looking for wid=$TERM_WID_HEX in dump"

# The dump emits one block per window separated by "---". Find the block
# containing this xterm's wid and check for a Tmux binding inside it.
AWK_SCRIPT='
BEGIN { RS="\n---\n"; }
$0 ~ wid_re { print; }
'
TERM_BLOCK=$(awk -v wid_re="$TERM_WID_HEX" "$AWK_SCRIPT" "$DUMP_FILE")
if [[ -z "$TERM_BLOCK" ]]; then
    echo "FAIL: no dump block found referencing wid $TERM_WID_HEX"
    echo "dump:"; sed 's/^/  /' "$DUMP_FILE"
    exit 1
fi

if ! printf '%s' "$TERM_BLOCK" | grep -q "Tmux binding:.*session=.*${SESSION}"; then
    echo "FAIL: xterm row's block has no Tmux binding for session '$SESSION'"
    echo "      → PTM did NOT bind item.session (symptom #2)"
    echo "block:"; printf '%s\n' "$TERM_BLOCK" | sed 's/^/  /'
    exit 1
fi

echo "[ok  ] (b) PTM bound item.session to '$SESSION' for wid $TERM_WID_HEX"
echo "PASS: tmux is attached AND PTM has bound the session for the new row"
exit 0
