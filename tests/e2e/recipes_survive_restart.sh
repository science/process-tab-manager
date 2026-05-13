#!/usr/bin/env bash
# E2E: a saved tmux row reattaches to its live xterm via Phase 5c Tier 0a
# (session-name match) after PTM starts up against a v2 groups file.
#
# Flow:
#   1. Pre-seed $HOME/.config/ptm/profiles/default/groups with a v2 file
#      whose only MEMBER carries a TMUX line. pane_pid is a stale sentinel
#      (99999) so we can prove the capture path saw the LIVE binding.
#   2. Start a tmux session matching the saved TMUX session_name, attach
#      an xterm to it (so the xterm gets bound to that session via PTM's
#      walk_to_window_owner path).
#   3. Launch PTM. It loads the file, runs restore_groups, and Tier 0a
#      finds the live xterm by session, claiming it into the saved
#      "mvp-test" group.
#   4. SIGUSR1 PTM. The Phase 5a dump path captures /proc + tmux fresh,
#      writes a markdown snapshot.
#   5. Verify the snapshot shows the xterm row attributed to the
#      "mvp-test" group AND its TMUX binding has a pane_pid that's NOT
#      the stale 99999.
#
# Tools required: Xvfb, xdotool, tmux, xterm, openbox, xdpyinfo.

set -uo pipefail

DISPLAY_NUM=":99"
SESSION="ptm_e2e_restart_$$"
PTM="${PTM_BIN:-/tmp/ptm-dev/release/ptm}"
HOME_DIR=$(mktemp -d -t ptm-e2e-restart.XXXXXX)
export TMUX_TMPDIR=$(mktemp -d -t ptm-e2e-restart-tmux.XXXXXX)

cleanup() {
    [[ -n "${PTM_PID:-}" ]] && kill "$PTM_PID" 2>/dev/null || true
    [[ -n "${XTERM_PID:-}" ]] && kill "$XTERM_PID" 2>/dev/null || true
    [[ -n "${OPENBOX_PID:-}" ]] && kill "$OPENBOX_PID" 2>/dev/null || true
    [[ -n "${XVFB_PID:-}" ]] && kill "$XVFB_PID" 2>/dev/null || true
    tmux kill-server 2>/dev/null || true
    rm -rf "$HOME_DIR" "$TMUX_TMPDIR"
    wait 2>/dev/null || true
}
trap cleanup EXIT

[[ -x "$PTM" ]] || { echo "FAIL: ptm binary not found at $PTM"; exit 2; }
for tool in Xvfb xdotool tmux xterm xdpyinfo openbox; do
    command -v "$tool" >/dev/null || { echo "FAIL: missing $tool"; exit 2; }
done

# Start Xvfb + openbox so newly-spawned windows show up in _NET_CLIENT_LIST.
Xvfb "$DISPLAY_NUM" -screen 0 1024x768x24 >/dev/null 2>&1 &
XVFB_PID=$!
for i in {1..20}; do
    DISPLAY="$DISPLAY_NUM" xdpyinfo >/dev/null 2>&1 && break
    sleep 0.1
done
DISPLAY="$DISPLAY_NUM" xdpyinfo >/dev/null 2>&1 || { echo "FAIL: Xvfb did not become responsive"; exit 2; }
export DISPLAY="$DISPLAY_NUM"

DISPLAY="$DISPLAY_NUM" openbox --sm-disable >/dev/null 2>&1 &
OPENBOX_PID=$!
sleep 0.4

# Pre-seed the v2 groups file. The TMUX line:
#   * session_name = $SESSION (we create it below)
#   * session_id = "" (no id recorded — empty-sentinel)
#   * pane = "" (empty)
#   * pane_pid = 99999 (stale; the freshly captured snapshot should differ)
mkdir -p "$HOME_DIR/.config/ptm/profiles/default"
GROUPS_FILE="$HOME_DIR/.config/ptm/profiles/default/groups"
{
    printf 'v2\n'
    printf 'GROUP\tmvp-test\t0\tnormal\n'
    printf 'MEMBER\tpre-seeded label\tXTerm\t\n'
    printf 'TMUX\t%s\t\t\t99999\n' "$SESSION"
} > "$GROUPS_FILE"
echo "[setup] pre-seeded groups file:"
sed 's/^/    /' "$GROUPS_FILE"

