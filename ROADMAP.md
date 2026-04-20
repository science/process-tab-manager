# Process Tab Manager — Roadmap

This file tracks proposed but not-yet-implemented work. Stages A and B (event-driven refresh, tmux-window detection with a session-marker dot) have already shipped; they're mentioned here only as prerequisites for what comes next.

---

## Stage C — Orphan tmux session inventory

### Intent
Show every tmux session in the sidebar, not just the terminal windows that happen to be currently attached. The real value is giving the user a single place that answers *"what persistent work contexts do I have?"* — the way browser-tab managers answer that question for web pages.

Today, if you detach from a tmux session (`C-b d`) and close the terminal, the session keeps running but disappears from your desktop. You have to remember it exists and type `tmux ls` in a new terminal to find it. Stage C makes those orphans visible so they can't be forgotten.

### UX
- At startup, and whenever `_NET_CLIENT_LIST` changes (already a cheap path after Stage A), also call `tmux list-sessions -F '#{session_name} #{session_attached}'`.
- Each session appears as a row in the sidebar:
  - **Attached** (already counted by Stage B on a visible window): normal row, green dot.
  - **Orphaned** (session exists but no client attached to it): distinct visual — suggestion: grey left-stripe instead of colored, hollow circle instead of filled green dot.
- Clicking an orphan row launches a new terminal attached to the session:
  `$TERMINAL -e tmux attach-session -t <name>` (falling back to `xterm` if `$TERMINAL` is unset).
- Right-click menu on an orphan: kill session (`tmux kill-session -t <name>`), rename session, add to group.

### Data-model changes
- New variant on `DisplaySlot`: `Session(String)` alongside the existing `Window(u32)` and `Group(u32)`.
- `DisplayRow` gains a `Session { name: String }` variant so the renderer knows to style it differently.
- Orphan sessions live in `App.display_order` like windows do, so groups can contain a mix of windows and sessions. Groups' `member_wids: Vec<u32>` becomes `members: Vec<GroupMember>` where `GroupMember` is an enum of `Window(u32) | Session(String)`.

### Persistence
- `SavedGroup.members` needs to distinguish "this was a window matched by title+class" from "this was a tmux session named X". Extend the serialization to add a `kind: "window" | "session"` discriminator. `load_groups` then reattaches by session name (cheap and stable) when `kind == "session"`.

### Refresh loop
- `_NET_CLIENT_LIST` changes don't fire when tmux state changes. For session liveness we need either:
  - A periodic poll (~5 s) that calls `tmux list-sessions`, or
  - A tmux control-mode client (`tmux -C attach`) that pushes events on a pipe. Heavier and more code, but zero-latency.
- Start with periodic polling — 5 s is cheap and predictable. Revisit if users notice the lag.

### Edge cases to handle up front
- tmux binary not installed → silently no sessions (Stage B already behaves this way).
- Session whose name contains characters PTM's Latin-1 renderer can't display → show a sanitised label but keep the real name for `tmux attach -t`.
- Two sessions with the same name: tmux forbids this, but guard anyway.
- The user starts tmux outside PTM, then launches PTM: the orphan must appear within one poll cycle of PTM starting.

### Why not now
Stage C needs a new `DisplaySlot` variant, a persistence-format change, drag-and-drop that works across two item types, right-click menu items that only make sense for sessions, and a terminal-launch code path. Each is small, but together they're a bigger change than Stage A+B combined, and the best test for "is the feature worth building" is *using* Stage B for a while first. If you find yourself wishing the sidebar remembered your tmux sessions across PTM restarts, that's the signal to build C. If you don't, don't bother.

---

## Stage D — "New terminal" button inside PTM

### Proposal
A `+` button (probably a fixed header row above the item list, or a footer) that launches a new terminal emulator wrapping `tmux new-session -s "ptm-<timestamp>"`. The original proposal: because PTM owns the launch, it can *guarantee* every terminal it spawns is session-backed, which makes Stage C's orphan inventory strictly more useful.

