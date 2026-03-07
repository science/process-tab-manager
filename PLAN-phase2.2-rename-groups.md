# Plan: Double-Click Rename Fix + Tab Groups

## Context

PTM has double-click rename code (`sidebar.rs:235-267`) but it likely doesn't work when PTM is unfocused — the focus pass-through mechanism (`sidebar.rs:503-555`) activates the target window on the first click, preventing GTK from seeing a true double-click. DnD reorder also reportedly broken/non-operational. Phase 2.2 (Tab Groups) is next on the roadmap.

User wants: (A) fix rename first, then (B) implement groups — two separate rounds.

---

## Phase A: Fix Double-Click Rename + Verify DnD

### Problem

Double-click rename doesn't work at all — even when PTM is in the foreground and focused. The `connect_rename()` handler (`sidebar.rs:235-267`) uses GestureClick with n_press==2 on the ListBox, but the rename entry never appears on double-click.

**Root cause hypothesis (to be validated in VM):** The ListBox's `row_activated` signal fires on the first click and calls `activate_window()`, which sends X11 focus to the target window. PTM loses focus between the first and second click. GTK4's GestureClick resets its press count when the window loses focus, so the second click registers as n_press==1 (not 2). Alternatively, GTK4 ListBox's internal gesture handler may claim the click sequence, preventing the external GestureClick from accumulating presses.

### Solution: Diagnose first, then fix properly

**Step 1 — Diagnose in VM:** Add temporary logging in `connect_rename()` to determine:
- Does the GestureClick `pressed` signal fire at all on double-click?
- What n_press value does it receive?
- Does `row_activated` fire before/between the clicks?

**Step 2 — Fix based on findings.** Likely approach (Firefox/Nemo pattern):

Replace `row_activated` with a unified GestureClick handler on the ListBox that handles both single and double clicks:
- n_press==1 → activate the window immediately (same as current row_activated behavior)
- n_press==2 → call `start_inline_rename()` (the activation from n_press==1 already happened, which is fine — Firefox does the same: single-click switches tab, double-click switches + starts rename)

The key insight: n_press==1 fires first, then n_press==2 fires on the second click. Both activation and rename happen. This is exactly how Firefox tab rename works — no delay needed.

If the diagnosis shows that GestureClick on the ListBox can't see n_press==2 due to ListBox internal gesture conflicts, fall back to: attach GestureClick to each individual ListBoxRow in `build_row()` (rows handle their own double-click independently of ListBox's internal selection).

### DnD verification

The existing DnD code in `sidebar.rs:336-415` and `row.rs:43-51` should work — DragSource provides wid as string, DropTarget accepts STRING with MOVE action. Need to verify in VM E2E. If broken, diagnose and fix.

### Files to modify
- `src/sidebar.rs` — modify `connect_rename()`, possibly `connect_click()` (replace row_activated with unified GestureClick)
- `src/row.rs` — possibly add per-row GestureClick if ListBox-level approach doesn't work

### Tests (TDD)
- **E2E** `test_double_click_rename_background`: focus xterm (PTM background) → double-click PTM row → type name → verify state.json rename
- **E2E** `test_dnd_reorder`: drag row to new position → verify order changed
- Run all existing 10 E2E tests for regression

---

## Phase B: Tab Groups

### Data Model (`src/state.rs`)

```
DisplaySlot::Window(u32)    — ungrouped window
DisplaySlot::Group(u32)     — group (followed by its members)

Group { id, name, collapsed, members: Vec<u32> }

AppState additions:
  groups: Vec<Group>
  display_order: Vec<DisplaySlot>   — freely interspersed ordering
  next_group_id: u32
```

**`display_items(filter) -> Vec<ItemKind>`** iterates `display_order`:
- `Window(wid)` → `ItemKind::UngroupedWindow(wid)` (if visible)
- `Group(gid)` → `ItemKind::GroupHeader(gid)` + (if not collapsed) `ItemKind::GroupedWindow(wid, gid)` for each member

**Group operations:**
- `create_group(name) -> u32` — creates group, returns ID
- `create_group_with_window(name, wid) -> u32` — creates group, moves wid into it, replaces `DisplaySlot::Window(wid)` with `DisplaySlot::Group(gid)` at same position
- `delete_group(gid)` — members become ungrouped (re-inserted into display_order)
- `rename_group(gid, name)`
- `toggle_group_collapsed(gid)`
- `add_to_group(wid, gid)` — removes from display_order, adds to group.members
- `remove_from_group(wid)` — removes from group.members, adds to display_order
- `reorder_display(from, to)` — reorders top-level slots
- `reorder_in_group(gid, from, to)` — reorders within group

**Cleanup in `update_windows()`:** remove closed windows from group.members and display_order.

### Persistence (`SavedState` extensions)

```
SavedGroup { id, name, collapsed, members: Vec<String> }  // class:instance keys
SavedDisplaySlot::Window(String) | Group(u32)             // class:instance or group ID
```

All new fields use `#[serde(default)]` for backward compatibility with old state.json.

### UI Rendering (`src/sidebar.rs`, `src/row.rs`)

