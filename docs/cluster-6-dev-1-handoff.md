# Cluster 6 — dev-1 debug handoff

**Status (2026-05-16, evening):** Cluster 6 shipped (banner / popup /
notify-send / overrides.toml / canonicalize fix / claim→watchdog
lifecycle fix). Unit + e2e tests green. **UAT on dev-1 has not validated
the core behaviour.** This file captures what's known, what's theorised
but unverified, and the directive for the next session.

The prior agent (the one writing this file) leaned heavily on theory in
the last few rounds. The user explicitly pushed back: pivot to real
debugging on dev-1, not more speculation.

---

## Hard rule for the next session

**Do NOT manually unwedge `gnome-terminal-server` on dev-1.** That
means:

- No `pkill -f gnome-terminal-server`
- No `pkill -f 'gnome-terminal --wait'` to clear the stuck wrappers
- No `sudo systemctl restart gdm` / cinnamon
- No reboot

The whole point of Cluster 6 is for **PTM to detect and offer to fix
this exact state**. If we unwedge it manually, we lose the only known
reproduction of the bug. The dev-1 VM is precious.

If at any point you find yourself wanting to "clean up the test
environment" before debugging — stop. The dirty environment IS the
test environment.

---

## What shipped

Six commits on `main` between `8ea53e8` and `e7621ee`:

| Commit | What it does |
| --- | --- |
| `8ea53e8` | Part C plan (docs only) |
| `23aff35` | HealthBoard state machine + persistence + sidebar banner |
| `c10ce75` | Watchdog → state wiring, startup PATH check, canonicalize-on-bare-name fix in `terminal_argv_for_attach` |
| `517fbd3` | Popup with `[Show command]` (no `[Run it]` yet) |
| `f002928` | `[Run it]` handlers + `overrides.toml` (terminal override) |
| `773d7fa` | `notify-send` on Healthy→Degraded and *→Broken transitions |
| `e7621ee` | **The key fix:** `claim_pending_spawns` no longer drops the head entry at 5s — the watchdog owns lifecycle now, so SpawnWedged at 10s can actually fire |

State files PTM now reads/writes:

- `~/.cache/ptm/health-state` — board snapshot, written on every
  transition or new event
- `~/.cache/ptm/ptm-warnings.log` — rolling log (256 KB cap) of
  watchdog stderr lines
- `~/.config/ptm/overrides.toml` — written by the `[Use xterm]` Run-it
  button

The banner sits above the `+ New Terminal` / `+ New tmux` buttons.
Hidden when state is Healthy (zero-height row). Click → opens an
override-redirect popup with a diagnosis body and 1–2 fix rows.

---

## What was actually observed on dev-1

Each is a verbatim user report.

### Observation 1: re-installed, re-opened, clicked, nothing happened

After `./install.sh` + relaunch:

- No banner at startup
- Clicked `+ New Terminal` / `+ New tmux` — terminals didn't open
  (same as before Cluster 6)
- No banner appeared after the click either

At that point we had **no diagnostic data** — no health-state file, no
log entries.

### Observation 2: diagnostic data after a fresh re-install

```
$ ptm --version
ptm 2.0.0

$ cat ~/.cache/ptm/health-state
cat: /home/steve/.cache/ptm/health-state: No such file or directory

$ tail -20 ~/.cache/ptm/ptm-warnings.log
tail: cannot open '/home/steve/.cache/ptm/ptm-warnings.log' for reading: No such file or directory

$ tail -10 /tmp/ptm-stderr.log
tmux 3.4

$ ls -la $(which ptm) /tmp/ptm-target/release/ptm
lrwxrwxrwx 1 steve steve      27 Mar 21 12:37 /home/steve/.local/bin/ptm -> /tmp/ptm-target/release/ptm
-rwxrwxr-x 2 steve steve 1420840 May 16 22:22 /tmp/ptm-target/release/ptm

$ ls -la ~/.cache/ptm/
total 16
drwxrwxr-x  2 steve steve 4096 May 16 14:39 .
drwx------ 20 steve steve 4096 May 16 14:39 ..
-rw-rw-r--  1 steve steve 4720 May 16 18:25 recipes-snapshot.md
```

Interpretation:
- Binary is the new one (timestamp May 16 22:22, version 2.0.0)
- **Health state file never got written.** Watchdog never logged
  anything to the warnings log either. Either the watchdog isn't
  firing or the writes are failing silently.
- User waited "a long time" (well past 12s) after clicking.

This triggered the `e7621ee` fix — `claim_pending_spawns` was dropping
the head entry at 5s, before the watchdog could escalate to wedge at
10s. The fix made the regression test
`claim_then_watchdog_escalates_to_wedge_when_no_wid_appears` pass.

