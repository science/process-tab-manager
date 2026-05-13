# Process Tab Manager

Vertical sidebar for managing application windows on Linux/X11. Pure X11 via x11rb, single binary.

## Build & Test

```bash
source "$HOME/.cargo/env"
CARGO_TARGET_DIR=/tmp/ptm-dev cargo test                          # 378 unit + 4 e2e (Xvfb)
CARGO_TARGET_DIR=/tmp/ptm-dev cargo test --bin ptm                # unit tests only (~50ms)
CARGO_TARGET_DIR=/tmp/ptm-dev cargo test --test e2e_kill_session  # only the e2e suite (~13s)
CARGO_TARGET_DIR=/tmp/ptm-dev cargo build --release               # Build dev binary
DISPLAY=:0 /tmp/ptm-dev/release/ptm                               # Run (needs X11 desktop)
PTM_BIN=/tmp/ptm-dev/release/ptm tests/e2e/<name>.sh              # Run a single e2e script standalone
```

The crate is a single-binary package (no `[lib]` target), so unit tests live under the `ptm` binary — `cargo test --lib` errors out; use `--bin ptm` instead.

**Dev vs production**: Dev builds use `/tmp/ptm-dev`. The installed launcher (`install.sh`) uses `/tmp/ptm-target`. This keeps `cargo build` during development from overwriting the production binary.

**System dependencies**: building needs Rust + X11 dev headers. Running needs an X11 display. The e2e suite is driven by `tests/e2e_kill_session.rs`, a thin Cargo integration wrapper that shells out to one shell script per `#[test]`. Four scripts live under `tests/e2e/`:

- `menu_kills_session.sh` — right-click → Kill Session via the context menu, popup-accept path.
- `x_button_kills_session.sh` — click the `[x]` glyph on a session row, popup-accept path.
- `spawn_position.sh` — clicking `+ New tmux` snaps the spawned terminal to the sidebar anchor.
- `recipes_survive_restart.sh` — Phase 5c Tier 0a: a saved tmux MEMBER reattaches to its live xterm by session-name match after PTM restart.

Each script needs `xvfb`, `xdotool`, `xterm`, `openbox`, `tmux`, `scrot`, `xdpyinfo` (`sudo apt install xvfb xdotool xterm openbox tmux scrot x11-utils`). They spin up an isolated Xvfb display on `:99` so they don't touch the desktop session, and use a fresh `HOME` so saved groups state can't perturb row layout. Tests share the user's tmux server (default socket) and serialize themselves via a `Mutex` in the wrapper (`E2E_LOCK`); each script picks PID-derived session names so reruns don't collide. The spawn-position test sets `PTM_TERMINAL_CMD=xterm` so it doesn't depend on gnome-terminal/DBus, runs openbox inside Xvfb so ptm sees `_NET_CLIENT_LIST` updates, and uses window-relative `xdotool mousemove --window` clicks so the openbox frame offset doesn't affect coordinates.

## Project Structure

```
Cargo.toml              # Package: process-tab-manager, binary: ptm
src/
  main.rs               # Everything: data model, EWMH, rendering, event loop, unit tests
tests/
  e2e_kill_session.rs   # Cargo integration harness: one #[test] per script under tests/e2e/
  e2e/
    menu_kills_session.sh       # right-click → Kill Session
    x_button_kills_session.sh   # [x] glyph + popup-accept
    spawn_position.sh           # `+ New tmux` snaps terminal to sidebar anchor
    recipes_survive_restart.sh  # Phase 5c: tmux row reattaches after PTM restart
LICENSE
README.md
```

## Architecture

Single-file binary (~12000 LOC) with clean separation of concerns within `main.rs`:

- **Data model** (top): `Item`, `Group`, `DisplaySlot`, `DisplayRow`, `App` — all state management, group operations, drag-and-drop resolution
- **EWMH helpers**: `get_client_list`, `get_active_window`, `get_window_title`, `activate_window`, `snap_to_sidebar` — thin wrappers over X11 properties
- **Renderer**: double-buffered drawing to pixmap, copies to window. Items, group headers, ghost drag, drop indicators, hover, active highlight
- **Context menu**: override-redirect popup with pointer grab. `build_menu_entries` / `open_context_menu` / `draw_context_menu`
- **Event loop**: single `wait_for_event` loop with two modes: context menu (grab active) and normal
- **Tests**: `#[cfg(test)] mod tests` at bottom of `src/main.rs` — 378 unit tests covering pure state logic. The Xvfb-driven e2e harness lives separately under `tests/`.

### What's testable without X11

All `App` methods: `build_display_rows`, `hit_test_row`, `drop_index_from_y`, group operations (`create_group`, `add_to_group`, `remove_from_group`, `delete_group`, `rename_group`, `toggle_collapse`), drag resolution (`handle_drop`, `is_gap_in_group`, `reorder_within_group`), and `build_menu_entries`.

### What requires X11

Rendering, EWMH property queries, window activation/snapping, context menu popup, color allocation.

