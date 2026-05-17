# PTM tmux install diagnostics — peer-VM procedure

**Audience:** A Claude Code session running on a peer VM (e.g. `dev-1`) where PTM was installed via `~/dev/process-tab-manager/install.sh` and is exhibiting failures the user suspects are tmux-related.

**Why you're reading this:** A planner Claude on the user's primary host wrote this procedure. PTM works there. Your job is to figure out *why it doesn't work on this VM* by collecting structured telemetry. The planner will read your report and design installer changes from it. You are **not** fixing anything in this pass.

---

## 0 — Read this first (role & constraints)

You are a diagnostic agent. The rules:

- **Read-only on user config.** Do not edit, rename, move, or delete `~/.tmux.conf`, `~/.config/tmux/tmux.conf`, `/etc/tmux.conf`, or anything else. Reading and copying contents into your report is fine.
- **Don't touch the user's tmux server.** Any test session you create must use a private socket: `tmux -L ptm-diag …`. Never run `tmux kill-server` without `-L ptm-diag`, and clean up your private socket at the end (Step 5 wrap-up).
- **No destructive actions without asking.** If something looks like it warrants modifying the system to confirm a hypothesis (e.g. "let's rename `~/.tmux.conf` and see if it works"), ask the user before doing it. The planner can decide remediations once it has the data.
- **No assumptions.** If a probe gives ambiguous output, capture it verbatim — don't summarize, don't interpret. The planner needs raw evidence.
- **PTM is the binary at `~/.local/bin/ptm`** (symlink to `/tmp/ptm-target/release/ptm`). The source tree is `~/dev/process-tab-manager`. The PTM-relevant tmux commands all live in `src/main.rs` at lines roughly 2392, 2564, 3014, 3659, 4882 — you can read them for context but don't need to.
- **PTM degrades gracefully** at every tmux-call site (`Command::new("tmux")` → returns `Err(_)` → empty result → next refresh reconciles). So if you see PTM *crash*, it's almost certainly not tmux; it's something else (X11, font, missing lib). Capture it anyway.

If at any point you're unsure whether an action is safe, ask the user.

---

## 1 — Interview the user (do this before any probes)

Ask the user the following questions plainly. Don't probe yet — the answers shape which probes matter.

1. "Briefly, what failures are you seeing in PTM? Does the sidebar appear at all, or does PTM not launch?"
2. "When you click the `+ New tmux` button, what happens — nothing? a window flashes and dies? a terminal opens but isn't bound?"
3. "Are tmux-attached terminal rows shown with a green session marker, or are they showing as plain terminals?"
4. "Do you have a custom `~/.tmux.conf` or `~/.config/tmux/tmux.conf` on this VM? Did you set one up yourself, or did it come from yadm/dotfiles?"
5. "Have you ever successfully run `tmux new-session` from a shell on this VM, outside of PTM?"
6. "What terminal emulator do you normally use here (gnome-terminal, xterm, alacritty, kitty, foot, ptyxis, something else)?"
7. "Is there anything different about this VM versus your main machine — different distro, no X11, headless, Wayland, snap-installed tmux, anything?"

Write the answers down — they go into the report under "User-reported failures."

If the user says PTM doesn't launch at all, jump ahead to Step 4 first (live behavior) — the tmux probes in Step 3 may be moot if the binary won't run.

---

## 2 — Environment telemetry

Run each command and capture stdout + stderr + exit code verbatim. Do not modify anything. Some commands may not exist on this system; that's fine, note "command not found" in the report.

```bash
# OS / kernel
uname -a
cat /etc/os-release 2>/dev/null
lsb_release -a 2>/dev/null

# Tmux
command -v tmux
tmux -V
ls -la ~/.tmux.conf ~/.config/tmux/tmux.conf /etc/tmux.conf 2>&1

# X11 / desktop
echo "DISPLAY=$DISPLAY"
echo "XDG_SESSION_TYPE=$XDG_SESSION_TYPE"
echo "XDG_CURRENT_DESKTOP=$XDG_CURRENT_DESKTOP"
echo "WAYLAND_DISPLAY=$WAYLAND_DISPLAY"
echo "TERM=$TERM"
wmctrl -m 2>&1
xdpyinfo 2>&1 | head -5

# Terminal emulators available
for t in gnome-terminal xterm alacritty kitty foot ptyxis konsole xfce4-terminal mate-terminal terminator; do
    printf '%-20s %s\n' "$t" "$(command -v "$t" 2>/dev/null || echo 'not installed')"
done
echo "PTM_TERMINAL_CMD=$PTM_TERMINAL_CMD"
echo "TERMINAL=$TERMINAL"

# PTM binary
ls -la ~/.local/bin/ptm
ls -la /tmp/ptm-target/release/ptm 2>&1
ldd /tmp/ptm-target/release/ptm 2>&1 | head -30

# Shell + locale
echo "SHELL=$SHELL"
locale
```

