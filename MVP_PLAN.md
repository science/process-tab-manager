# MVP Plan — Path from Alpha to MVP

> Sibling document to `ROADMAP.md` (which tracks Stages A–D, mostly shipped).
> This file picks up at Stage E and covers the gaps that block calling PTM an MVP.
> Drafted 2026-05-03 from a session reviewing five concrete pain points.
> Designed to be edited and discussed across multiple Claude sessions.

## Context

PTM today is a working alpha: people can group windows, drag-reorder, attach to tmux sessions, rename. Stages A–C from `ROADMAP.md` shipped; D was deliberately deferred. This plan addresses the next set of gaps, raised as five specific issues:

1. Windows are ephemeral — close PTM (or reboot) and the layout is gone.
2. Group state is fragile: sometimes *all* windows drop out of *all* groups. Suspected trigger is PTM close/reopen, but the failure isn't fully understood.
3. Dragging into a group requires precise targeting; near-misses do the wrong thing.
4. The rename text field lacks selection and standard keyboard shortcuts.
5. Tmux server lifecycle is invisible; sessions accumulate with no way to wipe them.

Each stage below is independently shippable. The recommended order is at the bottom of the file. Open design questions live inline in each stage and are summarised at the end so they can be answered before code starts.

All paths reference `src/main.rs` (the project is a single-binary crate as of the recent flatten). Tests live in `#[cfg(test)] mod tests` at the bottom of the same file.

---

## Stage E — Window-relaunch recipes (Issue 1)

### Intent

When PTM exits (reboot, crash, deliberate quit), it should be possible to bring the layout back. Today, group memberships and tmux session attribution survive on disk; the windows themselves don't. On next launch, PTM has nothing to attach back to. The fix is to record per-window enough information to spawn an equivalent window on restart, then offer to do so.

### Validation north star

Stage E is "successful" when a terminal that was running `claude` or `npm run dev` can come back with that workload running again — not just an empty terminal shell. The empty terminal is a building block; the workload restoration is the actual user value. UAT for Cluster 5 should test this end-to-end (start `claude` in a terminal, save state, kill the terminal, restart PTM, end up with claude running again — even if the user has to confirm).

### Safety model

**PTM must never auto-run captured workload commands.** A captured cmdline could be `rm -rf /tmp`, `git push --force`, `kubectl delete`, `docker rm -f` — destructive things we can't reliably distinguish from safe ones via the cmdline alone. The only safe assumption is that **every captured workload requires explicit user opt-in** before it runs.

This splits restore into two safety tiers:
- **Auto (always safe):** spawn the terminal window in the saved cwd. No state mutation; just a process and a directory.
- **Opt-in (every time):** re-run the captured workload command (e.g., `claude`, `npm run dev`). Always behind an explicit user action.

This rule applies even after a user has previously approved the same recipe — we don't remember "user always approves this". Each restore is a separate decision. (Per-session caching of "don't ask me again for this restore cycle" is fine; per-recipe whitelisting is not.)

### What "useful subset" looks like

Universal recovery is a research problem (browser tab state, editor unsaved buffers). MVP should target the boring 80%:

| Window class | Auto-restore | Workload replay | Confidence |
|---|---|---|---|
| Terminal hosting tmux | `${TERMINAL} -e tmux attach -t <session>` | Workload survives in the tmux session — no replay needed | High (best path) |
| Plain terminal with foreground job (e.g., `claude`) | `${TERMINAL}` + `cd <cwd>` | Click-indicator opt-in → `tmux send-keys`-only path; for non-tmux, show command for user to paste | Medium |
| Plain terminal at a shell prompt (no foreground job) | `${TERMINAL}` + `cd <cwd>` | None (nothing to replay) | High |
| PWA-style apps (`--app=URL`) | re-spawn with same `--app=` arg | Spawn IS the workload | High |
| Single-process GUI (firefox, vscode, gimp) | re-spawn `exe` with cmdline | Spawn IS the workload (can't restore inner state) | Medium |
| Multi-window apps with shared server (browser windows) | best-effort: spawn the app | Spawn IS the workload (can't restore tab) | Low |

**Tmux is the durable path.** A long-running workload inside a tmux session survives the terminal closing — `tmux attach` brings it back, no replay needed. PTM already handles this via session attribution. The whole replay-with-prompt machinery exists primarily for users who don't wrap their terminals in tmux. v1 should explicitly position tmux as the recommended pattern (link the shell-rc autowrap recipe from `ROADMAP.md`'s Stage D objections).

### Capture

The capture has to find the **foreground process inside the shell**, not just the terminal's own cmdline. The terminal's `_NET_WM_PID` is usually the GUI server (`gnome-terminal-server`); the *thing the user actually wants restored* is `claude` running inside `bash` running inside that terminal. Capture has to walk down past the shell to its foreground child.

Per-window, capture into a new sibling of `Item`:

```rust
struct LaunchRecipe {
    // Layer 1: the terminal/app itself (always restorable, safe).
    exe: String,                       // /proc/<pid>/exe of the WINDOW's process
    cmdline: Vec<String>,              // /proc/<pid>/cmdline of same
    cwd: Option<String>,               // /proc/<window-pid>/cwd

    // Layer 2: the workload running inside (replay-only with user opt-in).
    workload: Option<Workload>,
}

struct Workload {
    cmdline: Vec<String>,              // foreground job's cmdline (e.g. ["claude"])
    cwd: Option<String>,               // foreground job's cwd
    in_tmux: bool,                     // true if discovered via tmux pane walk
    tmux_session: Option<String>,      // for tmux-only injection on restore
}
```

**Walk-down algorithm (for terminals):**

1. From the window's `_NET_WM_PID`, scan `/proc/*/status` for processes with matching `PPid` to find direct children. Filter to the shell process (bash, zsh, fish — match `comm` or basename of exe).
2. From the shell PID, scan again for its children. If exactly one and not just a transient utility (filter known short-lived helpers), capture it as the workload. If none, the shell is at an idle prompt — no workload to capture. If multiple, pick the one in the foreground (heuristic: highest CPU time recent, or the one tmux/shell reports as foreground).
3. **For tmux-attached terminals:** instead of walking the GUI tree, query `tmux display-message -p -t <session>:.0 '#{pane_pid}'` for the active pane's PID, then walk down from there to find the foreground process. Set `in_tmux = true` and stash the session name.

Walk-down is expensive — only do it on save (debounced 250 ms after a state change), not every refresh.

**For tmux-attached terminals**, the *Layer 1* recipe collapses to `tmux attach -t <session>` (already wired via `spawn_attach_terminal`). The *Layer 2* workload survives in the tmux session — restoration is "reattach", no replay needed. The walk-down is only needed for cases where the user wants to see what's running.

**Capture failures degrade gracefully.** If the walk-down can't identify a foreground job, `workload = None` is fine — Layer 1 still works, the user just gets an empty terminal at the saved cwd.


### Persistence

Extend the existing format to a new `v2` that includes a `RECIPE` and optional `WORKLOAD` line per member:

```
v2
GROUP\t<name>\t<collapsed>
MEMBER\t<label>\t<wm_class>\t<custom_prefix>
RECIPE\t<exe>\t<cwd_or_->\t<cmdline_arg_count>\t<arg0>\t<arg1>\t…
TMUX\t<session_name>                              # if tmux-attached
WORKLOAD\t<in_tmux:0|1>\t<cwd_or_->\t<arg_count>\t<arg0>\t<arg1>\t…   # if a foreground job was captured
```

Tab-separated stays consistent with the existing format. Loader handles both `v1` and `v2` for forward compat.

The file path is the profile-aware path established in Stage F (`~/.config/ptm/profiles/default/groups`); recipes live alongside in the same file (no separate `.recipes` file).

Reuse: `save_groups_to`, `load_groups_from`, `extract_saved_state` (src/main.rs:2589–2682). The Cargo.toml stays serde-free.

### Restore — Layer 1: window + cwd (auto)

At startup, after the first refresh has matched any windows that already exist:

1. For each saved recipe with no live match, call a new `spawn_from_recipe(recipe)` that mimics `spawn_attach_terminal` but generic.
2. Use `pending_attach`-style claim mechanism (existing infrastructure at line 1373) to attribute the new window to the saved group. Generalise `pending_attach` from `Option<(String, Instant)>` to a queue of pending claims, each carrying the recipe identity it expects.
3. Skip recipes whose `exe` no longer exists or whose `cwd` no longer exists; surface in a header indicator (no crash).

This layer is auto — no user prompt. Spawning a terminal in a directory is not a state-mutating action; the worst case is a useless empty window the user can close.

### Restore — Layer 2: workload replay (opt-in, click-indicator UX)

When PTM has restored a Layer-1 terminal whose recipe also has a `workload`:

- The terminal row shows a small **restore-content indicator** (e.g., a `▶` glyph in the row's right-edge area, or a subtle background tint). Hover/tooltip: "Restorable workload available".
- Click the indicator → confirmation popup (the helper from Stage I Part 1):

  ```
  Restore workload in this terminal?
  Command: claude
  cwd:     /home/steve/dev/process-tab-manager   ✓ exists
  Mode:    tmux send-keys (session: ptm-dev)
  Env:     PTM does not capture environment variables
           (DATABASE_URL, etc. won't be set automatically)
  
  [Run]  [Edit and Run]  [Decline]
  ```

- **Run:** PTM injects the command. Indicator disappears.
- **Edit and Run:** opens an inline text input (reuse Stage H's rename machinery) pre-filled with the captured command. User edits, presses Enter, command runs.
- **Decline:** indicator disappears for this restore cycle. (Recipe stays on disk — next PTM restart, the indicator reappears. We don't remember per-recipe "user always declines this".)

**Injection mechanism — tmux-only in v1.** When `workload.in_tmux = true`, PTM runs `tmux send-keys -t <session> "<cmdline>" Enter`. Reliable, doesn't race with user typing, doesn't have xdotool's security implications.

For non-tmux workloads, the popup shows the command but does **not** offer a "Run" button — only "Copy to clipboard" + "Decline". This is the safest minimum: PTM displays what was captured, the user pastes it themselves into the terminal. The "Edit" button still works (edits what's copied to clipboard). Documents the gap; encourages tmux adoption.

**Warnings the popup must show:**
- If `cwd` doesn't exist: "Saved cwd `/foo/bar` no longer exists; will use terminal's default cwd."
- If `exe` of the workload doesn't exist on PATH: "`claude` not found in PATH; check installation before running."
- Always: env-not-captured caveat (one-line, dismissible-with-don't-show-again per OQ-E7 below).

### Detection of "already running"

Before spawning, check whether a current window matches the recipe:
- For tmux: session name match (already covered by Stage F's identity reattach).
- For exe + cmdline: exact `exe` match plus normalised cmdline match (drop timestamps, PIDs).
- For PWA-style: URL match in cmdline.

If matched, don't spawn — Stage F's identity loop already claims it into the group. The Layer 2 workload indicator still appears on the claimed terminal if a workload was captured, in case the user wants to replay it (e.g., the terminal is at a shell prompt because the workload exited).

### Profiles — directory layout shipped now, CLI flag deferred

Stage F (below) moves persistence under `~/.config/ptm/profiles/default/` in v1. That means when `--profile=work` ships in a later stage, there is **zero** migration: just route the path constant through a profile-name parameter that defaults to `default`. The `default` profile stays functional with no flag.

What v1 does:
- Always reads/writes `~/.config/ptm/profiles/default/{groups,geometry}`.
- One-time migration on first run: if `~/.config/ptm/{groups,geometry}` exists and the new path doesn't, move it.
- No CLI flag. No profile-selection UI.

What a future stage adds (NOT in MVP):
- `--profile=NAME` arg parsing; falls back to `default` when absent.
- Optional UI to switch profile mid-session (probably defer further; require a restart).
- Decisions about what happens when two PTMs run simultaneously with different profiles (probably "they don't share state and that's fine").

This is "anticipate a definite future change in the data layout" — different from speculative abstraction. The cost is ~5 LOC of path manipulation today; the saving is no migration script later.

### Open questions

- **OQ-E1** — **DECIDED 2026-05-03 (refined).** Layer 1 (window + cwd) auto-restores silently; Layer 2 (workload replay) is opt-in via click-indicator on the row. Resolves the original "auto vs prompt vs explicit" question by splitting safety tiers.
- **OQ-E2 (capture cadence):** Continuously (every refresh), or only after a stable window (≥10 s of consistent identity)? Stable is safer; continuous is simpler. **Recommendation:** capture continuously, save debounced (250 ms).
- **OQ-E3 (env capture):** No. Inherit PTM's env on respawn; document the limitation in the popup.
- **OQ-E4 (save trigger):** Same approach as Stage F's debounced save (250 ms).
- **OQ-E5** — **DECIDED 2026-05-03.** Workload injection in v1 is **tmux-only**. Non-tmux workloads show "Copy to clipboard" + "Decline", no auto-injection. Documents the gap, encourages tmux adoption.
- **OQ-E6** — **DECIDED 2026-05-03.** Prompt UX is click-indicator on the row, not prompt-on-focus. Lower friction, no ambush.
- **OQ-E7 (env-warning persistence):** Show the env-not-captured warning every time, or once-per-session-with-dismiss? **Answer:** dismiss-with-checkbox "don't show again for this PTM session"; never persist the dismiss across sessions (we want the user reminded if env matters for a new workload).
- **OQ-E8 (workload disambiguation when shell has multiple children):** Heuristic for foreground-job detection when capture finds >1 child of the shell. Options: (a) highest CPU time over last second; (b) the one whose state is `R` or `S` and most-recently-changed; (c) skip capture (set `workload = None`) and force user to start over. **Recommendation:** start with (c) — strict but predictable. Heuristics in v2 if users ask.

---

## Stage F — Group state durability (Issue 2)

### Intent

The user reports that sometimes **all** windows fall out of **all** groups, requiring a manual rebuild. The original framing was "group ↔ window associations get dropped when a window closes and reopens", but the symptom is broader: total mass-loss events that happen mysteriously, suspected to be tied to PTM close/reopen.

Stage F needs to make group state **durable through PTM's full lifecycle**, not just fix one specific re-association case. There are at least four distinct failure modes in the current code that can each silently nuke groups, and they compound.

### Failure modes in the current code

**FM-1 — Save only fires on clean WM_DELETE_WINDOW.** Line 2889 is the only `save_groups()` call. If PTM crashes, is killed by SIGTERM/SIGKILL (logout, reboot without quitting PTM cleanly, OOM, lost X11 connection), or panics — **no save ever runs**. State reverts to whatever was last clean-quit, possibly hours of group work ago. The user's "ptm closing and reopening" suspicion likely maps to this: a non-clean exit is invisible from the outside, but it discards everything.

**FM-2 — Empty matches silently drop groups, then save overwrites disk.** `restore_groups()` (line 2719) does `if matched_wids.is_empty() { continue; }` — a group whose members aren't currently visible **is removed entirely** from in-memory state. The test at line 4092 confirms this: `app.groups.len() == 0` after restore with no matches. Then on next clean exit, `save_groups()` serializes from `app.display_order`, which no longer contains those groups. **The on-disk state is overwritten with the worse state.** This is the "destroys saved groups permanently" path. Triggered when PTM starts before the apps it manages have come back up (autostart races, slow GTK window mapping, login-session ordering).

**FM-3 — Title drift breaks (label, wm_class) match.** Terminal titles change constantly (PWD, running command, tmux info). If PTM saved with `"claude-code: ptm — vim"` and the terminal now shows `"bash — ptm"`, the exact (label, wm_class) match fails. Fallback is **label-only** (line 2706–2711) — also fails. There's no `wm_class`-only fallback. Group gets zero matches → FM-2 kicks in → group is permanently lost.

**FM-4 — Non-atomic writes.** `save_groups_to()` (line 2616) opens the file, then writes line-by-line. A crash or kill mid-write leaves a truncated file. On next load, `load_groups_from()` returns `None` (parse failure on partial line) → no restore at all → next save wipes disk.

### Fix — multi-pronged

**A. Save aggressively, save atomically, save often.**

- Trigger `save_groups()` after every state-mutating user action: group create/rename/delete, member add/remove, drag-drop completion, custom_prefix edit. Debounce to 250 ms so a fast sequence of changes coalesces into one write.
- Trap process-exit signals (SIGTERM, SIGINT, X11 connection loss) and save before exit, in addition to WM_DELETE_WINDOW. Use a `ctrlc`-style handler or just check for the connection-broken error in the main loop and save then.
- Periodic backstop: if "dirty" flag is set, save every 30 s as a safety net.
- Atomic write: `save_groups_to()` writes to `<path>.tmp`, then `rename()` over the real path. Even if PTM crashes mid-write, the old file stays intact. Standard pattern, ~5 LOC change.

**A'. Profile-aware paths (groundwork for future `--profile` flag).**

Move the persistence root from `~/.config/ptm/` to `~/.config/ptm/profiles/default/`:

- `groups_path()` (src/main.rs:2580) returns `~/.config/ptm/profiles/default/groups`.
- `geometry_path()` similarly returns `~/.config/ptm/profiles/default/geometry`.
- One-time migration at startup: if the old `~/.config/ptm/groups` exists and the new path doesn't, move both `groups` and `geometry` into the new location. ~10 LOC. Idempotent (once migrated, the check is a no-op).
- No CLI flag yet. Profile name is hard-coded `"default"` in v1.

This is the only "thinking ahead" the plan permits — see Stage E's "Profiles" section for the full rationale.

**B. Never overwrite disk state with empty state.**

- Read the saved file at startup but **don't drop groups whose members aren't yet visible**. Keep them in memory in a pending state. Re-attempt matching on every `refresh_items` call (this also fixes the close-and-reopen-rejoin case, which is now a free side effect).
- This requires the data-model change below: in-memory groups must carry identity, not just live wids.

**C. Stronger identity matching.**

- Add a third fallback: `wm_class`-only (after exact match and label-only fail). Order: exact (label, wm_class) → label-only → wm_class-only.
- For terminals where titles change frequently, wm_class-only is the right matcher anyway. Optional refinement: skip "label-only" for windows whose wm_class is in a known-volatile-title set (terminals, browsers) — go straight from exact to wm_class-only.
- Document the matching order so future-us doesn't accidentally reorder it.

**D. Data model: identity in memory.**

This is the same change I originally proposed for "rejoin on respawn", now justified as the foundation for B above:

```rust
struct GroupMember {
    identity: MemberIdentity,    // (label, wm_class, custom_prefix)
    live_wid: Option<u32>,
}

struct Group {
    id: u32,
    name: String,
    collapsed: bool,
    members: Vec<GroupMember>,
}
```

`member_wids` becomes a derived helper. In `refresh_items` (src/main.rs:1030), after building `new_items`:

1. Existing members whose `live_wid` is no longer in `live_wids` → set `live_wid = None` (keep entry).
2. New windows not matched to any existing live wid → scan all `live_wid: None` members, apply the matching cascade from C above. On hit: set `live_wid = Some(new_wid)`.
3. Remaining new windows → ungrouped in `display_order`.

`extract_saved_state` (line 2589) serializes from `Group::members` directly, no longer needs `find_item` lookups by wid — so even ghost members serialize correctly. This is what makes B work.

### Cross-cuts with Stage E

Stage E captures relaunch recipes. The recipe lives on the `GroupMember`, not the transient `Item`. Restore becomes "for each group, for each member with `live_wid: None`, spawn from recipe" — the same loop Stage F builds.

**Stage F is a prerequisite for Stage E.** Build F first.

### Tests

Add to `#[cfg(test)] mod tests`:

- Save-then-load roundtrip with an atomic-write scenario simulating crash mid-write (verify old file intact).
- Restore where no members match → groups stay in memory as ghosts, next save preserves them.
- Re-match on refresh: ghost member becomes live when matching window appears.
- wm_class-only fallback when label drifts.
- Existing `restore_groups_no_match_skips_group` (line 4092) **needs updating**: with Stage F's new semantics, a no-match group should NOT be skipped — it should be retained as ghost. Update the test to assert the new behavior. Document the change in the commit message.

### Open questions

- **OQ-F1 (stale ghost members):** A member with `live_wid: None` for a long time — stay forever, or expire? **Recommendation:** never auto-expire. Removal is an explicit user action. Stage E's restore offer handles "do I want this back?".
- **OQ-F2 (identity collision on rejoin):** Two windows with same (label, wm_class) close, both reopen later — order? **Recommendation:** first-come-first-claimed by stored member order. Document the determinism.
- **OQ-F3 (volatile-title set):** Skip label-only fallback for known-volatile wm_classes (terminals, browsers) and go straight to wm_class-only? Or always try label-only first? **Recommendation:** always try label-only first; the cost of a false match is low (one window briefly in the wrong group, fixable in a click) compared to the value of "Vim - foo.rs" matching specifically when there are two terminal windows open.
- **OQ-F4 (save signal handling):** Use the `ctrlc` crate, install raw `signal()` handlers, or just rely on debounced save + dirty flag + 30 s backstop without explicit signal trapping? **Recommendation:** debounced + backstop is enough — signal handlers in Rust are tricky and the backstop limits worst-case data loss to 30 s of work.

---

## Stage G — Drag-and-drop fluency (Issue 3)

### Intent

The user reports drag-drop feels finicky in multiple ways. Stage G addresses the full set of observed papercuts (confirmed 2026-05-03), not just one.

### Issues to address

**G-1 — Drop into group's body doesn't join the group.** Reading `handle_drop` (src/main.rs:630–688): "Add to group" fires only when the drop hits the **group header row** itself (28 px tall). Drop one pixel below the header — into what looks visually like the group's body — and the source window is inserted as ungrouped, not as a new member.

**G-2 — Reordering inside a group ejects to ungrouped.** When dragging a window already in group X, `is_gap_in_group(drop_gap, src_gid)` returns true only for `gap > hr && gap <= last_member + 1`. A drop just outside that range — even by a few pixels — falls into the `else` branch that *removes from group* and inserts as ungrouped. The user perceives this as "I tried to reorder and got kicked out".

**G-3 — Drop indicator vs landing mismatch.** The visual indicator uses `drop_index_from_y` (no deadzones, every y maps to a gap) but the *semantics* of where the window lands depend on which branch of `handle_drop` fires. Same blue line; different outcomes.

**G-4 — "Bouncing" drops inside a group.** User reports: "if there are a bunch of items in a group, and I drop inside, sometimes the drop 'bounces' and sometimes it doesn't". Hypothesis: when `reorder_within_group`'s `target_member` math equals the source position (or off-by-one in the `if target_member > sp { target_member - 1 }` correction), the operation is a no-op — the user sees the dragged item snap back to where it started. Needs investigation; could also be related to G-2's eject path producing a false move-back. Investigation is its own task in Cluster 3.

**G-5 — No post-drop visual confirmation.** Drops succeed silently. Even when a drop does the right thing, the user can't easily tell *which* item moved or *where* it landed. User suggestion: "shading the dropped tab for 1.5s with a fade out" would clearly communicate the success and destination.

That's the full set: precision-required (G-1, G-2), feedback-mismatch (G-3), inconsistency (G-4), no-confirmation (G-5).

### Proposed model

**Fix for G-1, G-2, G-3** — replace the binary "header hit vs gap" with a drop-target classifier:

```rust
enum DropTarget {
    InsertBefore(usize),                // ungrouped, before display row N
    InsertAtEnd,
    JoinGroup { gid: u32, at: usize },  // add to group at member index
    ReorderInGroup { gid: u32, to: usize },
}
```

Hot-zone rules, top to bottom of a group:

- Group header row → `JoinGroup(g, 0)`.
- Gap between header and first member → `JoinGroup(g, 0)`.
- Each member row + the gap below it → `JoinGroup(g, idx+1)` if source is outside the group; `ReorderInGroup` if source is inside.
- Last member's bottom gap → `JoinGroup(g, len)`.
- Next visual element (gap between groups, ungrouped row, etc.) → `InsertBefore` semantics.

Net effect: **the entire visual extent of a group is a join-or-reorder target.** Only spaces between groups are ungrouped insertion gaps. This kills the eject-on-near-miss behaviour (G-2): you'd have to drop *clearly outside* the group's vertical extent to get extracted, not just drift a pixel out of the in-group range.

### Drop-feedback redesign (fixes G-3 and adds G-5)

**During drag** — make the indicator match the intent:
- `InsertBefore` → thin horizontal line (current).
- `JoinGroup` / `ReorderInGroup` → outline the target group with a faint border, plus the existing line at the precise insert slot. The user can tell at a glance whether the drop will join or merely insert nearby.

**After drop** — flash-fade the moved tab:
- On successful drop, the destination row gets a brief background highlight (accent colour at ~40% alpha) that fades to normal over 1.5 s. Implementation: add a `last_drop_highlight: Option<(u32 /* wid */, Instant)>` to App; renderer interpolates alpha based on elapsed time; a timer wakeup (similar to `tmux poll thread`) triggers redraws while the fade is active.
- Skip the fade when the drop was a no-op (same position, same group) — no visual change to communicate.

### G-4 investigation (research task in Cluster 3)

The "bouncing" symptom needs investigation before we know what to fix. Two main hypotheses:
- (a) `reorder_within_group`'s `target_member` math hits no-op cases (e.g., `target_member == sp` after the off-by-one correction at line 619) and the user perceives the snap-back as "bounce".
- (b) Tiny y-jitter near the in-group/out-of-group boundary causes G-2's eject path to fire, then immediately reorder back via a follow-up event — producing a visible bounce.

Investigation task: instrument `handle_drop` to log the inputs and the resulting branch, then reproduce the bug in real use. Decide on a fix once we have data. Could be a simple no-op-detect + skip, or it could expose a deeper issue.

### Tests

Add unit tests in `#[cfg(test)] mod tests` covering:
- Drop ungrouped window into group's body → joins (regression for G-1).
- Drop grouped window into another group's body → moves between groups (regression for G-1, group-to-group variant).
- Drop grouped window slightly outside the in-group range → still reorders within group (regression for G-2).
- Drop grouped window CLEARLY outside the group's extent → ungroups (existing behaviour preserved).
- Drop on group header → joins (existing test).
- Drop indicator returns the same `DropTarget` that `handle_drop` then acts on (regression for G-3).
- After-fix for G-4: cover whatever the investigation surfaces.

The fade animation is visual-test territory; manual review.

### Open questions

- **OQ-G1** — **CONFIRMED 2026-05-03**: classifier redesign is the right approach.
- **OQ-G1b** — **ANSWERED 2026-05-03**: additional pains are G-2 (eject on near-miss), G-3 (indicator/landing mismatch), G-4 (bouncing — needs investigation), G-5 (no post-drop confirmation). All folded into Stage G scope above.
- **OQ-G2 (drop-on-item-to-swap?):** Browser tab strips do this; PTM today only inserts. **Answer:** keep insert semantics, no swap.
- **OQ-G3 (drag a group into another group?):** Today only reorders groups in `display_order`. **Answer:** no nested groups in MVP, dropping group affects relative ordering: dropping a group tab on another group (header or child process) results in the group being placed above that group if the drop is >50% above the bottom of the group (in pixels). If drop is within the bottom 50% of the height of the other group display, then place the dropped group below the group being dropped onto.

---

## Stage H — Real text-input behavior in rename (Issues 4 + 4a)

### Intent

The rename field today supports cursor movement, insert, backspace, delete, Home/End. It doesn't support **selection** — so Shift+arrows, Ctrl+A, "select all on open", "type to replace" are all missing. The text input feels worse than a 1995 GTK entry.

### Fix

Extend `RenameState` (src/main.rs:225) with a selection anchor:

```rust
struct RenameState {
    target: RenameTarget,
    text: String,
    cursor: usize,                    // byte offset
    selection_anchor: Option<usize>,  // byte offset; None = no selection
}
```

Selection range = bytes between `cursor` and `selection_anchor` in either order.

Key handler additions in the rename branch (src/main.rs:3017–3127), inspecting `ev.state` for `Mod::SHIFT` and `Mod::CONTROL`:

| Key | No mod | Shift | Ctrl | Ctrl+Shift |
|---|---|---|---|---|
| Left | move 1 char, clear sel | extend sel 1 char | move 1 word, clear sel | extend sel 1 word |
| Right | mirror | mirror | mirror | mirror |
| Home | cursor=0, clear sel | extend to start | (same) | (same) |
| End | cursor=len, clear sel | extend to end | (same) | (same) |
| Backspace | delete char before / delete sel if any | (same) | delete word before | (same as Ctrl) |
| Delete | delete char after / delete sel | (same) | delete word after | (same as Ctrl) |
| `a` | insert `a` | n/a | select all | n/a |
| Any printable | insert / replace selection | (same) | n/a | n/a |

Word boundary: alnum vs non-alnum char-class transition. Good enough for tab labels.

### Pre-select on open (Issue 4a)

In `start_rename`, `start_session_rename`, `start_tab_rename` (src/main.rs:430–468): set `cursor = text.len()` and `selection_anchor = Some(0)`. Now opening rename → typing immediately replaces existing text. Standard behavior in every file manager / IDE / browser.

For `start_tab_rename` the existing `custom_prefix` is often empty — selection is a no-op there, no behavior change.

### Rendering

Add a selection rectangle to the renderer (existing rename-row drawing around src/main.rs:2154–2213): fill the byte range with a subdued accent colour, invert text colour over it. ~10-line change.

### Tests

Add unit tests for the pure logic: selection-anchor maths, word-boundary jump, replace-selection-on-insert. The X11 keyboard event handler stays mostly visual-test territory.

### Open questions

- ~~OQ-H1~~ (clipboard) — **DECIDED 2026-05-03: defer.** Stage H ships without X11 PRIMARY/CLIPBOARD integration. Revisit if users ask.
- ~~OQ-H2~~ (double-click word, triple-click line) — **DECIDED 2026-05-03: defer.** Stage H ships without click-based selection.

---

## Stage I — Tmux session control (Issue 5)

### Intent

The user wants explicit control over **tmux server-side sessions** — see what's there, kill what isn't needed. The user explicitly does **not** want PTM to manage the tmux daemon (kill-server, detach-all-clients, server status row, server PID display) — that's out of scope.

### Calibration

What PTM does today (src/main.rs:1149–1213, 2484–2493):
- ✅ List orphan sessions in the sidebar (Stage C, shipped).
- ✅ Right-click on orphan → Kill Session (`tmux kill-session -t <name>`).
- ✅ Right-click on orphan → Attach (spawn terminal with `tmux attach -t <name>`).
- ✅ Detach naturally by closing the terminal — session keeps running and shows as orphan after the next 5 s tmux poll.

What's missing (confirmed 2026-05-03):
- **Killing a session that's currently attached takes two steps:** close the terminal, wait for the orphan row to appear, right-click → Kill. A single-step "Kill underlying session" from the attached terminal's row would close the loop.
- **Forgotten sessions aren't visible enough.** Today, attached sessions are represented only by their window row (with a session marker dot from Stage B); the session name itself isn't surfaced. Users want a clearer at-a-glance view of all the persistent work contexts they have, not just the unattached orphans.

### Proposed additions

**Part 1 — One-step kill (confirmed, ~50 LOC + popup):**

Add to the context menu for an attached terminal window — i.e., an `Item` with `session = Some(name)` (renderer info around src/main.rs:2270 + menu builder):

- **Kill tmux session** — confirmation popup, then `tmux kill-session -t <name>`. Tmux's last-client-exits-when-session-dies semantics will close the terminal window automatically. Existing kill-session command path (line 2484–2493) is reused.

**Part 2 — "Tmux Sessions" group (confirmed scope 2026-05-03, ~150 LOC):**

The user observed that a per-row badge already mostly exists (Stage B's session marker dot). What's missing is a unified at-a-glance view of *all* sessions on the server, attached and orphan alike, in one collapsible place.

UX:
- A special **system group** named "Tmux Sessions" appears at the bottom of the sidebar (or wherever the user drags it; participates in the normal group reordering).
- Members are *all* tmux sessions on the server — attached (those PTM has matched to terminal windows) AND orphan (those without an attached terminal).
- Each member row shows: session name, attach state (attached → green dot like today; orphan → grey/hollow dot), and the new [x] close affordance from Part 3.
- Collapsible like a normal group. Default: collapsed (so users with many sessions don't get a wall of rows by default).
- The user can't drag windows INTO this group (it's auto-populated from `tmux list-sessions`); they CAN drag the group itself in display_order to reposition it.

Implementation:
- New `Group::kind: GroupKind { Normal, TmuxSystem }` field. System groups bypass the normal "members are wids" model — their members are derived from `list_tmux_sessions()` on every refresh.
- Or: extend `GroupMember` with a discriminator (Window vs Session, akin to `DisplayRow::Session`). Either works; pick the one with smaller blast radius during Cluster 4 design.
- The group is created automatically on first run if tmux is detected; persists across PTM restarts (so user's drag-position is remembered).

This deliberately reuses the group-rendering pattern instead of inventing a new sidebar section, which keeps render code paths consolidated.

**Part 3 — `[x]` close button on session rows (confirmed scope 2026-05-03, ~80 LOC):**

The user wants a close affordance on rows so that "something otherwise mostly invisible" (an orphan or forgotten session) can be killed with a click rather than a right-click → menu chase. Specifically required for session rows; optional/future for window rows.

UX (per user's "double click to really close" suggestion):
- Each session row in the "Tmux Sessions" group renders a small `[x]` glyph on the right edge of the row (~14 px wide, in the row's existing horizontal layout).
- Click [x] once → arms it (visual change: the glyph turns red, perhaps with the row outlined). Click [x] again within ~3 s → confirms, runs `tmux kill-session -t <name>`. Click anywhere else, or wait > 3 s → disarms.
- Alternative considered: single-click [x] → confirmation popup (the helper from Part 1). Pick one in Cluster 4 design — double-click-arm is lower-friction; popup is more explicit.

For attached sessions in this group, killing also closes the terminal window (tmux's last-client-exits semantics). That's the desired behaviour.

**Future option (not in MVP):** [x] on window rows (close the application). User said "Maybe all application 'bars/tabs' could receive a right hand x" — flagged as appealing but flagged this as "Maybe", explicitly making session rows the priority. Defer; revisit if the session-row [x] proves popular.

**Out of scope:** No status row, no server-level controls, no detach-client commands. Confirmation popup helper from Part 1 is reusable (Stage F's atomic-write "Are you sure?" deletes might use it too).

### Confirmation UX

The confirmation popup is the only new piece of UI. Render a modal-like override-redirect window ("Kill session 'main'? [Y / N]") using the existing context-menu machinery (src/main.rs:2154 region) with a pointer grab. One helper, ~50 LOC including draw.

Single-session kill from the orphan menu (existing) keeps no confirmation — users expect that one to be quick — but if OQ-I3 reveals other needs, we may add confirmation there too.

### Open questions

- **OQ-I3** — **ANSWERED 2026-05-03:** two confirmed needs, plus a third surfaced during the discussion:
  - (a) one-step kill of attached session (Part 1)
  - (b) unified "Tmux Sessions" group surfacing all server sessions (Part 2)
  - (c) [x] close affordance on session rows for fast removal of forgotten sessions (Part 3)
- **OQ-I3b** — **ANSWERED 2026-05-03:** rejected the badge-only and badge+section options in favour of a "Tmux Sessions" group pattern (reuses the existing group-rendering machinery rather than introducing a new sidebar section type).
- **OQ-I3c (NEW — design-shaping, not blocking):** [x] arming UX — double-click-to-arm vs single-click-to-popup. **Answer:** double-click-to-arm; lower friction. Re-evaluate during Cluster 4 design.
- **OQ-I3d (NEW — design-shaping):** Default collapse state for the "Tmux Sessions" group. **Answer:** collapsed by default to avoid wall-of-rows for users with many sessions; user's drag-position and collapse-state persist via Stage F's normal group persistence.
- ~~OQ-I1, OQ-I2~~ — **REMOVED 2026-05-03: out of scope.** Server-level visibility and control aren't wanted.

---

## Pushbacks on the original list

I committed to push back where appropriate. Where I'd push:

1. **Profiles in v1 (1a.2):** the *CLI flag and selection UX* are premature — defer. But the *directory layout* (`~/.config/ptm/profiles/default/`) goes in v1 as part of Stage F's persistence work, with a one-time migration from the old path. This costs ~15 LOC now and saves writing a migration script later when the flag ships. See Stage E's "Profiles" subsection for the rationale.

2. **"Don't open a second time if app already open" (1, second part):** correct intuition, but should fire **only on PTM startup**, not continuously. If the dedup applies on every refresh, users who explicitly want a second instance of an app will be surprised when PTM "absorbs" it into a group instead of letting them have two.

3. **"Can only end client, not server" (5):** clarified 2026-05-03 — the user does **not** want server-level control. Scope narrows to "kill an attached session in one step", not "kill the daemon". Stage I rewritten accordingly.

4. **No pushback on:** Issues 2, 3, 4, 4a — all real, fixes appropriate scope.

5. **Missing from the user's list — test coverage.** Stages E, F, G especially need new unit tests in the existing `#[cfg(test)] mod tests` block (drop classifier, identity reattach, recipe match). Flagging here for parity with the TDD workflow in CLAUDE.md.

---

## Implementation roadmap

This section is the project plan. It converts the stages above into sequenced clusters with explicit checkpoints, so we ship in small individually-verifiable increments rather than building everything in parallel and "checking it all at the end".

The high-level shape is intentionally coarse — each cluster gets its own fine-grained plan when it starts. This roadmap is the spine; the detail hangs off it.

### Anti-plan (what we explicitly will not do)

So the alternative is concrete:

- **No working in parallel across clusters.** One cluster at a time. Don't start cluster N+1 until cluster N has passed UAT (or has been explicitly waived).
- **No deferring tests within a task.** TDD is non-negotiable per CLAUDE.md. RED → GREEN → refactor → commit.
- **No skipping UAT gates "to keep momentum".** A gate failing means we fix or backlog before moving on.
- **No refactoring across cluster boundaries.** If cluster N exposes a need to refactor code touched in cluster N-2, that becomes a backlog item, not an in-cluster scope creep.
- **No adding scope mid-cluster.** New ideas → backlog, evaluated before next cluster starts.
- **No "I'll fix it later" comments in shipped code.** TODOs become tracked backlog items or get fixed before the cluster ships.

### Working agreement (TDD throughout, per CLAUDE.md)

For every task in every cluster:

1. Write a failing test in `#[cfg(test)] mod tests` (src/main.rs bottom). Run `CARGO_TARGET_DIR=/tmp/ptm-dev cargo test` — confirm RED.
2. Implement the behaviour change.
3. Re-run the suite — confirm GREEN, **no regressions** in the existing 25+ tests.
4. For user-visible changes: Tier-2 manual visual review — `CARGO_TARGET_DIR=/tmp/ptm-dev cargo build --release && DISPLAY=:0 /tmp/ptm-dev/release/ptm`.
5. Commit test + impl together (per CLAUDE.md "Local commits are encouraged").

UAT gate definition: an explicit pause-and-review checkpoint where the user runs the new build in their normal workflow for at least one working day, then either signs off or files specific issues that get triaged before the next cluster starts.

### Cluster sequencing

Default order (subject to OQ-Order at end of plan):

```
Cluster 1 — Stage H (rename UX)              ~150 LOC, 1-2 sessions
   ↓ UAT-1: rename smoke test
Cluster 2 — Stage F (durability)             ~400 LOC across 5 phases
   ↓ UAT-2: 1-day soak in real use
Cluster 3 — Stage G (drag fluency)           ~300 LOC, 7 tasks (5 issues)
   ↓ UAT-3: drag/drop fluency
Cluster 4 — Stage I (tmux session control)   ~280 LOC across 3 parts
   ↓ UAT-4: tmux session smoke
Cluster 5 — Stage E (window relaunch)        ~830 LOC across 6 phases
   ↓ UAT-5f: north-star test (claude running again after reboot)
```

#### Why this order

- **H first.** Smallest, independent, immediately user-visible. Builds workflow muscle without architectural risk.
- **F second.** Fixes the most-felt bug (mass group loss). Foundation for E. Phases inside F are independently shippable so durability gains arrive incrementally without one big-bang refactor.
- **G third.** Independent of F/E. Fixes a felt papercut.
- **I fourth.** Independent, smallest after rescoping.
- **E last.** Biggest, riskiest. Depends on F's data model. Capture-only Phase 5a runs early and silently to validate recipe quality before any restore behaviour ships.

### Cluster details

#### Cluster 1 — Stage H (rename UX)

Tasks (each task = one TDD cycle + one commit):

- **T1.1** — extend `RenameState` with `selection_anchor`. Pure data; tests cover anchor maths.
- **T1.2** — Shift+Left/Right/Home/End extending selection. Tests cover transitions.
- **T1.3** — Ctrl+A select-all, Ctrl+Backspace/Delete word delete. Tests cover word boundary.
- **T1.4** — printable input replaces selection if any. Tests cover replace behaviour.
- **T1.5** — `start_rename` / `start_session_rename` / `start_tab_rename` set anchor=0 cursor=len for pre-select.
- **T1.6** — selection rectangle in renderer. Visual-only; manual review.

**UAT-1:** open rename on a group, type immediately to replace; Shift+End extends selection; Ctrl+A then type replaces all; Ctrl+Backspace deletes word. No regressions in basic typing.

#### Cluster 2 — Stage F (durability)

Phased so each lands and gets exercised before the next:

- **Phase 2a — Path layout + atomic writes** (~50 LOC).
  - Move paths to `~/.config/ptm/profiles/default/`.
  - One-time migration from old path.
  - Atomic write (`<path>.tmp` + rename).
  - Tests: migration is idempotent; atomic write leaves old file intact on simulated mid-write crash.
  - **UAT-2a:** restart PTM; verify groups loaded from new path; verify old path is gone after first run.

- **Phase 2b — Save triggers + signal traps** (~80 LOC).
  - Save on every state-mutating action, debounced 250 ms.
  - Save on SIGTERM/SIGINT and on X11 connection loss (or just rely on debounce + 30 s backstop — see OQ-F4).
  - Periodic 30 s backstop when dirty.
  - Tests: dirty-flag transitions; debounce coalescing.
  - **UAT-2b:** make a group, `kill -TERM ptm`, restart, verify group survives.

- **Phase 2c — Ghost groups + identity-on-refresh** (~150 LOC).
  - In-memory `Group::members: Vec<GroupMember>` with `live_wid: Option<u32>`.
  - `restore_groups` keeps unmatched groups as ghosts (flip the `restore_groups_no_match_skips_group` test at line 4092).
  - `refresh_items` re-matches new windows against ghost members (extends startup matching to every refresh — also fixes the close-and-reopen-rejoin case as a free side effect).
  - **UAT-2c:** quit a grouped app outside PTM, watch ghost member appear; relaunch the app, watch it rejoin the group.

- **Phase 2d — wm_class-only fallback** (~30 LOC).
  - Add third tier to matching cascade: exact (label, wm_class) → label-only → wm_class-only.
  - Tests cover terminal-with-drifted-title scenario.

- **Phase 2e — Render/drag refactor for new data model** (~100 LOC).
  - Audit all callers of `member_wids` (drag, render, group operations) — replace with derived helper or direct iteration over `members`.
  - Pure refactor; tests should already cover behaviour.

**UAT-2 (cluster gate):** 1 day of normal PTM usage. Make groups, close windows, reboot the machine (not just PTM), verify state survives end-to-end.

#### Cluster 3 — Stage G (drag-and-drop fluency)

Five issues, broken into focused tasks. Order matters — investigate G-4 early so its data informs the classifier design:

- **T3.0** — investigate G-4 ("bouncing" drops). Instrument `handle_drop` with logging. Reproduce in real use. Document findings in plan as comment under G-4. Not a code change yet, but its outcome may shape T3.1.
- **T3.1** — implement `DropTarget` classifier as a pure function (input: y, drag source, app state; output: enum). Tests cover every region of a group's vertical extent including the previously-ejecting near-miss zones (G-1, G-2). Classifier must be the same function consulted by both the indicator renderer and `handle_drop` (G-3 fix).
- **T3.2** — wire classifier into `handle_drop`, replacing the existing branching. Existing DnD tests should still pass; G-2 regression test added.
- **T3.3** — wire same classifier into the drag indicator render path. The blue line is now a **derived view** of the same `DropTarget` that will fire on release (G-3).
- **T3.4** — render group-outline highlight on drag-over for `JoinGroup` / `ReorderInGroup` cases. Visual; manual review.
- **T3.5** — implement post-drop fade highlight (G-5). Add `last_drop_highlight: Option<(u32, Instant)>` to App. Renderer interpolates alpha for 1.5 s. Trigger redraws via the existing tmux poll thread or a dedicated short-lived timer. Skip when drop was a no-op.
- **T3.6** — fix G-4 based on T3.0 findings. May be trivial (no-op detect) or may surface deeper structural issue.

**UAT-3:** drag ungrouped window into the body of a group → joins. Drag grouped window between groups → moves. Drag onto header → joins at top. Drag below all groups → ungrouped at end. Reorder within a group with small/jittery motions → stays in group, no eject. Indicator visibly matches landing spot. Successful drop briefly highlights the moved item with a fade.

#### Cluster 4 — Stage I (tmux session control)

Three parts per Stage I scope (Parts 1–3). Order: popup helper first (Part 1 needs it, Part 3 might too); system-group second (foundation for Part 3); [x] last.

**Part 1 — One-step kill (~50 LOC):**
- **T4.1** — confirmation popup helper (override-redirect window with Y/N). Reusable for Part 3 and beyond.
- **T4.2** — "Kill tmux session" menu entry on attached terminal rows (`Item.session.is_some()`). Reuses existing kill-session command path.

**Part 2 — Tmux Sessions group (~150 LOC):**
- **T4.3** — design decision (in code review): `Group::kind` field vs `GroupMember` discriminator. Pick the one with smaller blast radius. Document the decision in the commit.
- **T4.4** — implement system-group rendering: derive members from `list_tmux_sessions()` on every refresh; merge with attached attribution from `Item.session`. Tests cover the synthesis logic.
- **T4.5** — auto-create the group on first run if tmux is detected. Persist position + collapse state via Stage F's normal group persistence.
- **T4.6** — disable drag-into for system groups (drag-out still allowed for the system group itself, like a normal group).

**Part 3 — `[x]` close affordance (~80 LOC):**
- **T4.7** — render `[x]` glyph on session rows in the system group (right edge). Add hit-test for it.
- **T4.8** — implement double-click-to-arm UX: first click sets `armed_close: Option<(u32 /* row id */, Instant)>` on App; second click within 3 s confirms; any other click or expiry disarms.
- **T4.9** — confirm action runs `tmux kill-session -t <name>`. For attached sessions, the terminal closes naturally.

**UAT-4 (full cluster gate):**
- Attach via PTM → right-click terminal row → Kill tmux session → confirm popup → terminal closes and session gone (Part 1).
- Open the "Tmux Sessions" group → see all sessions, attached and orphan, with correct dot colours (Part 2).
- Click [x] on an orphan session → glyph turns red → click again → session killed → row disappears (Part 3).
- Wait 3 s after first click without confirming → glyph reverts (Part 3).
- Restart PTM → "Tmux Sessions" group's position and collapse state are preserved (Part 2 + Stage F).

#### Cluster 5 — Stage E (window relaunch)

The big one. Phased to land risk early. Two safety tiers throughout: **Layer 1** (window+cwd, auto, always-safe) and **Layer 2** (workload replay, opt-in, click-indicator + popup).

- **Phase 5a — Recipe capture (silent observation)** (~250 LOC).
  - Capture Layer 1 (`exe`, `cmdline`, `cwd`) for every window into a new `LaunchRecipe` field on `GroupMember`.
  - Capture Layer 2 (`Workload`): walk DOWN the process tree from window PID to find shell, then to shell's foreground child. For tmux-attached: query active pane's PID via `tmux display-message`, walk down from there.
  - Disambiguation when shell has multiple children: per OQ-E8, set `workload = None` (strict — no heuristic guessing).
  - **No use of the data yet.** Just observe and log to a debug file.
  - **UAT-5a:** run for 1 day; review captured recipes for the user's common windows (terminal+claude, terminal+npm-run-dev, vim, firefox, etc.); confirm Layer 1 looks sensible and Layer 2 correctly identifies foreground jobs (or correctly returns `None` for ambiguous cases). **This is the gate that decides whether the rest of cluster 5 makes sense.**

- **Phase 5b — Persistence v2 format** (~80 LOC).
  - Extend persistence format with `RECIPE`, optional `TMUX`, optional `WORKLOAD` lines.
  - Forward-compat loader (handles v1 and v2).
  - Tests cover roundtrip including missing-workload case.
  - **UAT-5b:** quit PTM, inspect file, verify recipes serialised; restart PTM, verify recipes loaded.

- **Phase 5c — Layer 1 restore: claim existing matches** (~120 LOC).
  - At startup: for each saved recipe, if a current window matches by recipe identity (tmux session, exe+cmdline, etc.), claim it into the group **without spawning**.
  - Reuses Stage F Phase 2c's identity matching; 5c adds recipe-based identity as an additional matcher.
  - **UAT-5c:** save state with apps running; restart PTM (apps stay up); verify groups restore correctly without any spawning.

- **Phase 5d — Layer 1 restore: spawn missing recipes** (~200 LOC).
  - For each saved recipe with no live match: spawn the terminal/app + cd into saved cwd. **No workload replay yet.** Just the empty terminal at the right place.
  - Generalised `pending_attach` queue (multiple in-flight spawns, each tagged with expected recipe identity).
  - Skip recipes whose `exe` or `cwd` no longer exists; surface in a header indicator.
  - **UAT-5d:** save state, reboot the machine, log back in, launch PTM, watch terminals respawn at the right cwd in the right groups (still empty inside).

- **Phase 5e — Layer 2 click-indicator UI** (~100 LOC).
  - Render restore-content indicator on rows whose recipe has `workload = Some(...)`.
  - Hit-test the indicator. Clicking it opens the confirmation popup (reuse Stage I Part 1's popup helper).
  - Popup shows captured command, cwd existence check, env caveat, and Run/Edit/Decline (plus Copy-to-clipboard for non-tmux).
  - **UAT-5e:** click an indicator → popup appears with sensible content. Decline → indicator disappears for the session.

- **Phase 5f — Layer 2 tmux send-keys injection** (~80 LOC).
  - Run path runs `tmux send-keys -t <session> "<cmdline>" Enter` for tmux-attached recipes.
  - Edit path opens an inline rename-style input (reuse Stage H machinery), pre-filled, commits on Enter, then injects.
  - For non-tmux: Copy-to-clipboard wires up X11 PRIMARY/CLIPBOARD selection (this is the small clipboard chunk that OQ-H1 deferred — revisit whether to do it now or defer further; if defer, just show "Copy disabled — please type the command manually").
  - **UAT-5f (THE NORTH-STAR TEST):** start `claude` in a tmux-wrapped terminal; let PTM observe for >250 ms; quit PTM; kill the terminal externally (tmux session keeps running); restart PTM; click the indicator on the restored row; confirm popup; click Run; **claude is running in the terminal again, in the right cwd, in the right group.** This is the validation that says Stage E succeeded.

**UAT-5 (cluster gate):** Full reboot test. Open PTM with several groups containing tmux-wrapped terminals running real workloads (claude, npm run dev, htop). Reboot the machine. Log back in. Launch PTM. Verify: groups restore (Stage F + 5c), terminals respawn at correct cwd (5d), indicators appear on rows with workloads (5e), clicking → confirming → workload running (5f).

### Decision/gate points needing user input

Listed in chronological order — answers needed by the start of the relevant cluster:

- **Before Cluster 1:** *(none — all answered 2026-05-03)*.
- **Before Cluster 2 Phase 2c:** confirm OQ-F1, F2, F3 (ghost retention, collision order, label-only-vs-wm_class-only ordering). All have recommendations; rapid review during Phase 2b's UAT-2b session is sufficient.
- **Before Cluster 3 starts:** *(none — Stage G scope locked)*.
- **Before Cluster 4 starts:** *(none — Stage I scope locked)*. OQ-I3c (arming UX) and OQ-I3d (default collapse) are design-shaping; pick during cluster planning.
- **Before Cluster 5 Phase 5d:** review captured recipes from 5a — confirm Layer 1 + Layer 2 capture quality. (No remaining design decisions; UX and safety model already locked.)

### Progress tracking

Each cluster gets a checklist appended to this file (or a separate file under the same directory) when it starts. Format:

```
## Cluster N progress
- [x] T1.1 ... committed: <sha>
- [ ] T1.2 ... in progress
- [ ] UAT-N
```

The plan file itself stays the spine; per-cluster fine-grained plans extend it as they're written.

---

## Open questions, ranked

**Blocking — need answers before Cluster 1 starts:**
- *(none)* — Cluster 1 is unblocked.

**Blocking — need answers before Cluster 3 (Stage G) starts:**
- *(none)* — Stage G's scope is locked (5 issues, classifier + fade).

**Blocking — need answers before Cluster 4 (Stage I) starts:**
- *(none)* — Stage I's scope is locked (Parts 1–3).

**Blocking — need answers before Cluster 5 starts:**
- *(none)* — Stage E's safety model and UX are locked.

**Design-shaping (have a default; can be revised during the relevant cluster):**
- **OQ-E2, E3, E4, E7, E8, F1, F2, F3, F4, G2, G3, I3c, I3d**

**Decided 2026-05-03:**
- **OQ-Order** = H first (then F → G → I → E).
- **OQ-G1** = confirmed; classifier redesign is the right direction.
- **OQ-G1b** = G-2/G-3/G-4/G-5 all in Stage G scope.
- **OQ-I3** = three confirmed needs (Parts 1–3 of Stage I).
- **OQ-I3b** = "Tmux Sessions" group pattern (reuses group-rendering, not a new section type).
- **OQ-E1** = layered restore — Layer 1 (window+cwd) auto, Layer 2 (workload) opt-in via click-indicator.
- **OQ-E5** = workload injection in v1 is tmux-only via `tmux send-keys`; non-tmux gets copy-to-clipboard, no auto-injection.
- **OQ-E6** = click-indicator UX, not prompt-on-focus.
- ~~OQ-H1, OQ-H2~~ (clipboard, click-select) — defer (note: a tiny clipboard wire-up may land in Phase 5f for the non-tmux Copy-to-clipboard path; reconsider then).
- ~~OQ-I1, OQ-I2~~ — out of scope (no server-level controls).
- Profile directory layout ships in v1; CLI flag defers.
- Stage E validation north star: "can the workload (claude/npm run dev) be running again after restart?" — UAT-5f.

The blockers should be answered in the next session before code. The design-shapers can ride along with implementation.