## x11rb API Notes

- `AtomEnum` variants (e.g. `AtomEnum::WM_CLASS`) can be passed directly to `get_property` — do NOT call `.into()` on them (causes ambiguous type inference)
- Font: Nimbus Mono L 13px with fallback to fixed 13px
- `image_text8` is Latin-1 only — no Unicode, use ASCII (`+`/`-` for collapse)
- Override-redirect windows need explicit event masks + pointer grab for menus
- `grab_pointer(owner_events: false)` routes all pointer events to grab window with relative coords

## Development Environment

Development happens directly on the X11 desktop (Cinnamon on Debian/Ubuntu). The project directory is at `~/dev/process-tab-manager`.

### Desktop recovery

If Cinnamon crashes or becomes unresponsive:

1. Restart Cinnamon: `DISPLAY=:0 nohup cinnamon --replace &`
2. Restart LightDM: `sudo systemctl restart lightdm` (from SSH only — see warning below)
3. Reboot: `sudo reboot` (from SSH only — see warning below)

**NEVER run `systemctl restart lightdm` or `sudo reboot` from within the desktop session.** These commands kill the X11 session, which destroys ALL terminals — including Claude Code itself.

### Environment notes

- **DISPLAY=:0** — all GUI commands need this when running from a non-GUI terminal (SSH, tmux)
- **CARGO_TARGET_DIR=/tmp/ptm-dev** — dev builds; `/tmp/ptm-target` is reserved for the installed production binary via `install.sh`. Both needed because virtiofs mount doesn't support exec
- **sudo** uses fingerprint GUI popup (works from Claude Code shell)
- **git/gh** auth is configured for account `science` (the origin account)

## Git Workflow

- **Local commits are encouraged.** Make intermediate commits freely to checkpoint progress.
- **Never push to origin without asking the user first.**

## TDD Workflow

Every behavior change starts with a failing test (RED), then implementation (GREEN), then verification.

**Tier 1 — Unit tests (preferred).** `cargo test --bin ptm` for all state/group/DnD/parsing logic. No display needed; runs in ~50 ms.

**Tier 2 — Xvfb e2e tests.** `cargo test --test e2e_kill_session` for end-to-end flows that need a real X server, real tmux, and real keyboard/mouse events. Each `#[test]` shells out to a script under `tests/e2e/` (`menu_kills_session.sh`, `x_button_kills_session.sh`, `spawn_position.sh`). Use this tier for behaviors that can't be reproduced from pure state — popup focus interactions, window-spawn snapping, EWMH side effects. Slow (~10 s for the suite); add a script only when a Tier-1 test cannot reach the bug.

**Tier 3 — Manual visual review.** For rendering, colors, layout — launch the app and verify interactively.

### TDD Steps

1. Write or update a test:
   - Tier 1 → in the `#[cfg(test)] mod tests` block at the bottom of `src/main.rs`
   - Tier 2 → add a new `#[test]` in `tests/e2e_kill_session.rs` and a driver under `tests/e2e/<name>.sh`; both `chmod +x` and `set -euo pipefail`
2. Run the test — confirm it fails (RED)
3. Implement the change
4. Run the test — confirm it passes (GREEN)
5. Run the full test suite (`cargo test`) — confirm no regressions across both tiers
6. Commit test and implementation together

## Phase 5a — Recipe-capture UAT (Stage E)

Phase 5a observes — for every visible window — the info PTM would need to relaunch it after a reboot: the controlling executable (Layer 1: `exe` / `cmdline` / `cwd`) and the foreground job running inside any wrapping shell or tmux pane (Layer 2: workload). It does **not** persist or restore anything yet; the data is dumped on SIGUSR1 to a markdown file the user reviews against the live sidebar.

The UAT verdict gates everything else in Cluster 5: if /proc-based capture systematically misses workloads for the user's real apps, Phases 5b–5f don't ship.

### How to trigger a dump

```bash
kill -USR1 $(pgrep ptm)
```

PTM writes `$XDG_CACHE_HOME/ptm/recipes-snapshot.md` (falling back to `~/.cache/ptm/recipes-snapshot.md`) and prints the path on stderr if PTM was launched from a terminal. The dump is fast (a few ms — one /proc walk + one `tmux display-message` per attached session) and side-effect-free with respect to PTM state.

### How to review the dump

1. Open the windows you'd care about restoring after a reboot — at minimum the ones you remember relaunching manually after the last reboot. Include a mix: GUI apps (Firefox, file manager), plain terminals, tmux-attached terminals running real work (`claude`, `npm run dev`, `vim`).
2. Trigger the dump.
3. Open the markdown file. Each window gets a vertical block with a scan-signal header — e.g. `## 2 — ✓ Layer 1, ✓ Layer 2 (Job)` — followed by per-field breakdown.
4. Walk each block in sidebar order. The **PTM label** and **live title** fields disambiguate "which window is this row?" — match them against the sidebar and the window's title bar.
5. For each cell, judge correctness. Annotate inline with HTML comments where something looks wrong:
   ```markdown
   - cwd: `/home/steve`  <!-- ✗ should be /home/steve/Downloads -->
   ```

