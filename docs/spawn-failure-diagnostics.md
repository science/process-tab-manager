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

---

## Part C — Agreed plan (locked 2026-05-16, ready for implementation)

> **Audience:** the agent that picks up implementation (expected: dev-2
> Claude session). This section is self-contained — read Part A for the
> dev-1 findings that motivated it, Part B for the options considered,
> and this section for the design we're actually building.

### Context for the implementing agent (what got us here)

1. dev-1 reported `+ New Terminal` / `+ New tmux` failing. Diag found
   `gnome-terminal-server` (pid 2380, 29-day uptime) wedged for
   new-window spawns; existing windows fine. Workaround for the user:
   `pkill -f gnome-terminal-server` or `export PTM_TERMINAL_CMD=xterm`.
2. dev-2 then shipped four commits (`0f5e6f6 → 641c91a`) implementing
   **detection only**: symlink-canonicalize fix, spawn watchdog with
   5s/10s thresholds, `ptm --diagnose` CLI, and `install.sh` terminal
   nudge.
3. User retested on dev-1 after the new binary installed: `+ New
   Terminal` / `+ New tmux` **still appear to do nothing**. Reason: the
   watchdog *correctly* writes warnings to `~/.cache/ptm/ptm-warnings.log`
   and to stderr, but stderr is `/dev/null` for a detached PTM launch
   and the user has no UI signal that anything is wrong. Detection
   works; **surfacing is the gap.**
4. This Part C closes that gap.

### Locked decisions (please don't re-litigate)

| Decision | Rationale |
| --- | --- |
| Detection is **click-driven only**, plus a single PATH-resolve check at startup. No periodic polling. | User only cares about spawn working at the moment of clicking. Idle-time daemon health has no user-visible consequence; the next click catches it within 10 s anyway. |
| Surface via **sidebar banner + `notify-send` on state transitions + click-through popup**. | Banner is always-visible (sidebar is always-visible). Notification grabs attention on the Healthy→Degraded and Degraded→Broken edges only. Popup carries the diagnosis + actions. |
| Each fix has **two buttons: `[Show command]` and `[Run it]`** — show is non-destructive; run requires a second click. | Explicit consent at each step. User can audit before granting execute. |
| **No sudo, no auto-fix.** All actions run as the calling user; nothing happens without a click. | Silent remediation breaks the user's mental model. |
| Persist banner state across PTM restarts via `~/.cache/ptm/health-state.json`. Persist user terminal overrides via `~/.config/ptm/overrides.toml`. | The user's #1 stated concern is "6 weeks later, go through this debug nonsense again." State has to outlive a PTM restart. |

### Detection (two trigger points, one event stream)

Both feed the same `WatchdogEvent` stream the existing watchdog already
emits.

**Trigger 1 — Click-driven (existing).** `tick_watchdog` in `src/main.rs`
(around line 578) already emits `SpawnSlow` at 5 s and `SpawnWedged` at
10 s. **No change needed to this path.** Just consume the events.

**Trigger 2 — One-time PATH resolve at startup.** Right after PTM
constructs its initial `App`, call something like:

```rust
fn resolve_terminal_argv0() -> Result<PathBuf, String> {
    let argv = detect_terminal_command(/* env */, binary_on_path);
    let name = argv.first().ok_or("empty argv")?.clone();
    // PATH lookup (don't use canonicalize alone — see "Known bug")
    binary_on_path_resolved(&name).ok_or(format!("{} not found in PATH", name))
}
```

If `Err`, emit a synthetic `WatchdogEvent::TerminalUnavailable { name }`
event into the same queue the watchdog uses. Banner transitions
straight to Broken on startup. ~30 LOC.

### State machine

States: `Healthy → Degraded → Broken`. Two-way transitions:

| From | To | Trigger |
| --- | --- | --- |
| `Healthy` | `Degraded` | First `SpawnSlow` in session |
| `Healthy` | `Broken` | `SpawnWedged`, `TerminalUnavailable`, or 2× `SpawnSlow` within 5 min |
| `Degraded` | `Broken` | `SpawnWedged` or second `SpawnSlow` within 5 min |
| `Degraded` | `Healthy` | Successful spawn (a wid arrives that matches a `PendingSpawn`) |
| `Broken` | `Healthy` | Successful spawn, OR user clicks `[Run it]` on a fix and the next spawn succeeds, OR user clicks `[Dismiss]` in the popup |

