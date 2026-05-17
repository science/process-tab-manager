# PTM spawn-failure diagnostics — dev-1 findings + proactive design

**Companion to:** `docs/tmux-install-diagnostics.md` (the diag-agent procedure).

This file has two parts:

1. **Part A — dev-1 findings (2026-05-16).** What the diag agent uncovered on
   peer VM `dev-1` after the user reported both `+ New Terminal` and
   `+ New tmux` failing. The procedure was followed with the planner-supplied
   shift: weight terminal-setup over tmux-config.
2. **Part B — Proactive design proposals.** Concrete ideas for how PTM
   itself could notice this class of failure, surface it to the user, and
   apply a fix on consent. Ordered roughly by effort-to-payoff. Each
   proposal is a sketch, not an approved spec.

The verbatim raw report this file is distilled from lived at
`/tmp/ptm-dev1-diagnostic-20260516-144306.md` on dev-1 (ephemeral).

---

## Part A — dev-1 findings

### Environment (the bits that matter)

- Ubuntu 24.04.4 LTS, X11/Cinnamon, tmux 3.4.
- **No tmux config files exist** anywhere (`~/.tmux.conf`,
  `~/.config/tmux/tmux.conf`, `/etc/tmux.conf` all absent). Same as the
  planner's host; rules out tmux config entirely.
- Terminals installed: `gnome-terminal`, `xterm`.
  `x-terminal-emulator` → `/usr/bin/gnome-terminal.wrapper` via the
  Debian/Ubuntu alternatives system.
- `PTM_TERMINAL_CMD`, `TERMINAL`, `DEFAULT_TERMINAL` all empty.
- PTM source HEAD `1606b63`, binary built 2026-05-13, `ldd` clean.

### What PTM sees (SIGUSR1 dump, abridged)

- 7 visible windows tracked, all children of `gnome-terminal-server`
  (pid **2380**).