### What to check per block

- **Layer 1 cwd** matches `pwd` inside the terminal (or the app's actual working directory).
- **Layer 1 cmdline** reproduces what you'd type to relaunch the app. For terminals, this may be the wrapper (`gnome-terminal-server …`) rather than the shell — that's expected.
- **Tmux binding**, when present, names the correct session and points at a non-zero `pane_pid`.
- **Layer 2 workload** is what's actually running in the foreground. For an idle shell, the block should read `✓ Layer 2 (Idle)`. For a tmux'd `claude` session, it should read `✓ Layer 2 (Job)` with `cmdline: claude`. If it reads `✗ Layer 2 unreachable`, the printed `reason` should tell you why (no shell descendant, ambiguous shell parentage, etc.) — those are the bugs worth flagging.

### Verdict that gates Cluster 5

Green-light Phases 5b–5f when:

- ≥ 90 % of rows show ✓ on Layer 1, AND
- All tmux-attached workflow apps (claude, npm dev server, vim under tmux) show ✓ Layer 2 (Job) with sensible cmdlines.

If a non-trivial fraction of rows show ✗ Layer 2 with the same reason (e.g. "N shell descendants of gnome-terminal-server"), pause and decide whether to add a disambiguation strategy before 5b — restoring with the wrong workload would be worse than the current "you relaunch manually" status quo.

## Phases 5b + 5c — Persistence v2 + recipe-tier matching (MVP)

5b persists the Phase-5a-captured `LaunchRecipe` per group member to disk. 5c uses that data at restore time to anchor saved groups to live windows by tmux session or `_NET_WM_PID`, rather than the brittle (label, wm_class) cascade alone. Together they turn the "I restarted PTM and my groups re-shattered" failure mode into "groups re-attach cleanly".

### Wire format (v2)

Header bumps `v1` → `v2`. v1 files still load (members come back with `recipe: None`). Each MEMBER can carry up to three optional lines, in any order, until the next MEMBER or GROUP:

```
v2
GROUP\t<name>\t<collapsed>\t<kind>
MEMBER\t<label>\t<wm_class>\t<custom_prefix>
LAYER1\t<exe>\t<cwd>\t<pid>\t<argc>[\t<arg0>...]
TMUX\t<session_name>\t<session_id>\t<pane>\t<pane_pid>
LAYER2\tjob\t<exe>\t<cwd>\t<argc>[\t<arg0>...]
LAYER2\tidle
LAYER2\tunreachable\t<reason>
```

Field values are percent-encoded: `%` → `%25`, `\t` → `%09`, `\n` → `%0a`. Empty fields (`\t\t`) are the "no value" sentinel for exe/cwd/pid/session_id. v2 reader skips unknown line types (forward-compat for a hypothetical v3); v1 stays strict.

### Matching cascade (5 tiers, head-first)

`restore_groups` and `refresh_items` ghost re-match both route through the same logic:

1. **Tier 0a — Tmux session match** (Normal groups only). `member.recipe.tmux.session_name` against `item.session`.
2. **Tier 0b — Pid + corroborator** (Normal groups only). `member.recipe.pid_at_save` against `item.pid` AND (`item.label == member.label` OR `item.wm_class == member.wm_class`). The corroborator prevents pid-collision false matches when many windows share `gnome-terminal-server`'s pid.
3. **Tier 1 — Exact** `(label, wm_class)`.
4. **Tier 2 — Label-only**.
5. **Tier 3 — wm_class-only**.

TmuxSystem groups skip Tier 0a/0b because they're rebuilt every refresh from `list_tmux_sessions()`.

### Capture-at-save cost

`save_groups` calls `capture_recipes_for_save(&app)` first — one `ProcSnapshot::capture_all()` + one `query_tmux_pane` per distinct `item.session`. ~5–10 ms per call. Save fires at most a handful of times per minute under the existing debounce; cost is negligible.

### MVP UAT

After installing (`./install.sh`) and relaunching PTM:

1. Create a labelled group named `mvp-test` and drag a tmux-wrapped terminal row into it.
2. Inspect `~/.config/ptm/profiles/default/groups`: header is `v2`; the MEMBER for the tmux row is followed by a `LAYER1` line, a `TMUX` line, and a `LAYER2` line.
3. Quit PTM (Ctrl+Q or WM close). Reopen.
4. The `mvp-test` group should reappear with the tmux row already attached — no momentary ghosting, no row-shuffle.
5. Send `kill -USR1 $(pgrep ptm)` to dump current state; verify the dump shows the row under the `mvp-test` group with its `Tmux binding` line populated and a non-zero `pane_pid`.

If step 4 shows the row as a ghost (greyed) or in the wrong group, Tier 0a probably didn't fire — capture some data with `kill -USR1` and check that the saved member's TMUX line names the right session.