Implementation: `enum HealthState { Healthy, Degraded, Broken }` on
`App`, plus a `Vec<HealthEvent>` rolling buffer (cap ~20 — only used to
populate the popup's "what PTM noticed" line). State changes call
`persist_health_state()`.

### Persistence

**Health state** — `~/.cache/ptm/health-state.json` (alongside
`recipes-snapshot.md` and `ptm-warnings.log`):

```json
{
  "version": 1,
  "state": "Broken",
  "last_transition_at": "2026-05-16T18:25:24-07:00",
  "last_reason_short": "spawn wedged: + New Terminal — no window after 10.1s",
  "recent_events": [
    { "at": "2026-05-16T18:23:11-07:00", "kind": "SpawnWedged", "terminal": "x-terminal-emulator" },
    { "at": "2026-05-16T18:24:02-07:00", "kind": "SpawnWedged", "terminal": "x-terminal-emulator" }
  ],
  "dismissed_until_restart": false,
  "dismissed_at_startup": null
}
```

Loaded at `App::new`. If `dismissed_at_startup == current process startup
time`, the banner stays hidden until a fresh event. Tests must cover:
load-with-missing-file, load-with-malformed-json (recover to Healthy +
log), version-mismatch (treat as missing).

**User overrides** — `~/.config/ptm/overrides.toml`:

```toml
[terminal]
# Wins over PTM_TERMINAL_CMD and TERMINAL env vars. Whitespace-split.
# Empty/missing = no override.
command = "xterm"
```

Read at startup; merged into `detect_terminal_command`'s precedence as
**highest** (above `PTM_TERMINAL_CMD`). Tests: precedence over both env
vars; empty value behaves like absent; whitespace splitting matches
existing detect logic.

### Surfacing

**1. Sidebar banner row.** Reserve 16 px at the very top of the sidebar
(*above* the `+ New Terminal` / `+ New tmux` button row — that placement
puts it where the user's eye naturally lands on click). Layout:

| State | Appearance | Text |
| --- | --- | --- |
| `Healthy` | Hidden (no row, no real-estate cost) | — |
| `Degraded` | Amber background, dark text | `⚠ Terminal spawn slow — click for details` |
| `Broken` | Red background, white text | `❌ Terminals not opening — click for fix` |

Reuse the existing color palette in `src/main.rs` (OneDark). Click
anywhere on the row → open the popup (reuses
`open_context_menu`'s override-redirect + pointer-grab machinery —
search `build_menu_entries` / `draw_context_menu` for the existing
pattern).

**2. `notify-send` on state transitions only.** Fire on
`Healthy→Degraded` and `Healthy/Degraded→Broken`. Never per-event.
Shell out:

```rust
let _ = std::process::Command::new("notify-send")
    .args([
        "--app-name=PTM",
        if broken { "--urgency=critical" } else { "--urgency=normal" },
        if broken { "PTM: terminals not opening" } else { "PTM: terminal spawn slow" },
        &one_line_summary,
    ])
    .spawn();
```

`notify-send` may be absent (rare; it's in `libnotify-bin` on Debian /
Ubuntu). Don't error if the binary's missing; the banner still works.

**3. Popup** (override-redirect, pointer-grabbed, ESC/click-outside
dismisses). Layout target — keep it ~360 px wide × ~280 px tall so it
fits next to the sidebar without overlap. Rough wireframe:

```
┌──────────────────────────────────────────┐
│ PTM noticed                              │
│                                          │
│ 3 terminal spawns failed in 10 minutes.  │
│ Most likely: gnome-terminal-server is    │
│ wedged (running 29 days; 12 stuck        │
│ `gnome-terminal --wait` wrappers).       │
│                                          │
│ Fix 1: Restart gnome-terminal-server     │
│   [Show command]  [Run it]               │
│                                          │
│ Fix 2: Use xterm instead                 │
│   [Show command]  [Run it]               │
│                                          │
│ [Run `ptm --diagnose` for full report]   │
│                                          │
│ [Dismiss]   [Don't show until restart]   │
└──────────────────────────────────────────┘
```

The "PTM noticed" body text is derived from `recent_events` +
`format_watchdog_event` content (which already includes the
copy-paste-safe fix lines — see `src/main.rs:656`). Reuse that
formatter; don't fork the text.

**`[Show command]`** expands a sub-row showing the literal shell line.
No execution. Idempotent.

**`[Run it]`** confirms once (the second click of `Show → Run` IS the
confirmation; no extra dialog). Spawns the command via
`Command::new("sh").arg("-c").arg(line)`, captures stdout+stderr, shows
output in a sub-row for ~3 s, then closes the popup. State transitions
when the next watchdog tick observes the result.

### Diagnosis → fix table (the brain)

A small in-source registry; expand as new symptoms come up.

| Symptom (event pattern + sniff) | "PTM noticed" body | Fix 1 | Fix 2 |
| --- | --- | --- | --- |
| `SpawnWedged` × ≥1, argv[0] resolves to `*gnome-terminal*`, ≥1 stuck `gnome-terminal --wait` proc detected | "N terminal spawns failed; gnome-terminal-server appears unresponsive to new-window IPC (running N days, M stuck wrappers)." | Restart gnome-terminal-server → `pkill -f gnome-terminal-server` | Use xterm → write `overrides.toml` + apply in-memory |
| `TerminalUnavailable` (startup PATH resolve failed) | "PTM's terminal command `<name>` isn't installed." | Install it → `apt show <pkg>` (no auto-install) | Use a different terminal → list of installed terminals from `binary_on_path` |
| `SpawnWedged` × ≥1, argv[0] does NOT resolve to gnome-terminal | "Terminal `<name>` started but no window appeared." | Show `ptm --diagnose` output | Use xterm → as above |
| `SpawnExitedNonZero` × ≥1 | "Terminal exited with code N before opening a window." | Show stderr from log | Use xterm → as above |

Sniff helpers needed:
- `count_stuck_gnome_terminal_wait()`: `pgrep -f 'gnome-terminal --wait'`
  via `Command::new` + `wait_with_output`. ~15 LOC.
- `gnome_terminal_server_uptime_days()`: read `/proc/<pid>/stat` field
  22 (starttime) for the gnome-terminal-server pid (find via
  `pgrep -x gnome-terminal-server`), convert via
  `/proc/uptime` + `_SC_CLK_TCK`. PTM already has /proc walking code
  (`ProcSnapshot::capture_all` and friends in `src/main.rs` around
  line 2800+) — extend or sit alongside.

### Action handlers

**Restart gnome-terminal-server:**
```bash
pkill -f gnome-terminal-server
```
Runs as the calling user. Cinnamon auto-respawns the daemon on next
gnome-terminal launch. Returns exit code 0 if any process was killed,
1 if none matched. UI shows the exit code briefly.

**Use xterm (permanent override):**
1. Ensure `~/.config/ptm/` exists.
2. Read `overrides.toml`, set `[terminal] command = "xterm"`, write back.
3. Update PTM's in-memory `terminal_override` field so the next spawn
   uses xterm without restart.
4. State machine → Healthy. Banner clears. Popup closes after ~1 s
   confirmation text.

**Run `ptm --diagnose`:**
```bash
ptm --diagnose --output /tmp/ptm-diag-<ts>.md
```
PTM already implements this CLI (commit `135924b`). Write to a temp
path; show path in popup so the user can copy it.

### Implementation order (5 commits, each independently shippable)

1. **`feat(health): state machine + persistence + banner row`** (~200 LOC)
   - Add `HealthState` enum, `Vec<HealthEvent>` on App.
   - Add `health_state_path()` + JSON load/save with version handling
     (copy the pattern of `warnings_log_path` at `src/main.rs:713`).
   - Draw the banner row in the existing sidebar render path. Hidden
     when Healthy.
   - **No popup, no notify-send, no fix actions yet.** Just visible
     state.
   - Unit tests: state transitions, persistence round-trip, malformed
     JSON recovery, banner-row hit-testing.
   - **Manual UAT before merging:** force-write a Broken health file to
     `~/.cache/ptm/health-state.json` by hand; launch PTM; confirm red
     banner appears at top of sidebar.

2. **`feat(health): startup PATH check + watchdog → state wiring`** (~80 LOC)
   - Add `binary_on_path_resolved(name) → Option<PathBuf>` (walk PATH,
     return first executable match). **This also unblocks the
     canonicalize bug fix below.**
   - At `App::new` (or wherever startup finalizes), run the resolve
     check; on failure emit `TerminalUnavailable` and transition to
     Broken before the first event-loop iteration.
   - Wire `tick_watchdog`'s emitted events into the state machine. The
     watchdog already emits to stderr + log; add a *third* sink: a
     channel/queue the main event loop drains and passes to the state
     machine.
   - Unit tests: TerminalUnavailable transitions, watchdog event →
     state changes.
   - **Bug to fix in this commit (don't punt):**
     `terminal_argv_for_attach` at `src/main.rs:4114` calls
     `std::fs::canonicalize(&term_argv[0])` directly on a PATH name
     like `"x-terminal-emulator"`, which fails with ENOENT and
     silently falls through to the unfixed basename match. (Visible in
     `ptm --diagnose` output as `canonicalize failed: No such file or
     directory (os error 2)`.) Replace with: PATH-resolve first via
     the new helper, *then* canonicalize the resolved absolute path.

3. **`feat(health): popup with Show-command buttons`** (~150 LOC)
   - Reuse override-redirect popup machinery (search
     `open_context_menu`, `draw_context_menu` in `src/main.rs`).
   - Render the wireframe above. Buttons render but only `[Show
     command]` and `[Dismiss]` / `[Don't show until restart]` do
     anything.
   - Symptom→fix table as a `const` slice of structs.
   - Sniff helpers: stuck-wrapper count, gnome-terminal-server uptime.
   - Unit tests: symptom matching produces expected fix entries;
     formatting deterministic; sniff helpers parse synthetic
     `/proc`-shaped input.
   - **Manual UAT:** trigger Broken state, click the banner, verify
     popup appears with the right "PTM noticed" text and two fix rows.

4. **`feat(health): one-click Run-it action handlers + overrides.toml`**
   (~150 LOC)
   - Implement `overrides.toml` parser/writer (small enough to
     hand-roll — see `src/main.rs:5500+` for similar text-format
     handling, or add a single `toml` crate dep if the team prefers).
   - Wire `[Run it]` for the two initial fixes.
   - Update `detect_terminal_command` to consult the override file as
     highest-precedence.
   - Unit tests: overrides override env vars; malformed TOML doesn't
     crash; `Run it` for restart returns expected outcomes (mock via
     a Command-runner trait).
   - **Manual UAT on dev-1 (which already has a wedged daemon):**
     click `[Run it]` on Restart gnome-terminal-server; verify the
     daemon dies; click `+ New Terminal`; verify a window opens.

5. **`feat(health): notify-send on state transitions`** (~50 LOC)
   - Fire `notify-send` on Healthy→Degraded and *→Broken transitions.
   - Single-shot per transition; debounce on rapid event storms.
   - Don't error if `notify-send` missing; log once that it's absent.
   - Unit test for the debounce.
   - **Manual UAT:** force-trigger a state transition; confirm the
     notification appears in Cinnamon's notification tray and respects
     urgency=critical (persists vs. auto-dismiss).

### Testing strategy

- **Tier 1 (preferred):** unit tests in `#[cfg(test)] mod tests` at the
  bottom of `src/main.rs`. State machine, persistence I/O,
  symptom-matching, override precedence, sniff parsers — all pure
  logic.
- **Tier 2 (Xvfb e2e):** add one new script under `tests/e2e/`:
  `banner_appears_on_wedged_spawn.sh`. Spawn PTM under Xvfb with an
  environment that resolves to a deliberately-hanging fake terminal
  (a small shell script that just `sleep 9999`). Click `+ New
  Terminal`. Verify after 11 s a banner row exists at the top of the
  sidebar (red, click-able). Use `xdotool` for click + window
  inspection (already an e2e dependency).
- **No Tier 2 test** for the popup contents in the first pass — UI
  text changes will churn it. Add later if popup logic becomes
  load-bearing.

### Files & code touchpoints

- **Watchdog source of truth:** `src/main.rs:578` `tick_watchdog`,
  `:656` `format_watchdog_event`, `:773` `emit_watchdog_event`,
  `:737` `append_to_warnings_log`. Reuse the formatter; add a third
  sink (state machine queue).
- **Spawn paths to instrument:** `src/main.rs:3599`
  `spawn_default_terminal`, `:3639` `spawn_attach_terminal`. Already
  return `Option<Child>` after dev-2's refactor.
- **Terminal detection:** `src/main.rs:3558` `detect_terminal_command`.
  Add override precedence at the top of the chain.
- **Canonicalize bug to fix:** `src/main.rs:4114`. See step 2.
- **Sidebar rendering:** the renderer block at the top of `src/main.rs`
  (after the data-model section). Banner row is a new `DisplayRow`
  variant or a separate header-row code path — author's choice.
- **Popup machinery to reuse:** `open_context_menu`,
  `draw_context_menu`, `build_menu_entries`, plus the pointer-grab
  pattern in the event loop. Search for "context menu" in `main.rs`.
- **Persistence pattern to copy:**
  `~/.cache/ptm/recipes-snapshot.md` writer (the SIGUSR1 path) and
  `warnings_log_path` for the cache-dir convention; `XDG_CACHE_HOME`
  → `~/.cache/ptm/` with fallback.

### Things to verify before starting step 1

- Read this entire Part C section.
- Read Part A and Part B for context, but don't re-litigate decisions
  in the "Locked" table above.
- Run `ptm --diagnose --output /tmp/diag.md` on dev-2 and confirm the
  watchdog logfile path matches `warnings_log_path()` in source.
- Confirm the canonicalize bug still exists in `src/main.rs:4114`
  (it does as of 2026-05-16; if a later commit fixed it, skip that
  part of step 2).
- Confirm there's no in-flight branch on dev-2 already starting this
  work — coordinate before duplicating effort.

### What is **not** in this plan (out of scope for this iteration)

- Periodic / background health probes. Click-driven only by design
  decision above.
- Auto-execution of any fix without an explicit `[Run it]` click.
- Settings dialog or preferences UI beyond `overrides.toml`.
- Surfacing for failures *other than* terminal/tmux spawn (e.g.
  rendering bugs, X11 errors) — same machinery could be extended later
  but isn't in this commit set.