# Start the tmux session that the pre-seeded MEMBER references.
tmux kill-session -t "$SESSION" 2>/dev/null || true
tmux new-session -d -s "$SESSION"
echo "[setup] staged tmux session: $SESSION"

# Spawn an xterm attached to it. PTM's walk_to_window_owner binds the
# xterm wid to $SESSION via tmux's client_pid → window_pid ancestor walk.
xterm -class XTerm -e "tmux attach -t $SESSION" >/dev/null 2>&1 &
XTERM_PID=$!
sleep 1.0

XTERM_WID=""
for i in {1..30}; do
    XTERM_WID=$(xdotool search --class XTerm 2>/dev/null | head -1 || true)
    [[ -n "$XTERM_WID" ]] && break
    sleep 0.1
done
[[ -z "$XTERM_WID" ]] && { echo "FAIL: xterm window did not appear"; exit 1; }
echo "[setup] xterm wid=$XTERM_WID"

# Launch PTM under the pre-seeded HOME.
HOME="$HOME_DIR" "$PTM" >/tmp/ptm-e2e-restart.log 2>&1 &
PTM_PID=$!

PTM_WID=""
for i in {1..30}; do
    PTM_WID=$(xdotool search --name "^ptm$" 2>/dev/null | head -1 || true)
    [[ -n "$PTM_WID" ]] && break
    sleep 0.1
done
[[ -z "$PTM_WID" ]] && {
    echo "FAIL: ptm window did not appear"
    sed 's/^/    /' /tmp/ptm-e2e-restart.log
    exit 1
}
echo "[setup] ptm wid=$PTM_WID pid=$PTM_PID"

# Wait for PTM's first refresh + restore_groups + tmux client probe so the
# xterm has a chance to get bound to $SESSION. (One refresh runs at
# startup, but the tmux client list is populated synchronously inside that
# refresh, so the binding should be in place before we trigger the dump.)
sleep 2.0

# Trigger the Phase 5a dump (in-memory state → markdown).
echo "[act ] SIGUSR1 to ptm pid=$PTM_PID"
kill -USR1 "$PTM_PID"

DUMP_FILE="$HOME_DIR/.cache/ptm/recipes-snapshot.md"
for i in {1..30}; do
    [[ -f "$DUMP_FILE" ]] && break
    sleep 0.1
done
[[ -f "$DUMP_FILE" ]] || { echo "FAIL: dump file not written"; exit 1; }
# Give the dump a moment to fully flush.
sleep 0.3

echo "[diag] dump file contents:"
sed 's/^/    /' "$DUMP_FILE"

# Verification #1: the xterm row's block names group "mvp-test".
if ! grep -q "Group:\*\* mvp-test" "$DUMP_FILE"; then
    echo "FAIL: no row attributed to group 'mvp-test' — Tier 0a did not match"
    exit 1
fi

# Verification #2: a Tmux binding line referencing $SESSION exists.
TMUX_BIND_LINE=$(grep "Tmux binding:.*session=.*${SESSION}" "$DUMP_FILE" || true)
if [[ -z "$TMUX_BIND_LINE" ]]; then
    echo "FAIL: no tmux binding for $SESSION in dump"
    exit 1
fi

# Verification #3: the pane_pid in the dump is real (not 99999, not 0).
# The dump format is "...pane_pid=N" near the end of the binding line.
DUMP_PID=$(printf '%s\n' "$TMUX_BIND_LINE" | sed -n 's/.*pane_pid=\([0-9][0-9]*\).*/\1/p')
if [[ -z "$DUMP_PID" ]]; then
    echo "FAIL: pane_pid not parseable from dump line: $TMUX_BIND_LINE"
    exit 1
fi
if [[ "$DUMP_PID" == "99999" ]]; then
    echo "FAIL: dump's pane_pid is the stale 99999 — the live binding wasn't observed"
    exit 1
fi
if [[ "$DUMP_PID" == "0" ]]; then
    echo "FAIL: dump's pane_pid is zero — tmux query failed inside the capture path"
    exit 1
fi

echo "PASS: Tier 0a matched the live xterm into 'mvp-test'; pane_pid=$DUMP_PID (not 99999)"
exit 0