**`rebuild()`** switches from `filtered_windows()` to `display_items()`:
- `GroupHeader` → `row::build_group_header_row(group)` — [▼/▶ icon] [bold name], widget name `"group-{id}"`, CSS class `.ptm-group-header`, DragSource with `"group-{id}"` payload
- `GroupedWindow` → existing `row::build_row()` + CSS class `.ptm-grouped` (indented)
- `UngroupedWindow` → existing `row::build_row()` (unchanged)

**Group header visual (indent model):**
- Group headers: full-width, slightly taller, bold text, darker bg, no left margin
- App tabs inside groups: indented ~16-20px via `.ptm-grouped { margin-left: 20px; }`
- This makes groups "stick out to the left" relative to their children

### Interactions

**Context menu (`connect_context_menu`):**
- Right-click window row: add "Create Group" (creates group with this tab, starts inline rename)
- Right-click window row (if in group): add "Remove from Group"
- Right-click group header: "Rename Group", "Delete Group"

**Click (`row_activated`):**
- Click group header → toggle collapse/expand (don't activate an X11 window)
- Click window row → existing behavior (activate + snap)

**Keyboard (`connect_keyboard`):**
- F2 on group header → inline rename group
- Delete on group header → delete group
- Enter/Space on group header → toggle collapse
- Alt+Up/Down on group header → reorder group in display_order

**Double-click (`connect_rename`):**
- Extend to detect group headers → start group rename

**Group rename (`src/row.rs`):**
- `start_inline_group_rename()` + `commit_group_rename()` — parallel to window rename, calls `state.rename_group()`
- `build_group_header_row()` + `parse_group_id_from_name()`

### DnD Semantics (`connect_dnd`)

Change DragSource payload format: `"wid-{N}"` for windows, `"group-{N}"` for groups (breaking change — update build_row + connect_dnd in same commit).

Drop logic:
| Source | Target | Action |
|--------|--------|--------|
| Window | Window (same group or both ungrouped) | Reorder within group / display_order |
| Window | Window (different group) | Move to target's group |
| Window | Group header | Add to that group |
| Group header | Group header | Reorder groups in display_order |
| Group header | Window | Reorder in display_order (move group to that position) |

### CSS additions (`style.css`)

```css
.ptm-group-header { padding: 6px 4px; font-weight: bold; background-color: #2d2d2d; border-bottom: 1px solid #4a4a4a; }
.ptm-group-header:hover { background-color: #383838; }
.ptm-grouped { margin-left: 20px; }
```

### Unit Tests (`tests/state_test.rs`) — ~18 new tests

- create_group, create_multiple_unique_ids, delete_group, delete_removes_assignments
- rename_group, toggle_collapsed
- add_window_to_group, add_to_nonexistent_noop, remove_from_group, reassignment
- create_group_with_window (replaces DisplaySlot::Window with Group at same position)
- display_items: ungrouped_only, with_group, collapsed_hides_children, interspersed_ordering
- reorder_display, reorder_in_group, reorder_out_of_bounds
- update_windows_cleans_group_assignments
- groups_persist_across_save_restore, backward_compatible_no_groups

### E2E Tests (`test/vm-e2e-runner.sh`)

- `test_create_group`: right-click → Create Group → verify header appears + tab indented
- `test_group_collapse_expand`: click header → verify children hidden/shown
- `test_drag_tab_to_group`: drag ungrouped tab onto group header → verify membership
- `test_group_persistence`: create group → stop PTM → restart → verify group structure

### Implementation Order (TDD)

1. Unit tests (RED): all group tests in state_test.rs
2. State model (GREEN): Group, DisplaySlot, ItemKind, all methods in AppState, SavedState extensions
3. Row rendering: `build_group_header_row()`, `parse_group_id_from_name()`, CSS
4. Sidebar rebuild: switch to `display_items()`
5. Context menu: "Create Group" + group header menu
6. Click handler: collapse/expand on group header click
7. Keyboard: F2/Delete/Enter on group headers
8. DnD: payload format change, all 5 drop combinations
9. Persistence verification: E2E test_group_persistence
10. Full regression: all unit + E2E tests

### Files to modify
- `src/state.rs` — Group, DisplaySlot, ItemKind, all group methods, display_items(), persistence
- `src/row.rs` — build_group_header_row(), parse_group_id_from_name(), start_inline_group_rename(), DragSource payload change
- `src/sidebar.rs` — rebuild() with display_items(), extended DnD/context menu/keyboard/click handlers
- `src/app.rs` — wire up group-related connections if needed
- `style.css` — .ptm-group-header, .ptm-grouped
- `tests/state_test.rs` — ~18 new tests
- `test/vm-e2e-runner.sh` — 4 new E2E tests

---

## Verification

**Phase A:**
- `cargo test` — all 89 unit tests pass
- `PTM_VM=ptm-test bash test/vm-e2e-test.sh` — all E2E pass including new rename + DnD tests

**Phase B:**
- `cargo test` — all ~107 unit tests pass (89 existing + 18 new)
- `PTM_VM=ptm-test bash test/vm-e2e-test.sh` — all E2E pass including 4 new group tests
- Visual review via `virt-viewer ptm-test` — groups look correct, indent model works, DnD feels natural