**But it has NOT been retested on dev-1.** That's the immediate
TODO.

### Observation 3: terminals started opening intermittently

After installing the `e7621ee` build (which includes the watchdog
fix), the user reported "terminals were opening without waiting 12s."
But:

```
$ pgrep -af 'gnome-terminal --wait' | wc -l
8
$ ps -o etime= -p $(pgrep -x gnome-terminal-server)
30-01:19:24
```

Stuck wrappers grew from 6 → 8. Server uptime unchanged (30d+). **The
server did NOT restart.** The wedge condition was not cleared.

So gnome-terminal-server's IPC handler is **intermittently
responsive** on dev-1: some clicks get a window, some hang. The 8
stuck wrappers are the times it hung; the working spawns are the times
it didn't.

This means UAT on dev-1 is non-deterministic. Some clicks will catch
the wedge (and let us test the banner). Some won't.

### Observation 4: green bar missing on tmux windows

User reports the tmux green bar isn't appearing on tmux windows
launched by PTM on dev-1.

We don't yet know which "green bar" — PTM's sidebar marker glyph (a
small green digit at the right edge of an attached row) or tmux's
own status bar inside the terminal. **Needs clarification on dev-1
with a screenshot.**

The SIGUSR1 recipe dump on dev-1 shows nine gnome-terminal windows.
Exactly one (window 9) has a tmux binding (session `1`). So PTM's
sidebar should show its session marker glyph on row 9 only. If the
user means glyphs are missing from other rows, those rows are not
actually tmux clients per the dump.

### Observation 5: groups scrambled on every upgrade

User: "every time I upgrade ptm, it scrambles my tabs and group
associations."

Recipe dump shows a group called `Vector Traversals` containing one
window labelled `fg - ~/dev/movie-night` — almost certainly a
mis-association.

This is pre-existing behaviour, not introduced by Cluster 6. It's a
distinct issue and is captured here so we don't lose it.

---

## Theories I (the prior agent) advanced — flagged as theory, not fact

The user pushed back specifically on these. Including for the record:

| # | Theory | Status |
| --- | --- | --- |
| T1 | The watchdog kill_child path unwedged the server by killing the stuck wrappers | **Disproven** — server uptime still 30d, wrapper count grew not shrank |
| T2 | Server's IPC handler is intermittently responsive | **Unverified** but consistent with wrapper-count growing on hung clicks while some clicks succeed |
| T3 | PTM's session-marker glyph (not tmux's status bar) is what's missing | **Unverified** — needs screenshot |
| T4 | SIGTERM via pkill kills PTM without saving, causing group scrambling | **Unverified** — code review confirms no SIGTERM handler exists, but the causal chain to "scrambled groups" isn't proven |
| T5 | Multi-window-same-label rematch is fragile for gnome-terminal users (3 rows with identical label+wm_class can't be disambiguated) | **Unverified** — true in principle, not measured |

None of T2–T5 has been verified by instrumentation, only by code
reading and recipe-dump interpretation. The next session should treat
them as starting hypotheses, not conclusions.

---

## What the next debugging session should actually do

### Step 1: confirm the watchdog fix took effect

This is the most basic gate. After the `e7621ee` build is installed
and PTM is relaunched:

```bash
# Verify install
ptm --version  # should be 2.0.0

# Launch with stderr captured
ptm 2>&1 | tee /tmp/ptm-stderr.log &

# Click + New Terminal once.
# WAIT 12+ SECONDS.
# Then:
cat ~/.cache/ptm/health-state          # MUST exist and show STATE Broken
tail -30 ~/.cache/ptm/ptm-warnings.log # MUST contain "spawn wedged" event
```

If those files are still empty/missing after 12s of waiting on a hung
click, the fix did not take effect or there's a second bug. Don't move
on until this gate passes.

If gnome-terminal-server happens to be cooperative when you click
(non-deterministic per Observation 3), the click will succeed and the
watchdog will silently note success. Try several times.

### Step 2: add debug logging to the spawn / claim / watchdog hot path

The prior agent's instinct on every dev-1 failure was to add more
theory. Resist that. Instead, instrument the live code so the next
report has data, not guesses.

Suggested additions (all gated by an env var so prod is quiet):

```rust
// At enqueue_spawn:
if std::env::var("PTM_DEBUG_SPAWN").is_ok() {
    eprintln!("[ptm-debug] enqueue_spawn kind={:?} queue_depth={}",
              kind, self.pending_spawns.len());
}

// At record_dispatch:
if ... {
    eprintln!("[ptm-debug] record_dispatch child_pid={:?}",
              child.as_ref().map(|c| c.id()));
}

// At every tick_watchdog call site (or inside the function):
if ... {
    eprintln!("[ptm-debug] tick_watchdog now={:?} queue_depth={} head_state={:?} elapsed={:?}",
              now, spawns.len(), spawns.first().map(|s| s.state),
              spawns.first().map(|s| now.saturating_duration_since(s.spawned_at)));
}

// At every claim_pending_spawns call site:
if ... {
    eprintln!("[ptm-debug] claim queue_depth={} new_wids={:?} prior_wids={}",
              pending.len(), new_wids, prior_wids.len());
}
```

Then on dev-1:

```bash
PTM_DEBUG_SPAWN=1 ptm 2>&1 | tee /tmp/ptm-debug.log
```

Click `+ New Terminal`, wait 15s, then `cat /tmp/ptm-debug.log`. The
output will tell us exactly which path the spawn followed — whether
the watchdog ticked, whether the entry was in the queue at each tick,
and whether the click even reached `enqueue_spawn` (button hit-test
geometry might be wrong with the banner taking offset, for example).

Real data from this run will obviate ~80% of the theorising done so
far.

### Step 3: clarify the green-bar question

On dev-1, take a screenshot of PTM's sidebar with at least one tmux
client visible. Also: open a terminal NOT via PTM, run `tmux attach
-t 1`, and screenshot that. Compare.

If the missing element is PTM's row marker → bug in PTM's marker
draw or `item.session` assignment.

If the missing element is tmux's own status bar (the strip inside the
terminal window) → tmux config issue, nothing PTM can fix.