- All 7 Layer-1 captured; all 7 Layer-2 unreachable with the same reason:
  "8 shell descendants under window pid 2380; title `<…>` did not
  uniquely match" — the standard `gnome-terminal-server` PID-collision
  problem (already known to PTM, irrelevant to today's bug).
- **All 7 rows have `Tmux binding: none`** — despite titles like
  `claude - ~/dev/process-tab-manager`, no row is actually inside tmux.
  The user's default-socket tmux server is not running.

### Tmux probes (private socket `ptm-diag`)

All 7 probes from the diagnostic procedure pass cleanly. Tmux on dev-1
is fully healthy:

| Probe | Command shape | Result |
| --- | --- | --- |
| 1 | `tmux -V` | exit 0, `tmux 3.4` |
| 2 | `new-session -d -P -F '#{session_name}'` | exit 0, name `0` |
| 3 | `list-sessions` | exit 0, `$0 0 0` |
| 4 | `display-message ... pane_id pane_pid` | exit 0, `%0 3929167` |
| 5 | `list-clients` | exit 0, empty |
| 6 | `kill-session -t 0` | exit 0 |
| 7 | cleanup | socket file removed |

### The actual bug: wedged `gnome-terminal-server`

**Direct evidence on dev-1, 2026-05-16:**

- `gnome-terminal-server` (pid 2380) etime **29 d 16 h 50 m**. Hosts the
  7 existing windows fine. **New** window spawns hang forever.
- Repro: `DISPLAY=:0 x-terminal-emulator &` →
  `python3 /usr/bin/gnome-terminal --wait` spins in state `R`,
  `wchan = -`. No new wid appears in `_NET_CLIENT_LIST` after 15 s of
  polling. stderr empty.
- DBus introspect on `org.gnome.Terminal /org/gnome/Terminal/Factory0`
  returns normally — the daemon is alive, only the spawn-new-window IPC
  handler is hung.
- Wedge is **persistent**: 6 pairs of `gnome-terminal --wait` +
  `gnome-terminal.real --wait` are stuck on this VM with start times
  spanning Apr 22 → May 10 — one pair per past user attempt that
  silently never produced a window.
- 7 `[gnome-terminal] <defunct>` zombie children parented directly to
  PTM (pid 3911606), timestamps 10:49 and 14:33 today — these are PTM's
  spawn attempts whose grandchildren died without ever being reaped.
- xterm, spawned the **exact same way** (`Command::new("xterm").spawn()`),
  creates a real 484x316 viewable window in <1 s on the same display.

The proximate user-visible bug is therefore a long-uptime hang in
`gnome-terminal-server` — outside PTM's control. **But** PTM has zero
visibility into it: `let _ = Command::new(...).spawn()` is fire-and-forget,
so every click of `+ New Terminal` / `+ New tmux` looks "successful" to
PTM even though nothing happens for the user.

### Latent secondary bug found while investigating

`terminal_argv_for_attach` (src/main.rs:3618) chooses the
shell-command separator by basename match:

```rust
let term_name = std::path::Path::new(&term_argv[0])
    .file_name().and_then(|s| s.to_str()).unwrap_or("");
let separator = match term_name {
    "gnome-terminal" | "ptyxis" => "--",
    _ => "-e",
};
```

When `detect_terminal_command` falls back to `["x-terminal-emulator"]`
(common on Ubuntu/Debian when neither env var is set), the basename is
`"x-terminal-emulator"` and `-e` is chosen. The binary that runs is
`gnome-terminal.wrapper`, which on Debian does translate `-e CMD` → `-- CMD`
as a compatibility shim. So today this happens to work. But:

- It only works because of a Debian-specific wrapper convention.
- If `update-alternatives` ever points `x-terminal-emulator` at a
  terminal that doesn't accept `-e` (or stops translating it), the
  attach path breaks silently.
- It is **not** what PTM intends — the code clearly thinks
  "if gnome-terminal, use `--`".

**Fix:** before the basename match, canonicalize the symlink and strip
trailing `.wrapper`/`.real` suffixes:

```rust
let resolved = std::fs::canonicalize(&term_argv[0])
    .unwrap_or_else(|_| std::path::PathBuf::from(&term_argv[0]));
let raw = resolved.file_name().and_then(|s| s.to_str()).unwrap_or("");
let term_name = raw
    .strip_suffix(".wrapper").unwrap_or(raw)
    .strip_suffix(".real").unwrap_or(raw);
let separator = match term_name { "gnome-terminal" | "ptyxis" => "--", _ => "-e" };
```

This is a small, low-risk change that should land regardless of which
proactive proposals below are adopted.

### Immediate workarounds the user can apply

1. **Bypass gnome-terminal:** `export PTM_TERMINAL_CMD=xterm` in
   `~/.bashrc`, then relaunch PTM. xterm works perfectly on this VM.
2. **Restart the wedged daemon:** `pkill -f gnome-terminal-server`.
   Cinnamon will auto-respawn it on next launch. New windows will work
   until the daemon wedges again (currently every ~29 days on this VM).
3. Both, for resilience.

### What is **not** the cause

Listed because the procedure asks the planner to see what was ruled
out:

- Not tmux: no conf, all probes pass.
- Not a missing binary: both `gnome-terminal` and `xterm` installed.
- Not X11 access: xdpyinfo / xwininfo / xprop / xset all work.
- Not display lock: cinnamon-screensaver `GetActive → false`.
- Not a missing shared lib: ldd clean.
- Not a stale PTM build: HEAD matches expected commit.

---

## Part B — Proactive design proposals

Goal: PTM should *notice* failures like the one above, *tell* the user in
language they can act on, *propose* concrete fixes, and on consent
*apply* them. The current design is invisible-when-broken: a click does
nothing, and PTM keeps drawing as if everything is fine.

The proposals below are independent — each one stands alone, none
depends on another. Effort estimates are rough order-of-magnitude.

### Proposal 1 — Spawn watchdog (highest payoff per LOC)

**Problem it solves:** PTM has no idea whether its `Command::new(...).spawn()`
actually produced a window. The user sees nothing happen and assumes PTM
is broken.

**Sketch:**

When PTM spawns a terminal (either `spawn_default_terminal` or
`spawn_attach_terminal`), instead of fire-and-forget:

1. Snapshot the current set of `_NET_CLIENT_LIST` wids → `before`.
2. `spawn()` and hold onto the child PID.
3. Register a `PendingSpawn { pid, before_wids, deadline: now + 5s }`
   in a small Vec on `App`.
4. On every event-loop refresh (we already poll the X server), check
   each `PendingSpawn`:
   - If a new wid has appeared in `_NET_CLIENT_LIST` since `before_wids`
     AND its `_NET_WM_PID` maps somewhere into the spawned process
     subtree (or its WM_CLASS is `Gnome-terminal` and our spawn's argv
     resolves to gnome-terminal), the spawn worked — drop the entry.
   - If `now > deadline` and the child process is still alive but no new
     wid arrived, **the spawn is wedged.** Surface a non-modal warning
     (see Proposal 2 for *how*).
   - If `now > deadline` and the child has exited with non-zero status,
     log the exit code; the spawn failed differently.

PTM already has the EWMH walking, the process-subtree analysis
(`walk_to_window_owner`), and the refresh loop. This is plumbing on
top of existing primitives.

**Estimated effort:** ~150 LOC + 5-10 unit tests. The watchdog logic is
pure and testable without X11.

**Risks:**

- Multiple in-flight spawns within 5 s could race for the same new wid.
  Mitigate by matching on `_NET_WM_PID` ∩ child subtree, not order.
- `gnome-terminal-server` reparents new windows under its own pid (2380),
  not the spawning python wrapper. PTM already special-cases
  `gnome-terminal-server` in attribution code (see comments around
  src/main.rs:2660 / 2806); reuse that.

### Proposal 2 — Sidebar health indicator

**Problem it solves:** PTM needs a *surface* to communicate non-fatal
problems without modal interruption. Modals are wrong here — the user
might not even be looking at PTM when the spawn failed.

**Sketch:**

Reserve a thin strip at the very top or very bottom of the sidebar
(maybe ~14 px tall) for a status indicator. Three states:

- **Hidden / no row.** Everything's healthy. (Default; no real estate cost.)
- **Amber row** with a glyph + short text: `⚠ Terminal spawn slow` or
  `⚠ 3 windows didn't open`. Click to expand.
- **Red row:** something tmux- or X11-level is broken; same UX.

Clicking the row opens a small override-redirect popup (same machinery
PTM uses for the context menu) listing:

- **What PTM observed** (one sentence): "Clicked `+ New Terminal` 3 times
  in the last 2 minutes; none of them produced a visible window."
- **Most likely cause** (looked up from a small built-in table — see
  Proposal 3).
- **Suggested fix** with action buttons (Proposal 3).
- **Dismiss** button + **Don't show again until restart** option.

**Estimated effort:** ~250 LOC. Re-uses the existing override-redirect
popup + pointer-grab plumbing. The drawing is one extra
`build_display_rows` row plus the popup.

**Risks:**

- Sidebar is intentionally minimal. Visual budget is tight. Indicator
  must be opt-out friendly and stay quiet when nothing's wrong.
- Don't auto-show the popup; require a click. Reduces interruption.

### Proposal 3 — Built-in remediation table + one-click fixes

**Problem it solves:** Detection is useless without a fix recipe. The
table converts a detected failure into 1-3 concrete user actions.

**Sketch:**

A small in-source registry of `(SymptomPattern, Remediation)` pairs:

```rust
struct Remediation {
    title: &'static str,            // "Terminal spawn appears wedged"
    cause: &'static str,            // One-sentence explanation
    actions: &'static [Action],     // Buttons to render
}

enum Action {
    /// Run a shell command, ask before, show output in popup.
    RunShell { label: &'static str, cmd: &'static str, needs_root: bool },
    /// Set a runtime override + persist to ~/.config/ptm/overrides.toml
    SetTerminalOverride { label: &'static str, argv: Vec<String> },
    /// Open a URL (xdg-open) for docs.
    OpenDoc { label: &'static str, anchor: &'static str },
}
```

Initial entries the dev-1 case calls for:

| Symptom | Cause shown to user | Actions |
| --- | --- | --- |
| Watchdog timeout + argv[0] resolves to `gnome-terminal*` + ≥1 stuck `gnome-terminal --wait` proc | "Your gnome-terminal daemon stopped responding to new-window requests. (Existing windows are unaffected.) This is usually fixed by restarting it." | `[Restart gnome-terminal-server]` (runs `pkill -f gnome-terminal-server`, no sudo) · `[Use xterm for this session]` · `[Use xterm permanently]` (writes config) · `[Help]` |
| Watchdog timeout + argv[0] not found | "PTM's terminal command `<argv>` doesn't exist on this system." | `[Pick a terminal]` (lists installed terminals from a sniff) · `[Help]` |
| `create_new_tmux_session()` returns None + tmux not in PATH | "tmux isn't installed. The `+ New tmux` button needs it." | `[Show install command]` (prints `sudo apt install tmux` to popup) · `[Hide this button]` |
| Repeated tmux-attach spawn that times out only when argv[0] is `x-terminal-emulator` | "PTM thinks `x-terminal-emulator` may not handle `-e` correctly on this system." | `[Use `--` separator]` (config flag) · `[Help]` |

**Estimated effort:** ~200 LOC for the table + button rendering, plus
~150 LOC of carefully-scoped action handlers. Each new Remediation
entry is then a few lines.

**Important constraints on the actions:**

- **Never run anything destructive without explicit click-through.**
  Even `pkill -f gnome-terminal-server` is *user-process-only* (no
  sudo) but still closes nothing — gnome-terminal-server has no live
  windows of its own, only daemon state. Still: confirm before running.
- **Never auto-fix.** Detection → suggestion → consent → action. No
  silent remediation.
- **No sudo.** Everything in the table should run as the calling user.
  If a fix needs root (e.g. apt install), print the command, don't run
  it.

### Proposal 4 — `ptm --diagnose` CLI subcommand

**Problem it solves:** Today, diagnosing a peer-VM issue requires a
human-Claude pair and a long procedure doc. A built-in `ptm --diagnose`
that runs the equivalent of `docs/tmux-install-diagnostics.md` makes
remote support a one-liner.

**Sketch:**

`~/.local/bin/ptm --diagnose [--output FILE]`:

- Does not start the X loop. Runs probes inline:
  - tmux: `-V`, then `-L ptm-diag-<pid>` new-session/list/kill cycle.
  - terminal: prints what `detect_terminal_command` would return for
    the current env; for each result, also reports
    `canonicalize(argv[0])` and a 5 s `Command::new(argv[0]).args(["--help"]).output()`
    timing probe (some terminals do `--help`, some don't — fall back to
    `--version`; if neither, just spawn and watchdog).
  - X11: connects (read-only), prints display, screen geometry,
    `_NET_SUPPORTING_WM_CHECK` value.
  - Process state: scans for stuck `gnome-terminal --wait` / defunct
    `[gnome-terminal]` children.
  - PTM's own state: dumps the same recipe-snapshot.md the SIGUSR1
    handler writes today, but inline.
- Writes a single markdown report. Same template as the manual procedure
  produces today.

**Estimated effort:** ~400 LOC. Significant share is just refactoring
the SIGUSR1 dump path to be callable without a live `App`.

**Payoff:** Eliminates 90% of the manual procedure. A user with a broken
PTM types `ptm --diagnose > report.md` and pastes.

### Proposal 5 — `~/.config/ptm/overrides.toml` for persistent fixes

**Problem it solves:** The "Use xterm permanently" button in Proposal 3
needs somewhere to write the choice. Env vars (`PTM_TERMINAL_CMD`) are
per-shell-rc and don't survive Claude-Code-style invocations cleanly.

**Sketch:**

A single TOML file PTM reads at startup:

```toml
# ~/.config/ptm/overrides.toml
[terminal]
# Wins over PTM_TERMINAL_CMD if set. Empty string = use env var / defaults.
command = "xterm"
# Force a specific separator for terminal_argv_for_attach. "auto" = current
# basename match. "--" or "-e" override.
attach_separator = "auto"

[health]
# Disable the sidebar status indicator. (Default: shown only when an issue
# is detected.)
indicator = "auto"  # "auto" | "off"
# How long PTM waits for a spawned terminal to produce a wid.
spawn_watchdog_seconds = 5

[remediation]
# Suppress specific remediation suggestions the user has dismissed
# permanently. PTM never adds to this list silently — only via the
# popup's "Don't show again ever" button.
suppressed = []
```

PTM already has a profiles directory at
`~/.config/ptm/profiles/default/groups`. The TOML lives alongside.

**Estimated effort:** ~150 LOC (parse, merge with env, surface to the
existing spawn paths). Use `toml` crate or a tiny hand-rolled parser
(file is small enough).

### Proposal 6 — Install-time probes in `install.sh`

**Problem it solves:** Catch problems at install instead of at first
use. dev-1's stuck-gnome-terminal-server condition is detectable in 8
seconds.

**Sketch (additions to `install.sh`):**

After building/installing the binary, before exiting:

```bash
echo "== PTM environment sanity =="

# What terminal would PTM pick?
PICK="$(~/.local/bin/ptm --print-terminal-command 2>/dev/null || true)"
echo "PTM will spawn: $PICK"

# If that resolves to gnome-terminal*, time-probe it.
case "$(readlink -f "$(echo "$PICK" | awk '{print $1}')")" in
    *gnome-terminal*)
        echo "Probing gnome-terminal responsiveness (8 s timeout)..."
        if ! timeout 8 gnome-terminal --wait -- /bin/true 2>/dev/null; then
            cat <<'WARN'
[warn] gnome-terminal did not respond within 8 seconds. New terminal
       spawns may hang. To fix:
         - Restart it:  pkill -f gnome-terminal-server
         - Or set:      export PTM_TERMINAL_CMD=xterm
WARN
        fi ;;
esac

# Always print the chosen terminal so the user sees what PTM will use.
echo "If that's not what you want, set PTM_TERMINAL_CMD."
```

The `ptm --print-terminal-command` flag is one new line on `App`'s argv
handling: print what `detect_terminal_command` returns and exit.

**Estimated effort:** ~50 LOC across install.sh + main.rs. Very cheap.

### Proposal 7 — Zombie / wedged-spawner observability

**Problem it solves:** PTM's own child processes accumulating as
defunct (or stuck-but-alive) is itself a useful signal. dev-1 had 7
`[gnome-terminal] <defunct>` parented to PTM today — that's PTM's own
spawn attempts dying. PTM doesn't notice or reap them.

**Sketch:**

- When PTM spawns a terminal, keep the `Child` handle (don't drop it).
- On each refresh, `child.try_wait()` non-blocking:
  - `Ok(Some(status))`: child exited. Log status. Reap.
  - `Ok(None)`: still running. If `now > spawn_time + watchdog_seconds`
    *and* no wid appeared, fire the Proposal-1 timeout path.
  - `Err(_)`: drop the entry, log.
- Periodically (every N refreshes) scan `/proc/<ptm_pid>/task/.../children`
  for defunct entries PTM didn't spawn directly (e.g. python wrappers
  that exec'd and died); reap with `waitpid(-1, WNOHANG)`.

**Estimated effort:** ~80 LOC. Mostly a small `PendingSpawn`/`Child`
table; the reap is one syscall.

**Bonus signal:** the *rate* of failed spawns is itself an alarm.
"3 spawn attempts in 90 s, 0 windows opened" is high-confidence broken.
The indicator in Proposal 2 should escalate yellow → red on this signal.

### Proposal 8 — Terminal sniff at startup

**Problem it solves:** Even before any spawn fails, PTM could verify
the terminal it would pick is plausible.

**Sketch:**

At startup, after `detect_terminal_command`:

1. Resolve `argv[0]` to a real path.
2. Check the binary exists and is executable.
3. If it's a known terminal (table of basenames), record the expected
   separator and any quirks (e.g. `gnome-terminal --wait` is a
   long-running blocker, `kitty` doesn't reparent, etc.).
4. If it's unknown, default to `-e` (current behaviour).

If any check fails, raise the Proposal-2 indicator at startup, before
the user even tries to click `+ New Terminal`.

**Estimated effort:** ~100 LOC. Pure logic, no X11 needed.

---

## Recommended sequencing

If we were to land *some* of this, the highest payoff in the shortest
diff is:

1. **Fix the latent separator bug** (Part A: canonicalize symlinks).
   ~20 LOC + 2 tests. Pure win, no UX surface.
2. **Proposal 1 (spawn watchdog)** + **Proposal 7 (Child reaping)**.
   ~250 LOC total. Backend work, no UI. Makes the failure
   *legible to PTM* — prerequisite for any UX surfacing.
3. **Proposal 4 (`ptm --diagnose`)** + **Proposal 6 (install probes)**.
   These give immediate value to *anyone* installing PTM on a new VM,
   including the peer-VM diag-Claude pattern. Both reuse plumbing
   PTM has.
4. **Proposal 2 (indicator) + Proposal 3 (remediation table)**.
   This is where UX work begins. Worth doing only after #2 lands.
5. **Proposal 5 (overrides.toml) + Proposal 8 (startup sniff)**.
   Polish + persistence.

Steps 1–3 alone would have turned the dev-1 failure mode from "PTM
silently does nothing, user files an issue" into "`ptm --diagnose`
prints a 2-line warning naming the wedged daemon and the exact one-line
fix" — without any change to the sidebar UI.

---

## Out of scope (intentionally)

- **Replacing the terminal-spawn API entirely** (e.g. talking DBus to
  gnome-terminal-server directly to skip the wrapper). PTM is meant to
  delegate terminal config to the user's environment; that boundary
  should not move.
- **Bundling a terminal** (e.g. shipping a vendored libvte build). PTM
  is one binary; that stays.
- **A full settings UI.** A single `overrides.toml` covers the cases
  detected so far without paying for a settings dialog.
- **Auto-restarting `gnome-terminal-server`** without user consent.
  Even though it's user-process-only and reversible, silent fixes
  break the user's mental model.