### My objections

I argued against Stage D while planning A–C. The objections stand; I'm writing them here so the reasoning isn't lost.

1. **The marginal value over existing launchers is small.** Every Linux desktop already has at least one way to open a terminal — a keybind, a panel launcher, a taskbar button. Adding *another* button specifically inside PTM saves maybe two seconds per session and costs a row of sidebar real-estate.

2. **The "guaranteed persistent" argument is thin.** You can get the same guarantee without PTM by putting a single line in your shell rc:
   ```sh
   # bash / zsh
   [[ -z "$TMUX" && -n "$PS1" ]] && exec tmux new-session -A -s main
   ```
   Now *every* terminal you open — regardless of how it was launched — attaches to a tmux session. PTM gets to inventory those sessions (Stage C) without needing to own their creation. This moves the policy out of PTM and into the user's shell, where it belongs.

3. **Stage D bakes policy into PTM.** Owning terminal creation means PTM has opinions about terminal emulator, shell, session naming, default working directory, tmux flags (`new-session` vs `new-session -A` vs `new-window`), and what happens when an ad-hoc session you created from the button later orphans. Each of these is a potential bikeshed. The shell-rc approach dodges all of it because the user is already the owner of their shell config.

4. **It conflates two concerns.** PTM's current job description is *organise existing windows*. Stage B and C both stay within that job — they just add smarter grouping and awareness of one kind of non-window entity. Stage D changes PTM's job description to *also a launcher*. Launchers are a different product category (think krunner, rofi, ulauncher) and PTM isn't positioned to compete there.

### Alternatives

If you still want a "fast path to a new session", consider these in order of smallest-change-first:

1. **Shell-rc autowrap (recommended).** The one-liner above. Zero PTM code. Works universally. The only downside: it doesn't let you *name* a session at creation time — but you can rename it with `C-b $` or via PTM's Stage C right-click menu once it exists.

2. **A standalone helper script.** `~/.local/bin/ptm-new-terminal`:
   ```sh
   #!/bin/sh
   exec "${TERMINAL:-xterm}" -e tmux new-session -As "${1:-ptm-$(date +%s)}"
   ```
   Wire it to a desktop keybind (Super+T). PTM stays out of it. You get an easy name-at-creation flow via `ptm-new-terminal my-session`.

3. **Stage D proper, but as an opt-in.** If Stages A–C ship and you still want the button, add it behind a config flag (`PTM_SHOW_NEW_TERMINAL=1` env var or a `~/.config/ptm/config.toml` entry). Default off. That way the button exists for the users who want it without forcing the policy on everyone.

If Stage D ever does get built, the minimum viable form is: a fixed header row with a `+`, click spawns `${TERMINAL:-xterm} -e tmux new-session -As "ptm-$(date +%s)"`. Tab into Stage C's orphan inventory once the session exists.

---

## Other future considerations

- **Support for session tools beyond tmux.** `abduco`, `dtach`, `screen`, `zellij` — each has its own session-listing CLI. Could live behind a `SessionBackend` trait with one impl per tool, auto-detected at startup. Low priority unless users ask.

- **30-second safety-net poll as a fallback to Stage A.** Some non-EWMH-compliant window managers might not update `_NET_CLIENT_LIST` reliably. If we see reports of missed refreshes, add a cheap periodic fallback poll (e.g. every 30 s via `select()` on the X11 fd with a timeout). The cost is minimal; the robustness gain may be worth it.

- **Process-tree-aware "long-running" detection beyond tmux.** If we ever care about non-tmux persistent workloads (say a window running `nohup long-job.sh`, or a Jupyter server), we'd need a more general "this window hosts a process you probably don't want to lose" heuristic. No concrete use case yet — deferred indefinitely.

- **Better session-name rendering.** Latin-1-only `image_text8` (the current text path) can't render session names with Unicode characters. If this becomes a pain point, switch to Xft/Xrender for proper UTF-8 text rendering — larger change, affects all rendering not just sessions.