### Step 4: fix the SIGTERM no-save bug if it's confirmed

Test: on dev-1, drag a row to a new group, immediately `pkill ptm`,
relaunch, see if the move persisted. If it didn't, install a SIGTERM
handler that saves geometry + groups before exit.

This is small and contained — ~30 LOC — but it should follow Step 1
gate-passing, not preempt it.

### Step 5: only after Steps 1–4 land — tackle rematch fragility

The "groups scrambled on upgrade" pattern is more architectural. It
likely needs at least one of:

- Tier 0c: window-order-at-save matching (preserve display_order of
  same-label-same-class members so the Nth one in the saved file
  matches the Nth live one in `_NET_CLIENT_LIST` order)
- Title-prefix history (track recently-seen titles so a saved member
  with prior label X matches a live window currently labelled X even
  if other windows currently share the active label)
- `_NET_WM_WINDOW_ID` reuse — X11 sometimes reuses wids for short-lived
  windows, but for long-running ones the wid is stable. Saving the
  wid as a hint (lowest-priority match tier) might help cases where
  the user didn't close any windows between save and reload.

Don't speculate further until Step 1 actually passes — the rematch
issue might be hiding behind a more fundamental problem (e.g., save
not firing at all because of SIGTERM).

---

## Anti-patterns to avoid in the next session

- **Don't add features before validating Cluster 6 works.** The whole
  cluster is unvalidated end-to-end on the real failure mode. Adding
  more code before the existing code is proven working makes the
  surface bigger, not smaller.
- **Don't rationalise away a missing file.** When the user reports
  `~/.cache/ptm/health-state` doesn't exist after a wedged click,
  that's the bug — not "user didn't wait long enough" (the user
  waited; they reported it explicitly).
- **Don't propose unwedging the server.** Stated above; restating
  here because it's the single most tempting wrong move.
- **Don't write more docs about theories.** This doc is the last
  theory doc. The next artefact should be either (a) instrumented
  debug output from dev-1 or (b) a fix backed by that output.

---

## Files / code touch-points

- Watchdog source of truth: `src/main.rs` around `tick_watchdog`
  (~line 580), `format_watchdog_event` (~656), `emit_watchdog_event`
  (~773), `append_to_warnings_log` (~737)
- Health board state machine: `record_health_failure` and
  `note_successful_spawn` on `App`
- The claim / watchdog lifecycle: `claim_pending_spawns` (~5013),
  `tick_watchdog` is invoked in `refresh_items` (~3084-3120)
- Persistence: `health_state_path`, `save_health_board_to`,
  `load_health_board_from`, `overrides_toml_path`
- Popup: `open_health_popup`, `close_health_popup`,
  `draw_health_popup`, `execute_health_fix`
- Banner draw: `Renderer::draw_health_banner`
- The bug fix in question: `claim_pending_spawns` — timeout drop
  removed in `e7621ee`

---

## TL;DR for whoever picks this up

1. **dev-1 still has the wedged server.** Don't fix it manually.
2. Cluster 6 unit tests pass; **dev-1 UAT does not.**
3. Most likely remaining bug surface: the spawn click path on dev-1
   isn't doing what the unit tests model. Add debug logging gated by
   `PTM_DEBUG_SPAWN`, run on dev-1, look at actual data.
4. Resist new theory until step 3 produces output.
