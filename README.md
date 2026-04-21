# Process Tab Manager

A vertical tab bar for terminals on Linux/X11. An IDE-style sidebar for your shell sessions, because terminals are where AI-assisted development lives now.

## Why this exists

Editors have tabs for files — VS Code, JetBrains, Sublime, Vim. Everyone agrees "keep many files open, switch between them fast" is the right model for writing code. Nothing like this exists for terminals.

That matters more now than it used to. AI-assisted development happens inside terminals: Claude Code, aider, opencode, codex-cli, local-model shells. A real project runs five or six terminals at once — one with the agent, one tailing logs, one for git, one for ad-hoc shell work, one for a dev server. Your WM's Alt-Tab treats them all as "Terminal" and your taskbar shows six identical icons.

Process Tab Manager is a vertical tab bar for those sessions. Group them. Rename them. See which ones are attached to a tmux session. Open a new one with a click.

It also works for non-terminal windows (browsers, editors) — anything the WM reports via EWMH shows up — but the design centre is terminal workflows.

## Features

- **Live window list** — automatically discovers all windows on the current desktop via EWMH
- **Click to focus + snap** — click a row to activate that window and snap it beside the sidebar
- **Tab grouping** — right-click to create groups, add/remove windows, collapse/expand
- **Drag-and-drop reorder** — drag rows to reorder, drag between groups, drag out to ungroup
- **Right-click context menu** — New Group, Add to Group, Remove from Group, Rename, Delete
- **Active window highlight** — blue accent stripe and tinted background on the focused window
- **Hover feedback** — subtle highlight on mouse-over
- **OneDark color scheme** — dark background, warm gray text, colorful accent stripes

## Building

Requires Rust and X11 development headers. `tmux` is an optional runtime dependency — install it to get the session-marker dot next to tmux-backed windows; PTM runs fine without it.

```bash
cargo build --release
```

The binary is at `target/release/ptm`.

To install the binary, icon, and desktop entry into `~/.local/` (and install tmux if missing):

```bash
./install.sh
```

## Running

```bash
DISPLAY=:0 ./target/release/ptm
```

Or if using a virtiofs mount (no exec):
```bash
CARGO_TARGET_DIR=/tmp/ptm-target cargo build --release
DISPLAY=:0 /tmp/ptm-target/release/ptm
```

## Configuration

### Custom terminal command

The "+" button and click-to-attach resolve the terminal command in this order:

1. `$PTM_TERMINAL_CMD` — PTM-specific override. Whitespace-split into argv.
2. `$TERMINAL` — generic fallback.
3. `x-terminal-emulator` — Debian/Ubuntu default alternative.
4. `xdg-terminal-exec` — freedesktop default.
5. `xterm` — last resort.

`$PTM_TERMINAL_CMD` exists separately from `$TERMINAL` because the latter is widely consumed by other tools (git editors, `less`, `man`) which expect a plain emulator binary and misbehave when extra args are baked in.

To make PTM launch a specific terminal with your daily profile across every machine, set it for your graphical session and sync via yadm or similar:

```
# ~/.config/environment.d/99-ptm.conf
PTM_TERMINAL_CMD=gnome-terminal --profile=MyProfile
```

Values are split on whitespace, so args containing spaces aren't supported directly — wrap those in a shell script (`~/.local/bin/my-terminal`) and point `PTM_TERMINAL_CMD` at it.

### Tmux status bar

Click-to-attach runs `tmux attach-session` inside the new terminal, which shows tmux's default green status bar at the bottom. To disable it globally:

```
# ~/.tmux.conf
set -g status off
```

Reload with `tmux source-file ~/.tmux.conf` in any attached session, or start a new one.

## Testing

```bash
cargo test     # 25 unit tests, no X11 display needed
```

Tests cover state management, group operations, drag-and-drop resolution, hit testing, and context menu entry generation — all pure logic that runs without a display server.

## Architecture

Single Rust binary (~1800 LOC) talking directly to X11 via [x11rb](https://github.com/psychon/x11rb). No toolkit, no framework, no webview — just rectangles, text, and event handling.

- **One event loop** — `wait_for_event` dispatches to context menu mode or normal mode
- **Double-buffered rendering** — draw to pixmap, copy to window
- **Override-redirect popup** — context menu is a borderless window with pointer grab
- **EWMH protocol** — `_NET_CLIENT_LIST`, `_NET_ACTIVE_WINDOW`, `_NET_WM_NAME` for window discovery

## License

MIT
