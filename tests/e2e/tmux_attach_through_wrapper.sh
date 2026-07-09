#!/usr/bin/env bash
# E2E: clicking "+ New tmux" must actually run tmux when PTM's chosen
# terminal is a Debian-style `.wrapper` Perl shim — the path that the
# user's real UAT exercises via `x-terminal-emulator` →
# `/usr/bin/gnome-terminal.wrapper`.
#
# Why this exists separately from `tmux_attach_runs_tmux.sh`:
# the other script sets PTM_TERMINAL_CMD=xterm to dodge gnome-terminal's
# DBus/accessibility dependencies under Xvfb. That dodge accidentally
# also dodges the bug — xterm uses `-e`, which works. Real wrappers
# silently drop `--` and exec a bare terminal with no command, so the
# user sees a plain shell instead of a tmux session. This script
# installs a fake `gnome-terminal.wrapper` (basename triggers PTM's
# gnome-terminal separator rule; suffix triggers the .wrapper override)
# that mimics the Debian wrapper's `-e CMD [ARGS…]` semantics, and
# verifies that the typed marker round-trips through tmux.
#
# Pass: marker round-trips → tmux IS attached → wrapper saw `-e ARGS…`.
# Fail: marker missing → wrapper got `--` and dropped everything →
#       spawned terminal is a plain shell.
#
# Tools required: Xvfb, xdotool, tmux, xterm, openbox, xdpyinfo, scrot.

source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

e2e_mktemp_dir HOME_DIR ptm-e2e-wrap-home.XXXXXX
e2e_mktemp_dir WRAP_DIR ptm-e2e-wrap-bin.XXXXXX
# Plain mktemp (not e2e_mktemp_dir): screenshots survive for debugging.
SHOTS=$(mktemp -d -t ptm-e2e-wrap-shots.XXXXXX)
MARKER="PTM_E2E_WRAP_$$_$(date +%s%N)"

e2e_require xdotool tmux xterm scrot openbox

# ── Fake Debian wrapper ─────────────────────────────────────────────
# Behavioural match for the bug-relevant subset of the real
# /usr/bin/gnome-terminal.wrapper:
#   -e CMD [ARGS…]   →  exec xterm running that command
#   anything else    →  silently drop and exec plain xterm (= the bug)
# Named `gnome-terminal.wrapper` so PTM's basename match treats it as
# gnome-terminal and (pre-fix) picks `--`; the `.wrapper` suffix is
# what the fix keys off to override and pick `-e`.
WRAPPER="$WRAP_DIR/gnome-terminal.wrapper"
cat > "$WRAPPER" <<'WRAPPER_EOF'
#!/usr/bin/env bash
# Mimic /usr/bin/gnome-terminal.wrapper: handle -e; drop everything else.
while [[ $# -gt 0 ]]; do
    case "$1" in
        -e)
            shift
            exec xterm -class XTerm -e "$@"
            ;;
        *)
            shift
            ;;
    esac
done
exec xterm -class XTerm
WRAPPER_EOF
chmod +x "$WRAPPER"
echo "[setup] fake wrapper installed at $WRAPPER"

# ── Xvfb + openbox ──────────────────────────────────────────────────
e2e_start_xvfb
e2e_start_wm

# Point PTM at the fake wrapper. PTM_TERMINAL_CMD wins over the
# PATH-based detection, so this is what spawn_attach_terminal will
# launch.
export PTM_TERMINAL_CMD="$WRAPPER"

e2e_launch_ptm "$HOME_DIR" /tmp/ptm-e2e-wrap.log

sleep 0.6
scrot "$SHOTS/01-before-click.png"

BEFORE_SESSIONS=$(tmux ls -F '#{session_name}' 2>/dev/null | sort -u || true)
echo "[setup] sessions before: ${BEFORE_SESSIONS:-<none>}"

echo "[act ] click '+ New tmux' at window-relative 184,18"
xdotool mousemove --window "$WID" --sync 184 18; sleep 0.1
xdotool click 1

TERM_WID=""
for i in {1..40}; do
    TERM_WID=$(xdotool search --class "xterm" 2>/dev/null | head -1 || true)
    [[ -n "$TERM_WID" ]] && break
    sleep 0.1
done
if [[ -z "$TERM_WID" ]]; then
    echo "FAIL: no xterm appeared within 4s of clicking '+ New tmux'"
    echo "ptm log tail:"; tail -20 /tmp/ptm-e2e-wrap.log 2>&1 | sed 's/^/  /'
    exit 1
fi
echo "[diag] xterm wid=$TERM_WID"

SESSION=""
for i in {1..30}; do
    AFTER=$(tmux ls -F '#{session_name}' 2>/dev/null | sort -u || true)
    SESSION=$(comm -13 <(printf '%s\n' "$BEFORE_SESSIONS") <(printf '%s\n' "$AFTER") | head -1)
    [[ -n "$SESSION" ]] && break
    sleep 0.1
done
if [[ -z "$SESSION" ]]; then
    echo "FAIL: '+ New tmux' did not create a new tmux session"
    exit 1
fi
echo "[diag] new tmux session: $SESSION"

sleep 0.6
scrot "$SHOTS/02-after-spawn.png"

# Keystroke round-trip — the same probe as tmux_attach_runs_tmux.sh,
# but here it exercises the wrapper-shaped path.
echo "[act ] type marker into xterm: $MARKER"
xdotool windowfocus --sync "$TERM_WID"
sleep 0.1
xdotool type --delay 20 "echo $MARKER"
xdotool key Return
sleep 0.6
scrot "$SHOTS/03-after-type.png"

PANE_CAPTURE=$(tmux capture-pane -t "$SESSION" -p 2>/dev/null || true)
if ! printf '%s' "$PANE_CAPTURE" | grep -qF "$MARKER"; then
    echo "FAIL: marker '$MARKER' did not round-trip through tmux session $SESSION"
    echo "      → wrapper got '--' (or some other arg it dropped) and the"
    echo "      → spawned xterm is a plain shell, not a tmux client."
    echo "pane capture:"; printf '%s\n' "$PANE_CAPTURE" | sed 's/^/  /'
    echo "ptm log tail:"; tail -20 /tmp/ptm-e2e-wrap.log | sed 's/^/  /'
    exit 1
fi
echo "PASS: marker round-tripped through tmux via .wrapper invocation"
exit 0
