# Questions accumulated for the user (Cluster 1 + Cluster 2 session)

Created 2026-05-03 night session, working unattended on Clusters 1 and 2 from MVP_PLAN.md.

These are questions where I made a defensible default choice and proceeded, but want explicit user sign-off when they're back online.

---

## Cluster 1 (Stage H — rename UX)

(empty — will populate as I encounter ambiguity)

---

## Cluster 2 (Stage F — durability)

### Q1: wm_class-only fallback grabs unrelated windows on PTM restart

**What I observed live:**
- Created Group 1 with `ptm-test-window` (gnome-terminal).
- Closed PTM.
- Closed `ptm-test-window` externally.
- Restarted PTM. `claude` (the gnome-terminal hosting Claude Code) was the only Gnome-terminal alive.
- restore_groups matched `claude` into Group 1 via the wm_class-only fallback (the third cascade tier from Phase 2d).

**Why I shipped it anyway:**
This is exactly what `MVP_PLAN.md`'s OQ-F3 describes:
> "the cost of a false match is low (one window briefly in the wrong group,
> fixable in a click) compared to the value of 'Vim - foo.rs' matching
> specifically when there are two terminal windows open."

So it's by design. But the live UX is more surprising than I expected — without context, it really looks like PTM "ate" your unrelated terminal.

**Decision the user might want to revisit:**
- Keep as-is (per OQ-F3 default).
- Tighten wm_class-only to only fire when the saved label is "obviously
  drifted" (some heuristic like "this title was a path or a tmux status
  string that's mutable"). Probably impractical.
- Skip wm_class-only at restore time but keep it at refresh-time
  re-match (where we have the additional safety check that the wid is
  brand new). This trades terminal-recoverability for less surprise.
- **Implemented mitigation (refresh-time only):** my `refresh_items`
  re-match now DOES respect "already_known" wids — i.e., a class match
  won't pull a currently-displayed ungrouped window into a group at
  runtime. This is a partial mitigation: only the boot-time restore
  exhibits the surprising behavior.

I have NOT changed the restore-time logic. Awaiting user opinion.

### Q2: Auto-save backstop interval (30 s) and debounce (250 ms)

These are the values from MVP_PLAN.md OQ-F4. Not asking permission, just
flagging that these are tunable if the user wants different values
(e.g. "save every 5 seconds regardless" for paranoid mode).

---

## Cluster 3 (Stage G — drag fluency)

### Q3: Drop-on-header now inserts at TOP of group (was: append)

The Stage G classifier strictly applies the plan's hot-zone rule
"Group header row → JoinGroup(g, 0)". Pre-Stage-G the drop-on-header
appended. The one regression test asserting the old behaviour was
updated to match the new semantics.

If the user prefers append, we just change `at: 0` to
`at: g.members.len()` in the classifier's GroupHeader branch.

### Q4: G-4 "bouncing drops" — was the no-op-detect enough?

T3.2 added "if to == sp || to == sp+1, return false (no-op)" in
do_reorder_in_group. This is the most-likely root cause of the
bouncing symptom (dropping at a position that resolves to source's
own slot). Without real-use repro I can't be sure it's the only one;
T3.0's logging instrumentation + T3.6 fix are deferred until the
user can confirm whether the bug still happens.

### Q5: Post-drop highlight is binary, not graduated alpha

T3.5 v1: highlight on for 1.5 s, then off. Plan called for fade-out.
Graduated alpha needs ~10 pre-allocated intermediate colours and a
~33 ms wake cadence (vs current 250 ms save tick). Deferred — binary
still communicates "your drop landed here" cleanly. Easy to upgrade
later if the user wants smoother visuals.

## General / cross-cluster

(empty)