For each tmux config file that exists (per the `ls -la` above), also capture its **full verbatim contents** into the report. Use a fenced code block per file. Do not summarize.

---

## 3 — Tmux probes (PTM-shaped, private socket)

These are the *exact* command shapes PTM uses at runtime. Run each on a private socket `ptm-diag` so the user's existing tmux server is untouched. Capture exit code + stdout + stderr separately.

```bash
# Probe 0: cleanup any stale private socket from a previous run
tmux -L ptm-diag kill-server 2>/dev/null; true

# Probe 1: tmux is callable (mirrors is_tmux_available, src/main.rs:2392)
tmux -V
echo "exit=$?"

# Probe 2: create a detached session and capture its auto-assigned name
#          (mirrors create_new_tmux_session, src/main.rs:3659)
TMUX_NAME=$(tmux -L ptm-diag new-session -d -P -F '#{session_name}' 2>&1)
echo "exit=$? name=$TMUX_NAME"

# Probe 3: list sessions on the private socket
#          (mirrors list_tmux_sessions, src/main.rs:2564)
tmux -L ptm-diag list-sessions -F '#{session_id} #{session_name} #{session_attached}' 2>&1
echo "exit=$?"

# Probe 4: query the pane on the session we just created
#          (mirrors query_tmux_pane, src/main.rs:3014)
#          Substitute the name printed by Probe 2 in place of <name>.
tmux -L ptm-diag display-message -p -t "$TMUX_NAME" '#{pane_id} #{pane_pid}' 2>&1
echo "exit=$?"

# Probe 5: clients on the private socket (mirrors list_tmux_clients, src/main.rs:2515)
tmux -L ptm-diag list-clients -F '#{client_pid} #{session_name}' 2>&1
echo "exit=$?"

# Probe 6: kill the test session (mirrors kill_tmux_session, src/main.rs:4882)
tmux -L ptm-diag kill-session -t "$TMUX_NAME" 2>&1
echo "exit=$?"

# Probe 7: clean up the private socket completely
tmux -L ptm-diag kill-server 2>/dev/null; true
ls /tmp/tmux-* 2>/dev/null | grep ptm-diag || echo "private socket cleaned"
```

Capture each probe's exit code + stdout + stderr in the report. Even successful probes go in — the report is for the planner, who needs to see what *worked* as well as what didn't.

**If Probe 2 fails**, this is the single most likely root cause: a custom config (from Step 2) is breaking `new-session`. Note in the report which config files exist *and* whether Probe 2's stderr references any of them. **Do not modify the config files.** The planner will decide.

**If Probe 1 succeeds but Probe 3 returns "no server running"** even though Probe 2 succeeded, that's odd — capture verbatim and flag.

---

## 4 — Live PTM behavior

### 4a — Does PTM launch?

```bash
# Make sure no PTM is already running first
pgrep -a ptm

# If yes, ask the user whether to kill it before launching a fresh instance.
# If no, launch with stderr captured:
DISPLAY=:0 ~/.local/bin/ptm 2> /tmp/ptm-stderr.log &
PTM_PID=$!
sleep 2
ps -p "$PTM_PID" -o pid=,stat=,cmd= 2>&1
```

If PTM is dead after the `sleep 2`:

```bash
cat /tmp/ptm-stderr.log
```

Capture the stderr verbatim into the report. If it's a panic, that's the most useful single piece of telemetry — quote the full backtrace. **Skip the rest of Step 4 in that case.** Step 5 (Report) still applies.

If PTM is alive:

### 4b — SIGUSR1 dump

```bash
kill -USR1 "$PTM_PID"
sleep 1
# Find the dump file
DUMP="${XDG_CACHE_HOME:-$HOME/.cache}/ptm/recipes-snapshot.md"
ls -la "$DUMP"
cat "$DUMP"
```

Include the dump verbatim in the report. This is gold for the planner: it shows what PTM *thinks* about every window, including tmux bindings.

### 4c — Reproduce each user-reported failure

For each failure the user mentioned in Step 1:

1. Note the timestamp before the action: `date '+%H:%M:%S.%3N'`
2. Have the user perform the action (or perform it yourself if it's reproducible without their input — e.g. clicking `+ New tmux` needs the user; running a CLI command doesn't).
3. Note the timestamp after.
4. Capture any new stderr output: `tail -n 100 /tmp/ptm-stderr.log`
5. If the failure involves a tmux window, also run `tmux list-sessions` (on the **default** socket — no `-L ptm-diag` here, since we want what PTM sees) and capture.

Put each failure in its own subsection of the report with timestamps, the action, the expected behavior, the actual behavior, and the stderr delta.

### 4d — Shut down PTM cleanly

```bash
kill "$PTM_PID" 2>/dev/null
wait "$PTM_PID" 2>/dev/null
```

---

## 5 — Write the report

Write the report to a timestamped file:

```bash
REPORT="/tmp/ptm-dev1-diagnostic-$(date +%Y%m%d-%H%M%S).md"
```

Use this exact template structure (so the planner can scan it consistently):

```markdown
# PTM dev-VM diagnostic — <timestamp> — <hostname>

## 1. User-reported failures
<verbatim answers to interview questions from Step 1>

## 2. Environment
### OS
<output blocks>

### Tmux config files
<for each existing file: path + verbatim contents in a fenced block>

### X11 / desktop
<output blocks>

### Terminal emulators
<table from Step 2>

### PTM binary
<ldd output, file checks>

## 3. Tmux probes
### Probe 1 — tmux -V
- exit: <code>
- stdout: <verbatim>
- stderr: <verbatim>

### Probe 2 — tmux -L ptm-diag new-session -d -P -F '#{session_name}'
...
(repeat for probes 3-7)

## 4. Live PTM behavior
### 4a — Launch
- pid: <or "did not start">
- stderr: <verbatim or "(empty)">

### 4b — SIGUSR1 dump
<verbatim recipes-snapshot.md contents, or "PTM not running, skipped">

### 4c — Reproductions
#### Failure 1: <short title from user>
- timestamp before: <hh:mm:ss>
- action: <what was done>
- expected: <user's words>
- observed: <what actually happened>
- stderr delta:
  ```
  <new lines from /tmp/ptm-stderr.log between the two timestamps>
  ```

(repeat per failure)

## 5. Diagnostic notes / hypotheses
<your read on what looks like the root cause, EXPLICITLY MARKED as hypothesis, not fact. Keep this short — the planner will form its own conclusions from the raw data above. Most useful: name probes/files that point at a likely cause.>

## 6. Cleanup verification
- Private socket cleaned? <yes/no>
- PTM process killed? <yes/no>
- Any files created during this procedure outside `/tmp`? <list, or "none">
```

After writing the file, print its path. Tell the user:

> "Diagnostic report written to `<path>`. Open it, copy the full contents, and paste it back into the Claude Code session on your primary host. The planner there will design installer fixes based on what's in this report."

---

## 6 — Cleanup checklist (do not skip)

Before ending the session, verify:

1. **Private socket gone:** `ls /tmp/tmux-* 2>/dev/null | grep ptm-diag` returns nothing.
2. **PTM process killed if you launched it:** `pgrep ptm` returns empty (or only the user's pre-existing PTM, if they had one running).
3. **No edits to user config:** confirm verbally to the user that you did not modify `~/.tmux.conf`, `~/.config/tmux/`, or any system file.
4. **No new files outside `/tmp`:** `find ~ -newer /tmp/ptm-diag-marker -type f 2>/dev/null` is empty (you can `touch /tmp/ptm-diag-marker` at the start of Step 1 to use this check).

If any of these are dirty, fix before reporting done.

---

## Notes for the diag Claude

- The host planner has already inspected this machine and confirmed: stock tmux 3.4, no custom config, all PTM tmux call sites degrade gracefully. So the most useful thing you can do is rule out that environment (find what's *different* about this VM).
- If you discover the failure isn't tmux at all (e.g. PTM panics on X11 init, or a missing shared lib in `ldd`), still produce the full report — the planner will write a separate non-tmux fix.
- Be terse in your prose, exhaustive in your data capture. The planner can read 1000 lines of `tmux -V` output; it can't recover information you summarized away.
- If you finish and have spare cycles, you can also `git log --oneline -20` in `~/dev/process-tab-manager` and confirm dev-1 is at the same commit as the planner expects (commit `1606b63` or later as of writing). A stale checkout would explain failures with no environmental cause.
