use std::collections::{HashMap, HashSet};
use std::io::Write;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::COPY_DEPTH_FROM_PARENT;

// Layout constants
const WIN_W: u16 = 250;
const WIN_H: u16 = 600;
const ITEM_MARGIN: i16 = 8; // margin on each side
const ITEM_H: u16 = 28;
const ITEM_SPACING: i16 = 2;
const HEADER_H: u16 = ITEM_H; // "+ New terminal" button row at the top
const ITEM_Y_START: i16 = HEADER_H as i16 + ITEM_SPACING + 8;
const DRAG_THRESHOLD: i16 = 5;
const GROUP_INDENT: i16 = 16;
const MENU_ITEM_H: u16 = 24;
const MENU_PADDING: i16 = 4;
const MENU_MIN_W: u16 = 180;
const CONFIRM_MIN_W: u16 = 240;
const CONFIRM_BUTTON_H: u16 = 26;
const CONFIRM_BUTTON_W: u16 = 64;
const CONFIRM_PADDING: i16 = 12;
const TOP_BUTTON_GAP: i16 = 4;
const CHAR_WIDTH: i16 = 8; // approximate for Nimbus Mono L 13px
/// Width of the right-edge band on session rows that responds to a click as
/// "close this session". The renderer paints "x" inside this band; label
/// truncation uses this same width as its right-edge reserve.
const SESSION_CLOSE_BAND_WIDTH: i16 = 16;

// How long a pending attach claim stays live waiting for its new window to
// appear before giving up. Cheap upper bound — typical gnome-terminal launch
// is ~300 ms on this machine; anything past a few seconds means the spawn
// failed or the user closed the window before it registered.
const PENDING_ATTACH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

// Colors — OneDark-inspired palette
const BG_COLOR: u32 = 0x282c34;
const TEXT_COLOR: u32 = 0xabb2bf;
const TEXT_DIM_COLOR: u32 = 0x5c6370;
const INDICATOR_COLOR: u32 = 0x61afef;
const GHOST_COLOR: u32 = 0x3e4451;
const ITEM_COLOR: u32 = 0x2c313a;
const ITEM_HOVER_COLOR: u32 = 0x333842;
const ITEM_ACTIVE_COLOR: u32 = 0x2d3340;
const ACTIVE_STRIPE_COLOR: u32 = 0x61afef;
const MENU_BG_COLOR: u32 = 0x21252b;
const MENU_BORDER_COLOR: u32 = 0x3e4451;
const MENU_HOVER_COLOR: u32 = 0x2c313a;
const GROUP_HEADER_COLOR: u32 = 0x21252b;
const SESSION_MARKER_COLOR: u32 = 0x98c379; // OneDark green — tmux-backed window indicator
// Subdued blue for the rename selection background; dark enough that the
// regular text colour stays readable on top. Also reused for the
// post-drop row flash (T3.5).
const SELECTION_BG_COLOR: u32 = 0x3a5a7e;

// Accent colors for left-edge stripe (OneDark)
const ACCENT_COLORS: &[u32] = &[0xe06c75, 0x98c379, 0x61afef, 0xc678dd, 0xe5c07b, 0x56b6c2];
const GROUP_COLORS: &[u32] = &[0x61afef, 0xe06c75, 0x98c379, 0xc678dd, 0xe5c07b, 0x56b6c2];


// ── Atoms ──

struct Atoms {
    net_client_list: Atom,
    net_active_window: Atom,
    net_wm_name: Atom,
    net_wm_desktop: Atom,
    net_current_desktop: Atom,
    net_frame_extents: Atom,
    net_workarea: Atom,
    utf8_string: Atom,
    net_wm_window_type: Atom,
    net_wm_window_type_normal: Atom,
    wm_protocols: Atom,
    wm_delete_window: Atom,
    net_wm_pid: Atom,
    ptm_wake: Atom,
    ptm_save_tick: Atom,
}

impl Atoms {
    fn new(conn: &impl Connection) -> Result<Self, Box<dyn std::error::Error>> {
        let c0 = conn.intern_atom(false, b"_NET_CLIENT_LIST")?;
        let c1 = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW")?;
        let c2 = conn.intern_atom(false, b"_NET_WM_NAME")?;
        let c3 = conn.intern_atom(false, b"_NET_WM_DESKTOP")?;
        let c4 = conn.intern_atom(false, b"_NET_CURRENT_DESKTOP")?;
        let c5 = conn.intern_atom(false, b"_NET_FRAME_EXTENTS")?;
        let c6 = conn.intern_atom(false, b"_NET_WORKAREA")?;
        let c7 = conn.intern_atom(false, b"UTF8_STRING")?;
        let c8 = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE")?;
        let c9 = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_NORMAL")?;
        let c10 = conn.intern_atom(false, b"WM_PROTOCOLS")?;
        let c11 = conn.intern_atom(false, b"WM_DELETE_WINDOW")?;
        let c12 = conn.intern_atom(false, b"_NET_WM_PID")?;
        let c13 = conn.intern_atom(false, b"_PTM_WAKE")?;
        let c14 = conn.intern_atom(false, b"_PTM_SAVE_TICK")?;
        Ok(Self {
            net_client_list: c0.reply()?.atom,
            net_active_window: c1.reply()?.atom,
            net_wm_name: c2.reply()?.atom,
            net_wm_desktop: c3.reply()?.atom,
            net_current_desktop: c4.reply()?.atom,
            net_frame_extents: c5.reply()?.atom,
            net_workarea: c6.reply()?.atom,
            utf8_string: c7.reply()?.atom,
            net_wm_window_type: c8.reply()?.atom,
            net_wm_window_type_normal: c9.reply()?.atom,
            wm_protocols: c10.reply()?.atom,
            wm_delete_window: c11.reply()?.atom,
            net_wm_pid: c12.reply()?.atom,
            ptm_wake: c13.reply()?.atom,
            ptm_save_tick: c14.reply()?.atom,
        })
    }
}

// Spawn a thread that wakes PTM every `interval` by sending an X11
// ClientMessage to our own window. The message carries the `ptm_wake` atom as
// its type, which the main event loop recognises and uses as a cue to
// refresh items. This is how tmux state changes (sessions created or
// destroyed outside PTM) get picked up — tmux doesn't fire X11 events of
// its own, so without this poll the sidebar would only update when some
// other X activity happens to trigger a refresh.
/// Send a wake-atom ClientMessage to ourselves so the main loop processes
/// the wake on its next iteration. Used right after spawning a new tmux
/// session so the system group picks it up without waiting for the next
/// 5-second poll. Errors are swallowed — failure here is purely cosmetic
/// (the next periodic poll catches up).
fn poke_self(conn: &impl Connection, window: Window, wake_atom: Atom) {
    let data = ClientMessageData::from([0u32; 5]);
    let ev = ClientMessageEvent {
        response_type: 33,
        format: 32,
        sequence: 0,
        window,
        type_: wake_atom,
        data,
    };
    let _ = conn.send_event(false, window, EventMask::NO_EVENT, ev);
    let _ = conn.flush();
}

fn spawn_tmux_poll_thread(window: Window, wake_atom: Atom, interval: std::time::Duration) {
    std::thread::spawn(move || {
        // Give the main loop a moment to reach wait_for_event before we start
        // pinging, so our first wake doesn't race the initial refresh.
        std::thread::sleep(interval);
        let Ok((c, _)) = x11rb::connect(None) else {
            return;
        };
        loop {
            let data = ClientMessageData::from([0u32; 5]);
            let ev = ClientMessageEvent {
                response_type: 33, // ClientMessage
                format: 32,
                sequence: 0,
                window,
                type_: wake_atom,
                data,
            };
            let _ = c.send_event(false, window, EventMask::NO_EVENT, ev);
            let _ = c.flush();
            std::thread::sleep(interval);
        }
    });
}

/// Wakes PTM every `interval` so the dirty-flag debounce can be checked
/// during pure-idle periods (no X events flowing). Identical mechanism to
/// `spawn_tmux_poll_thread` but with a distinct atom so the main loop can
/// avoid the cost of refreshing tmux state on every save tick.
fn spawn_save_tick_thread(window: Window, save_tick_atom: Atom, interval: std::time::Duration) {
    std::thread::spawn(move || {
        std::thread::sleep(interval);
        let Ok((c, _)) = x11rb::connect(None) else {
            return;
        };
        loop {
            let data = ClientMessageData::from([0u32; 5]);
            let ev = ClientMessageEvent {
                response_type: 33,
                format: 32,
                sequence: 0,
                window,
                type_: save_tick_atom,
                data,
            };
            let _ = c.send_event(false, window, EventMask::NO_EVENT, ev);
            let _ = c.flush();
            std::thread::sleep(interval);
        }
    });
}

// ── Data model ──

struct Item {
    wid: u32,
    label: String,
    #[allow(dead_code)]
    wm_class: String,
    accent_pixel: u32,
    custom_prefix: String,
    session: Option<String>,
}

impl Item {
    fn display_label(&self) -> String {
        if self.custom_prefix.is_empty() {
            self.label.clone()
        } else {
            format!("{}: {}", self.custom_prefix, self.label)
        }
    }
}

/// Distinguishes user-created groups from the auto-managed "Tmux Sessions"
/// system group. A `TmuxSystem` group can't be deleted from the UI and its
/// members are derived from `list_tmux_sessions()` rather than user actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupKind {
    Normal,
    TmuxSystem,
}

struct Group {
    id: u32,
    name: String,
    collapsed: bool,
    kind: GroupKind,
    /// Members in display order. For `Normal` groups each member carries an
    /// identity tuple (label, wm_class) for cross-restart matching plus an
    /// optional `live_wid`. A member with `live_wid: None` is a "ghost" —
    /// saved on disk but not currently mapped to an X11 window. Ghost
    /// members are preserved across PTM lifecycle events so groups don't get
    /// wiped when their windows briefly disappear (Phase 2c / Stage F fix
    /// for FM-2 in MVP_PLAN.md).
    ///
    /// For `TmuxSystem` groups, members reuse the same struct with this
    /// convention: `label = session_name`, `wm_class = ""`, `live_wid = None`.
    /// The session-name slot is derived every refresh from
    /// `list_tmux_sessions()`, so persistence of these members is incidental
    /// (the next refresh fully reconstructs them).
    members: Vec<GroupMember>,
}

#[derive(Clone, Debug)]
struct GroupMember {
    /// Window title at the time the member was added; used for matching
    /// when a window with the same identity reappears.
    label: String,
    /// WM_CLASS, also used for matching.
    wm_class: String,
    /// Restored onto the matched item; not used for matching.
    custom_prefix: String,
    /// Some(wid) when bound to a live X11 window; None for ghosts.
    live_wid: Option<u32>,
}

impl Group {
    /// Wids for currently-live members, in display order. Ghost members
    /// (live_wid: None) are skipped.
    fn live_wids(&self) -> Vec<u32> {
        self.members.iter().filter_map(|m| m.live_wid).collect()
    }

    /// Number of live (non-ghost) members.
    fn live_count(&self) -> usize {
        self.members.iter().filter(|m| m.live_wid.is_some()).count()
    }

    /// Count rendered next to the group name in headers. Normal groups
    /// surface their live (non-ghost) member count; TmuxSystem session
    /// members carry their session name in `label` and have `live_wid:
    /// None` by design (sessions aren't X11 windows), so we fall back to
    /// the raw member count there.
    fn display_count(&self) -> usize {
        match self.kind {
            GroupKind::Normal => self.live_count(),
            GroupKind::TmuxSystem => self.members.len(),
        }
    }

    /// Index in `members` of the member currently bound to `wid`, if any.
    fn position_of_live_wid(&self, wid: u32) -> Option<usize> {
        self.members.iter().position(|m| m.live_wid == Some(wid))
    }
}

#[derive(Clone, Debug)]
enum DisplaySlot {
    Window(u32),
    Group(u32),
}

#[derive(Clone, Debug)]
enum DisplayRow {
    GroupHeader { group_id: u32 },
    Window { wid: u32, group_id: Option<u32> },
    Session { name: String, group_id: Option<u32> },
}

#[derive(Clone, Debug)]
enum MenuAction {
    CreateGroup,
    AddToGroup(u32),
    RemoveFromGroup,
    RenameGroup,
    DeleteGroup,
    RenameTab,
    AttachSession,
    RenameSession,
    KillSession,
}

struct MenuEntry {
    label: String,
    action: MenuAction,
}

struct ContextMenu {
    window: Window,
    pixmap: Pixmap,
    entries: Vec<MenuEntry>,
    target_row: usize,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    hover_index: Option<usize>,
}

#[derive(Clone, Debug)]
enum ConfirmAction {
    /// Run `tmux kill-session -t <name>` after the user confirms. The session
    /// name is captured at popup-open time so it survives any later refresh.
    KillSession(String),
}

/// What `pending_spawn` is waiting for. Whichever variant is set, the next
/// newly-detected wid (claimed by `claim_pending_spawn` from refresh_items)
/// is snapped to the sidebar anchor. The Attach variant additionally binds
/// the wid to its tmux session in `Item::session`.
#[derive(Clone, Debug)]
enum PendingSpawnKind {
    /// Spawned terminal is expected to attach to this tmux session.
    Attach(String),
    /// Plain terminal launch (no tmux). Snap-only; no session binding.
    Terminal,
}

#[derive(Clone, Debug)]
struct PendingSpawn {
    kind: PendingSpawnKind,
    spawned_at: std::time::Instant,
}

/// Successful claim from `claim_pending_spawn`. The caller snaps the wid
/// and, when present, treats `attach_session` as already pre-assigned in
/// the ancestor-walk loop (mirrors the previous `claim_pending_attach`
/// HashSet<String> return).
struct PendingClaim {
    wid: u32,
    attach_session: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConfirmButton {
    Yes,
    No,
}

struct ConfirmPopup {
    window: Window,
    pixmap: Pixmap,
    message: String,
    action: ConfirmAction,
    width: u16,
    height: u16,
    yes_rect: Rectangle,
    no_rect: Rectangle,
    hover_button: Option<ConfirmButton>,
}

/// Identity for the buttons in the header row above the item list. When tmux
/// isn't installed the top row holds a single full-width "+ New terminal";
/// when tmux is available it splits into left "+ New terminal" and right
/// "+ New tmux".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TopButton {
    NewTerminal,
    NewTmux,
}

struct DragState {
    source_row: usize,
    start_x: i16,
    start_y: i16,
    current_y: i16,
    started: bool,
}

enum RenameTarget {
    Group(u32),
    Window(u32),
    Session(String), // rename a tmux session via `tmux rename-session`
}

struct RenameState {
    target: RenameTarget,
    text: String,
    cursor: usize, // byte position in text
    selection_anchor: Option<usize>, // byte position; None = no active selection
}

impl RenameState {
    /// Returns (start, end) byte positions, normalized so start < end.
    /// None when no anchor is set or when the anchor is collapsed onto the cursor.
    fn selection_range(&self) -> Option<(usize, usize)> {
        match self.selection_anchor {
            Some(a) if a != self.cursor => Some((a.min(self.cursor), a.max(self.cursor))),
            _ => None,
        }
    }

    #[allow(dead_code)] // exposed for tests / external callers
    fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    /// Set anchor to current cursor only if it isn't already set. Used at the
    /// start of a Shift+arrow extension so subsequent Shift+arrows extend from
    /// the original cursor position.
    fn anchor_if_none(&mut self) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
    }

    fn prev_char_boundary(&self) -> usize {
        self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    fn next_char_boundary(&self) -> usize {
        self.text[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| self.cursor + i)
            .unwrap_or(self.text.len())
    }

    /// Apply a cursor move to `target`. With shift, extend selection (anchor at
    /// the original cursor). Without shift, clear selection.
    fn apply_motion(&mut self, target: usize, shift: bool) {
        if shift {
            self.anchor_if_none();
        } else {
            self.clear_selection();
        }
        self.cursor = target;
    }

    fn move_left_char(&mut self, shift: bool) {
        // Plain Left with active selection collapses to selection start without
        // moving the cursor further (gtk-style; matches modern text inputs).
        if !shift {
            if let Some((start, _)) = self.selection_range() {
                self.cursor = start;
                self.clear_selection();
                return;
            }
        }
        let target = self.prev_char_boundary();
        self.apply_motion(target, shift);
    }

    fn move_right_char(&mut self, shift: bool) {
        if !shift {
            if let Some((_, end)) = self.selection_range() {
                self.cursor = end;
                self.clear_selection();
                return;
            }
        }
        let target = self.next_char_boundary();
        self.apply_motion(target, shift);
    }

    fn move_home(&mut self, shift: bool) {
        self.apply_motion(0, shift);
    }

    fn move_end(&mut self, shift: bool) {
        let len = self.text.len();
        self.apply_motion(len, shift);
    }

    fn move_left_word(&mut self, shift: bool) {
        let target = prev_word_boundary(&self.text, self.cursor);
        self.apply_motion(target, shift);
    }

    fn move_right_word(&mut self, shift: bool) {
        let target = next_word_boundary(&self.text, self.cursor);
        self.apply_motion(target, shift);
    }

    fn select_all(&mut self) {
        self.selection_anchor = Some(0);
        self.cursor = self.text.len();
    }

    /// Delete from cursor back to the previous word boundary.
    /// If a selection exists, deletes only the selection (selection takes
    /// precedence — standard UX).
    fn delete_word_left(&mut self) {
        if self.delete_selection() {
            return;
        }
        let target = prev_word_boundary(&self.text, self.cursor);
        if target < self.cursor {
            self.text.drain(target..self.cursor);
            self.cursor = target;
        }
    }

    /// Delete from cursor forward to the next word boundary.
    /// If a selection exists, deletes only the selection.
    fn delete_word_right(&mut self) {
        if self.delete_selection() {
            return;
        }
        let target = next_word_boundary(&self.text, self.cursor);
        if target > self.cursor {
            self.text.drain(self.cursor..target);
        }
    }

    /// If a selection exists, drop the selected bytes and return true.
    /// Cursor lands at the start of the deleted range. Mirrors the helper
    /// shape used internally by the editing operations below.
    fn delete_selection(&mut self) -> bool {
        if let Some((lo, hi)) = self.selection_range() {
            self.text.drain(lo..hi);
            self.cursor = lo;
            self.selection_anchor = None;
            true
        } else {
            false
        }
    }

    /// Insert a char at the cursor. If a selection exists, replace it first.
    fn insert_char(&mut self, ch: char) {
        self.delete_selection();
        self.text.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    /// Backspace: if selection, delete it; else delete the char before cursor.
    fn delete_back_char(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor == 0 {
            return;
        }
        let prev = self.prev_char_boundary();
        self.text.drain(prev..self.cursor);
        self.cursor = prev;
    }

    /// Delete (forward): if selection, delete it; else delete the char after cursor.
    fn delete_forward_char(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor >= self.text.len() {
            return;
        }
        let next = self.next_char_boundary();
        self.text.drain(self.cursor..next);
    }
}

/// Find the byte offset of the next word boundary at or after `pos`.
/// Word = run of `is_alphanumeric` chars; anything else is a separator.
/// Stops at the end of the next word (readline-style: skip separators, then
/// skip word). Unicode-safe via `chars()`.
fn next_word_boundary(s: &str, pos: usize) -> usize {
    let mut i = pos;
    let len = s.len();
    // Skip leading non-alnum chars.
    while i < len {
        let ch = s[i..].chars().next().unwrap();
        if ch.is_alphanumeric() {
            break;
        }
        i += ch.len_utf8();
    }
    // Then skip the alnum run.
    while i < len {
        let ch = s[i..].chars().next().unwrap();
        if !ch.is_alphanumeric() {
            break;
        }
        i += ch.len_utf8();
    }
    i
}

/// Find the byte offset of the previous word boundary at or before `pos`.
/// Mirror of `next_word_boundary`.
fn prev_word_boundary(s: &str, pos: usize) -> usize {
    let mut i = pos;
    // Skip trailing non-alnum chars.
    while i > 0 {
        let prev = s[..i].char_indices().next_back().map(|(j, _)| j).unwrap_or(0);
        let ch = s[prev..i].chars().next().unwrap();
        if ch.is_alphanumeric() {
            break;
        }
        i = prev;
    }
    // Then skip the alnum run.
    while i > 0 {
        let prev = s[..i].char_indices().next_back().map(|(j, _)| j).unwrap_or(0);
        let ch = s[prev..i].chars().next().unwrap();
        if !ch.is_alphanumeric() {
            break;
        }
        i = prev;
    }
    i
}

// ── App ──

struct App {
    items: Vec<Item>,
    groups: Vec<Group>,
    display_order: Vec<DisplaySlot>,
    display_rows: Vec<DisplayRow>,
    next_group_id: u32,
    context_menu: Option<ContextMenu>,
    confirm: Option<ConfirmPopup>,
    rename: Option<RenameState>,
    active_wid: Option<u32>,
    hover_row: Option<usize>,
    top_button_hover: Option<TopButton>,
    /// Whether `tmux -V` succeeds. Probed once at startup; cached here so
    /// the renderer / hit-test path doesn't fork tmux on every paint. Drives
    /// visibility of the "+ New tmux" button.
    tmux_available: bool,
    drag: Option<DragState>,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    our_wid: u32,
    subscribed_wids: HashSet<u32>,
    // PTM tracks "we just spawned a window, please claim and snap it" via
    // pending_spawn. Set when the user clicks an orphan session row, when
    // either of the `+ New *` buttons fire, or any other ptm-initiated
    // window spawn. The next newly-appearing wid (per refresh_items'
    // delta) is snapped to the sidebar anchor; for Attach kinds the wid
    // is also bound to its tmux session. Process-tree-based marker
    // detection can't reliably do this — terminals that fork through a
    // shared server pid (gnome-terminal, konsole) hide their parent.
    pending_spawn: Option<PendingSpawn>,
    /// Maps each tmux `#{session_id}` (e.g. "$0") to the session's ORIGINAL
    /// name as observed at first sighting. Survives user-initiated renames
    /// so the UI can keep showing the origin (typically a small integer)
    /// alongside the new label. Pruned when sessions disappear; rebuilt
    /// from scratch every PTM start (not persisted).
    session_origins: HashMap<String, String>,
    /// Set on the FIRST mutation in a dirty epoch; preserved across
    /// subsequent mutations. Used by the 30-second backstop to bound the
    /// worst-case data loss when a long burst of edits would otherwise
    /// keep the debounce window open indefinitely.
    first_dirty_at: Option<std::time::Instant>,
    /// Updated on EVERY mutation; the debounced save fires when enough idle
    /// time has elapsed since this timestamp.
    last_dirty_at: Option<std::time::Instant>,
    /// (wid, started_at) for a drop the user just completed. The renderer
    /// uses this to flash the destination row briefly so the user sees
    /// where their dragged window landed (G-5 / Stage G T3.5). Cleared
    /// implicitly: after DROP_HIGHLIGHT_DURATION elapses, the renderer
    /// stops drawing the highlight even if the field is still set.
    last_drop_highlight: Option<(u32, std::time::Instant)>,
}

/// How long a successful-drop row highlight stays visible before fading out.
/// (Drops that resolved as no-ops do not set the highlight.)
const DROP_HIGHLIGHT_DURATION: std::time::Duration = std::time::Duration::from_millis(1500);

impl App {
    fn new(our_wid: u32) -> Self {
        Self {
            items: Vec::new(),
            groups: Vec::new(),
            display_order: Vec::new(),
            display_rows: Vec::new(),
            next_group_id: 0,
            context_menu: None,
            confirm: None,
            rename: None,
            active_wid: None,
            hover_row: None,
            top_button_hover: None,
            tmux_available: false,
            drag: None,
            x: 0,
            y: 0,
            width: WIN_W,
            height: WIN_H,
            our_wid,
            subscribed_wids: HashSet::new(),
            pending_spawn: None,
            session_origins: HashMap::new(),
            first_dirty_at: None,
            last_dirty_at: None,
            last_drop_highlight: None,
        }
    }

    /// Returns true while a post-drop highlight is still within its
    /// fade duration. Used by the renderer to flash the destination row.
    fn drop_highlight_active_for(&self, wid: u32) -> bool {
        match self.last_drop_highlight {
            Some((hwid, when)) if hwid == wid => {
                when.elapsed() < DROP_HIGHLIGHT_DURATION
            }
            _ => false,
        }
    }

    fn hit_test_header_button(&self, y: i16) -> bool {
        y >= 0 && y < HEADER_H as i16
    }

    /// Returns which top button (if any) is at point (x, y) in window coords.
    /// `None` means outside the header row, or in the gap between the two
    /// buttons. Resolves the layout from the cached `tmux_available` flag.
    fn hit_test_top_buttons(&self, x: i16, y: i16) -> Option<TopButton> {
        if !self.hit_test_header_button(y) {
            return None;
        }
        let (left, right_opt) = top_buttons_layout(self.tmux_available, self.width);
        if point_in_rect(x, y, &left) {
            return Some(TopButton::NewTerminal);
        }
        if let Some(right) = right_opt {
            if point_in_rect(x, y, &right) {
                return Some(TopButton::NewTmux);
            }
        }
        None
    }

    /// Mark persistence-relevant state as dirty. Idempotent within a single
    /// dirty epoch — `first_dirty_at` is preserved, `last_dirty_at` advances.
    fn mark_dirty(&mut self) {
        let now = std::time::Instant::now();
        if self.first_dirty_at.is_none() {
            self.first_dirty_at = Some(now);
        }
        self.last_dirty_at = Some(now);
    }

    fn clear_dirty(&mut self) {
        self.first_dirty_at = None;
        self.last_dirty_at = None;
    }

    #[allow(dead_code)] // exposed for tests / external callers
    fn is_dirty(&self) -> bool {
        self.first_dirty_at.is_some()
    }

    fn build_display_rows(&mut self) {
        self.display_rows.clear();
        for slot in &self.display_order {
            match slot {
                DisplaySlot::Window(wid) => {
                    self.display_rows.push(DisplayRow::Window {
                        wid: *wid,
                        group_id: None,
                    });
                }
                DisplaySlot::Group(gid) => {
                    self.display_rows.push(DisplayRow::GroupHeader { group_id: *gid });
                    if let Some(group) = self.groups.iter().find(|g| g.id == *gid) {
                        if !group.collapsed {
                            match group.kind {
                                GroupKind::Normal => {
                                    for member_wid in group.live_wids() {
                                        self.display_rows.push(DisplayRow::Window {
                                            wid: member_wid,
                                            group_id: Some(*gid),
                                        });
                                    }
                                }
                                GroupKind::TmuxSystem => {
                                    // Members of a TmuxSystem group hold session
                                    // names in `label` (see GroupMember docs).
                                    // Render every member as a Session row, regardless
                                    // of attached/orphan state — that detail is
                                    // computed at draw time from app.items.
                                    for member in &group.members {
                                        self.display_rows.push(DisplayRow::Session {
                                            name: member.label.clone(),
                                            group_id: Some(*gid),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn item_x(&self) -> i16 {
        ITEM_MARGIN
    }

    fn item_w(&self) -> u16 {
        (self.width as i16 - ITEM_MARGIN * 2).max(20) as u16
    }

    fn row_y(&self, index: usize) -> i16 {
        ITEM_Y_START + (index as i16) * (ITEM_H as i16 + ITEM_SPACING)
    }

    fn hit_test_row(&self, y: i16) -> Option<usize> {
        for i in 0..self.display_rows.len() {
            let iy = self.row_y(i);
            if y >= iy && y < iy + ITEM_H as i16 {
                return Some(i);
            }
        }
        None
    }

    fn drop_index_from_y(&self, y: i16) -> usize {
        for i in 0..self.display_rows.len() {
            let mid = self.row_y(i) + (ITEM_H as i16 / 2);
            if y < mid {
                return i;
            }
        }
        self.display_rows.len()
    }

    fn find_item(&self, wid: u32) -> Option<&Item> {
        self.items.iter().find(|i| i.wid == wid)
    }

    // ── Group operations ──

    fn create_group(&mut self, wid: u32) -> u32 {
        let gid = self.next_group_id;
        self.next_group_id += 1;
        let name = format!("Group {}", gid + 1);
        let member = self.make_group_member_for_wid(wid);
        self.groups.push(Group {
            id: gid,
            name,
            collapsed: false,
            kind: GroupKind::Normal,
            members: vec![member],
        });
        for slot in &mut self.display_order {
            if matches!(slot, DisplaySlot::Window(w) if *w == wid) {
                *slot = DisplaySlot::Group(gid);
                break;
            }
        }
        self.build_display_rows();
        self.mark_dirty();
        gid
    }

    fn add_to_group(&mut self, gid: u32, wid: u32) {
        self.display_order
            .retain(|s| !matches!(s, DisplaySlot::Window(w) if *w == wid));
        // Remove the wid from any other group's member list. The user's
        // intent in moving a window between groups is "leave the source",
        // so we remove the entry entirely rather than turning it into a
        // ghost — they made an explicit choice.
        for group in &mut self.groups {
            group.members.retain(|m| m.live_wid != Some(wid));
        }
        let new_member = self.make_group_member_for_wid(wid);
        if let Some(group) = self.groups.iter_mut().find(|g| g.id == gid) {
            group.members.push(new_member);
        }
        self.build_display_rows();
        self.mark_dirty();
    }

    fn remove_from_group(&mut self, wid: u32) {
        let mut group_gid = None;
        for group in &mut self.groups {
            if let Some(pos) = group.position_of_live_wid(wid) {
                group_gid = Some(group.id);
                // Explicit user remove → drop the member entry entirely
                // (no ghost retention; user chose to ungroup).
                group.members.remove(pos);
                break;
            }
        }
        if let Some(gid) = group_gid {
            let pos = self
                .display_order
                .iter()
                .position(|s| matches!(s, DisplaySlot::Group(g) if *g == gid));
            let insert_at = pos.map(|p| p + 1).unwrap_or(self.display_order.len());
            self.display_order
                .insert(insert_at, DisplaySlot::Window(wid));
        }
        self.build_display_rows();
        self.mark_dirty();
    }

    fn delete_group(&mut self, gid: u32) {
        let group_pos = self.groups.iter().position(|g| g.id == gid);
        if let Some(gpos) = group_pos {
            // Promote currently-live members back to ungrouped slots in the
            // group's old position. Ghost members (live_wid = None) have no
            // window to ungroup, so they're discarded with the group.
            let live_wids = self.groups[gpos].live_wids();
            self.groups.remove(gpos);
            let slot_pos = self
                .display_order
                .iter()
                .position(|s| matches!(s, DisplaySlot::Group(g) if *g == gid));
            if let Some(sp) = slot_pos {
                self.display_order.remove(sp);
                for (i, wid) in live_wids.iter().enumerate() {
                    self.display_order
                        .insert(sp + i, DisplaySlot::Window(*wid));
                }
            }
        }
        self.build_display_rows();
        self.mark_dirty();
    }

    /// Build a `GroupMember` from a live wid by reading the matching item.
    /// Returns a ghost (Some(wid) but blank label/wm_class) if the wid isn't
    /// currently in `items` — this should be unreachable in practice but we
    /// don't want to panic on the edge case.
    fn make_group_member_for_wid(&self, wid: u32) -> GroupMember {
        match self.find_item(wid) {
            Some(item) => GroupMember {
                label: item.label.clone(),
                wm_class: item.wm_class.clone(),
                custom_prefix: item.custom_prefix.clone(),
                live_wid: Some(wid),
            },
            None => GroupMember {
                label: String::new(),
                wm_class: String::new(),
                custom_prefix: String::new(),
                live_wid: Some(wid),
            },
        }
    }

    fn start_rename(&mut self, gid: u32) {
        let text = self
            .groups
            .iter()
            .find(|g| g.id == gid)
            .map(|g| g.name.clone())
            .unwrap_or_default();
        let cursor = text.len();
        let selection_anchor = if text.is_empty() { None } else { Some(0) };
        self.rename = Some(RenameState {
            target: RenameTarget::Group(gid),
            text,
            cursor,
            selection_anchor,
        });
    }

    fn start_session_rename(&mut self, session_name: &str) {
        let text = session_name.to_string();
        let cursor = text.len();
        let selection_anchor = if text.is_empty() { None } else { Some(0) };
        self.rename = Some(RenameState {
            target: RenameTarget::Session(session_name.to_string()),
            text,
            cursor,
            selection_anchor,
        });
    }

    fn start_tab_rename(&mut self, wid: u32) {
        let text = self
            .items
            .iter()
            .find(|i| i.wid == wid)
            .map(|i| i.custom_prefix.clone())
            .unwrap_or_default();
        let cursor = text.len();
        let selection_anchor = if text.is_empty() { None } else { Some(0) };
        self.rename = Some(RenameState {
            target: RenameTarget::Window(wid),
            text,
            cursor,
            selection_anchor,
        });
    }

    fn commit_rename(&mut self) {
        if let Some(rs) = self.rename.take() {
            match rs.target {
                RenameTarget::Group(gid) => {
                    let name = rs.text.trim().to_string();
                    if !name.is_empty() {
                        if let Some(group) =
                            self.groups.iter_mut().find(|g| g.id == gid)
                        {
                            if group.name != name {
                                group.name = name;
                                self.mark_dirty();
                            }
                        }
                    }
                }
                RenameTarget::Window(wid) => {
                    let prefix = rs.text.trim().to_string();
                    if let Some(item) =
                        self.items.iter_mut().find(|i| i.wid == wid)
                    {
                        if item.custom_prefix != prefix {
                            item.custom_prefix = prefix;
                            self.mark_dirty();
                        }
                    }
                }
                RenameTarget::Session(old) => {
                    let new_name = rs.text.trim().to_string();
                    if new_name.is_empty() || new_name == old {
                        return;
                    }
                    // Ask tmux to rename the session. If it fails (bad name,
                    // collision, server gone), leave the UI unchanged and
                    // let the next refresh reconcile with reality.
                    let ok = std::process::Command::new("tmux")
                        .args(["rename-session", "-t", &old, &new_name])
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false);
                    if ok {
                        // Sessions live as members of the TmuxSystem group;
                        // rewrite the matching member's label in place so the
                        // row keeps its position until the next refresh
                        // re-derives membership from list_tmux_sessions.
                        for group in &mut self.groups {
                            if group.kind == GroupKind::TmuxSystem {
                                for member in &mut group.members {
                                    if member.label == old {
                                        member.label = new_name.clone();
                                    }
                                }
                            }
                        }
                        self.build_display_rows();
                        // Tmux session names aren't persisted to PTM's groups
                        // file (tmux is the source of truth there), so no
                        // mark_dirty needed for this branch.
                    }
                }
            }
        }
    }

    fn cancel_rename(&mut self) {
        self.rename = None;
    }

    fn toggle_collapse(&mut self, gid: u32) {
        let mut toggled = false;
        if let Some(group) = self.groups.iter_mut().find(|g| g.id == gid) {
            group.collapsed = !group.collapsed;
            toggled = true;
        }
        self.build_display_rows();
        if toggled {
            self.mark_dirty();
        }
    }

    fn remove_wid_from_group(&mut self, gid: u32, wid: u32) {
        // Used by drag operations that explicitly leave the source group;
        // remove the entry entirely (matches user intent — see add_to_group).
        if let Some(group) = self.groups.iter_mut().find(|g| g.id == gid) {
            group.members.retain(|m| m.live_wid != Some(wid));
        }
    }

    // ── Drag-and-drop ──

    #[allow(dead_code)] // pre-Stage-G helper; superseded by classify_drop, kept for tests
    fn is_gap_in_group(&self, gap: usize, gid: u32) -> bool {
        let header_row = self
            .display_rows
            .iter()
            .position(|r| matches!(r, DisplayRow::GroupHeader { group_id } if *group_id == gid));
        let Some(hr) = header_row else {
            return false;
        };
        let mut last_member = hr;
        for (i, row) in self.display_rows.iter().enumerate().skip(hr + 1) {
            if matches!(row, DisplayRow::Window { group_id: Some(g), .. } if *g == gid) {
                last_member = i;
            } else {
                break;
            }
        }
        gap > hr && gap <= last_member + 1
    }

    fn display_row_to_slot_position(&self, gap: usize) -> usize {
        let mut row_count = 0;
        for (slot_idx, slot) in self.display_order.iter().enumerate() {
            if row_count >= gap {
                return slot_idx;
            }
            match slot {
                DisplaySlot::Window(_) => {
                    row_count += 1;
                }
                DisplaySlot::Group(gid) => {
                    row_count += 1;
                    if let Some(group) = self.groups.iter().find(|g| g.id == *gid) {
                        if !group.collapsed {
                            // Normal groups list live windows; TmuxSystem
                            // groups list one row per session-name member.
                            row_count += match group.kind {
                                GroupKind::Normal => group.live_count(),
                                GroupKind::TmuxSystem => group.members.len(),
                            };
                        }
                    }
                }
            }
        }
        self.display_order.len()
    }

    #[allow(dead_code)] // pre-Stage-G helper; replaced by do_insert_at_slot
    fn move_slot_to(&mut self, target: &DisplaySlot, drop_gap: usize) {
        let src_pos = self.display_order.iter().position(|s| match (target, s) {
            (DisplaySlot::Window(a), DisplaySlot::Window(b)) => a == b,
            (DisplaySlot::Group(a), DisplaySlot::Group(b)) => a == b,
            _ => false,
        });
        if let Some(sp) = src_pos {
            let dst = self.display_row_to_slot_position(drop_gap);
            let slot = self.display_order.remove(sp);
            let insert_at = if dst > sp {
                (dst - 1).min(self.display_order.len())
            } else {
                dst.min(self.display_order.len())
            };
            self.display_order.insert(insert_at, slot);
        }
    }

    #[allow(dead_code)] // pre-Stage-G helper; replaced by do_reorder_in_group
    fn reorder_within_group(&mut self, gid: u32, wid: u32, drop_gap: usize) {
        let header_row = self
            .display_rows
            .iter()
            .position(|r| matches!(r, DisplayRow::GroupHeader { group_id } if *group_id == gid));
        if let Some(hr) = header_row {
            if let Some(group) = self.groups.iter_mut().find(|g| g.id == gid) {
                // Direct port of pre-2c semantics. With no ghost members
                // (the common case while the user is reordering visible
                // rows), live_wids() and `members` are positionally
                // equivalent so the existing index math stays correct.
                let src_pos = group.position_of_live_wid(wid);
                if let Some(sp) = src_pos {
                    let target_member = if drop_gap > hr + 1 {
                        drop_gap - hr - 1
                    } else {
                        0
                    };
                    let m = group.members.remove(sp);
                    let insert_at = if target_member > sp {
                        (target_member - 1).min(group.members.len())
                    } else {
                        target_member.min(group.members.len())
                    };
                    group.members.insert(insert_at, m);
                }
            }
        }
    }

    fn handle_drop(&mut self, source_row: usize, current_y: i16) {
        let target = classify_drop(self, source_row, current_y);
        self.apply_drop_target(source_row, target);
    }

    /// Apply a pre-computed DropTarget. Returns early without mutating
    /// (no build_display_rows, no mark_dirty) when the drop is a no-op.
    fn apply_drop_target(&mut self, source_row: usize, target: DropTarget) {
        if matches!(target, DropTarget::NoOp) || source_row >= self.display_rows.len() {
            return;
        }
        let source = self.display_rows[source_row].clone();
        let highlight_wid = match source {
            DisplayRow::Window { wid, .. } => Some(wid),
            _ => None,
        };

        let mutated = match target {
            DropTarget::NoOp => false,
            DropTarget::JoinGroup { gid, at } => self.do_join_group(&source, gid, at),
            DropTarget::ReorderInGroup { gid, to } => {
                self.do_reorder_in_group(&source, gid, to)
            }
            DropTarget::InsertBefore(row_idx) => {
                let dst_slot = self.display_row_to_slot_position(row_idx);
                self.do_insert_at_slot(&source, Some(dst_slot))
            }
            DropTarget::InsertAtEnd => self.do_insert_at_slot(&source, None),
        };

        if mutated {
            // T3.5 (G-5): flash the destination row so the user sees where
            // it landed. Only set on actual moves — no-ops skip this.
            if let Some(wid) = highlight_wid {
                self.last_drop_highlight = Some((wid, std::time::Instant::now()));
            }
            self.build_display_rows();
            self.mark_dirty();
        }
    }

    /// Add a Window source to group `gid` at member position `at`, removing
    /// it from its prior location. Returns true if anything actually changed.
    fn do_join_group(&mut self, source: &DisplayRow, gid: u32, at: usize) -> bool {
        let (wid, src_gid) = match source {
            DisplayRow::Window { wid, group_id } => (*wid, *group_id),
            _ => return false, // groups/sessions can't join groups
        };
        // Source already in target group? Classifier should have returned
        // ReorderInGroup; defensive no-op here.
        if src_gid == Some(gid) {
            return false;
        }
        // Detach from prior location.
        if let Some(src_g) = src_gid {
            self.remove_wid_from_group(src_g, wid);
        } else {
            self.display_order
                .retain(|s| !matches!(s, DisplaySlot::Window(w) if *w == wid));
        }
        let new_member = self.make_group_member_for_wid(wid);
        if let Some(g) = self.groups.iter_mut().find(|g| g.id == gid) {
            let pos = at.min(g.members.len());
            g.members.insert(pos, new_member);
        }
        true
    }

    /// Reorder a Window within its current group to position `to`.
    /// Returns false (no-op, no mark_dirty) if `to` resolves to source's
    /// existing position — fixing the G-4 "bouncing" snap-back.
    fn do_reorder_in_group(&mut self, source: &DisplayRow, gid: u32, to: usize) -> bool {
        let wid = match source {
            DisplayRow::Window { wid, .. } => *wid,
            _ => return false,
        };
        let group = match self.groups.iter_mut().find(|g| g.id == gid) {
            Some(g) => g,
            None => return false,
        };
        let sp = match group.position_of_live_wid(wid) {
            Some(p) => p,
            None => return false,
        };
        // No-op: target slot equals current position (either side of the
        // member). Both `to == sp` (insert before self) and `to == sp + 1`
        // (insert after self) collapse to identity once the remove+insert
        // index math runs.
        if to == sp || to == sp + 1 {
            return false;
        }
        let m = group.members.remove(sp);
        let insert_at = if to > sp {
            (to - 1).min(group.members.len())
        } else {
            to.min(group.members.len())
        };
        group.members.insert(insert_at, m);
        true
    }

    /// Insert the source as an ungrouped slot at `dst_slot` (or end if None).
    /// Returns false when the move would be a no-op (same position).
    fn do_insert_at_slot(&mut self, source: &DisplayRow, dst_slot: Option<usize>) -> bool {
        let (new_slot, src_in_display_order) = match source {
            DisplayRow::Window { wid, group_id } => {
                if let Some(gid) = group_id {
                    self.remove_wid_from_group(*gid, *wid);
                    (DisplaySlot::Window(*wid), None)
                } else {
                    let pos = self.display_order.iter().position(
                        |s| matches!(s, DisplaySlot::Window(w) if *w == *wid),
                    );
                    if let Some(p) = pos {
                        self.display_order.remove(p);
                    }
                    (DisplaySlot::Window(*wid), pos)
                }
            }
            DisplayRow::GroupHeader { group_id } => {
                let pos = self.display_order.iter().position(
                    |s| matches!(s, DisplaySlot::Group(g) if *g == *group_id),
                );
                if let Some(p) = pos {
                    self.display_order.remove(p);
                }
                (DisplaySlot::Group(*group_id), pos)
            }
            // Session rows are always inside the TmuxSystem group; classify_drop
            // returns NoOp for session sources (T4.6) so this fn is never
            // called with one. Treat unexpectedly arriving here as no-op.
            DisplayRow::Session { .. } => return false,
        };
        // Adjust dst_slot if source was in display_order before the dst.
        let insert_at = match dst_slot {
            Some(p) => {
                let adjusted = match src_in_display_order {
                    Some(sp) if p > sp => p - 1,
                    _ => p,
                };
                adjusted.min(self.display_order.len())
            }
            None => self.display_order.len(),
        };
        // No-op: removing then inserting at the same position changes nothing.
        if let Some(sp) = src_in_display_order {
            if insert_at == sp {
                self.display_order.insert(sp, new_slot);
                return false;
            }
        }
        self.display_order.insert(insert_at, new_slot);
        true
    }
}

// ── Stage G: drop-target classifier (T3.1) ──
//
// Pure function consumed by both `handle_drop` (action) and the renderer
// (drop-indicator preview), so the visual indicator can never show one
// landing while the actual drop produces another (G-3 fix).
//
// Hot-zone rules (per MVP_PLAN.md Stage G):
//   - Group header row → JoinGroup(g, 0).
//   - Member row of group g (upper half) → JoinGroup(g, idx) [or
//     ReorderInGroup if source is in g].
//   - Member row + spacing below (lower half) → JoinGroup(g, idx+1).
//   - Above the first row → InsertBefore(0).
//   - Past the last row → InsertAtEnd.
//   - Anywhere else → InsertBefore at the row's gap position.
//
// Net effect: a group's full vertical extent (header + members + bottom
// spacing) is a join-or-reorder zone — small overshoots no longer eject
// (G-2 fix), and dropping into a group's body actually joins (G-1 fix).

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum DropTarget {
    /// Insert at display_rows position N (i.e., before the row currently
    /// at index N), as an ungrouped slot.
    InsertBefore(usize),
    /// Insert at the very end of display_order as an ungrouped slot.
    InsertAtEnd,
    /// Add the source to group `gid` at member position `at`.
    JoinGroup { gid: u32, at: usize },
    /// Reorder source within group `gid` to member position `to`.
    ReorderInGroup { gid: u32, to: usize },
    /// Drop is invalid (source row out of range) — produce no change.
    NoOp,
}

fn classify_drop(app: &App, source_row: usize, current_y: i16) -> DropTarget {
    if source_row >= app.display_rows.len() {
        return DropTarget::NoOp;
    }
    let source = app.display_rows[source_row].clone();
    let source_gid = match source {
        DisplayRow::Window { group_id, .. } => group_id,
        _ => None,
    };

    // Session-row drag is never user-actionable: members of the TmuxSystem
    // group are derived from `tmux list-sessions` every refresh, so
    // user-driven reordering would be silently overwritten.
    if matches!(source, DisplayRow::Session { .. }) {
        return DropTarget::NoOp;
    }

    // Header source: simple gap-based reorder. (OQ-G3's group-on-group
    // dropping is deferred.) Applies to both Normal and TmuxSystem groups —
    // the system group itself moves like any other group.
    if matches!(source, DisplayRow::GroupHeader { .. }) {
        let gap = app.drop_index_from_y(current_y);
        if gap >= app.display_rows.len() {
            return DropTarget::InsertAtEnd;
        }
        return DropTarget::InsertBefore(gap);
    }

    // Window source. Find which row's drop zone current_y sits in, where
    // each row's zone extends from its top to the next row's top
    // (= row + bottom spacing). The last row's zone extends one full
    // row-height below.
    if app.display_rows.is_empty() {
        return DropTarget::InsertAtEnd;
    }
    if current_y < app.row_y(0) {
        return DropTarget::InsertBefore(0);
    }
    let n = app.display_rows.len();
    let mut target_row_index: Option<usize> = None;
    for i in 0..n {
        let zone_top = app.row_y(i);
        let zone_bottom = if i + 1 < n {
            app.row_y(i + 1)
        } else {
            app.row_y(i) + ITEM_H as i16 + ITEM_SPACING
        };
        if current_y >= zone_top && current_y < zone_bottom {
            target_row_index = Some(i);
            break;
        }
    }
    let Some(t) = target_row_index else {
        return DropTarget::InsertAtEnd;
    };

    let target_row = &app.display_rows[t];
    let zone_top = app.row_y(t);
    let row_mid = zone_top + (ITEM_H as i16 / 2);
    let upper_half = current_y < row_mid;

    match target_row {
        DisplayRow::GroupHeader { group_id } => {
            if is_target_system_group(app, *group_id) {
                // Window can't be dropped onto the system group — its
                // members are derived, not user-added.
                return DropTarget::NoOp;
            }
            // Whole header row → JoinGroup(g, 0).
            DropTarget::JoinGroup { gid: *group_id, at: 0 }
        }
        DisplayRow::Window {
            group_id: Some(target_gid),
            ..
        } => {
            // Find this group's header row to compute member index.
            let hr = match app
                .display_rows
                .iter()
                .position(|r| matches!(r, DisplayRow::GroupHeader { group_id } if *group_id == *target_gid))
            {
                Some(p) => p,
                None => return DropTarget::InsertBefore(t),
            };
            let member_idx = t - hr - 1;
            let pos = if upper_half { member_idx } else { member_idx + 1 };
            if source_gid == Some(*target_gid) {
                DropTarget::ReorderInGroup {
                    gid: *target_gid,
                    to: pos,
                }
            } else {
                DropTarget::JoinGroup {
                    gid: *target_gid,
                    at: pos,
                }
            }
        }
        DisplayRow::Session { group_id: Some(target_gid), .. } => {
            // Session rows live inside the TmuxSystem group — drops onto
            // them implicitly target that group, which doesn't accept
            // window members. Defensive NoOp (also covers a future variant
            // where Session rows somehow land outside the system group).
            if is_target_system_group(app, *target_gid) {
                return DropTarget::NoOp;
            }
            let pos = if upper_half { t } else { t + 1 };
            if pos >= app.display_rows.len() {
                DropTarget::InsertAtEnd
            } else {
                DropTarget::InsertBefore(pos)
            }
        }
        DisplayRow::Window {
            group_id: None, ..
        }
        | DisplayRow::Session { group_id: None, .. } => {
            let pos = if upper_half { t } else { t + 1 };
            if pos >= app.display_rows.len() {
                DropTarget::InsertAtEnd
            } else {
                DropTarget::InsertBefore(pos)
            }
        }
    }
}

fn is_target_system_group(app: &App, gid: u32) -> bool {
    app.groups
        .iter()
        .find(|g| g.id == gid)
        .map_or(false, |g| g.kind == GroupKind::TmuxSystem)
}

/// Map a DropTarget back to a y-coordinate for the drag indicator line.
/// Used by both the renderer (T3.3) and tested separately so the indicator
/// position can never silently diverge from the action location.
fn indicator_y_for_target(app: &App, target: &DropTarget) -> i16 {
    let last_row_bottom = || {
        if app.display_rows.is_empty() {
            ITEM_Y_START
        } else {
            app.row_y(app.display_rows.len() - 1) + ITEM_H as i16 + (ITEM_SPACING / 2)
        }
    };
    match target {
        DropTarget::NoOp => -1,
        DropTarget::InsertBefore(n) => {
            if *n < app.display_rows.len() {
                app.row_y(*n) - (ITEM_SPACING / 2)
            } else {
                last_row_bottom()
            }
        }
        DropTarget::InsertAtEnd => last_row_bottom(),
        DropTarget::JoinGroup { gid, at } | DropTarget::ReorderInGroup { gid, to: at } => {
            // Find this group's header row in display_rows; the at-th
            // member's row is hr+1+at (assuming the group is expanded —
            // collapsed groups have no member rows so we draw at the
            // header itself).
            let hr = app
                .display_rows
                .iter()
                .position(|r| matches!(r, DisplayRow::GroupHeader { group_id } if *group_id == *gid));
            let hr = match hr {
                Some(h) => h,
                None => return last_row_bottom(),
            };
            let row = hr + 1 + at;
            if row < app.display_rows.len() {
                app.row_y(row) - (ITEM_SPACING / 2)
            } else {
                // Past the last member of this group → indicator below it.
                let group = app.groups.iter().find(|g| g.id == *gid);
                let live = group.map(|g| g.live_count()).unwrap_or(0);
                if live == 0 {
                    // Collapsed or empty group — indicator just below header.
                    app.row_y(hr) + ITEM_H as i16 + (ITEM_SPACING / 2)
                } else {
                    let last_member_row = hr + live;
                    app.row_y(last_member_row) + ITEM_H as i16 + (ITEM_SPACING / 2)
                }
            }
        }
    }
}

/// Return the gid of the group to outline during drag, if the target is a
/// Join/Reorder. Other DropTargets return None.
fn target_group_for_outline(target: &DropTarget) -> Option<u32> {
    match target {
        DropTarget::JoinGroup { gid, .. } | DropTarget::ReorderInGroup { gid, .. } => Some(*gid),
        _ => None,
    }
}

/// Compute the (top, bottom) y bounds of a group's vertical extent — header
/// row top through bottom of last visible member. Returns None if the group
/// id isn't found in display_rows.
fn group_outline_bounds(app: &App, gid: u32) -> Option<(i16, i16)> {
    let hr = app
        .display_rows
        .iter()
        .position(|r| matches!(r, DisplayRow::GroupHeader { group_id } if *group_id == gid))?;
    let mut last_row = hr;
    for (i, row) in app.display_rows.iter().enumerate().skip(hr + 1) {
        if matches!(row, DisplayRow::Window { group_id: Some(g), .. } if *g == gid) {
            last_row = i;
        } else {
            break;
        }
    }
    let top = app.row_y(hr);
    let bottom = app.row_y(last_row) + ITEM_H as i16;
    Some((top, bottom))
}

// ── EWMH helpers ──

fn get_client_list(
    conn: &impl Connection,
    root: Window,
    atoms: &Atoms,
) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            root,
            atoms.net_client_list,
            AtomEnum::WINDOW,
            0,
            4096,
        )?
        .reply()?;
    Ok(reply
        .value
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

fn get_active_window(
    conn: &impl Connection,
    root: Window,
    atoms: &Atoms,
) -> Result<Option<u32>, Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            root,
            atoms.net_active_window,
            AtomEnum::WINDOW,
            0,
            1,
        )?
        .reply()?;
    if reply.value.len() >= 4 {
        let wid = u32::from_le_bytes([
            reply.value[0],
            reply.value[1],
            reply.value[2],
            reply.value[3],
        ]);
        if wid == 0 {
            Ok(None)
        } else {
            Ok(Some(wid))
        }
    } else {
        Ok(None)
    }
}

fn get_window_pid(conn: &impl Connection, wid: u32, atoms: &Atoms) -> Option<u32> {
    let reply = conn
        .get_property(false, wid, atoms.net_wm_pid, AtomEnum::CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    if reply.value.len() >= 4 {
        Some(u32::from_le_bytes([
            reply.value[0],
            reply.value[1],
            reply.value[2],
            reply.value[3],
        ]))
    } else {
        None
    }
}

fn get_window_title(
    conn: &impl Connection,
    wid: u32,
    atoms: &Atoms,
) -> Result<String, Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(false, wid, atoms.net_wm_name, atoms.utf8_string, 0, 1024)?
        .reply()?;
    let title = String::from_utf8_lossy(&reply.value).into_owned();
    if !title.is_empty() {
        return Ok(title);
    }
    let reply = conn
        .get_property(false, wid, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)?
        .reply()?;
    Ok(String::from_utf8_lossy(&reply.value).into_owned())
}

fn get_wm_class(
    conn: &impl Connection,
    wid: u32,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(false, wid, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)?
        .reply()?;
    let s = String::from_utf8_lossy(&reply.value);
    let mut parts = s.split('\0').filter(|p| !p.is_empty());
    let instance = parts.next().unwrap_or("").to_string();
    let class = parts.next().unwrap_or("").to_string();
    Ok((instance, class))
}

fn get_current_desktop(
    conn: &impl Connection,
    root: Window,
    atoms: &Atoms,
) -> Result<u32, Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            root,
            atoms.net_current_desktop,
            AtomEnum::CARDINAL,
            0,
            1,
        )?
        .reply()?;
    if reply.value.len() >= 4 {
        Ok(u32::from_le_bytes([
            reply.value[0],
            reply.value[1],
            reply.value[2],
            reply.value[3],
        ]))
    } else {
        Ok(0)
    }
}

fn get_window_desktop(
    conn: &impl Connection,
    wid: u32,
    atoms: &Atoms,
) -> Result<Option<u32>, Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            wid,
            atoms.net_wm_desktop,
            AtomEnum::CARDINAL,
            0,
            1,
        )?
        .reply()?;
    if reply.value.len() >= 4 {
        let d = u32::from_le_bytes([
            reply.value[0],
            reply.value[1],
            reply.value[2],
            reply.value[3],
        ]);
        if d == 0xFFFFFFFF {
            Ok(None)
        } else {
            Ok(Some(d))
        }
    } else {
        Ok(None)
    }
}

fn is_normal_window(
    conn: &impl Connection,
    wid: u32,
    atoms: &Atoms,
) -> Result<bool, Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            wid,
            atoms.net_wm_window_type,
            AtomEnum::ATOM,
            0,
            32,
        )?
        .reply()?;
    if reply.value.is_empty() {
        return Ok(true); // No type set → treat as normal
    }
    let type_atoms: Vec<u32> = reply
        .value
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    Ok(type_atoms.contains(&atoms.net_wm_window_type_normal))
}

fn get_frame_extents(
    conn: &impl Connection,
    wid: u32,
    atoms: &Atoms,
) -> Result<(i32, i32, i32, i32), Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            wid,
            atoms.net_frame_extents,
            AtomEnum::CARDINAL,
            0,
            4,
        )?
        .reply()?;
    if reply.value.len() >= 16 {
        let left =
            u32::from_le_bytes([reply.value[0], reply.value[1], reply.value[2], reply.value[3]])
                as i32;
        let right =
            u32::from_le_bytes([reply.value[4], reply.value[5], reply.value[6], reply.value[7]])
                as i32;
        let top = u32::from_le_bytes([
            reply.value[8],
            reply.value[9],
            reply.value[10],
            reply.value[11],
        ]) as i32;
        let bottom = u32::from_le_bytes([
            reply.value[12],
            reply.value[13],
            reply.value[14],
            reply.value[15],
        ]) as i32;
        Ok((left, right, top, bottom))
    } else {
        Ok((0, 0, 0, 0))
    }
}

fn get_workarea(
    conn: &impl Connection,
    root: Window,
    atoms: &Atoms,
    desktop: u32,
) -> Result<(i32, i32, u32, u32), Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            root,
            atoms.net_workarea,
            AtomEnum::CARDINAL,
            0,
            1024,
        )?
        .reply()?;
    let offset = (desktop as usize) * 16;
    if reply.value.len() >= offset + 16 {
        let v = &reply.value[offset..];
        let x = u32::from_le_bytes([v[0], v[1], v[2], v[3]]) as i32;
        let y = u32::from_le_bytes([v[4], v[5], v[6], v[7]]) as i32;
        let w = u32::from_le_bytes([v[8], v[9], v[10], v[11]]);
        let h = u32::from_le_bytes([v[12], v[13], v[14], v[15]]);
        Ok((x, y, w, h))
    } else {
        Ok((0, 0, 1920, 1080))
    }
}

fn get_window_geometry(
    conn: &impl Connection,
    wid: u32,
) -> Result<(i32, i32, u32, u32), Box<dyn std::error::Error>> {
    let geo = conn.get_geometry(wid)?.reply()?;
    let trans = conn.translate_coordinates(wid, geo.root, 0, 0)?.reply()?;
    Ok((
        trans.dst_x as i32,
        trans.dst_y as i32,
        geo.width as u32,
        geo.height as u32,
    ))
}

fn activate_window(
    conn: &impl Connection,
    root: Window,
    wid: u32,
    atoms: &Atoms,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = ClientMessageData::from([2u32, 0, 0, 0, 0]);
    let event = ClientMessageEvent {
        response_type: 33,
        format: 32,
        sequence: 0,
        window: wid,
        type_: atoms.net_active_window,
        data,
    };
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )?;
    conn.flush()?;
    Ok(())
}

fn move_window(
    conn: &impl Connection,
    wid: u32,
    x: i32,
    y: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    conn.configure_window(wid, &ConfigureWindowAux::new().x(x).y(y))?;
    conn.flush()?;
    Ok(())
}

fn snap_to_sidebar(
    conn: &impl Connection,
    root: Window,
    our_wid: u32,
    target_wid: u32,
    atoms: &Atoms,
) -> Result<(), Box<dyn std::error::Error>> {
    let (our_x, our_y, our_w, _our_h) = get_window_geometry(conn, our_wid)?;
    let our_frame = get_frame_extents(conn, our_wid, atoms)?;
    let target_frame = get_frame_extents(conn, target_wid, atoms)?;

    let desktop = get_current_desktop(conn, root, atoms)?;
    let (wa_x, wa_y, wa_w, _wa_h) = get_workarea(conn, root, atoms, desktop)?;

    let our_frame_right = our_x + our_w as i32 + our_frame.1;
    let target_x = our_frame_right + target_frame.0;
    let our_frame_top = our_y - our_frame.2;
    let target_y = our_frame_top + target_frame.2;

    let max_x = wa_x + wa_w as i32;
    let target_x = target_x.min(max_x);
    let target_y = target_y.max(wa_y + target_frame.2);

    move_window(conn, target_wid, target_x, target_y)?;
    Ok(())
}

// ── Refresh ──

fn refresh_items(
    conn: &impl Connection,
    root: Window,
    atoms: &Atoms,
    app: &mut App,
    colormap: Colormap,
) -> Result<(), Box<dyn std::error::Error>> {
    let wids = get_client_list(conn, root, atoms)?;
    let current_desktop = get_current_desktop(conn, root, atoms).unwrap_or(0);
    let tmux_clients = list_tmux_clients();

    // Snapshot wids we already knew about so claim_pending_attach can spot
    // windows that appeared since the last refresh.
    let prior_wids: HashSet<u32> = app.items.iter().map(|i| i.wid).collect();

    let mut live_wids = HashSet::new();
    let mut new_items = Vec::new();
    let mut color_idx = 0usize;
    // Multiple windows can share a PID (e.g. every gnome-terminal window
    // reports the server PID). Keep a list so the walk can detect collisions
    // and refuse to guess, rather than silently attributing a tmux session
    // to whichever window happened to enumerate first.
    let mut pid_to_wid: HashMap<u32, Vec<u32>> = HashMap::new();

    for wid in wids {
        if wid == app.our_wid {
            continue;
        }
        match get_window_desktop(conn, wid, atoms) {
            Ok(Some(d)) if d != current_desktop => continue,
            _ => {}
        }
        if !is_normal_window(conn, wid, atoms).unwrap_or(true) {
            continue;
        }

        live_wids.insert(wid);

        // Subscribe to property changes on this window once, so title edits
        // and other updates land in the main event loop without polling.
        if !app.subscribed_wids.contains(&wid) {
            let _ = conn.change_window_attributes(
                wid,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            );
            app.subscribed_wids.insert(wid);
        }

        let title = get_window_title(conn, wid, atoms).unwrap_or_default();
        let (_instance, class) = get_wm_class(conn, wid).unwrap_or_default();
        let display = if title.is_empty() { class.clone() } else { title };

        let accent = ACCENT_COLORS[color_idx % ACCENT_COLORS.len()];
        let accent_pixel = alloc_color(conn, colormap, accent)?;
        color_idx += 1;

        let custom_prefix = app
            .items
            .iter()
            .find(|i| i.wid == wid)
            .map(|i| i.custom_prefix.clone())
            .unwrap_or_default();

        if let Some(pid) = get_window_pid(conn, wid, atoms) {
            pid_to_wid.entry(pid).or_insert_with(Vec::new).push(wid);
        }

        new_items.push(Item {
            wid,
            label: display,
            wm_class: class,
            accent_pixel,
            custom_prefix,
            session: None,
        });
    }

    // Polled once per refresh and used by three downstream paths:
    //   1. carry_over_session_bindings — drop bindings whose session died
    //   2. walk_to_window_owner block — (no direct use; see #1)
    //   3. sync_system_group_members — populate the Tmux Sessions group rows
    //   4. session-origin tracking — preserves the name a session was first
    //      seen with so the UI can keep showing it after a rename
    let live_sessions = list_tmux_sessions();
    update_session_origins(&mut app.session_origins, &live_sessions);
    let live_session_names: Vec<String> =
        live_sessions.iter().map(|(_, n, _)| n.clone()).collect();
    let live_session_set: HashSet<String> =
        live_session_names.iter().cloned().collect();

    // Pending-spawn takes priority over the ancestor walk. If the user
    // just clicked an orphan row or hit a `+ New *` button, we know the
    // next new window is ours — much more reliable than walking through
    // a gnome-terminal-server pid collision.
    let claim = claim_pending_spawn(
        &mut app.pending_spawn,
        &prior_wids,
        &mut new_items,
        PENDING_ATTACH_TIMEOUT,
        std::time::Instant::now(),
    );
    let mut pre_assigned: HashSet<String> = HashSet::new();
    if let Some(c) = &claim {
        if let Some(name) = &c.attach_session {
            pre_assigned.insert(name.clone());
        }
    }

    // Carry forward any session bindings from the prior refresh's items.
    // Necessary because walk_to_window_owner returns None for users on
    // gnome-terminal (every window shares gnome-terminal-server's PID),
    // so the only path that ever sets item.session for them is the
    // one-shot claim_pending_spawn — and without carry-over, the marker
    // disappeared on the very next refresh.
    carry_over_session_bindings(&app.items, &mut new_items, &live_session_set);

    // Assign tmux sessions by walking UP from each tmux client's PID until
    // we hit a pid that's a tracked window's _NET_WM_PID. This is the
    // closest-window-to-client match — walking DOWN from a window's pid is
    // wrong when many windows share a launcher pid (e.g. cinnamon-session),
    // because the launcher's descendants include unrelated terminals too.
    if !tmux_clients.is_empty() && !pid_to_wid.is_empty() {
        for (&client_pid, session_name) in &tmux_clients {
            if pre_assigned.contains(session_name) {
                continue;
            }
            if let Some(wid) =
                walk_to_window_owner(client_pid, &pid_to_wid, read_ppid, 20)
            {
                if let Some(item) = new_items.iter_mut().find(|i| i.wid == wid) {
                    item.session = Some(session_name.clone());
                }
            }
        }
    }

    // FM-2 fix (Phase 2c): preserve members whose live wid disappeared as
    // GHOSTS rather than removing them. This means a group whose windows
    // briefly close (e.g. closing then reopening a terminal) will not
    // collapse to zero-members and trigger the wipe path.
    for group in &mut app.groups {
        for member in &mut group.members {
            if let Some(wid) = member.live_wid {
                if !live_wids.contains(&wid) {
                    member.live_wid = None;
                }
            }
        }
    }

    // Re-match: only NEWLY-APPEARED wids (not already placed anywhere) get
    // offered to ghost slots. Wids that are currently ungrouped — including
    // ones the user explicitly removed from a group — must not be silently
    // re-claimed when a class-only match would otherwise pull them in.
    let already_known: HashSet<u32> = app
        .groups
        .iter()
        .flat_map(|g| g.live_wids())
        .chain(app.display_order.iter().filter_map(|s| match s {
            DisplaySlot::Window(w) => Some(*w),
            _ => None,
        }))
        .collect();
    for wid in &live_wids {
        if already_known.contains(wid) {
            continue;
        }
        let (label, wm_class) = match new_items.iter().find(|i| i.wid == *wid) {
            Some(i) => (i.label.clone(), i.wm_class.clone()),
            None => continue,
        };
        let mut restored_prefix: Option<String> = None;
        for group in &mut app.groups {
            // Same cascade as restore_groups (Phase 2d): exact → label →
            // wm_class. wm_class-only catches terminals whose title drifted
            // since the saved snapshot.
            let exact_pos = group.members.iter().position(|m| {
                m.live_wid.is_none() && m.label == label && m.wm_class == wm_class
            });
            let label_only = || group.members.iter().position(|m| {
                m.live_wid.is_none() && m.label == label
            });
            let class_only = || group.members.iter().position(|m| {
                m.live_wid.is_none() && m.wm_class == wm_class
            });
            let pos = exact_pos.or_else(label_only).or_else(class_only);
            if let Some(p) = pos {
                group.members[p].live_wid = Some(*wid);
                if !group.members[p].custom_prefix.is_empty() {
                    restored_prefix = Some(group.members[p].custom_prefix.clone());
                }
                break;
            }
        }
        if let Some(prefix) = restored_prefix {
            if let Some(item) = new_items.iter_mut().find(|i| i.wid == *wid) {
                item.custom_prefix = prefix;
            }
        }
    }

    // Drop subscription bookkeeping for wids that have gone away.
    // (X11 already stopped delivering events for the destroyed window; we just
    // prune our HashSet so a wid that's later re-used gets a fresh subscribe.)
    app.subscribed_wids.retain(|w| live_wids.contains(w));

    // Tmux sessions: surface ALL live sessions (attached + orphan) inside
    // the TmuxSystem group, if one exists. The group itself is auto-created
    // at startup (T4.5); refresh_items only syncs membership against the
    // current `tmux list-sessions` output. The visual attached-vs-orphan
    // distinction is computed at draw time from app.items.
    if let Some(group) = app
        .groups
        .iter_mut()
        .find(|g| g.kind == GroupKind::TmuxSystem)
    {
        sync_system_group_members(group, &live_session_names);
    }

    // Remove dead entries from display_order.
    app.display_order.retain(|slot| match slot {
        DisplaySlot::Window(wid) => live_wids.contains(wid),
        DisplaySlot::Group(gid) => app.groups.iter().any(|g| g.id == *gid),
    });

    // Collect wids already tracked (live members of groups + ungrouped
    // entries in display_order). Ghost members (live_wid: None) are not
    // included — by construction, no live wid can match a ghost.
    let mut known_wids = HashSet::new();
    for slot in &app.display_order {
        if let DisplaySlot::Window(wid) = slot {
            known_wids.insert(*wid);
        }
    }
    for group in &app.groups {
        for wid in group.live_wids() {
            known_wids.insert(wid);
        }
    }

    // Add new wids to display_order
    for wid in &live_wids {
        if !known_wids.contains(wid) {
            app.display_order.push(DisplaySlot::Window(*wid));
        }
    }

    // Update active window
    app.active_wid = get_active_window(conn, root, atoms).unwrap_or(None);

    app.items = new_items;
    app.build_display_rows();

    // Snap a freshly-claimed spawn to the sidebar anchor. Mirrors the
    // click-to-activate snap path; errors are swallowed (the window
    // appears, just unpositioned) the same way that path does it.
    if let Some(c) = claim {
        let _ = snap_to_sidebar(conn, root, app.our_wid, c.wid, atoms);
    }

    Ok(())
}

/// Layout for the top-row buttons. Returns the left button's rect and the
/// right button's rect (None when tmux isn't installed — left button takes
/// the full width). Pure: width comes in as a parameter so tests can pin
/// behavior without constructing an App.
fn top_buttons_layout(tmux_available: bool, width: u16) -> (Rectangle, Option<Rectangle>) {
    let y: i16 = 4;
    let total_w = (width as i16 - ITEM_MARGIN * 2).max(20);
    let h = HEADER_H;
    if !tmux_available {
        let left = Rectangle {
            x: ITEM_MARGIN,
            y,
            width: total_w as u16,
            height: h,
        };
        return (left, None);
    }
    let half = (total_w - TOP_BUTTON_GAP) / 2;
    let left = Rectangle {
        x: ITEM_MARGIN,
        y,
        width: half as u16,
        height: h,
    };
    // Right button absorbs the remainder so the rounding error from `/2` lands
    // in the right rect rather than leaving a 1px sliver.
    let right_w = total_w - half - TOP_BUTTON_GAP;
    let right = Rectangle {
        x: ITEM_MARGIN + half + TOP_BUTTON_GAP,
        y,
        width: right_w as u16,
        height: h,
    };
    (left, Some(right))
}

/// True if `tmux -V` runs and exits 0 — i.e., the binary is on PATH. We don't
/// probe whether the server is running because we want the "+ New tmux" button
/// (T4.5b) to keep showing even after the user kills their last session — that
/// is exactly when they'd want to make a new one.
fn is_tmux_available() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Append a TmuxSystem group to `app` if none exists. Idempotent — safe to
/// call every startup. Default state: collapsed=true, empty members (the
/// next refresh syncs them from `tmux list-sessions`). Does NOT mark dirty:
/// auto-create is pure derivation, the actual persistable state (position,
/// collapse) only changes via explicit user actions which already mark dirty.
fn ensure_tmux_system_group(app: &mut App) {
    if app.groups.iter().any(|g| g.kind == GroupKind::TmuxSystem) {
        return;
    }
    let gid = app.next_group_id;
    app.next_group_id += 1;
    app.groups.push(Group {
        id: gid,
        name: "Tmux Sessions".to_string(),
        collapsed: true,
        kind: GroupKind::TmuxSystem,
        members: Vec::new(),
    });
    app.display_order.push(DisplaySlot::Group(gid));
    app.build_display_rows();
}

/// True if `local_x` (relative to the session row's left edge) lands inside
/// the close-button band on the right edge. Pure — keyed only on geometry
/// so it can be unit-tested without a renderer.
fn hit_test_session_close_button(local_x: i16, row_w: i16) -> bool {
    let band_left = row_w - SESSION_CLOSE_BAND_WIDTH;
    local_x >= band_left && local_x < row_w
}

/// Pure click dispatcher for a session row. Returns `Some(req)` to open the
/// kill-confirm popup when the click landed in the close band; `None` means
/// "fall through to the normal session-row action" (attach).
fn dispatch_session_click(
    name: &str,
    group_id: Option<u32>,
    local_x: i16,
    row_w: i16,
) -> Option<ConfirmRequest> {
    if group_id.is_some() && hit_test_session_close_button(local_x, row_w) {
        return Some(ConfirmRequest {
            message: format!("Kill tmux session '{}'?", name),
            action: ConfirmAction::KillSession(name.to_string()),
        });
    }
    None
}

/// True if any tracked window is currently attached to `session_name`.
/// Pure — no X11 or tmux calls. Currently has no production callers (Bug B
/// removed the marker that branched on it); kept under `#[allow(dead_code)]`
/// because the predicate is small, well-tested, and likely to revive if a
/// future feature wants to differentiate attached vs orphan sessions.
#[allow(dead_code)]
fn is_session_attached(app: &App, session_name: &str) -> bool {
    app.items
        .iter()
        .any(|i| i.session.as_deref() == Some(session_name))
}

/// Idempotent membership sync for the TmuxSystem group: drop members whose
/// session vanished, preserve order of survivors, append new sessions at the
/// end. Pure — no X11 or tmux calls; the caller passes the live name list.
fn sync_system_group_members(group: &mut Group, live_sessions: &[String]) {
    use std::collections::HashSet;
    let live_set: HashSet<&str> = live_sessions.iter().map(String::as_str).collect();
    group.members.retain(|m| live_set.contains(m.label.as_str()));
    let existing: HashSet<String> = group.members.iter().map(|m| m.label.clone()).collect();
    for name in live_sessions {
        if !existing.contains(name) {
            group.members.push(GroupMember {
                label: name.clone(),
                wm_class: String::new(),
                custom_prefix: String::new(),
                live_wid: None,
            });
        }
    }
}

// ── tmux session detection ──
//
// Strategy: ask tmux for its attached clients (PID + session name), then for
// each client walk UP the /proc parent chain until we hit a PID that matches
// some tracked window's _NET_WM_PID. That ancestor is the window hosting the
// tmux client, so we tag it with the session name.
//
// The obvious alternative — walk DOWN from each window's _NET_WM_PID looking
// for tmux-client PIDs — is wrong on desktops where the WM sets _NET_WM_PID
// to a shared launcher process (e.g. cinnamon-session) for many windows.
// The launcher's descendant tree contains every GUI app the user ever
// launched, so every one of those windows would falsely match the tmux
// client. Walking up from the client instead finds the CLOSEST owning
// window and stops there.

fn parse_tmux_list_clients(stdout: &str) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((pid_str, name)) = line.split_once(char::is_whitespace) {
            if let Ok(pid) = pid_str.parse::<u32>() {
                let name = name.trim();
                if !name.is_empty() {
                    map.insert(pid, name.to_string());
                }
            }
        }
    }
    map
}

fn list_tmux_clients() -> HashMap<u32, String> {
    match std::process::Command::new("tmux")
        .args(["list-clients", "-F", "#{client_pid} #{session_name}"])
        .output()
    {
        Ok(o) => parse_tmux_list_clients(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => HashMap::new(),
    }
}

// Parse `tmux list-sessions -F '#{session_id} #{session_name} #{session_attached}'`
// output into (id, name, attached_count>0) tuples. The id always starts with
// `$` and is whitespace-free; the attached count is the trailing token; the
// name is whatever sits between, even when it contains spaces. Lines whose
// first token is not a `$`-prefixed id, or whose final token is not numeric,
// are silently dropped.
fn parse_tmux_list_sessions(stdout: &str) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Split off the id at the first whitespace.
        let Some(id_end) = line.find(char::is_whitespace) else {
            continue;
        };
        let id = &line[..id_end];
        if !id.starts_with('$') {
            continue;
        }
        let rest = line[id_end + 1..].trim_start();
        // Split off the attached count at the last whitespace; the middle
        // is the name (preserving any internal spaces).
        let Some(name_end) = rest.rfind(char::is_whitespace) else {
            continue;
        };
        let name = rest[..name_end].trim();
        let attached_str = rest[name_end + 1..].trim();
        if name.is_empty() {
            continue;
        }
        if let Ok(n) = attached_str.parse::<u32>() {
            out.push((id.to_string(), name.to_string(), n > 0));
        }
    }
    out
}

fn list_tmux_sessions() -> Vec<(String, String, bool)> {
    match std::process::Command::new("tmux")
        .args([
            "list-sessions",
            "-F",
            "#{session_id} #{session_name} #{session_attached}",
        ])
        .output()
    {
        Ok(o) => parse_tmux_list_sessions(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => Vec::new(),
    }
}

// ── Session origin tracking ──
//
// These helpers track each tmux session's ORIGINAL name (the name observed
// the first time we ever saw the session_id) so the UI can keep showing it
// after a user-initiated `tmux rename-session`. The `+ New tmux` button uses
// `tmux new-session -d -P` without `-s`, so origins are typically the
// auto-assigned numeric ids ("0", "1", ...). Externally-created sessions
// keep whatever name `-s` gave them.
//
// All four functions are pure and unit-testable. They are STUBS in this
// commit — bodies are deliberately minimal so the new RED tests fail. Each
// gets its real body in subsequent commits.

/// Update the origins map from this refresh's session list. First sighting
/// of a session_id records its current name; subsequent sightings preserve
/// the original. Sessions absent from the current list are pruned.
fn update_session_origins(
    origins: &mut HashMap<String, String>,
    sessions: &[(String, String, bool)],
) {
    let live_ids: HashSet<&str> = sessions.iter().map(|s| s.0.as_str()).collect();
    origins.retain(|id, _| live_ids.contains(id.as_str()));
    for (id, name, _) in sessions {
        origins.entry(id.clone()).or_insert_with(|| name.clone());
    }
}

/// Resolve the origin name for a session given its CURRENT name. Returns
/// the current name unchanged when no origin is recorded (defensive — happens
/// when refresh sees a session for the first time after a rename and the
/// caller queries before update_session_origins runs).
fn session_origin_for_name<'a>(
    name: &'a str,
    sessions: &[(String, String, bool)],
    origins: &'a HashMap<String, String>,
) -> &'a str {
    // STUB: passthrough. Implemented in a later commit.
    let _ = sessions;
    let _ = origins;
    name
}

/// Format a session row's display label given its current name and origin.
/// Origin == current → just the name. Renamed → "name (origin)".
fn format_session_row_label(name: &str, origin: &str) -> String {
    // STUB: returns name only. Implemented in a later commit.
    let _ = origin;
    name.to_string()
}

/// Render text for the green marker on attached-terminal window rows.
/// Origin truncated to 2 chars; ASCII-only since the renderer uses
/// `image_text8` (Latin-1).
fn marker_glyph_for_origin(origin: &str) -> String {
    // STUB: returns origin verbatim. Implemented in a later commit.
    origin.to_string()
}

fn parse_proc_status_ppid(s: &str) -> Option<u32> {
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("PPid:") {
            return rest.trim().parse::<u32>().ok();
        }
    }
    None
}

fn read_ppid(pid: u32) -> Option<u32> {
    let s = std::fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
    parse_proc_status_ppid(&s)
}

// Walk up from `start_pid` via `read_ppid_fn` until we hit a pid that exists
// in `pid_to_wid`, or we hit init (ppid=1), or we run out of depth. Returns
// the matched wid of the closest owning window, if any. Pure — the /proc
// reader is injected so this can be tested without a running process tree.
//
// Collision semantics: if the hit pid maps to multiple windows (e.g.
// gnome-terminal-server hosts every gnome-terminal window under one pid),
// we give up and return None rather than pick arbitrarily. "No marker" is
// strictly better than "wrong marker." Climbing further past an ambiguous
// hit would only find unrelated ancestors (systemd, init), so stop here.
fn walk_to_window_owner(
    start_pid: u32,
    pid_to_wid: &HashMap<u32, Vec<u32>>,
    mut read_ppid_fn: impl FnMut(u32) -> Option<u32>,
    max_depth: usize,
) -> Option<u32> {
    let mut cur = start_pid;
    for _ in 0..max_depth {
        if let Some(wids) = pid_to_wid.get(&cur) {
            if wids.len() == 1 {
                return Some(wids[0]);
            }
            return None;
        }
        match read_ppid_fn(cur) {
            Some(ppid) if ppid > 1 => cur = ppid,
            _ => return None,
        }
    }
    None
}

// True if there's already an in-flight attach for the same session. Used
// to debounce rapid repeat clicks on an orphan row — otherwise every
// click spawns another terminal while we wait for the first spawn's
// window to register.
fn is_attach_pending_for(
    pending: &Option<PendingSpawn>,
    session_name: &str,
) -> bool {
    matches!(pending, Some(PendingSpawn { kind: PendingSpawnKind::Attach(n), .. }) if n == session_name)
}

// Consume a pending spawn: if exactly one new wid has appeared since the
// previous refresh, claim it. For Attach kinds the wid is bound to its
// tmux session in `Item::session`; for Terminal kinds nothing is bound
// (the caller still snaps the wid). Returns Some(PendingClaim) when a
// claim is made, None otherwise.
//
// "Exactly one new wid" is the safe case — we know which window is ours.
// Zero new wids → window hasn't appeared yet, keep waiting. Multiple new
// wids → can't disambiguate (user opened something else in parallel), also
// keep waiting. Either way, the pending entry stays until it times out.
//
// Pure: `now` is injected so tests can simulate timeout without sleeping.
fn claim_pending_spawn(
    pending: &mut Option<PendingSpawn>,
    prior_wids: &HashSet<u32>,
    new_items: &mut [Item],
    timeout: std::time::Duration,
    now: std::time::Instant,
) -> Option<PendingClaim> {
    let spawn = pending.as_ref()?;
    if now.saturating_duration_since(spawn.spawned_at) > timeout {
        *pending = None;
        return None;
    }
    let new_wids: Vec<u32> = new_items
        .iter()
        .map(|i| i.wid)
        .filter(|w| !prior_wids.contains(w))
        .collect();
    if new_wids.len() != 1 {
        return None;
    }
    let claimed_wid = new_wids[0];
    let attach_session = match &spawn.kind {
        PendingSpawnKind::Attach(name) => {
            let session = name.clone();
            if let Some(item) = new_items.iter_mut().find(|i| i.wid == claimed_wid) {
                item.session = Some(session.clone());
            }
            Some(session)
        }
        PendingSpawnKind::Terminal => None,
    };
    *pending = None;
    Some(PendingClaim { wid: claimed_wid, attach_session })
}

/// Copy `item.session` forward from the prior refresh's items into the
/// freshly-built `new_items`. Caller passes the live tmux session-name set
/// so a binding pointing at a session that died (or was renamed) gets
/// dropped rather than ghosting forward.
///
/// This closes the gnome-terminal gap: `walk_to_window_owner` returns None
/// when the tmux client's owning ancestor PID has multiple X windows
/// attached (every gnome-terminal window shares `gnome-terminal-server`'s
/// PID), so once `claim_pending_spawn`'s one-shot binding is consumed,
/// subsequent refreshes had no way to keep the attribution. Carry-over is
/// the missing path.
///
/// Skips items whose `session` is already `Some(_)` so a fresh
/// `claim_pending_spawn` win takes precedence over carrying.
fn carry_over_session_bindings(
    prior_items: &[Item],
    new_items: &mut [Item],
    live_sessions: &HashSet<String>,
) {
    let prior: HashMap<u32, &str> = prior_items
        .iter()
        .filter_map(|i| i.session.as_deref().map(|s| (i.wid, s)))
        .collect();
    for item in new_items.iter_mut() {
        if item.session.is_some() {
            continue;
        }
        if let Some(name) = prior.get(&item.wid) {
            if live_sessions.contains(*name) {
                item.session = Some((*name).to_string());
            }
        }
    }
}

// ── Terminal launch ──
//
// PTM delegates terminal configuration to the system: whatever the user has
// set up as their default is what PTM launches. No tmux wrapping, no shell
// arguments, no session naming owned by PTM. If the user's shell rc
// auto-attaches to tmux, they get tmux; otherwise they get a plain shell.

fn detect_terminal_command(
    env_ptm_terminal_cmd: Option<&str>,
    env_terminal: Option<&str>,
    has_binary: impl Fn(&str) -> bool,
) -> Vec<String> {
    // PTM-specific override comes first. Lets the user point PTM at a
    // specific terminal+profile without polluting $TERMINAL (which many
    // CLI tools use as a plain terminal emulator and would break on args).
    for candidate in [env_ptm_terminal_cmd, env_terminal] {
        if let Some(val) = candidate {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return trimmed.split_whitespace().map(String::from).collect();
            }
        }
    }
    for candidate in ["x-terminal-emulator", "xdg-terminal-exec"] {
        if has_binary(candidate) {
            return vec![candidate.to_string()];
        }
    }
    vec!["xterm".to_string()]
}

fn binary_on_path(name: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    for dir in path.split(':') {
        if dir.is_empty() {
            continue;
        }
        let mut p = std::path::PathBuf::from(dir);
        p.push(name);
        if p.is_file() {
            return true;
        }
    }
    false
}

fn spawn_default_terminal() {
    let argv = detect_terminal_command(
        std::env::var("PTM_TERMINAL_CMD").ok().as_deref(),
        std::env::var("TERMINAL").ok().as_deref(),
        binary_on_path,
    );
    if argv.is_empty() {
        return;
    }
    let _ = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .spawn();
}

// Build the argv for launching a terminal attached to an existing tmux
// session. Different terminal emulators use different separators before
// the command: gnome-terminal / ptyxis need `--`, almost everything else
// (xterm, urxvt, alacritty, kitty, st, konsole) uses `-e`. Unknown
// terminals fall through to `-e` as a reasonable default.
fn terminal_argv_for_attach(term_argv: &[String], session_name: &str) -> Vec<String> {
    if term_argv.is_empty() {
        return Vec::new();
    }
    let term_name = std::path::Path::new(&term_argv[0])
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let separator = match term_name {
        "gnome-terminal" | "ptyxis" => "--",
        _ => "-e",
    };
    let mut out: Vec<String> = term_argv.to_vec();
    out.push(separator.to_string());
    out.push("tmux".to_string());
    out.push("attach-session".to_string());
    out.push("-t".to_string());
    out.push(session_name.to_string());
    out
}

fn spawn_attach_terminal(session_name: &str) {
    let term = detect_terminal_command(
        std::env::var("PTM_TERMINAL_CMD").ok().as_deref(),
        std::env::var("TERMINAL").ok().as_deref(),
        binary_on_path,
    );
    let argv = terminal_argv_for_attach(&term, session_name);
    if argv.is_empty() {
        return;
    }
    let _ = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .spawn();
}

/// Create a fresh tmux session with an auto-generated name (detached) and
/// return the assigned name. `tmux new-session -d -P -F '#{session_name}'`
/// prints the assigned name on stdout. Caller is responsible for any
/// follow-up (pending_spawn registration, then `spawn_attach_terminal`).
fn create_new_tmux_session() -> Option<String> {
    let out = std::process::Command::new("tmux")
        .args(["new-session", "-d", "-P", "-F", "#{session_name}"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

// ── Property-change classification (pure, testable) ──

#[derive(Debug, PartialEq, Eq)]
enum PropertyAction {
    RefreshClientList,
    UpdateActiveWindow,
    UpdateWindowTitle,
    Ignore,
}

fn classify_property_event(
    atom: u32,
    is_root: bool,
    net_client_list: u32,
    net_active_window: u32,
    net_wm_name: u32,
) -> PropertyAction {
    if is_root {
        if atom == net_client_list {
            PropertyAction::RefreshClientList
        } else if atom == net_active_window {
            PropertyAction::UpdateActiveWindow
        } else {
            PropertyAction::Ignore
        }
    } else if atom == net_wm_name || atom == u32::from(AtomEnum::WM_NAME) {
        PropertyAction::UpdateWindowTitle
    } else {
        PropertyAction::Ignore
    }
}

// ── Helpers ──

fn alloc_color(
    conn: &impl Connection,
    cmap: Colormap,
    rgb: u32,
) -> Result<u32, Box<dyn std::error::Error>> {
    let r = ((rgb >> 16) & 0xff) as u16 * 257;
    let g = ((rgb >> 8) & 0xff) as u16 * 257;
    let b = (rgb & 0xff) as u16 * 257;
    let reply = conn.alloc_color(cmap, r, g, b)?.reply()?;
    Ok(reply.pixel)
}

// ── Renderer ──

struct Renderer {
    window: Window,
    pixmap: Pixmap,
    gc: Gcontext,
    depth: u8,
    #[allow(dead_code)]
    colormap: Colormap,
    bg_pixel: u32,
    text_pixel: u32,
    text_dim_pixel: u32,
    indicator_pixel: u32,
    ghost_pixel: u32,
    item_pixel: u32,
    item_hover_pixel: u32,
    item_active_pixel: u32,
    active_stripe_pixel: u32,
    menu_bg_pixel: u32,
    menu_border_pixel: u32,
    menu_hover_pixel: u32,
    group_header_pixel: u32,
    group_color_pixels: Vec<u32>,
    session_marker_pixel: u32,
    selection_bg_pixel: u32,
}

impl Renderer {
    fn new(
        conn: &impl Connection,
        screen: &Screen,
        window: Window,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let colormap = screen.default_colormap;
        let depth = screen.root_depth;

        let bg_pixel = alloc_color(conn, colormap, BG_COLOR)?;
        let text_pixel = alloc_color(conn, colormap, TEXT_COLOR)?;
        let text_dim_pixel = alloc_color(conn, colormap, TEXT_DIM_COLOR)?;
        let indicator_pixel = alloc_color(conn, colormap, INDICATOR_COLOR)?;
        let ghost_pixel = alloc_color(conn, colormap, GHOST_COLOR)?;
        let item_pixel = alloc_color(conn, colormap, ITEM_COLOR)?;
        let item_hover_pixel = alloc_color(conn, colormap, ITEM_HOVER_COLOR)?;
        let item_active_pixel = alloc_color(conn, colormap, ITEM_ACTIVE_COLOR)?;
        let active_stripe_pixel = alloc_color(conn, colormap, ACTIVE_STRIPE_COLOR)?;
        let menu_bg_pixel = alloc_color(conn, colormap, MENU_BG_COLOR)?;
        let menu_border_pixel = alloc_color(conn, colormap, MENU_BORDER_COLOR)?;
        let menu_hover_pixel = alloc_color(conn, colormap, MENU_HOVER_COLOR)?;
        let group_header_pixel = alloc_color(conn, colormap, GROUP_HEADER_COLOR)?;
        let session_marker_pixel = alloc_color(conn, colormap, SESSION_MARKER_COLOR)?;
        let selection_bg_pixel = alloc_color(conn, colormap, SELECTION_BG_COLOR)?;

        let mut group_color_pixels = Vec::new();
        for &c in GROUP_COLORS {
            group_color_pixels.push(alloc_color(conn, colormap, c)?);
        }

        // Try scalable Nimbus Mono L, fall back to fixed 13px
        let font = conn.generate_id()?;
        let opened = conn.open_font(
            font,
            b"-urw-nimbus mono l-regular-r-normal--13-*-*-*-*-*-iso8859-1",
        );
        if opened.is_err() {
            conn.open_font(
                font,
                b"-misc-fixed-medium-r-normal--13-120-75-75-c-70-iso8859-1",
            )?;
        }

        let gc = conn.generate_id()?;
        let gc_aux = CreateGCAux::new()
            .foreground(text_pixel)
            .background(bg_pixel)
            .font(font)
            .graphics_exposures(0);
        conn.create_gc(gc, window, &gc_aux)?;

        let pixmap = conn.generate_id()?;
        conn.create_pixmap(depth, pixmap, window, WIN_W, WIN_H)?;

        Ok(Self {
            window,
            pixmap,
            gc,
            depth,
            colormap,
            bg_pixel,
            text_pixel,
            text_dim_pixel,
            indicator_pixel,
            ghost_pixel,
            item_pixel,
            item_hover_pixel,
            item_active_pixel,
            active_stripe_pixel,
            menu_bg_pixel,
            menu_border_pixel,
            menu_hover_pixel,
            group_header_pixel,
            group_color_pixels,
            session_marker_pixel,
            selection_bg_pixel,
        })
    }

    fn resize(
        &mut self,
        conn: &impl Connection,
        width: u16,
        height: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        conn.free_pixmap(self.pixmap)?;
        self.pixmap = conn.generate_id()?;
        conn.create_pixmap(self.depth, self.pixmap, self.window, width, height)?;
        Ok(())
    }

    fn redraw(
        &self,
        conn: &impl Connection,
        app: &App,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let pix = self.pixmap;

        // Clear background. Reset GC's `background` attribute too — image_text8
        // draws each glyph cell with the GC's bg, and the rename overlay's
        // selection-segment draw can leave bg = selection_bg_pixel. Without
        // this, the next paint's text rows render with red cell backgrounds
        // until the rename overlay redraws and resets bg.
        conn.change_gc(
            self.gc,
            &ChangeGCAux::new().foreground(self.bg_pixel).background(self.bg_pixel),
        )?;
        conn.poly_fill_rectangle(
            pix,
            self.gc,
            &[Rectangle {
                x: 0,
                y: 0,
                width: app.width,
                height: app.height,
            }],
        )?;

        // Draw the "+ New terminal" header button above the item list.
        self.draw_top_buttons(conn, pix, app)?;

        let dragged_row = app
            .drag
            .as_ref()
            .filter(|d| d.started)
            .map(|d| d.source_row);

        // Draw display rows
        for (i, row) in app.display_rows.iter().enumerate() {
            if Some(i) == dragged_row {
                continue;
            }
            let y = app.row_y(i);
            let hovered = app.hover_row == Some(i);
            match row {
                DisplayRow::GroupHeader { group_id } => {
                    self.draw_group_header(conn, pix, app, *group_id, y, hovered)?;
                }
                DisplayRow::Window { wid, group_id } => {
                    if let Some(item) = app.find_item(*wid) {
                        let is_active = app.active_wid == Some(*wid);
                        let drop_hi = app.drop_highlight_active_for(*wid);
                        let ix = app.item_x();
                        let iw = app.item_w();
                        let (x, w) = if group_id.is_some() {
                            (ix + GROUP_INDENT, iw - GROUP_INDENT as u16)
                        } else {
                            (ix, iw)
                        };
                        self.draw_item(conn, pix, x, y, w, ITEM_H as u16, item, false, hovered, is_active, drop_hi)?;
                    }
                }
                DisplayRow::Session { name, group_id } => {
                    let ix = app.item_x();
                    let iw = app.item_w();
                    let (x, w) = if group_id.is_some() {
                        (ix + GROUP_INDENT, iw - GROUP_INDENT as u16)
                    } else {
                        (ix, iw)
                    };
                    self.draw_session_row(
                        conn,
                        pix,
                        x,
                        y,
                        w,
                        ITEM_H as u16,
                        name,
                        hovered,
                    )?;
                }
            }
        }

        // Draw rename overlay (draws on top of the target row)
        if let Some(ref rs) = app.rename {
            let row_idx = match &rs.target {
                RenameTarget::Group(gid) => app.display_rows.iter().position(
                    |r| matches!(r, DisplayRow::GroupHeader { group_id } if group_id == gid),
                ),
                RenameTarget::Window(wid) => app.display_rows.iter().position(
                    |r| matches!(r, DisplayRow::Window { wid: w, .. } if w == wid),
                ),
                RenameTarget::Session(name) => app.display_rows.iter().position(
                    |r| matches!(r, DisplayRow::Session { name: n, .. } if n == name),
                ),
            };
            if let Some(row_idx) = row_idx {
                self.draw_rename_input(conn, pix, app, rs, app.row_y(row_idx))?;
            }
        }

        // Draw drag visuals
        if let Some(drag) = &app.drag {
            if drag.started {
                // T3.3: indicator position is derived from the SAME
                // DropTarget that handle_drop will fire on release, so the
                // visual line never disagrees with the actual landing.
                let target = classify_drop(app, drag.source_row, drag.current_y);
                if !matches!(target, DropTarget::NoOp) {
                    // T3.4: when the drop will Join/Reorder a group, outline
                    // that group so the user sees the join target at a
                    // glance (especially valuable for the lower-half /
                    // bottom-spacing gestures that pre-Stage-G ejected).
                    if let Some(target_gid) = target_group_for_outline(&target) {
                        if let Some((top, bottom)) = group_outline_bounds(app, target_gid) {
                            let ix = app.item_x();
                            let iw = app.item_w();
                            conn.change_gc(
                                self.gc,
                                &ChangeGCAux::new().foreground(self.indicator_pixel),
                            )?;
                            // Top edge
                            conn.poly_fill_rectangle(pix, self.gc, &[Rectangle { x: ix, y: top, width: iw, height: 1 }])?;
                            // Bottom edge
                            conn.poly_fill_rectangle(pix, self.gc, &[Rectangle { x: ix, y: bottom - 1, width: iw, height: 1 }])?;
                            // Left edge
                            conn.poly_fill_rectangle(pix, self.gc, &[Rectangle { x: ix, y: top, width: 1, height: (bottom - top) as u16 }])?;
                            // Right edge
                            conn.poly_fill_rectangle(pix, self.gc, &[Rectangle { x: ix + iw as i16 - 1, y: top, width: 1, height: (bottom - top) as u16 }])?;
                        }
                    }
                    let indicator_y = indicator_y_for_target(app, &target);
                    conn.change_gc(
                        self.gc,
                        &ChangeGCAux::new().foreground(self.indicator_pixel),
                    )?;
                    conn.poly_fill_rectangle(
                        pix,
                        self.gc,
                        &[Rectangle {
                            x: app.item_x(),
                            y: indicator_y,
                            width: app.item_w(),
                            height: 2,
                        }],
                    )?;
                }

                // Ghost at cursor
                if drag.source_row < app.display_rows.len() {
                    let ghost_y = drag.current_y - (ITEM_H as i16 / 2);
                    match &app.display_rows[drag.source_row] {
                        DisplayRow::GroupHeader { group_id } => {
                            self.draw_ghost_header(conn, pix, app, *group_id, ghost_y)?;
                        }
                        DisplayRow::Window { wid, group_id } => {
                            if let Some(item) = app.find_item(*wid) {
                                let ix = app.item_x();
                                let iw = app.item_w();
                                let (x, w) = if group_id.is_some() {
                                    (ix + GROUP_INDENT, iw - GROUP_INDENT as u16)
                                } else {
                                    (ix, iw)
                                };
                                self.draw_item(
                                    conn, pix, x, ghost_y, w, ITEM_H as u16, item, true, false, false, false,
                                )?;
                            }
                        }
                        DisplayRow::Session { name, group_id } => {
                            let ix = app.item_x();
                            let iw = app.item_w();
                            let (x, w) = if group_id.is_some() {
                                (ix + GROUP_INDENT, iw - GROUP_INDENT as u16)
                            } else {
                                (ix, iw)
                            };
                            // Ghost reuses the session-row drawing; hovered=false, no special ghost style.
                            self.draw_session_row(
                                conn,
                                pix,
                                x,
                                ghost_y,
                                w,
                                ITEM_H as u16,
                                name,
                                false,
                            )?;
                        }
                    }
                }
            }
        }

        conn.copy_area(pix, self.window, self.gc, 0, 0, 0, 0, app.width, app.height)?;
        conn.flush()?;
        Ok(())
    }

    fn draw_top_buttons(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        app: &App,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (left, right_opt) = top_buttons_layout(app.tmux_available, app.width);
        self.draw_top_button(
            conn,
            drawable,
            &left,
            "+ New terminal",
            app.top_button_hover == Some(TopButton::NewTerminal),
        )?;
        if let Some(right) = right_opt {
            self.draw_top_button(
                conn,
                drawable,
                &right,
                "+ New tmux",
                app.top_button_hover == Some(TopButton::NewTmux),
            )?;
        }
        Ok(())
    }

    fn draw_top_button(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        rect: &Rectangle,
        label: &str,
        hovered: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let bg = if hovered {
            self.item_hover_pixel
        } else {
            self.item_pixel
        };
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(bg))?;
        conn.poly_fill_rectangle(drawable, self.gc, &[*rect])?;

        let label_width = label.len() as i16 * CHAR_WIDTH;
        let text_x = rect.x + (rect.width as i16 - label_width) / 2;
        let text_y = rect.y + (rect.height as i16 / 2) + 4;
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
        conn.image_text8(drawable, self.gc, text_x, text_y, label.as_bytes())?;
        Ok(())
    }

    fn draw_item(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        item: &Item,
        ghost: bool,
        hovered: bool,
        is_active: bool,
        drop_highlighted: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Drop-highlight (T3.5) overrides hover/active so the user can't
        // miss the post-drop flash, but ghost (drag preview) still wins
        // because the dragged item shouldn't pretend it landed yet.
        let bg = if ghost {
            self.ghost_pixel
        } else if drop_highlighted {
            self.selection_bg_pixel
        } else if is_active {
            self.item_active_pixel
        } else if hovered {
            self.item_hover_pixel
        } else {
            self.item_pixel
        };
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(bg))?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x,
                y,
                width: w,
                height: h,
            }],
        )?;

        // Left accent stripe (3px) — blue override for active window
        let stripe_color = if is_active {
            self.active_stripe_pixel
        } else {
            item.accent_pixel
        };
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(stripe_color))?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x,
                y,
                width: 3,
                height: h,
            }],
        )?;

        // Reserve right-side space for the session marker if present,
        // so the label truncates cleanly instead of overlapping the dot.
        let marker_reserve: i16 = if item.session.is_some() { 14 } else { 0 };

        // Label
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
        let text_x = x + 8;
        let text_y = y + (h as i16 / 2) + 4;
        let max_chars = ((w as i16 - 12 - marker_reserve) / CHAR_WIDTH).max(0) as usize;
        let full_label = item.display_label();
        let display: String = full_label.chars().take(max_chars).collect();
        if !display.is_empty() {
            conn.image_text8(drawable, self.gc, text_x, text_y, display.as_bytes())?;
        }

        // Session marker: small filled circle on the right edge when this
        // window is attached to a tmux session.
        if item.session.is_some() {
            let marker_size: u16 = 6;
            let marker_x = x + w as i16 - marker_size as i16 - 6;
            let marker_y = y + (h as i16 - marker_size as i16) / 2;
            conn.change_gc(
                self.gc,
                &ChangeGCAux::new().foreground(self.session_marker_pixel),
            )?;
            conn.poly_fill_arc(
                drawable,
                self.gc,
                &[Arc {
                    x: marker_x,
                    y: marker_y,
                    width: marker_size,
                    height: marker_size,
                    angle1: 0,
                    angle2: 360 * 64,
                }],
            )?;
        }

        Ok(())
    }

    fn draw_session_row(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        name: &str,
        hovered: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Background
        let bg = if hovered {
            self.item_hover_pixel
        } else {
            self.item_pixel
        };
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(bg))?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle { x, y, width: w, height: h }],
        )?;

        // Grey left-edge stripe so session rows read distinctly from
        // window rows.
        conn.change_gc(
            self.gc,
            &ChangeGCAux::new().foreground(self.text_dim_pixel),
        )?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle { x, y, width: 3, height: h }],
        )?;

        // Reserve right-edge space for the [x] glyph so long session names
        // truncate cleanly. Layout (right → left): [x glyph][gap]<text>.
        let marker_reserve: i16 = SESSION_CLOSE_BAND_WIDTH;
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
        let text_x = x + 8;
        let text_y = y + (h as i16 / 2) + 4;
        let max_chars = ((w as i16 - 12 - marker_reserve) / CHAR_WIDTH).max(0) as usize;
        let display: String = name.chars().take(max_chars).collect();
        if !display.is_empty() {
            conn.image_text8(drawable, self.gc, text_x, text_y, display.as_bytes())?;
        }

        // [x] close glyph at the right edge. Hit-test
        // (hit_test_session_close_button) keys on `local_x` only, so
        // positioning here must agree with the close band's right edge.
        let x_glyph_x = x + w as i16 - SESSION_CLOSE_BAND_WIDTH;
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_dim_pixel))?;
        conn.image_text8(drawable, self.gc, x_glyph_x, text_y, b"x")?;

        Ok(())
    }

    fn draw_group_header(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        app: &App,
        group_id: u32,
        y: i16,
        hovered: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let group = match app.groups.iter().find(|g| g.id == group_id) {
            Some(g) => g,
            None => return Ok(()),
        };

        let ix = app.item_x();
        let iw = app.item_w();

        // Background
        let bg = if hovered {
            self.item_hover_pixel
        } else {
            self.group_header_pixel
        };
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(bg))?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x: ix,
                y,
                width: iw,
                height: ITEM_H as u16,
            }],
        )?;

        // Top edge line for visual separation
        conn.change_gc(
            self.gc,
            &ChangeGCAux::new().foreground(self.menu_border_pixel),
        )?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x: ix,
                y,
                width: iw,
                height: 1,
            }],
        )?;

        // Left accent stripe
        let color_idx = group_id as usize % self.group_color_pixels.len();
        conn.change_gc(
            self.gc,
            &ChangeGCAux::new().foreground(self.group_color_pixels[color_idx]),
        )?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x: ix,
                y,
                width: 3,
                height: ITEM_H as u16,
            }],
        )?;

        // Arrow + name
        let arrow = if group.collapsed { "+" } else { "-" };
        let name_text = format!("{} {}", arrow, group.name);

        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
        let text_x = ix + 8;
        let text_y = y + (ITEM_H as i16 / 2) + 4;
        let max_chars = ((iw as i16 - 12) / CHAR_WIDTH).max(0) as usize;
        let display: String = name_text.chars().take(max_chars).collect();
        conn.image_text8(drawable, self.gc, text_x, text_y, display.as_bytes())?;

        // Member count (dimmed) when collapsed
        if group.collapsed {
            let count_text = format!("({})", group.display_count());
            let name_width = (display.len() as i16 + 1) * CHAR_WIDTH;
            conn.change_gc(
                self.gc,
                &ChangeGCAux::new().foreground(self.text_dim_pixel),
            )?;
            conn.image_text8(
                drawable,
                self.gc,
                text_x + name_width,
                text_y,
                count_text.as_bytes(),
            )?;
        }

        Ok(())
    }

    fn draw_ghost_header(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        app: &App,
        group_id: u32,
        y: i16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ix = app.item_x();
        let iw = app.item_w();
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.ghost_pixel))?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x: ix,
                y,
                width: iw,
                height: ITEM_H as u16,
            }],
        )?;
        if let Some(group) = app.groups.iter().find(|g| g.id == group_id) {
            let text = format!("{} (+{})", group.name, group.display_count());
            conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
            conn.image_text8(
                drawable,
                self.gc,
                ix + 8,
                y + (ITEM_H as i16 / 2) + 4,
                text.as_bytes(),
            )?;
        }
        Ok(())
    }

    fn draw_rename_input(
        &self,
        conn: &impl Connection,
        drawable: Drawable,
        app: &App,
        rs: &RenameState,
        y: i16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ix = app.item_x();
        let iw = app.item_w();

        // Dark background
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.bg_pixel))?;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x: ix,
                y,
                width: iw,
                height: ITEM_H as u16,
            }],
        )?;

        // Border
        conn.change_gc(
            self.gc,
            &ChangeGCAux::new().foreground(self.indicator_pixel),
        )?;
        conn.poly_fill_rectangle(drawable, self.gc, &[Rectangle { x: ix, y, width: iw, height: 1 }])?;
        conn.poly_fill_rectangle(drawable, self.gc, &[Rectangle { x: ix, y: y + ITEM_H as i16 - 1, width: iw, height: 1 }])?;
        conn.poly_fill_rectangle(drawable, self.gc, &[Rectangle { x: ix, y, width: 1, height: ITEM_H as u16 }])?;
        conn.poly_fill_rectangle(drawable, self.gc, &[Rectangle { x: ix + iw as i16 - 1, y, width: 1, height: ITEM_H as u16 }])?;

        let text_x = ix + 8;
        let text_y = y + (ITEM_H as i16 / 2) + 4;
        let max_chars = ((iw as i16 - 16) / CHAR_WIDTH).max(0) as usize;
        let display_chars: Vec<char> = rs.text.chars().take(max_chars).collect();

        // Map byte selection to visible char range (if any).
        let (sel_lo_chars, sel_hi_chars) = match rs.selection_range() {
            Some((lo_byte, hi_byte)) => {
                let lo_b = lo_byte.min(rs.text.len());
                let hi_b = hi_byte.min(rs.text.len());
                let lo_c = rs.text[..lo_b].chars().count().min(max_chars);
                let hi_c = rs.text[..hi_b].chars().count().min(max_chars);
                (lo_c, hi_c)
            }
            None => (0, 0),
        };

        // Selection highlight (drawn underneath the text so the per-glyph
        // backgrounds from image_text8 overwrite it cleanly within character
        // cells; we then re-draw the selected cells with the selection
        // background to recover the highlight).
        if sel_hi_chars > sel_lo_chars {
            let sel_x = text_x + (sel_lo_chars as i16) * CHAR_WIDTH;
            let sel_w = ((sel_hi_chars - sel_lo_chars) as i16 * CHAR_WIDTH) as u16;
            conn.change_gc(
                self.gc,
                &ChangeGCAux::new().foreground(self.selection_bg_pixel),
            )?;
            conn.poly_fill_rectangle(
                drawable,
                self.gc,
                &[Rectangle {
                    x: sel_x,
                    y: y + 4,
                    width: sel_w,
                    height: ITEM_H as u16 - 8,
                }],
            )?;
        }

        // Text — draw in 1 or 3 segments so each glyph's image_text8 background
        // matches the underlying selection highlight.
        let visible = display_chars.len();
        let draw_text_segment = |start: usize, end: usize, bg: u32| -> Result<(), Box<dyn std::error::Error>> {
            if end <= start { return Ok(()); }
            let segment: String = display_chars[start..end].iter().collect();
            conn.change_gc(
                self.gc,
                &ChangeGCAux::new().foreground(self.text_pixel).background(bg),
            )?;
            let seg_x = text_x + (start as i16) * CHAR_WIDTH;
            conn.image_text8(drawable, self.gc, seg_x, text_y, segment.as_bytes())?;
            Ok(())
        };
        if sel_hi_chars > sel_lo_chars {
            draw_text_segment(0, sel_lo_chars, self.bg_pixel)?;
            draw_text_segment(sel_lo_chars, sel_hi_chars, self.selection_bg_pixel)?;
            draw_text_segment(sel_hi_chars, visible, self.bg_pixel)?;
        } else {
            draw_text_segment(0, visible, self.bg_pixel)?;
        }

        // Cursor bar
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(self.text_pixel))?;
        let cursor_chars = rs.text[..rs.cursor].chars().count().min(max_chars);
        let cursor_x = text_x + (cursor_chars as i16) * CHAR_WIDTH;
        conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x: cursor_x,
                y: y + 4,
                width: 1,
                height: ITEM_H as u16 - 8,
            }],
        )?;

        Ok(())
    }
}

// ── Context menu ──

/// Session name attached to the window at `row`, if any. Used both by
/// build_menu_entries (decide whether to show "Kill tmux session") and by
/// execute_menu_action (extract the name when dispatching the popup).
fn window_session_for_row(app: &App, row: usize) -> Option<String> {
    if let Some(DisplayRow::Window { wid, .. }) = app.display_rows.get(row) {
        if let Some(item) = app.find_item(*wid) {
            return item.session.clone();
        }
    }
    None
}

fn build_menu_entries(app: &App, row: usize) -> Vec<MenuEntry> {
    if row >= app.display_rows.len() {
        return vec![];
    }
    let attached_session = window_session_for_row(app, row);
    match &app.display_rows[row] {
        DisplayRow::GroupHeader { group_id } => {
            let mut entries = vec![MenuEntry {
                label: "Rename Group".to_string(),
                action: MenuAction::RenameGroup,
            }];
            // System groups can't be deleted from the UI — they're auto-managed.
            if !is_target_system_group(app, *group_id) {
                entries.push(MenuEntry {
                    label: "Delete Group".to_string(),
                    action: MenuAction::DeleteGroup,
                });
            }
            entries
        }
        DisplayRow::Window {
            group_id: Some(_), ..
        } => {
            let mut entries = vec![
                MenuEntry {
                    label: "Rename Tab".to_string(),
                    action: MenuAction::RenameTab,
                },
                MenuEntry {
                    label: "Remove from Group".to_string(),
                    action: MenuAction::RemoveFromGroup,
                },
            ];
            if attached_session.is_some() {
                entries.push(MenuEntry {
                    label: "Kill tmux session".to_string(),
                    action: MenuAction::KillSession,
                });
            }
            entries
        }
        DisplayRow::Window {
            group_id: None, ..
        } => {
            let mut entries = vec![
                MenuEntry {
                    label: "Rename Tab".to_string(),
                    action: MenuAction::RenameTab,
                },
                MenuEntry {
                    label: "New Group".to_string(),
                    action: MenuAction::CreateGroup,
                },
            ];
            for group in &app.groups {
                entries.push(MenuEntry {
                    label: format!("Add to {}", group.name),
                    action: MenuAction::AddToGroup(group.id),
                });
            }
            if attached_session.is_some() {
                entries.push(MenuEntry {
                    label: "Kill tmux session".to_string(),
                    action: MenuAction::KillSession,
                });
            }
            entries
        }
        DisplayRow::Session { .. } => {
            vec![
                MenuEntry {
                    label: "Attach".to_string(),
                    action: MenuAction::AttachSession,
                },
                MenuEntry {
                    label: "Rename Session".to_string(),
                    action: MenuAction::RenameSession,
                },
                MenuEntry {
                    label: "Kill Session".to_string(),
                    action: MenuAction::KillSession,
                },
            ]
        }
    }
}

fn open_context_menu(
    conn: &impl Connection,
    screen: &Screen,
    renderer: &Renderer,
    app: &mut App,
    target_row: usize,
    root_x: i16,
    root_y: i16,
) -> Result<(), Box<dyn std::error::Error>> {
    if app.context_menu.is_some() {
        close_context_menu(conn, app)?;
    }

    let entries = build_menu_entries(app, target_row);
    if entries.is_empty() {
        return Ok(());
    }

    let height = (entries.len() as u16) * MENU_ITEM_H + (MENU_PADDING as u16 * 2);
    let width = MENU_MIN_W;

    let x = root_x.min((screen.width_in_pixels as i16) - width as i16);
    let y = root_y.min((screen.height_in_pixels as i16) - height as i16);

    let win = conn.generate_id()?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win,
        screen.root,
        x,
        y,
        width,
        height,
        1,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .override_redirect(1u32)
            .background_pixel(renderer.menu_bg_pixel)
            .border_pixel(renderer.menu_border_pixel)
            .event_mask(EventMask::EXPOSURE),
    )?;

    let pix = conn.generate_id()?;
    conn.create_pixmap(screen.root_depth, pix, win, width, height)?;

    conn.map_window(win)?;

    let _grab_reply = conn
        .grab_pointer(
            false,
            win,
            EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            0u32,
            0u32,
            0u32,
        )?
        .reply()?;

    conn.flush()?;

    app.context_menu = Some(ContextMenu {
        window: win,
        pixmap: pix,
        entries,
        target_row,
        x,
        y,
        width,
        height,
        hover_index: None,
    });

    draw_context_menu(conn, renderer, app)?;
    Ok(())
}

fn close_context_menu(
    conn: &impl Connection,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(menu) = app.context_menu.take() {
        conn.ungrab_pointer(0u32)?;
        conn.free_pixmap(menu.pixmap)?;
        conn.destroy_window(menu.window)?;
        conn.flush()?;
    }
    Ok(())
}

fn draw_context_menu(
    conn: &impl Connection,
    renderer: &Renderer,
    app: &App,
) -> Result<(), Box<dyn std::error::Error>> {
    let menu = match app.context_menu.as_ref() {
        Some(m) => m,
        None => return Ok(()),
    };
    let pix = menu.pixmap;
    let gc = renderer.gc;

    // Clear
    conn.change_gc(gc, &ChangeGCAux::new().foreground(renderer.menu_bg_pixel))?;
    conn.poly_fill_rectangle(
        pix,
        gc,
        &[Rectangle {
            x: 0,
            y: 0,
            width: menu.width,
            height: menu.height,
        }],
    )?;

    // Draw entries
    for (i, entry) in menu.entries.iter().enumerate() {
        let entry_y = MENU_PADDING + (i as i16) * MENU_ITEM_H as i16;

        if Some(i) == menu.hover_index {
            conn.change_gc(gc, &ChangeGCAux::new().foreground(renderer.menu_hover_pixel))?;
            conn.poly_fill_rectangle(
                pix,
                gc,
                &[Rectangle {
                    x: 2,
                    y: entry_y,
                    width: menu.width - 4,
                    height: MENU_ITEM_H,
                }],
            )?;
        }

        conn.change_gc(gc, &ChangeGCAux::new().foreground(renderer.text_pixel))?;
        let text_y = entry_y + (MENU_ITEM_H as i16 / 2) + 4;
        conn.image_text8(pix, gc, 8, text_y, entry.label.as_bytes())?;
    }

    conn.copy_area(pix, menu.window, gc, 0, 0, 0, 0, menu.width, menu.height)?;
    conn.flush()?;
    Ok(())
}

/// Returned by `execute_menu_action` when the action wants to open a
/// confirmation popup as a follow-up. Caller (event loop) materializes it via
/// `open_confirm_popup` so that fn stays X11-free for tests.
struct ConfirmRequest {
    message: String,
    action: ConfirmAction,
}

fn execute_menu_action(
    app: &mut App,
    action: MenuAction,
    target_row: usize,
) -> Option<ConfirmRequest> {
    if target_row >= app.display_rows.len() {
        return None;
    }
    match action {
        MenuAction::CreateGroup => {
            if let DisplayRow::Window { wid, .. } = &app.display_rows[target_row] {
                let wid = *wid;
                let gid = app.create_group(wid);
                // Drop straight into rename so a single keystroke replaces
                // the default "Group N" name; Enter accepts the default.
                app.start_rename(gid);
            }
        }
        MenuAction::AddToGroup(gid) => {
            if let DisplayRow::Window { wid, .. } = &app.display_rows[target_row] {
                app.add_to_group(gid, *wid);
            }
        }
        MenuAction::RemoveFromGroup => {
            if let DisplayRow::Window { wid, .. } = &app.display_rows[target_row] {
                app.remove_from_group(*wid);
            }
        }
        MenuAction::RenameGroup => {
            if let DisplayRow::GroupHeader { group_id } = &app.display_rows[target_row] {
                app.start_rename(*group_id);
            }
        }
        MenuAction::DeleteGroup => {
            if let DisplayRow::GroupHeader { group_id } = &app.display_rows[target_row] {
                app.delete_group(*group_id);
            }
        }
        MenuAction::RenameTab => {
            if let DisplayRow::Window { wid, .. } = &app.display_rows[target_row] {
                app.start_tab_rename(*wid);
            }
        }
        MenuAction::AttachSession => {
            if let DisplayRow::Session { name, .. } = &app.display_rows[target_row] {
                if !is_attach_pending_for(&app.pending_spawn, name) {
                    app.pending_spawn = Some(PendingSpawn {
                        kind: PendingSpawnKind::Attach(name.clone()),
                        spawned_at: std::time::Instant::now(),
                    });
                    spawn_attach_terminal(name);
                }
            }
        }
        MenuAction::RenameSession => {
            if let DisplayRow::Session { name, .. } = &app.display_rows[target_row] {
                app.start_session_rename(&name.clone());
            }
        }
        MenuAction::KillSession => {
            // Two paths: orphan session row (existing direct invocation —
            // popup feels excessive for already-detached sessions) versus
            // attached terminal row (open confirm popup; killing an attached
            // session also kills the terminal so confirmation is warranted).
            match &app.display_rows[target_row] {
                DisplayRow::Session { name, .. } => {
                    let target = name.clone();
                    kill_tmux_session(app, &target);
                }
                DisplayRow::Window { .. } => {
                    if let Some(session) = window_session_for_row(app, target_row) {
                        return Some(ConfirmRequest {
                            message: format!("Kill tmux session '{}'?", session),
                            action: ConfirmAction::KillSession(session),
                        });
                    }
                }
                _ => {}
            }
        }
    }
    None
}

// ── Confirmation popup ──

/// Pure dispatch for the confirmation popup. Returns the pending action if
/// the user accepted and a popup is armed; otherwise `None`. Non-destructive
/// — leaves `app.confirm` untouched so the caller can capture the action
/// BEFORE running `close_confirm_popup` (which is what tears the popup
/// down and frees its X11 resources). Calling close before dispatch loses
/// the action, which used to silently break the popup-accept kill path.
fn dispatch_confirm(app: &App, accepted: bool) -> Option<ConfirmAction> {
    if !accepted {
        return None;
    }
    app.confirm.as_ref().map(|p| p.action.clone())
}

/// Kill a tmux session by name and optimistically prune the matching
/// member from any TmuxSystem group, rebuilding `display_rows` so the UI
/// reflects the kill before the next ~5s tmux poll. The kill command's
/// status is ignored — if it failed (e.g. session already gone), the
/// next refresh will re-add the member from `tmux list-sessions`, so the
/// optimistic prune is self-healing. Used by both the right-click →
/// Kill Session orphan path and the `[x]` popup-accept path; without
/// the latter, orphan rows linger because killing a detached session
/// produces no `_NET_CLIENT_LIST` event to drive a refresh.
fn kill_tmux_session(app: &mut App, name: &str) {
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", name])
        .status();
    for group in &mut app.groups {
        if group.kind == GroupKind::TmuxSystem {
            group.members.retain(|m| m.label != name);
        }
    }
    app.build_display_rows();
}

/// Side-effecting runner for confirmation popup actions. Today the only
/// action is `KillSession`, which routes through `kill_tmux_session` so
/// the row disappears immediately on accept (whether the session was
/// orphan or attached).
fn execute_confirm_action(app: &mut App, action: ConfirmAction) {
    match action {
        ConfirmAction::KillSession(name) => kill_tmux_session(app, &name),
    }
}

fn confirm_popup_layout(message: &str) -> (u16, u16, Rectangle, Rectangle) {
    // Width is the larger of the message + padding and the constant min so
    // longer session names don't get truncated awkwardly.
    let msg_w = message.len() as i16 * CHAR_WIDTH + CONFIRM_PADDING * 2;
    let width = (msg_w as u16).max(CONFIRM_MIN_W);
    // Single message line + button row.
    let height = (CONFIRM_PADDING as u16) * 3 + ITEM_H + CONFIRM_BUTTON_H;
    let buttons_y = CONFIRM_PADDING * 2 + ITEM_H as i16;
    let total_buttons_w = (CONFIRM_BUTTON_W * 2) as i16 + CONFIRM_PADDING;
    let buttons_start_x = (width as i16 - total_buttons_w) / 2;
    let yes_rect = Rectangle {
        x: buttons_start_x,
        y: buttons_y,
        width: CONFIRM_BUTTON_W,
        height: CONFIRM_BUTTON_H,
    };
    let no_rect = Rectangle {
        x: buttons_start_x + CONFIRM_BUTTON_W as i16 + CONFIRM_PADDING,
        y: buttons_y,
        width: CONFIRM_BUTTON_W,
        height: CONFIRM_BUTTON_H,
    };
    (width, height, yes_rect, no_rect)
}

fn point_in_rect(x: i16, y: i16, r: &Rectangle) -> bool {
    x >= r.x && x < r.x + r.width as i16 && y >= r.y && y < r.y + r.height as i16
}

fn open_confirm_popup(
    conn: &impl Connection,
    screen: &Screen,
    renderer: &Renderer,
    app: &mut App,
    message: String,
    action: ConfirmAction,
    root_x: i16,
    root_y: i16,
) -> Result<(), Box<dyn std::error::Error>> {
    if app.confirm.is_some() {
        close_confirm_popup(conn, app)?;
    }

    let (width, height, yes_rect, no_rect) = confirm_popup_layout(&message);

    let x = root_x.min((screen.width_in_pixels as i16) - width as i16).max(0);
    let y = root_y.min((screen.height_in_pixels as i16) - height as i16).max(0);

    let win = conn.generate_id()?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win,
        screen.root,
        x,
        y,
        width,
        height,
        1,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .override_redirect(1u32)
            .background_pixel(renderer.menu_bg_pixel)
            .border_pixel(renderer.menu_border_pixel)
            .event_mask(EventMask::EXPOSURE | EventMask::KEY_PRESS),
    )?;

    let pix = conn.generate_id()?;
    conn.create_pixmap(screen.root_depth, pix, win, width, height)?;

    conn.map_window(win)?;

    let _grab_reply = conn
        .grab_pointer(
            false,
            win,
            EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            0u32,
            0u32,
            0u32,
        )?
        .reply()?;

    // Keyboard grab so Enter / Y / Esc / N work without focus shenanigans —
    // matches the override-redirect popup pattern we already use elsewhere.
    let _ = conn
        .grab_keyboard(false, win, 0u32, GrabMode::ASYNC, GrabMode::ASYNC)?
        .reply()?;

    conn.flush()?;

    app.confirm = Some(ConfirmPopup {
        window: win,
        pixmap: pix,
        message,
        action,
        width,
        height,
        yes_rect,
        no_rect,
        hover_button: None,
    });

    draw_confirm_popup(conn, renderer, app)?;
    Ok(())
}

fn close_confirm_popup(
    conn: &impl Connection,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(popup) = app.confirm.take() {
        conn.ungrab_keyboard(0u32)?;
        conn.ungrab_pointer(0u32)?;
        conn.free_pixmap(popup.pixmap)?;
        conn.destroy_window(popup.window)?;
        conn.flush()?;
    }
    Ok(())
}

fn draw_confirm_popup(
    conn: &impl Connection,
    renderer: &Renderer,
    app: &App,
) -> Result<(), Box<dyn std::error::Error>> {
    let popup = match app.confirm.as_ref() {
        Some(p) => p,
        None => return Ok(()),
    };
    let pix = popup.pixmap;
    let gc = renderer.gc;

    // Reset GC bg too — image_text8 paints each cell with the GC bg, and the
    // rename overlay can leave bg = selection_bg_pixel. Without this, the
    // popup text would render with the wrong cell background.
    conn.change_gc(
        gc,
        &ChangeGCAux::new().foreground(renderer.menu_bg_pixel).background(renderer.menu_bg_pixel),
    )?;
    conn.poly_fill_rectangle(
        pix,
        gc,
        &[Rectangle { x: 0, y: 0, width: popup.width, height: popup.height }],
    )?;

    // Message text — centred horizontally on the upper line.
    let max_chars = ((popup.width as i16 - CONFIRM_PADDING * 2) / CHAR_WIDTH).max(0) as usize;
    let display: String = popup.message.chars().take(max_chars).collect();
    let text_width = display.len() as i16 * CHAR_WIDTH;
    let text_x = (popup.width as i16 - text_width) / 2;
    let text_y = CONFIRM_PADDING + ITEM_H as i16 / 2 + 4;
    conn.change_gc(gc, &ChangeGCAux::new().foreground(renderer.text_pixel))?;
    if !display.is_empty() {
        conn.image_text8(pix, gc, text_x, text_y, display.as_bytes())?;
    }

    // Buttons — Yes / No.
    for (rect, label, button) in [
        (&popup.yes_rect, "Yes", ConfirmButton::Yes),
        (&popup.no_rect, "No", ConfirmButton::No),
    ] {
        let bg = if popup.hover_button == Some(button) {
            renderer.menu_hover_pixel
        } else {
            renderer.item_pixel
        };
        conn.change_gc(gc, &ChangeGCAux::new().foreground(bg))?;
        conn.poly_fill_rectangle(pix, gc, &[*rect])?;
        // 1px border so buttons are visible even without hover.
        conn.change_gc(gc, &ChangeGCAux::new().foreground(renderer.menu_border_pixel))?;
        conn.poly_rectangle(pix, gc, &[Rectangle {
            x: rect.x,
            y: rect.y,
            width: rect.width.saturating_sub(1),
            height: rect.height.saturating_sub(1),
        }])?;

        let lbl_w = label.len() as i16 * CHAR_WIDTH;
        let lx = rect.x + (rect.width as i16 - lbl_w) / 2;
        let ly = rect.y + (rect.height as i16 / 2) + 4;
        conn.change_gc(gc, &ChangeGCAux::new().foreground(renderer.text_pixel))?;
        conn.image_text8(pix, gc, lx, ly, label.as_bytes())?;
    }

    conn.copy_area(pix, popup.window, gc, 0, 0, 0, 0, popup.width, popup.height)?;
    conn.flush()?;
    Ok(())
}

// ── Keyboard helpers ──

fn keysym_from_keycode(
    conn: &impl Connection,
    keycode: u8,
    state: KeyButMask,
) -> Result<u32, Box<dyn std::error::Error>> {
    let setup = conn.setup();
    let min_kc = setup.min_keycode;
    let max_kc = setup.max_keycode;
    let reply = conn
        .get_keyboard_mapping(min_kc, max_kc - min_kc + 1)?
        .reply()?;
    let syms_per_kc = reply.keysyms_per_keycode as usize;
    let offset = (keycode - min_kc) as usize * syms_per_kc;
    if offset >= reply.keysyms.len() {
        return Ok(0);
    }
    let row_end = (offset + syms_per_kc).min(reply.keysyms.len());
    Ok(select_keysym(&reply.keysyms[offset..row_end], u16::from(state)))
}

/// Pure keysym-column picker. Given a single keycode's row of keysyms and
/// the X11 modifier state, return the keysym we should act on.
///
/// Column 0 = unshifted, column 1 = shifted. Per X11 protocol semantics, if
/// the shifted column is NoSymbol (0) — which is typical for non-printing
/// keys like arrows where Shift+Left has no distinct keysym — fall back to
/// the unshifted symbol so callers see the expected key.
fn select_keysym(row: &[u32], state: u16) -> u32 {
    if row.is_empty() {
        return 0;
    }
    let unshifted = row[0];
    let shift_held = state & 1 != 0; // X11 ShiftMask
    if shift_held {
        let shifted_sym = row.get(1).copied().unwrap_or(0);
        if shifted_sym != 0 {
            return shifted_sym;
        }
    }
    unshifted
}

fn printable_char_from_sym(sym: u32) -> Option<char> {
    // Latin-1 range: keysym 0x20..0xff maps directly to Unicode
    if (0x20..=0x7e).contains(&sym) || (0xa0..=0xff).contains(&sym) {
        char::from_u32(sym)
    } else {
        None
    }
}

// ── Persistence: dirty/save policy ──

/// How long to wait after the last mutation before debouncing a save out.
/// Short bursts of edits coalesce into one write; this is the lower bound
/// on the post-burst idle period before disk activity.
const SAVE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(250);

/// Hard upper bound on time between mutation and save. If the user keeps
/// mutating fast enough that the debounce window never closes, the backstop
/// fires anyway so worst-case data loss is bounded.
const SAVE_BACKSTOP: std::time::Duration = std::time::Duration::from_secs(30);

/// Pure debounce/backstop check. Returns true when the dirty state has
/// satisfied either the post-edit idle window OR the absolute backstop.
fn should_save_now(
    first: Option<std::time::Instant>,
    last: Option<std::time::Instant>,
    now: std::time::Instant,
) -> bool {
    let (Some(first), Some(last)) = (first, last) else {
        return false;
    };
    if now.saturating_duration_since(last) >= SAVE_DEBOUNCE {
        return true;
    }
    if now.saturating_duration_since(first) >= SAVE_BACKSTOP {
        return true;
    }
    false
}

// ── Persistence: paths, atomic writes, legacy migration ──

/// The data directory for the active profile. v1 hard-codes the profile name
/// to "default"; the directory layout already supports a future `--profile`
/// flag with no migration cost.
fn data_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    data_dir_in(std::path::Path::new(&home))
}

fn data_dir_in(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".config").join("ptm").join("profiles").join("default")
}

/// The pre-Phase-2a data directory. v1 migrates files out of this once.
fn legacy_data_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    legacy_data_dir_in(std::path::Path::new(&home))
}

fn legacy_data_dir_in(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".config").join("ptm")
}

/// Atomic-write `content` to `path`. Writes to `<path>.tmp` first, then
/// renames over the destination. The rename is atomic on the same
/// filesystem, so a crash mid-write leaves the previous file intact (or
/// absent if there was none) — never a partial new file. Any pre-existing
/// `.tmp` from a prior crash is overwritten cleanly.
fn write_atomic(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(content)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Move legacy `groups` and `geometry` files from the old data dir to the
/// new one, when the new files don't already exist. Idempotent; safe to call
/// every startup (the second invocation is a no-op).
fn migrate_legacy_files(legacy: &std::path::Path, new: &std::path::Path) {
    for filename in ["groups", "geometry"] {
        let from = legacy.join(filename);
        let to = new.join(filename);
        if from.exists() && !to.exists() {
            if let Some(parent) = to.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::rename(&from, &to);
        }
    }
}

// ── Geometry persistence ──

fn geometry_path() -> std::path::PathBuf {
    data_dir().join("geometry")
}

fn save_geometry(x: i16, y: i16, w: u16, h: u16) {
    let path = geometry_path();
    let content = format!("{} {} {} {}\n", x, y, w, h);
    let _ = write_atomic(&path, content.as_bytes());
}

fn load_geometry() -> Option<(i16, i16, u16, u16)> {
    let data = std::fs::read_to_string(geometry_path()).ok()?;
    let parts: Vec<&str> = data.trim().split_whitespace().collect();
    if parts.len() != 4 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
        parts[3].parse().ok()?,
    ))
}

// ── Group persistence ──

struct SavedMember {
    label: String,
    wm_class: String,
    custom_prefix: String,
}

struct SavedGroup {
    name: String,
    collapsed: bool,
    kind: GroupKind,
    members: Vec<SavedMember>,
}

fn groups_path() -> std::path::PathBuf {
    data_dir().join("groups")
}

fn extract_saved_state(app: &App) -> Vec<SavedGroup> {
    let mut saved = Vec::new();
    for slot in &app.display_order {
        if let DisplaySlot::Group(gid) = slot {
            if let Some(group) = app.groups.iter().find(|g| g.id == *gid) {
                // Serialize ALL members (live and ghost) — that's the
                // whole point of Phase 2c: a group with only ghost
                // members must still round-trip across PTM restarts.
                let members = group
                    .members
                    .iter()
                    .map(|m| SavedMember {
                        label: m.label.clone(),
                        wm_class: m.wm_class.clone(),
                        custom_prefix: m.custom_prefix.clone(),
                    })
                    .collect();
                saved.push(SavedGroup {
                    name: group.name.clone(),
                    collapsed: group.collapsed,
                    kind: group.kind,
                    members,
                });
            }
        }
    }
    saved
}

fn save_groups_to(path: &std::path::Path, groups: &[SavedGroup]) {
    let mut buf = String::new();
    buf.push_str("v1\n");
    for group in groups {
        let collapsed = if group.collapsed { "1" } else { "0" };
        let kind = match group.kind {
            GroupKind::Normal => "normal",
            GroupKind::TmuxSystem => "tmux_system",
        };
        // Always emit 4-field GROUP so on-disk format stays predictable.
        // Loader (T4.4a) tolerates the legacy 3-field shape, which is what
        // makes downgrade safe.
        buf.push_str(&format!("GROUP\t{}\t{}\t{}\n", group.name, collapsed, kind));
        for member in &group.members {
            buf.push_str(&format!(
                "MEMBER\t{}\t{}\t{}\n",
                member.label, member.wm_class, member.custom_prefix
            ));
        }
    }
    let _ = write_atomic(path, buf.as_bytes());
}

fn save_groups(app: &App) {
    let groups = extract_saved_state(app);
    save_groups_to(&groups_path(), &groups);
}

fn load_groups_from(path: &std::path::Path) -> Option<Vec<SavedGroup>> {
    let data = std::fs::read_to_string(path).ok()?;
    let mut lines = data.lines();
    if lines.next()? != "v1" {
        return None;
    }
    let mut groups: Vec<SavedGroup> = Vec::new();
    for line in lines {
        let parts: Vec<&str> = line.split('\t').collect();
        match parts.first() {
            Some(&"GROUP") => {
                // 3-field GROUP lines remain valid (downgrade-friendly); the
                // optional 4th field carries `kind`. Reject any other arity
                // so corrupt files fail loudly rather than partially-loading.
                if !matches!(parts.len(), 3 | 4) {
                    return None;
                }
                let collapsed = match parts[2] {
                    "1" => true,
                    "0" => false,
                    _ => return None,
                };
                let kind = if parts.len() == 4 {
                    match parts[3] {
                        "normal" => GroupKind::Normal,
                        "tmux_system" => GroupKind::TmuxSystem,
                        // Unknown kind = corrupt or future-version data.
                        // Fail the whole load rather than silently fall back —
                        // partial-loads are how data gets quietly mangled.
                        _ => return None,
                    }
                } else {
                    GroupKind::Normal
                };
                groups.push(SavedGroup {
                    name: parts[1].to_string(),
                    collapsed,
                    kind,
                    members: Vec::new(),
                });
            }
            Some(&"MEMBER") => {
                if parts.len() != 4 {
                    return None;
                }
                if groups.is_empty() {
                    return None;
                }
                groups.last_mut().unwrap().members.push(SavedMember {
                    label: parts[1].to_string(),
                    wm_class: parts[2].to_string(),
                    custom_prefix: parts[3].to_string(),
                });
            }
            _ => return None,
        }
    }
    Some(groups)
}

fn load_groups() -> Option<Vec<SavedGroup>> {
    load_groups_from(&groups_path())
}

fn restore_groups(app: &mut App, saved: &[SavedGroup]) {
    let available: Vec<(String, String, u32)> = app
        .items
        .iter()
        .map(|item| (item.label.clone(), item.wm_class.clone(), item.wid))
        .collect();
    let mut claimed: HashSet<u32> = HashSet::new();

    for sg in saved {
        // Phase 2c: ALWAYS construct the group, even if no members match
        // any current window. Unmatched members are kept as ghosts so the
        // group survives PTM restarts where its windows aren't yet up.
        let mut members: Vec<GroupMember> = Vec::new();
        for sm in &sg.members {
            // Matching cascade (Phase 2c + 2d):
            //   1. exact (label, wm_class)
            //   2. label-only (titles often match across restarts)
            //   3. wm_class-only (terminals churn their titles every command,
            //      so without this their groups would re-shatter every restart)
            let exact = available.iter().find(|(l, c, w)| {
                l == &sm.label && c == &sm.wm_class && !claimed.contains(w)
            });
            let matched = exact
                .or_else(|| {
                    available
                        .iter()
                        .find(|(l, _, w)| l == &sm.label && !claimed.contains(w))
                })
                .or_else(|| {
                    available
                        .iter()
                        .find(|(_, c, w)| c == &sm.wm_class && !claimed.contains(w))
                });
            let live_wid = matched.map(|(_, _, w)| {
                claimed.insert(*w);
                *w
            });
            // Restore custom_prefix on matched items
            if let Some(wid) = live_wid {
                if !sm.custom_prefix.is_empty() {
                    if let Some(item) = app.items.iter_mut().find(|i| i.wid == wid) {
                        item.custom_prefix = sm.custom_prefix.clone();
                    }
                }
            }
            members.push(GroupMember {
                label: sm.label.clone(),
                wm_class: sm.wm_class.clone(),
                custom_prefix: sm.custom_prefix.clone(),
                live_wid,
            });
        }

        let gid = app.next_group_id;
        app.next_group_id += 1;
        app.groups.push(Group {
            id: gid,
            name: sg.name.clone(),
            collapsed: sg.collapsed,
            kind: sg.kind,
            members,
        });
    }

    // Rebuild display_order: groups first (in saved order), then ungrouped
    let mut new_order: Vec<DisplaySlot> = Vec::new();
    for group in &app.groups {
        new_order.push(DisplaySlot::Group(group.id));
    }
    for slot in &app.display_order {
        if let DisplaySlot::Window(wid) = slot {
            if !claimed.contains(wid) {
                new_order.push(DisplaySlot::Window(*wid));
            }
        }
    }
    app.display_order = new_order;
    app.build_display_rows();
}

#[cfg(test)]
fn geometry_path_in(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("geometry")
}

#[cfg(test)]
fn save_geometry_to(path: &std::path::Path, x: i16, y: i16, w: u16, h: u16) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::File::create(path) {
        let _ = write!(f, "{} {} {} {}\n", x, y, w, h);
    }
}

#[cfg(test)]
fn load_geometry_from(path: &std::path::Path) -> Option<(i16, i16, u16, u16)> {
    let data = std::fs::read_to_string(path).ok()?;
    let parts: Vec<&str> = data.trim().split_whitespace().collect();
    if parts.len() != 4 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
        parts[3].parse().ok()?,
    ))
}

// ── Main ──

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let colormap = screen.default_colormap;

    let atoms = Atoms::new(&conn)?;

    // Subscribe to property changes on the root window so we're notified
    // when _NET_CLIENT_LIST, _NET_ACTIVE_WINDOW, etc. change — this is how
    // PTM learns about new/closed/focused windows without polling.
    conn.change_window_attributes(
        root,
        &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?;

    let window: Window = conn.generate_id()?;
    let event_mask = EventMask::BUTTON_PRESS
        | EventMask::BUTTON_RELEASE
        | EventMask::POINTER_MOTION
        | EventMask::EXPOSURE
        | EventMask::STRUCTURE_NOTIFY
        | EventMask::LEAVE_WINDOW
        | EventMask::KEY_PRESS;

    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        window,
        root,
        0,
        0,
        WIN_W,
        WIN_H,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .background_pixel(screen.black_pixel)
            .event_mask(event_mask),
    )?;

    conn.change_property8(
        PropMode::REPLACE,
        window,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        b"ptm",
    )?;

    // Register WM_DELETE_WINDOW so the WM sends ClientMessage instead of killing us
    conn.change_property32(
        PropMode::REPLACE,
        window,
        atoms.wm_protocols,
        AtomEnum::ATOM,
        &[atoms.wm_delete_window],
    )?;

    let mut app = App::new(window);
    let mut renderer = Renderer::new(&conn, screen, window)?;

    conn.map_window(window)?;
    conn.flush()?;

    // One-time migration: move pre-Phase-2a files (~/.config/ptm/{groups,
    // geometry}) into the new profile-aware location
    // (~/.config/ptm/profiles/default/) before any load happens. Idempotent.
    migrate_legacy_files(&legacy_data_dir(), &data_dir());

    // Restore saved geometry (position + size) from previous session
    if let Some((x, y, w, h)) = load_geometry() {
        conn.configure_window(
            window,
            &ConfigureWindowAux::new()
                .x(x as i32)
                .y(y as i32)
                .width(w as u32)
                .height(h as u32),
        )?;
        conn.flush()?;
        app.x = x;
        app.y = y;
        app.width = w;
        app.height = h;
        renderer.resize(&conn, w, h)?;
    }

    refresh_items(&conn, root, &atoms, &mut app, colormap)?;

    if let Some(saved) = load_groups() {
        restore_groups(&mut app, &saved);
    }

    // Probe tmux once at startup; cache for the renderer / hit-test path.
    app.tmux_available = is_tmux_available();

    // Auto-create the TmuxSystem group on first run when tmux is installed.
    // Idempotent — subsequent runs find the restored group and short-circuit.
    // Re-run refresh so derived members populate before the first paint.
    if app.tmux_available {
        ensure_tmux_system_group(&mut app);
        refresh_items(&conn, root, &atoms, &mut app, colormap)?;
    }

    // Background thread that pokes the main loop every 5 s so tmux state
    // changes (sessions created or destroyed outside PTM) show up promptly.
    spawn_tmux_poll_thread(window, atoms.ptm_wake, std::time::Duration::from_secs(5));

    // Save-tick thread: pings the main loop every 250 ms so the dirty-flag
    // debounce can fire even during pure-idle periods. Distinct atom from
    // the tmux poll so we don't pay tmux-list-sessions cost 4x/s.
    spawn_save_tick_thread(
        window,
        atoms.ptm_save_tick,
        std::time::Duration::from_millis(250),
    );

    loop {
        let event = conn.wait_for_event()?;

        // Handle WM_DELETE_WINDOW and our own wake pings in any mode.
        if let Event::ClientMessage(ev) = &event {
            if ev.window == window && ev.data.as_data32()[0] == atoms.wm_delete_window {
                save_geometry(app.x, app.y, app.width, app.height);
                save_groups(&app);
                app.clear_dirty();
                break;
            }
            if ev.type_ == atoms.ptm_save_tick {
                // Cheap idle tick: just check whether the dirty-flag debounce
                // has elapsed. Skip during user gestures so a drag/rename
                // bursting through a save tick doesn't write a half-state.
                if app.drag.is_none() && app.rename.is_none() && app.confirm.is_none() {
                    let now = std::time::Instant::now();
                    if should_save_now(app.first_dirty_at, app.last_dirty_at, now) {
                        save_groups(&app);
                        save_geometry(app.x, app.y, app.width, app.height);
                        app.clear_dirty();
                    }
                }
                // T3.5: while a post-drop highlight is mid-fade, request a
                // redraw on every save tick so the fade visibly progresses
                // and clears once the duration expires.
                if let Some((_, when)) = app.last_drop_highlight {
                    if when.elapsed() < DROP_HIGHLIGHT_DURATION + std::time::Duration::from_millis(250) {
                        renderer.redraw(&conn, &app)?;
                    } else {
                        // Past the visible window — clear so it doesn't
                        // accumulate across many drops.
                        app.last_drop_highlight = None;
                    }
                }
                continue;
            }
            if ev.type_ == atoms.ptm_wake {
                // Scheduled tmux-state poll from the background thread.
                // Same gating as PropertyNotify refresh: don't rebuild while
                // the user is mid-gesture.
                if app.drag.is_none()
                    && app.context_menu.is_none()
                    && app.rename.is_none()
                    && app.confirm.is_none()
                {
                    refresh_items(&conn, root, &atoms, &mut app, colormap)?;
                    renderer.redraw(&conn, &app)?;
                }
                continue;
            }
        }

        // Event-driven refresh: react to PropertyNotify before mode dispatch.
        // Handled here so menu/rename/drag states stay consistent.
        if let Event::PropertyNotify(ev) = &event {
            let action = classify_property_event(
                ev.atom,
                ev.window == root,
                atoms.net_client_list,
                atoms.net_active_window,
                atoms.net_wm_name,
            );
            match action {
                PropertyAction::RefreshClientList => {
                    // Skip full refresh while the user is mid-gesture — the next
                    // PropertyNotify (or the gesture's completion) will catch up.
                    if app.drag.is_none()
                        && app.context_menu.is_none()
                        && app.rename.is_none()
                        && app.confirm.is_none()
                    {
                        refresh_items(&conn, root, &atoms, &mut app, colormap)?;
                        renderer.redraw(&conn, &app)?;
                    }
                }
                PropertyAction::UpdateActiveWindow => {
                    app.active_wid =
                        get_active_window(&conn, root, &atoms).unwrap_or(None);
                    if app.context_menu.is_none() && app.rename.is_none() && app.confirm.is_none() {
                        renderer.redraw(&conn, &app)?;
                    }
                }
                PropertyAction::UpdateWindowTitle => {
                    if let Some(item) = app.items.iter_mut().find(|i| i.wid == ev.window) {
                        if let Ok(t) = get_window_title(&conn, ev.window, &atoms) {
                            if !t.is_empty() {
                                item.label = t;
                            }
                        }
                    }
                    if app.drag.is_none()
                        && app.context_menu.is_none()
                        && app.rename.is_none()
                        && app.confirm.is_none()
                    {
                        renderer.redraw(&conn, &app)?;
                    }
                }
                PropertyAction::Ignore => {}
            }
            continue;
        }

        // ── Context menu mode (pointer grab routes events here) ──
        if app.context_menu.is_some() {
            match event {
                Event::ButtonPress(ev) => {
                    let menu = app.context_menu.as_ref().unwrap();
                    let in_menu = ev.event_x >= 0
                        && ev.event_y >= 0
                        && (ev.event_x as u16) < menu.width
                        && (ev.event_y as u16) < menu.height;
                    if in_menu && ev.detail == 1 {
                        let idx = ((ev.event_y - MENU_PADDING) as usize) / MENU_ITEM_H as usize;
                        if idx < menu.entries.len() {
                            let action = menu.entries[idx].action.clone();
                            let target_row = menu.target_row;
                            let menu_x = menu.x;
                            let menu_y = menu.y;
                            close_context_menu(&conn, &mut app)?;
                            let follow_up =
                                execute_menu_action(&mut app, action, target_row);
                            if let Some(req) = follow_up {
                                open_confirm_popup(
                                    &conn,
                                    screen,
                                    &renderer,
                                    &mut app,
                                    req.message,
                                    req.action,
                                    menu_x,
                                    menu_y,
                                )?;
                            }
                        } else {
                            close_context_menu(&conn, &mut app)?;
                        }
                    } else {
                        close_context_menu(&conn, &mut app)?;
                    }
                    renderer.redraw(&conn, &app)?;
                }
                Event::MotionNotify(ev) => {
                    if let Some(ref mut menu) = app.context_menu {
                        let in_menu = ev.event_x >= 0
                            && ev.event_y >= 0
                            && (ev.event_x as u16) < menu.width
                            && (ev.event_y as u16) < menu.height;
                        let new_hover = if in_menu {
                            let idx =
                                ((ev.event_y - MENU_PADDING) as usize) / MENU_ITEM_H as usize;
                            if idx < menu.entries.len() {
                                Some(idx)
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        if new_hover != menu.hover_index {
                            menu.hover_index = new_hover;
                            draw_context_menu(&conn, &renderer, &app)?;
                        }
                    }
                }
                Event::Expose(ev) if ev.count == 0 => {
                    if Some(ev.window) == app.context_menu.as_ref().map(|m| m.window) {
                        draw_context_menu(&conn, &renderer, &app)?;
                    }
                    if ev.window == window {
                        renderer.redraw(&conn, &app)?;
                    }
                }
                _ => {}
            }
            continue;
        }

        // ── Confirmation popup mode (pointer + keyboard grab routes here) ──
        if app.confirm.is_some() {
            match event {
                Event::ButtonPress(ev) => {
                    let popup = app.confirm.as_ref().unwrap();
                    let in_yes = point_in_rect(ev.event_x, ev.event_y, &popup.yes_rect);
                    let in_no = point_in_rect(ev.event_x, ev.event_y, &popup.no_rect);
                    let in_popup = ev.event_x >= 0
                        && ev.event_y >= 0
                        && (ev.event_x as u16) < popup.width
                        && (ev.event_y as u16) < popup.height;
                    if ev.detail == 1 && in_yes {
                        let action = dispatch_confirm(&app, true);
                        close_confirm_popup(&conn, &mut app)?;
                        if let Some(action) = action {
                            execute_confirm_action(&mut app, action);
                        }
                    } else if ev.detail == 1 && in_no {
                        close_confirm_popup(&conn, &mut app)?;
                    } else if !in_popup {
                        // Click outside dismisses (treated as cancel).
                        close_confirm_popup(&conn, &mut app)?;
                    }
                    renderer.redraw(&conn, &app)?;
                }
                Event::MotionNotify(ev) => {
                    if let Some(ref mut popup) = app.confirm {
                        let new_hover = if point_in_rect(ev.event_x, ev.event_y, &popup.yes_rect) {
                            Some(ConfirmButton::Yes)
                        } else if point_in_rect(ev.event_x, ev.event_y, &popup.no_rect) {
                            Some(ConfirmButton::No)
                        } else {
                            None
                        };
                        if new_hover != popup.hover_button {
                            popup.hover_button = new_hover;
                            draw_confirm_popup(&conn, &renderer, &app)?;
                        }
                    }
                }
                Event::KeyPress(ev) => {
                    let sym = keysym_from_keycode(&conn, ev.detail, ev.state)?;
                    // Accept: Enter / KP_Enter / Y / y. Cancel: Esc / N / n.
                    let accept = matches!(sym, 0xff0d | 0xff8d | 0x59 | 0x79);
                    let cancel = matches!(sym, 0xff1b | 0x4e | 0x6e);
                    if accept {
                        let action = dispatch_confirm(&app, true);
                        close_confirm_popup(&conn, &mut app)?;
                        if let Some(action) = action {
                            execute_confirm_action(&mut app, action);
                        }
                        renderer.redraw(&conn, &app)?;
                    } else if cancel {
                        close_confirm_popup(&conn, &mut app)?;
                        renderer.redraw(&conn, &app)?;
                    }
                }
                Event::Expose(ev) if ev.count == 0 => {
                    if Some(ev.window) == app.confirm.as_ref().map(|p| p.window) {
                        draw_confirm_popup(&conn, &renderer, &app)?;
                    }
                    if ev.window == window {
                        renderer.redraw(&conn, &app)?;
                    }
                }
                _ => {}
            }
            continue;
        }

        // ── Rename mode (inline text editing) ──
        if app.rename.is_some() {
            match event {
                Event::KeyPress(ev) => {
                    let sym = keysym_from_keycode(&conn, ev.detail, ev.state)?;
                    let shift = u16::from(ev.state) & 0x01 != 0;
                    let ctrl = u16::from(ev.state) & 0x04 != 0;
                    match sym {
                        0xff0d | 0xff8d => {
                            // Return / KP_Enter → commit
                            app.commit_rename();
                        }
                        0xff1b => {
                            // Escape → cancel
                            app.cancel_rename();
                        }
                        0xff08 => {
                            // Backspace
                            if let Some(ref mut rs) = app.rename {
                                if ctrl { rs.delete_word_left(); } else { rs.delete_back_char(); }
                            }
                        }
                        0xffff => {
                            // Delete
                            if let Some(ref mut rs) = app.rename {
                                if ctrl { rs.delete_word_right(); } else { rs.delete_forward_char(); }
                            }
                        }
                        0xff51 => {
                            // Left arrow
                            if let Some(ref mut rs) = app.rename {
                                if ctrl { rs.move_left_word(shift); } else { rs.move_left_char(shift); }
                            }
                        }
                        0xff53 => {
                            // Right arrow
                            if let Some(ref mut rs) = app.rename {
                                if ctrl { rs.move_right_word(shift); } else { rs.move_right_char(shift); }
                            }
                        }
                        0xff50 => {
                            // Home
                            if let Some(ref mut rs) = app.rename {
                                rs.move_home(shift);
                            }
                        }
                        0xff57 => {
                            // End
                            if let Some(ref mut rs) = app.rename {
                                rs.move_end(shift);
                            }
                        }
                        // Ctrl+A → select all. Match keysym 'a' (0x61) only;
                        // when ctrl+shift is held the keysym from
                        // keysym_from_keycode comes back as 'A' (0x41), so
                        // accept either to match user intent.
                        0x61 | 0x41 if ctrl => {
                            if let Some(ref mut rs) = app.rename {
                                rs.select_all();
                            }
                        }
                        _ => {
                            // Printable character — but ignore when ctrl is held
                            // so Ctrl+<other letter> doesn't accidentally type.
                            if !ctrl {
                                if let Some(ch) = printable_char_from_sym(sym) {
                                    if let Some(ref mut rs) = app.rename {
                                        rs.insert_char(ch);
                                    }
                                }
                            }
                        }
                    }
                    renderer.redraw(&conn, &app)?;
                }
                Event::ButtonPress(ev) if ev.event == window => {
                    // Click outside the rename row → commit
                    app.commit_rename();
                    renderer.redraw(&conn, &app)?;
                }
                Event::Expose(ev) if ev.count == 0 && ev.window == window => {
                    renderer.redraw(&conn, &app)?;
                }
                Event::ConfigureNotify(ev) if ev.window == window => {
                    app.x = ev.x;
                    app.y = ev.y;
                    let (new_w, new_h) = (ev.width, ev.height);
                    if new_w != app.width || new_h != app.height {
                        app.width = new_w;
                        app.height = new_h;
                        renderer.resize(&conn, new_w, new_h)?;
                        renderer.redraw(&conn, &app)?;
                    }
                }
                _ => {}
            }
            continue;
        }

        // ── Normal mode ──
        match event {
            Event::Expose(ev) if ev.count == 0 && ev.window == window => {
                renderer.redraw(&conn, &app)?;
            }
            Event::ConfigureNotify(ev) if ev.window == window => {
                app.x = ev.x;
                app.y = ev.y;
                let (new_w, new_h) = (ev.width, ev.height);
                if new_w != app.width || new_h != app.height {
                    app.width = new_w;
                    app.height = new_h;
                    renderer.resize(&conn, new_w, new_h)?;
                    renderer.redraw(&conn, &app)?;
                }
            }
            Event::ButtonPress(ev) if ev.event == window => {
                match ev.detail {
                    1 => {
                        if let Some(button) = app.hit_test_top_buttons(ev.event_x, ev.event_y) {
                            match button {
                                TopButton::NewTerminal => {
                                    // Register a Terminal pending_spawn BEFORE
                                    // forking so the next refresh's wid-delta
                                    // grabs and snaps the new window.
                                    app.pending_spawn = Some(PendingSpawn {
                                        kind: PendingSpawnKind::Terminal,
                                        spawned_at: std::time::Instant::now(),
                                    });
                                    spawn_default_terminal();
                                }
                                TopButton::NewTmux => {
                                    if let Some(name) = create_new_tmux_session() {
                                        // Register Attach BEFORE the terminal
                                        // forks so the resulting window is
                                        // bound to its session AND snapped.
                                        app.pending_spawn = Some(PendingSpawn {
                                            kind: PendingSpawnKind::Attach(name.clone()),
                                            spawned_at: std::time::Instant::now(),
                                        });
                                        spawn_attach_terminal(&name);
                                    }
                                    // Wake the main loop so the new session
                                    // shows up immediately rather than waiting
                                    // for the next 5s tmux poll.
                                    poke_self(&conn, window, atoms.ptm_wake);
                                }
                            }
                        } else if let Some(row) = app.hit_test_row(ev.event_y) {
                            app.drag = Some(DragState {
                                source_row: row,
                                start_x: ev.event_x,
                                start_y: ev.event_y,
                                current_y: ev.event_y,
                                started: false,
                            });
                        }
                    }
                    3 => {
                        if app.drag.is_none() && !app.hit_test_header_button(ev.event_y) {
                            if let Some(row) = app.hit_test_row(ev.event_y) {
                                open_context_menu(
                                    &conn,
                                    screen,
                                    &renderer,
                                    &mut app,
                                    row,
                                    ev.root_x,
                                    ev.root_y,
                                )?;
                            }
                        }
                    }
                    _ => {}
                }
                renderer.redraw(&conn, &app)?;
            }
            Event::MotionNotify(ev) if ev.event == window => {
                let mut needs_redraw = false;
                if let Some(ref mut drag) = app.drag {
                    drag.current_y = ev.event_y;
                    if !drag.started
                        && (drag.current_y - drag.start_y).abs() > DRAG_THRESHOLD
                    {
                        drag.started = true;
                    }
                    needs_redraw = true;

                    // Drain queued motion events during drag
                    while let Some(queued) = conn.poll_for_event()? {
                        if let Event::MotionNotify(mn) = queued {
                            if let Some(ref mut drag) = app.drag {
                                drag.current_y = mn.event_y;
                            }
                        } else {
                            match queued {
                                Event::ButtonRelease(br)
                                    if br.detail == 1 && br.event == window =>
                                {
                                    if let Some(req) =
                                        handle_release(&conn, root, &atoms, &mut app)
                                    {
                                        open_confirm_popup(
                                            &conn,
                                            screen,
                                            &renderer,
                                            &mut app,
                                            req.message,
                                            req.action,
                                            br.root_x,
                                            br.root_y,
                                        )?;
                                    }
                                    needs_redraw = true;
                                }
                                Event::Expose(ex) if ex.count == 0 => {
                                    needs_redraw = true;
                                }
                                _ => {}
                            }
                            break;
                        }
                    }
                } else {
                    // Hover tracking (no drag active)
                    let new_hover = app.hit_test_row(ev.event_y);
                    let new_button = app.hit_test_top_buttons(ev.event_x, ev.event_y);
                    if new_hover != app.hover_row {
                        app.hover_row = new_hover;
                        needs_redraw = true;
                    }
                    if new_button != app.top_button_hover {
                        app.top_button_hover = new_button;
                        needs_redraw = true;
                    }
                    // Drain queued motion for hover too
                    while let Some(queued) = conn.poll_for_event()? {
                        if let Event::MotionNotify(mn) = queued {
                            let h = app.hit_test_row(mn.event_y);
                            let hb = app.hit_test_top_buttons(mn.event_x, mn.event_y);
                            if h != app.hover_row {
                                app.hover_row = h;
                                needs_redraw = true;
                            }
                            if hb != app.top_button_hover {
                                app.top_button_hover = hb;
                                needs_redraw = true;
                            }
                        } else {
                            // Non-motion event during hover drain — break and let main loop handle next
                            // We can't easily re-queue, so just handle common cases
                            match queued {
                                Event::Expose(ex) if ex.count == 0 => {
                                    needs_redraw = true;
                                }
                                _ => {}
                            }
                            break;
                        }
                    }
                }
                if needs_redraw {
                    renderer.redraw(&conn, &app)?;
                }
            }
            Event::LeaveNotify(_) => {
                let was_hovering = app.hover_row.is_some() || app.top_button_hover.is_some();
                app.hover_row = None;
                app.top_button_hover = None;
                if was_hovering {
                    renderer.redraw(&conn, &app)?;
                }
            }
            Event::ButtonRelease(ev) if ev.detail == 1 && ev.event == window => {
                if let Some(req) = handle_release(&conn, root, &atoms, &mut app) {
                    open_confirm_popup(
                        &conn,
                        screen,
                        &renderer,
                        &mut app,
                        req.message,
                        req.action,
                        ev.root_x,
                        ev.root_y,
                    )?;
                }
                renderer.redraw(&conn, &app)?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn handle_release(
    conn: &impl Connection,
    root: Window,
    atoms: &Atoms,
    app: &mut App,
) -> Option<ConfirmRequest> {
    let drag = app.drag.take()?;
    if drag.started {
        app.handle_drop(drag.source_row, drag.current_y);
        return None;
    }
    // Click (no drag)
    if drag.source_row >= app.display_rows.len() {
        return None;
    }
    let row = app.display_rows[drag.source_row].clone();
    match row {
        DisplayRow::GroupHeader { group_id } => {
            app.toggle_collapse(group_id);
            None
        }
        DisplayRow::Window { wid, .. } => {
            let _ = activate_window(conn, root, wid, atoms);
            app.active_wid = Some(wid);
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = snap_to_sidebar(conn, root, app.our_wid, wid, atoms);
            None
        }
        DisplayRow::Session { name, group_id } => {
            // Compute click x relative to the row's left edge so the close
            // band hit-test agrees with the rendered glyph position.
            let row_left = if group_id.is_some() {
                app.item_x() + GROUP_INDENT
            } else {
                app.item_x()
            };
            let row_w = if group_id.is_some() {
                (app.item_w() as i16 - GROUP_INDENT) as i16
            } else {
                app.item_w() as i16
            };
            let local_x = drag.start_x - row_left;
            if let Some(req) =
                dispatch_session_click(&name, group_id, local_x, row_w)
            {
                return Some(req);
            }
            if !is_attach_pending_for(&app.pending_spawn, &name) {
                app.pending_spawn = Some(PendingSpawn {
                    kind: PendingSpawnKind::Attach(name.clone()),
                    spawned_at: std::time::Instant::now(),
                });
                spawn_attach_terminal(&name);
            }
            None
        }
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> App {
        App::new(999) // dummy wid for our sidebar window
    }

    fn add_item(app: &mut App, wid: u32, label: &str) {
        app.items.push(Item {
            wid,
            label: label.to_string(),
            wm_class: "test".to_string(),
            accent_pixel: 0,
            custom_prefix: String::new(),
            session: None,
        });
        app.display_order.push(DisplaySlot::Window(wid));
    }

    // ── build_display_rows ──

    #[test]
    fn empty_state_produces_no_rows() {
        let mut app = make_app();
        app.build_display_rows();
        assert_eq!(app.display_rows.len(), 0);
    }

    #[test]
    fn ungrouped_windows_produce_flat_rows() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        add_item(&mut app, 3, "C");
        app.build_display_rows();

        assert_eq!(app.display_rows.len(), 3);
        assert!(matches!(app.display_rows[0], DisplayRow::Window { wid: 1, group_id: None }));
        assert!(matches!(app.display_rows[1], DisplayRow::Window { wid: 2, group_id: None }));
        assert!(matches!(app.display_rows[2], DisplayRow::Window { wid: 3, group_id: None }));
    }

    #[test]
    fn group_produces_header_plus_members() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1); // group containing window 1

        assert!(matches!(app.display_rows[0], DisplayRow::GroupHeader { group_id: 0 }));
        assert!(matches!(app.display_rows[1], DisplayRow::Window { wid: 1, group_id: Some(0) }));
        assert!(matches!(app.display_rows[2], DisplayRow::Window { wid: 2, group_id: None }));
    }

    #[test]
    fn collapsed_group_hides_members() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);
        app.toggle_collapse(0);

        assert_eq!(app.display_rows.len(), 2); // header + window B
        assert!(matches!(app.display_rows[0], DisplayRow::GroupHeader { group_id: 0 }));
        assert!(matches!(app.display_rows[1], DisplayRow::Window { wid: 2, group_id: None }));
    }

    #[test]
    fn expand_collapsed_group_shows_members() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);
        app.toggle_collapse(0); // collapse
        assert_eq!(app.display_rows.len(), 1);
        app.toggle_collapse(0); // expand
        assert_eq!(app.display_rows.len(), 2);
        assert!(matches!(app.display_rows[1], DisplayRow::Window { wid: 1, group_id: Some(0) }));
    }

    // ── Hit testing + responsive layout ──

    #[test]
    fn row_y_is_sequential() {
        let app = make_app();
        let y0 = app.row_y(0);
        let y1 = app.row_y(1);
        assert_eq!(y0, ITEM_Y_START);
        assert_eq!(y1, ITEM_Y_START + ITEM_H as i16 + ITEM_SPACING);
    }

    #[test]
    fn item_width_adapts_to_window_width() {
        let mut app = make_app();
        app.width = 250;
        assert_eq!(app.item_w(), 250 - ITEM_MARGIN as u16 * 2);

        app.width = 400;
        assert_eq!(app.item_w(), 400 - ITEM_MARGIN as u16 * 2);

        app.width = 100;
        assert_eq!(app.item_w(), 100 - ITEM_MARGIN as u16 * 2);
    }

    #[test]
    fn hit_test_returns_correct_row() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.build_display_rows();

        let mid_row0 = app.row_y(0) + ITEM_H as i16 / 2;
        assert_eq!(app.hit_test_row(mid_row0), Some(0));

        let mid_row1 = app.row_y(1) + ITEM_H as i16 / 2;
        assert_eq!(app.hit_test_row(mid_row1), Some(1));
    }

    #[test]
    fn hit_test_returns_none_in_gap() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.build_display_rows();

        // Below all items
        let below = app.row_y(0) + ITEM_H as i16 + 100;
        assert_eq!(app.hit_test_row(below), None);
        // Above all items
        assert_eq!(app.hit_test_row(0), None);
    }

    #[test]
    fn drop_index_at_top_is_zero() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.build_display_rows();

        assert_eq!(app.drop_index_from_y(0), 0);
    }

    #[test]
    fn drop_index_past_end() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.build_display_rows();

        assert_eq!(app.drop_index_from_y(9999), 1);
    }

    // ── Group operations ──

    #[test]
    fn create_group_replaces_window_slot() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);

        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].live_wids(), vec![1]);
        assert!(matches!(app.display_order[0], DisplaySlot::Group(0)));
        assert!(matches!(app.display_order[1], DisplaySlot::Window(2)));
    }

    #[test]
    fn add_to_group_moves_window() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);
        app.add_to_group(0, 2);

        assert_eq!(app.groups[0].live_wids(), vec![1, 2]);
        assert_eq!(app.display_order.len(), 1); // only the group slot remains
    }

    #[test]
    fn remove_from_group_inserts_after_group() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);
        app.add_to_group(0, 2);
        app.remove_from_group(1); // remove window 1

        assert_eq!(app.groups[0].live_wids(), vec![2]);
        assert_eq!(app.display_order.len(), 2);
        assert!(matches!(app.display_order[0], DisplaySlot::Group(0)));
        assert!(matches!(app.display_order[1], DisplaySlot::Window(1)));
    }

    #[test]
    fn delete_group_restores_members() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        add_item(&mut app, 3, "C");
        app.create_group(1);
        app.add_to_group(0, 2);
        app.delete_group(0);

        assert_eq!(app.groups.len(), 0);
        assert_eq!(app.display_order.len(), 3);
        // Members inserted where group was, then remaining window
        assert!(matches!(app.display_order[0], DisplaySlot::Window(1)));
        assert!(matches!(app.display_order[1], DisplaySlot::Window(2)));
        assert!(matches!(app.display_order[2], DisplaySlot::Window(3)));
    }

    #[test]
    fn rename_group_inline() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);
        assert_eq!(app.groups[0].name, "Group 1");

        // Start rename — populates RenameState with current name
        app.start_rename(0);
        assert!(app.rename.is_some());
        assert_eq!(app.rename.as_ref().unwrap().text, "Group 1");

        // Simulate typing: clear and type new name
        if let Some(ref mut rs) = app.rename {
            rs.text = "My Project".to_string();
            rs.cursor = rs.text.len();
        }

        // Commit
        app.commit_rename();
        assert!(app.rename.is_none());
        assert_eq!(app.groups[0].name, "My Project");
    }

    #[test]
    fn rename_cancel_preserves_old_name() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);

        app.start_rename(0);
        if let Some(ref mut rs) = app.rename {
            rs.text = "New Name".to_string();
        }
        app.cancel_rename();
        assert_eq!(app.groups[0].name, "Group 1");
    }

    #[test]
    fn rename_empty_string_preserves_old_name() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);

        app.start_rename(0);
        if let Some(ref mut rs) = app.rename {
            rs.text = "   ".to_string();
        }
        app.commit_rename();
        assert_eq!(app.groups[0].name, "Group 1"); // blank rejected
    }

    // ── Drag-and-drop ──

    #[test]
    fn is_gap_in_group_true_between_members() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        add_item(&mut app, 3, "C");
        app.create_group(1);
        app.add_to_group(0, 2);
        // display_rows: [Header(0), Window(1,g0), Window(2,g0), Window(3,None)]

        assert!(app.is_gap_in_group(1, 0)); // between header and first member
        assert!(app.is_gap_in_group(2, 0)); // between two members
        assert!(app.is_gap_in_group(3, 0)); // after last member
        assert!(!app.is_gap_in_group(0, 0)); // before header
    }

    #[test]
    fn reorder_ungrouped_windows() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        add_item(&mut app, 3, "C");
        app.build_display_rows();

        // Drag window at row 0 to gap 2 (between B and C)
        let y_gap2 = app.row_y(2); // just above row 2's midpoint
        app.handle_drop(0, y_gap2);

        // A moved from position 0 to after B
        assert!(matches!(app.display_order[0], DisplaySlot::Window(2)));
        assert!(matches!(app.display_order[1], DisplaySlot::Window(1)));
        assert!(matches!(app.display_order[2], DisplaySlot::Window(3)));
    }

    #[test]
    fn drag_window_onto_group_header_adds_to_group() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);
        // display_rows: [Header(0), Window(1,g0), Window(2,None)]

        // Drag window 2 (row 2) onto group header (row 0)
        let header_y = app.row_y(0) + ITEM_H as i16 / 2;
        app.handle_drop(2, header_y);

        // Stage G classifier: drop on header → JoinGroup at position 0,
        // so the dragged window inserts at the TOP of the group rather
        // than appending. (Pre-Stage G behaviour was append.)
        assert_eq!(app.groups[0].live_wids(), vec![2, 1]);
        assert_eq!(app.display_order.len(), 1); // only group remains
    }

    #[test]
    fn handle_drop_g2_regression_small_overshoot_still_in_group() {
        // G-2: dragging a window already in group X to the gap just past
        // the last member used to eject it from the group. With the Stage G
        // classifier, a small overshoot (still within the spacing below the
        // last member) keeps the window in the group.
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        add_item(&mut app, 3, "C");
        app.create_group(1);
        app.add_to_group(0, 2);
        // display_rows: [Header(0), Window(1,g0), Window(2,g0), Window(3,None)]

        // Drag B (row 2, in group) to y just past row 2's bottom — within the
        // ITEM_SPACING gap before row 3 starts. Pre-G this ejected B; post-G
        // it stays in group as a (no-op) reorder.
        let last_member_bottom = app.row_y(2) + ITEM_H as i16 + 1;
        app.handle_drop(2, last_member_bottom);

        assert_eq!(app.groups[0].live_wids(), vec![1, 2], "still in group");
        // Window 3 still ungrouped at row 3.
        assert!(matches!(
            app.display_order.last(),
            Some(DisplaySlot::Window(3))
        ));
    }

    #[test]
    fn handle_drop_sets_last_drop_highlight_on_success() {
        // T3.5 (G-5): a real (non-no-op) drop sets last_drop_highlight to
        // the moved wid, so the renderer can flash the destination row.
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.build_display_rows();
        // Drop B (row 1) above A (row 0)
        let above = app.row_y(0) - 5;
        app.handle_drop(1, above);
        let (wid, _) = app.last_drop_highlight.expect("highlight set");
        assert_eq!(wid, 2);
        assert!(app.drop_highlight_active_for(2));
        assert!(!app.drop_highlight_active_for(1));
    }

    #[test]
    fn handle_drop_does_not_set_highlight_on_noop() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);
        app.add_to_group(0, 2);
        app.last_drop_highlight = None;
        // Drop B onto itself (no-op)
        let on_self = app.row_y(2) + ITEM_H as i16 / 2;
        app.handle_drop(2, on_self);
        assert!(app.last_drop_highlight.is_none());
    }

    #[test]
    fn handle_drop_g4_regression_no_op_does_not_mutate() {
        // G-4: dropping at the same position the source already occupies
        // should be a no-op (no mutation, no mark_dirty). This prevents the
        // "bouncing" snap-back the user reported.
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);
        app.add_to_group(0, 2);
        // display_rows: [Header(0), Window(1,g0), Window(2,g0)]

        app.clear_dirty();
        // Drop window 2 (row 2) right at row 2's mid — should classify as
        // ReorderInGroup{gid:0,to:2} which equals sp+1=2 → no-op.
        let on_self = app.row_y(2) + ITEM_H as i16 / 2;
        app.handle_drop(2, on_self);

        assert_eq!(app.groups[0].live_wids(), vec![1, 2], "order preserved");
        assert!(!app.is_dirty(), "no-op should not mark dirty");
    }

    #[test]
    fn drag_grouped_window_outside_ungroups() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        add_item(&mut app, 3, "C");
        app.create_group(1);
        app.add_to_group(0, 2);
        // display_rows: [Header(0), Window(1,g0), Window(2,g0), Window(3,None)]

        // Drag window 1 (row 1) past end of list (well below everything)
        app.handle_drop(1, 9999);

        assert_eq!(app.groups[0].live_wids(), vec![2]); // only window 2 remains
        // Window 1 should now be ungrouped in display_order
        let ungrouped_wids: Vec<u32> = app
            .display_order
            .iter()
            .filter_map(|s| if let DisplaySlot::Window(w) = s { Some(*w) } else { None })
            .collect();
        assert!(ungrouped_wids.contains(&1));
    }

    // ── Menu entries ──

    #[test]
    fn menu_for_ungrouped_window_has_new_group() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.build_display_rows();

        let entries = build_menu_entries(&app, 0);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].label, "Rename Tab");
        assert_eq!(entries[1].label, "New Group");
    }

    #[test]
    fn menu_for_ungrouped_shows_existing_groups() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);
        // display_rows: [Header(0), Window(1,g0), Window(2,None)]

        let entries = build_menu_entries(&app, 2); // right-click on ungrouped window 2
        assert_eq!(entries.len(), 3); // "Rename Tab" + "New Group" + "Add to Group 1"
        assert!(matches!(entries[2].action, MenuAction::AddToGroup(0)));
    }

    #[test]
    fn menu_for_grouped_window_has_remove() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);
        // display_rows: [Header(0), Window(1,g0)]

        let entries = build_menu_entries(&app, 1);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].label, "Rename Tab");
        assert_eq!(entries[1].label, "Remove from Group");
    }

    #[test]
    fn menu_for_group_header_has_rename_and_delete() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);

        let entries = build_menu_entries(&app, 0); // group header
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].label, "Rename Group");
        assert_eq!(entries[1].label, "Delete Group");
    }

    // ── Tab rename ──

    #[test]
    fn display_label_without_prefix() {
        let mut app = make_app();
        add_item(&mut app, 1, "Firefox");
        assert_eq!(app.items[0].display_label(), "Firefox");
    }

    #[test]
    fn display_label_with_prefix() {
        let mut app = make_app();
        add_item(&mut app, 1, "steve@bambam: ~/dev");
        app.items[0].custom_prefix = "Dev Terminal".to_string();
        assert_eq!(
            app.items[0].display_label(),
            "Dev Terminal: steve@bambam: ~/dev"
        );
    }

    #[test]
    fn menu_for_ungrouped_window_has_rename_tab() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.build_display_rows();

        let entries = build_menu_entries(&app, 0);
        assert!(entries.iter().any(|e| e.label == "Rename Tab"));
    }

    #[test]
    fn menu_for_grouped_window_has_rename_tab() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);

        let entries = build_menu_entries(&app, 1); // window row inside group
        assert!(entries.iter().any(|e| e.label == "Rename Tab"));
    }

    #[test]
    fn start_tab_rename_initializes_with_prefix() {
        let mut app = make_app();
        add_item(&mut app, 1, "Firefox");
        app.items[0].custom_prefix = "Browser".to_string();
        app.build_display_rows();

        app.start_tab_rename(1);
        assert!(app.rename.is_some());
        let rs = app.rename.as_ref().unwrap();
        assert_eq!(rs.text, "Browser");
    }

    #[test]
    fn start_tab_rename_empty_prefix() {
        let mut app = make_app();
        add_item(&mut app, 1, "Firefox");
        app.build_display_rows();

        app.start_tab_rename(1);
        assert!(app.rename.is_some());
        let rs = app.rename.as_ref().unwrap();
        assert_eq!(rs.text, "");
    }

    #[test]
    fn commit_tab_rename_sets_prefix() {
        let mut app = make_app();
        add_item(&mut app, 1, "Firefox");
        app.build_display_rows();

        app.start_tab_rename(1);
        if let Some(ref mut rs) = app.rename {
            rs.text = "Browser".to_string();
            rs.cursor = rs.text.len();
        }
        app.commit_rename();
        assert_eq!(app.items[0].custom_prefix, "Browser");
    }

    #[test]
    fn commit_tab_rename_empty_clears_prefix() {
        let mut app = make_app();
        add_item(&mut app, 1, "Firefox");
        app.items[0].custom_prefix = "Browser".to_string();
        app.build_display_rows();

        app.start_tab_rename(1);
        if let Some(ref mut rs) = app.rename {
            rs.text = "   ".to_string(); // whitespace only
        }
        app.commit_rename();
        assert_eq!(app.items[0].custom_prefix, ""); // cleared
    }

    #[test]
    fn cancel_tab_rename_preserves_prefix() {
        let mut app = make_app();
        add_item(&mut app, 1, "Firefox");
        app.items[0].custom_prefix = "Browser".to_string();
        app.build_display_rows();

        app.start_tab_rename(1);
        if let Some(ref mut rs) = app.rename {
            rs.text = "Something else".to_string();
        }
        app.cancel_rename();
        assert_eq!(app.items[0].custom_prefix, "Browser"); // unchanged
    }

    // ── find_item ──

    #[test]
    fn find_item_by_wid() {
        let mut app = make_app();
        add_item(&mut app, 42, "Test Window");
        assert!(app.find_item(42).is_some());
        assert_eq!(app.find_item(42).unwrap().label, "Test Window");
        assert!(app.find_item(99).is_none());
    }

    // ── display_row_to_slot_position ──

    #[test]
    fn slot_position_maps_correctly() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        add_item(&mut app, 3, "C");
        app.create_group(1);
        app.add_to_group(0, 2);
        // display_order: [Group(0), Window(3)]
        // display_rows: [Header(0), Window(1,g0), Window(2,g0), Window(3,None)]

        assert_eq!(app.display_row_to_slot_position(0), 0); // before header → slot 0
        assert_eq!(app.display_row_to_slot_position(3), 1); // before Window(3) → slot 1
        assert_eq!(app.display_row_to_slot_position(4), 2); // past end → slot 2
    }

    // ── geometry persistence ──

    #[test]
    fn geometry_round_trip() {
        let dir = std::env::temp_dir().join("ptm_test_geom_rt");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = geometry_path_in(&dir);

        save_geometry_to(&path, 100, 200, 300, 400);
        let loaded = load_geometry_from(&path);
        assert_eq!(loaded, Some((100, 200, 300, 400)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn geometry_negative_coords() {
        let dir = std::env::temp_dir().join("ptm_test_geom_neg");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = geometry_path_in(&dir);

        save_geometry_to(&path, -50, -100, 250, 600);
        let loaded = load_geometry_from(&path);
        assert_eq!(loaded, Some((-50, -100, 250, 600)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn geometry_missing_file_returns_none() {
        let dir = std::env::temp_dir().join("ptm_test_geom_missing");
        let _ = std::fs::remove_dir_all(&dir);
        let path = geometry_path_in(&dir);

        assert_eq!(load_geometry_from(&path), None);
    }

    #[test]
    fn geometry_malformed_file_returns_none() {
        let dir = std::env::temp_dir().join("ptm_test_geom_bad");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = geometry_path_in(&dir);

        std::fs::write(&path, "not valid geometry\n").unwrap();
        assert_eq!(load_geometry_from(&path), None);

        std::fs::write(&path, "1 2 3\n").unwrap();
        assert_eq!(load_geometry_from(&path), None);

        std::fs::write(&path, "1 2 3 4 5\n").unwrap();
        assert_eq!(load_geometry_from(&path), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Group persistence ──

    fn groups_path_in(dir: &std::path::Path) -> std::path::PathBuf {
        dir.join("groups")
    }

    fn add_item_with_class(app: &mut App, wid: u32, label: &str, wm_class: &str) {
        app.items.push(Item {
            wid,
            label: label.to_string(),
            wm_class: wm_class.to_string(),
            accent_pixel: 0,
            custom_prefix: String::new(),
            session: None,
        });
        app.display_order.push(DisplaySlot::Window(wid));
    }

    #[test]
    fn groups_save_load_round_trip() {
        let dir = std::env::temp_dir().join("ptm_test_groups_rt");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = groups_path_in(&dir);

        let groups = vec![
            SavedGroup {
                name: "Browsers".to_string(),
                collapsed: true,
                kind: GroupKind::Normal,
                members: vec![
                    SavedMember {
                        label: "Firefox".to_string(),
                        wm_class: "Navigator".to_string(),
                        custom_prefix: "FF".to_string(),
                    },
                    SavedMember {
                        label: "Chrome".to_string(),
                        wm_class: "google-chrome".to_string(),
                        custom_prefix: String::new(),
                    },
                ],
            },
            SavedGroup {
                name: "Terminals".to_string(),
                collapsed: false,
                kind: GroupKind::Normal,
                members: vec![SavedMember {
                    label: "Terminal".to_string(),
                    wm_class: "gnome-terminal-server".to_string(),
                    custom_prefix: "Dev".to_string(),
                }],
            },
        ];

        save_groups_to(&path, &groups);
        let loaded = load_groups_from(&path).unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "Browsers");
        assert!(loaded[0].collapsed);
        assert_eq!(loaded[0].members.len(), 2);
        assert_eq!(loaded[0].members[0].label, "Firefox");
        assert_eq!(loaded[0].members[0].wm_class, "Navigator");
        assert_eq!(loaded[0].members[0].custom_prefix, "FF");
        assert_eq!(loaded[0].members[1].label, "Chrome");
        assert_eq!(loaded[0].members[1].custom_prefix, "");
        assert_eq!(loaded[1].name, "Terminals");
        assert!(!loaded[1].collapsed);
        assert_eq!(loaded[1].members.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn groups_load_missing_file_returns_none() {
        let dir = std::env::temp_dir().join("ptm_test_groups_missing");
        let _ = std::fs::remove_dir_all(&dir);
        let path = groups_path_in(&dir);
        assert!(load_groups_from(&path).is_none());
    }

    #[test]
    fn groups_load_malformed_returns_none() {
        let dir = std::env::temp_dir().join("ptm_test_groups_bad");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = groups_path_in(&dir);

        // Wrong version
        std::fs::write(&path, "v2\nGROUP\tFoo\t0\n").unwrap();
        assert!(load_groups_from(&path).is_none());

        // Bad collapsed value
        std::fs::write(&path, "v1\nGROUP\tFoo\t2\n").unwrap();
        assert!(load_groups_from(&path).is_none());

        // MEMBER before any GROUP
        std::fs::write(&path, "v1\nMEMBER\tFoo\tbar\t\n").unwrap();
        assert!(load_groups_from(&path).is_none());

        // Wrong number of fields in GROUP
        std::fs::write(&path, "v1\nGROUP\tFoo\n").unwrap();
        assert!(load_groups_from(&path).is_none());

        // Wrong number of fields in MEMBER
        std::fs::write(&path, "v1\nGROUP\tFoo\t0\nMEMBER\tBar\n").unwrap();
        assert!(load_groups_from(&path).is_none());

        // Unknown line type
        std::fs::write(&path, "v1\nGROUP\tFoo\t0\nBADLINE\n").unwrap();
        assert!(load_groups_from(&path).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_saved_state_from_app() {
        let mut app = make_app();
        add_item_with_class(&mut app, 1, "Firefox", "Navigator");
        add_item_with_class(&mut app, 2, "Terminal", "gnome-terminal");
        add_item_with_class(&mut app, 3, "Code", "code");
        app.items[0].custom_prefix = "FF".to_string();
        app.create_group(1);
        app.add_to_group(0, 2);
        // Group 0 has windows 1, 2; window 3 is ungrouped

        let saved = extract_saved_state(&app);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].name, "Group 1");
        assert!(!saved[0].collapsed);
        assert_eq!(saved[0].members.len(), 2);
        assert_eq!(saved[0].members[0].label, "Firefox");
        assert_eq!(saved[0].members[0].wm_class, "Navigator");
        assert_eq!(saved[0].members[0].custom_prefix, "FF");
        assert_eq!(saved[0].members[1].label, "Terminal");
        assert_eq!(saved[0].members[1].wm_class, "gnome-terminal");
    }

    #[test]
    fn restore_groups_basic() {
        let mut app = make_app();
        add_item_with_class(&mut app, 10, "Firefox", "Navigator");
        add_item_with_class(&mut app, 20, "Terminal", "gnome-terminal");
        add_item_with_class(&mut app, 30, "Code", "code");

        let saved = vec![SavedGroup {
            name: "Browsers".to_string(),
            collapsed: true,
            kind: GroupKind::Normal,
            members: vec![SavedMember {
                label: "Firefox".to_string(),
                wm_class: "Navigator".to_string(),
                custom_prefix: String::new(),
            }],
        }];

        restore_groups(&mut app, &saved);

        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].name, "Browsers");
        assert!(app.groups[0].collapsed);
        assert_eq!(app.groups[0].live_wids(), vec![10]);
        // display_order: Group first, then ungrouped windows
        assert!(matches!(app.display_order[0], DisplaySlot::Group(0)));
        assert!(matches!(app.display_order[1], DisplaySlot::Window(20)));
        assert!(matches!(app.display_order[2], DisplaySlot::Window(30)));
    }

    #[test]
    fn restore_groups_partial_match() {
        let mut app = make_app();
        add_item_with_class(&mut app, 10, "Firefox", "Navigator");
        add_item_with_class(&mut app, 20, "Code", "code");

        let saved = vec![SavedGroup {
            name: "Dev".to_string(),
            collapsed: false,
            kind: GroupKind::Normal,
            members: vec![
                SavedMember {
                    label: "Firefox".to_string(),
                    wm_class: "Navigator".to_string(),
                    custom_prefix: String::new(),
                },
                SavedMember {
                    label: "Terminal".to_string(),
                    wm_class: "gnome-terminal".to_string(),
                    custom_prefix: String::new(),
                },
                SavedMember {
                    label: "Code".to_string(),
                    wm_class: "code".to_string(),
                    custom_prefix: String::new(),
                },
            ],
        }];

        restore_groups(&mut app, &saved);

        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].live_wids(), vec![10, 20]); // Terminal not found
    }

    #[test]
    fn restore_groups_no_match_keeps_group_as_ghost() {
        // Phase 2c semantics flip: a saved group whose members aren't
        // currently present must be RETAINED with all members as ghosts
        // (live_wid: None), so it survives PTM-restart-while-app-not-running
        // and the member can rejoin when the window reappears.
        // (Previous behavior: the group was silently dropped — see FM-2 in
        // MVP_PLAN.md.)
        let mut app = make_app();
        add_item_with_class(&mut app, 10, "Firefox", "Navigator");

        let saved = vec![SavedGroup {
            name: "Gone".to_string(),
            collapsed: false,
            kind: GroupKind::Normal,
            members: vec![SavedMember {
                label: "Terminal".to_string(),
                wm_class: "gnome-terminal".to_string(),
                custom_prefix: String::new(),
            }],
        }];

        restore_groups(&mut app, &saved);

        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].name, "Gone");
        assert_eq!(app.groups[0].members.len(), 1);
        assert_eq!(app.groups[0].members[0].live_wid, None, "member is a ghost");
        assert_eq!(app.groups[0].members[0].label, "Terminal");
        assert_eq!(app.groups[0].live_wids().len(), 0);
        // Firefox window 10 still appears as ungrouped (it didn't match anything).
        assert!(app.display_order.iter().any(|s| matches!(s, DisplaySlot::Window(10))));
        // The group also appears in display_order (so its header renders).
        assert!(app.display_order.iter().any(|s| matches!(s, DisplaySlot::Group(_))));
    }

    // ── Stage F / Phase 2c: ghost members + identity-on-refresh ──

    #[test]
    fn group_live_wids_skips_ghosts() {
        let g = Group {
            id: 0,
            name: "G".to_string(),
            collapsed: false,
            kind: GroupKind::Normal,
            members: vec![
                GroupMember {
                    label: "A".into(),
                    wm_class: "ac".into(),
                    custom_prefix: "".into(),
                    live_wid: Some(11),
                },
                GroupMember {
                    label: "B".into(),
                    wm_class: "bc".into(),
                    custom_prefix: "".into(),
                    live_wid: None,
                },
                GroupMember {
                    label: "C".into(),
                    wm_class: "cc".into(),
                    custom_prefix: "".into(),
                    live_wid: Some(33),
                },
            ],
        };
        assert_eq!(g.live_wids(), vec![11, 33]);
        assert_eq!(g.live_count(), 2);
        assert_eq!(g.position_of_live_wid(33), Some(2));
        assert_eq!(g.position_of_live_wid(99), None);
    }

    #[test]
    fn restore_groups_partial_match_keeps_unmatched_as_ghost() {
        let mut app = make_app();
        add_item_with_class(&mut app, 10, "Firefox", "Navigator");

        let saved = vec![SavedGroup {
            name: "Mixed".to_string(),
            collapsed: false,
            kind: GroupKind::Normal,
            members: vec![
                SavedMember {
                    label: "Firefox".into(),
                    wm_class: "Navigator".into(),
                    custom_prefix: "FF".into(),
                },
                SavedMember {
                    label: "Slack".into(),
                    wm_class: "Slack".into(),
                    custom_prefix: "Chat".into(),
                },
            ],
        }];
        restore_groups(&mut app, &saved);

        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].members.len(), 2);
        assert_eq!(app.groups[0].members[0].live_wid, Some(10));
        assert_eq!(app.groups[0].members[1].live_wid, None);
        // Custom prefix restored on the matched live item
        assert_eq!(app.find_item(10).unwrap().custom_prefix, "FF");
        // The ghost still carries its saved custom_prefix for later restoration
        assert_eq!(app.groups[0].members[1].custom_prefix, "Chat");
    }

    #[test]
    fn restore_groups_wm_class_only_fallback_when_label_drifted() {
        // Phase 2d: terminals especially churn their titles (PWD, running
        // command, tmux info). When neither the exact (label, wm_class) match
        // nor the label-only fallback hits, fall back to wm_class-only so the
        // window still rejoins its group.
        let mut app = make_app();
        // Window currently has a different title than what was saved.
        add_item_with_class(&mut app, 10, "bash - other-dir", "Gnome-terminal");

        let saved = vec![SavedGroup {
            name: "Terms".into(),
            collapsed: false,
            kind: GroupKind::Normal,
            members: vec![SavedMember {
                label: "claude - process-tab-manager".into(),
                wm_class: "Gnome-terminal".into(),
                custom_prefix: String::new(),
            }],
        }];
        restore_groups(&mut app, &saved);

        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].members.len(), 1);
        assert_eq!(
            app.groups[0].members[0].live_wid,
            Some(10),
            "should match by wm_class even when label differs"
        );
    }

    #[test]
    fn restore_groups_wm_class_fallback_does_not_match_different_class() {
        // Sanity: wm_class fallback should NOT cross class boundaries.
        let mut app = make_app();
        add_item_with_class(&mut app, 10, "anything", "Firefox");

        let saved = vec![SavedGroup {
            name: "Terms".into(),
            collapsed: false,
            kind: GroupKind::Normal,
            members: vec![SavedMember {
                label: "different".into(),
                wm_class: "Gnome-terminal".into(), // not Firefox
                custom_prefix: String::new(),
            }],
        }];
        restore_groups(&mut app, &saved);

        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].members[0].live_wid, None, "no class match");
    }

    #[test]
    fn restore_groups_class_fallback_claims_other_terminal_when_one_member_gone() {
        // Exact UAT-2 scenario (uncomfortable but per OQ-F3 design):
        // saved group has [UAT-window-B, UAT-window-A]. Live windows are
        // UAT-window-B and claude (both Gnome-terminal). A is gone.
        // Expected per current cascade: B exact matches; A's slot grabs
        // claude via wm_class fallback because no other Gnome-terminal is
        // unclaimed.
        let mut app = make_app();
        add_item_with_class(&mut app, 100, "UAT-window-B", "Gnome-terminal");
        add_item_with_class(&mut app, 200, "claude - ~/dev/x", "Gnome-terminal");

        let saved = vec![SavedGroup {
            name: "Group 1".into(),
            collapsed: false,
            kind: GroupKind::Normal,
            members: vec![
                SavedMember {
                    label: "UAT-window-B".into(),
                    wm_class: "Gnome-terminal".into(),
                    custom_prefix: String::new(),
                },
                SavedMember {
                    label: "UAT-window-A".into(),
                    wm_class: "Gnome-terminal".into(),
                    custom_prefix: String::new(),
                },
            ],
        }];
        restore_groups(&mut app, &saved);

        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].members.len(), 2);
        // B matches exact
        assert_eq!(app.groups[0].members[0].live_wid, Some(100));
        // A grabs claude via wm_class
        assert_eq!(app.groups[0].members[1].live_wid, Some(200));
        // claude shouldn't also appear ungrouped
        let claude_in_display = app
            .display_order
            .iter()
            .any(|s| matches!(s, DisplaySlot::Window(200)));
        assert!(!claude_in_display, "claude should be in group, not ungrouped");
    }

    #[test]
    fn restore_groups_does_not_re_claim_already_displayed_window() {
        // Regression: a ghost member with class-only match would pull a
        // currently-ungrouped window INTO the group on every restart,
        // surprising the user who explicitly had it ungrouped. Ghosts
        // should only re-match brand-new wids, not currently-placed ones.
        //
        // Note: this asserts behaviour of the LIVE re-match in
        // refresh_items, which is harder to unit-test directly. As a
        // proxy, we exercise restore_groups (which uses an analogous
        // claim algorithm) and verify that it doesn't double-place a
        // window into both the group and display_order.
        let mut app = make_app();
        add_item_with_class(&mut app, 10, "live-bash", "Gnome-terminal");
        // Put the live window in display_order as ungrouped.
        // (add_item_with_class already does this.)

        let saved = vec![SavedGroup {
            name: "Old".into(),
            collapsed: false,
            kind: GroupKind::Normal,
            members: vec![SavedMember {
                label: "ptm-test-window".into(),
                wm_class: "Gnome-terminal".into(),
                custom_prefix: String::new(),
            }],
        }];
        restore_groups(&mut app, &saved);

        // The class-only fallback in restore_groups *will* match because
        // restore-time matching has no display_order to defer to (the user
        // is restarting PTM). That's expected and OK at restore time.
        // The runtime re-match guard is exercised by the live UAT.
        assert_eq!(app.groups.len(), 1);
        // No duplicate display: the matched wid appears either in the
        // group OR in display_order, not both.
        let in_display = app.display_order.iter().any(
            |s| matches!(s, DisplaySlot::Window(10))
        );
        let in_group = app.groups[0].live_wids().contains(&10);
        assert!(in_group ^ in_display, "wid 10 should be in exactly one place");
    }

    #[test]
    fn extract_saved_state_serializes_ghost_members() {
        // Round-trip: a ghost member must serialize so the next save→load
        // cycle preserves it.
        let mut app = make_app();
        let gid = app.next_group_id;
        app.next_group_id += 1;
        app.groups.push(Group {
            id: gid,
            name: "Persistent".into(),
            collapsed: false,
            kind: GroupKind::Normal,
            members: vec![GroupMember {
                label: "Vim".into(),
                wm_class: "vim".into(),
                custom_prefix: String::new(),
                live_wid: None, // ghost
            }],
        });
        app.display_order.push(DisplaySlot::Group(gid));

        let saved = extract_saved_state(&app);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].members.len(), 1);
        assert_eq!(saved[0].members[0].label, "Vim");
        assert_eq!(saved[0].members[0].wm_class, "vim");
    }

    #[test]
    fn restore_groups_duplicate_titles_different_class() {
        let mut app = make_app();
        add_item_with_class(&mut app, 10, "Terminal", "gnome-terminal");
        add_item_with_class(&mut app, 20, "Terminal", "xterm");

        let saved = vec![SavedGroup {
            name: "Terms".to_string(),
            collapsed: false,
            kind: GroupKind::Normal,
            members: vec![SavedMember {
                label: "Terminal".to_string(),
                wm_class: "xterm".to_string(),
                custom_prefix: String::new(),
            }],
        }];

        restore_groups(&mut app, &saved);

        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].live_wids(), vec![20]); // matched xterm, not gnome-terminal
    }

    #[test]
    fn restore_groups_custom_prefix() {
        let mut app = make_app();
        add_item_with_class(&mut app, 10, "Firefox", "Navigator");
        add_item_with_class(&mut app, 20, "Terminal", "gnome-terminal");

        let saved = vec![SavedGroup {
            name: "Dev".to_string(),
            collapsed: false,
            kind: GroupKind::Normal,
            members: vec![
                SavedMember {
                    label: "Firefox".to_string(),
                    wm_class: "Navigator".to_string(),
                    custom_prefix: "Browser".to_string(),
                },
                SavedMember {
                    label: "Terminal".to_string(),
                    wm_class: "gnome-terminal".to_string(),
                    custom_prefix: String::new(),
                },
            ],
        }];

        restore_groups(&mut app, &saved);

        assert_eq!(app.items[0].custom_prefix, "Browser");
        assert_eq!(app.items[1].custom_prefix, ""); // empty prefix not overwritten
    }

    // ── Property-change classification ──

    #[test]
    fn classify_property_root_client_list() {
        assert_eq!(
            classify_property_event(100, true, 100, 101, 102),
            PropertyAction::RefreshClientList
        );
    }

    #[test]
    fn classify_property_root_active_window() {
        assert_eq!(
            classify_property_event(101, true, 100, 101, 102),
            PropertyAction::UpdateActiveWindow
        );
    }

    #[test]
    fn classify_property_window_net_wm_name() {
        assert_eq!(
            classify_property_event(102, false, 100, 101, 102),
            PropertyAction::UpdateWindowTitle
        );
    }

    #[test]
    fn classify_property_window_legacy_wm_name() {
        assert_eq!(
            classify_property_event(u32::from(AtomEnum::WM_NAME), false, 100, 101, 102),
            PropertyAction::UpdateWindowTitle
        );
    }

    #[test]
    fn classify_property_ignores_unknown_root_atom() {
        assert_eq!(
            classify_property_event(999, true, 100, 101, 102),
            PropertyAction::Ignore
        );
    }

    #[test]
    fn classify_property_ignores_unknown_window_atom() {
        assert_eq!(
            classify_property_event(999, false, 100, 101, 102),
            PropertyAction::Ignore
        );
    }

    #[test]
    fn classify_property_wm_name_on_root_is_ignored() {
        // _NET_WM_NAME on the root window is not the taskbar signal we care about
        assert_eq!(
            classify_property_event(102, true, 100, 101, 102),
            PropertyAction::Ignore
        );
    }

    // ── tmux detection (pure helpers) ──

    #[test]
    fn parse_tmux_list_clients_empty() {
        assert!(parse_tmux_list_clients("").is_empty());
    }

    #[test]
    fn parse_tmux_list_clients_basic() {
        let input = "1234 main\n5678 dev-work\n";
        let m = parse_tmux_list_clients(input);
        assert_eq!(m.len(), 2);
        assert_eq!(m.get(&1234).map(String::as_str), Some("main"));
        assert_eq!(m.get(&5678).map(String::as_str), Some("dev-work"));
    }

    #[test]
    fn parse_tmux_list_clients_session_with_spaces() {
        // tmux allows session names with spaces; split on first whitespace only
        let input = "1234 my cool session\n";
        let m = parse_tmux_list_clients(input);
        assert_eq!(m.get(&1234).map(String::as_str), Some("my cool session"));
    }

    #[test]
    fn parse_tmux_list_clients_skips_blank_lines() {
        let input = "\n1234 main\n\n\n5678 dev\n";
        let m = parse_tmux_list_clients(input);
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn parse_tmux_list_clients_skips_malformed() {
        // Missing session name, non-numeric PID, etc. — drop silently.
        let input = "notanum main\n1234\n5678 ok\n";
        let m = parse_tmux_list_clients(input);
        assert_eq!(m.len(), 1);
        assert_eq!(m.get(&5678).map(String::as_str), Some("ok"));
    }

    // ── tmux list-sessions parsing ──

    #[test]
    fn parse_tmux_list_sessions_empty() {
        assert!(parse_tmux_list_sessions("").is_empty());
    }

    #[test]
    fn parse_tmux_list_sessions_single_attached() {
        let input = "$0 demo 1\n";
        let v = parse_tmux_list_sessions(input);
        assert_eq!(v, vec![("$0".to_string(), "demo".to_string(), true)]);
    }

    #[test]
    fn parse_tmux_list_sessions_single_orphan() {
        let input = "$0 demo 0\n";
        let v = parse_tmux_list_sessions(input);
        assert_eq!(v, vec![("$0".to_string(), "demo".to_string(), false)]);
    }

    #[test]
    fn parse_tmux_list_sessions_mixed() {
        let input = "$0 work 2\n$1 orphan 0\n$2 dev 1\n";
        let v = parse_tmux_list_sessions(input);
        assert_eq!(
            v,
            vec![
                ("$0".to_string(), "work".to_string(), true),
                ("$1".to_string(), "orphan".to_string(), false),
                ("$2".to_string(), "dev".to_string(), true),
            ]
        );
    }

    #[test]
    fn parse_tmux_list_sessions_preserves_spaces_in_name() {
        // Session names with spaces — id is the first whitespace-bounded
        // token, attached count is the trailing token, name is everything
        // in between.
        let input = "$3 my cool session 0\n";
        let v = parse_tmux_list_sessions(input);
        assert_eq!(
            v,
            vec![("$3".to_string(), "my cool session".to_string(), false)]
        );
    }

    #[test]
    fn parse_tmux_list_sessions_skips_malformed() {
        // Trailing token isn't a number → drop.
        let input = "$0 bad notanumber\n$1 good 1\n";
        let v = parse_tmux_list_sessions(input);
        assert_eq!(v, vec![("$1".to_string(), "good".to_string(), true)]);
    }

    #[test]
    fn parse_tmux_list_sessions_skips_blank_lines() {
        let input = "\n\n$0 a 0\n\n$1 b 1\n\n";
        let v = parse_tmux_list_sessions(input);
        assert_eq!(
            v,
            vec![
                ("$0".to_string(), "a".to_string(), false),
                ("$1".to_string(), "b".to_string(), true),
            ]
        );
    }

    // ── new (3-field format) coverage ──

    #[test]
    fn parse_tmux_list_sessions_parses_id_name_attached() {
        // The plan calls this out as test #8: the parser must lift the
        // session_id off the front and still preserve embedded spaces in
        // the name.
        let input = "$5 hello world 1\n";
        let v = parse_tmux_list_sessions(input);
        assert_eq!(
            v,
            vec![("$5".to_string(), "hello world".to_string(), true)]
        );
    }

    #[test]
    fn parse_tmux_list_sessions_skips_malformed_session_id() {
        // The plan calls this out as test #9: lines whose first token is
        // not a `$`-prefixed id are dropped, even if the rest looks valid.
        let input = "noprefix demo 0\n$0 ok 1\n";
        let v = parse_tmux_list_sessions(input);
        assert_eq!(v, vec![("$0".to_string(), "ok".to_string(), true)]);
    }

    // ── /proc/<pid>/status parsing ──

    #[test]
    fn parse_proc_status_ppid_basic() {
        let s = "Name:\ttmux: client\nState:\tS (sleeping)\nTgid:\t300\nPid:\t300\nPPid:\t483411\nUid:\t1000\n";
        assert_eq!(parse_proc_status_ppid(s), Some(483411));
    }

    #[test]
    fn parse_proc_status_ppid_missing() {
        assert_eq!(parse_proc_status_ppid("Name:\tfoo\nState:\tR\n"), None);
        assert_eq!(parse_proc_status_ppid(""), None);
    }

    #[test]
    fn parse_proc_status_ppid_malformed() {
        assert_eq!(parse_proc_status_ppid("PPid:\tnotanumber\n"), None);
    }

    // ── Ancestor walk from tmux client up to its owning window ──

    fn ppid_reader(map: HashMap<u32, u32>) -> impl FnMut(u32) -> Option<u32> {
        move |p| map.get(&p).copied()
    }

    fn mk_pid_map(pairs: &[(u32, u32)]) -> HashMap<u32, Vec<u32>> {
        let mut m: HashMap<u32, Vec<u32>> = HashMap::new();
        for &(pid, wid) in pairs {
            m.entry(pid).or_default().push(wid);
        }
        m
    }

    #[test]
    fn walk_to_window_owner_start_pid_is_window() {
        // tmux client is itself running directly under xterm, and xterm's
        // pid was already harvested as a window pid.
        let pid_to_wid = mk_pid_map(&[(100, 42)]);
        let read = ppid_reader(HashMap::new());
        assert_eq!(walk_to_window_owner(100, &pid_to_wid, read, 10), Some(42));
    }

    #[test]
    fn walk_to_window_owner_walks_up_chain() {
        // client 300 -> shell 200 -> xterm 100 (window wid=42)
        let pid_to_wid = mk_pid_map(&[(100, 42)]);
        let read = ppid_reader([(300, 200), (200, 100), (100, 1)].iter().cloned().collect());
        assert_eq!(walk_to_window_owner(300, &pid_to_wid, read, 10), Some(42));
    }

    #[test]
    fn walk_to_window_owner_stops_at_closest_match() {
        // Both 200 and the grand-ancestor 100 are window pids (e.g. cinnamon
        // is also reported as a window pid). The client is under 200, so we
        // must stop at 200 and not keep walking to 100.
        let pid_to_wid = mk_pid_map(&[(200, 42), (100, 99)]);
        let read = ppid_reader([(300, 200), (200, 100), (100, 1)].iter().cloned().collect());
        assert_eq!(walk_to_window_owner(300, &pid_to_wid, read, 10), Some(42));
    }

    #[test]
    fn walk_to_window_owner_reaches_init_without_match() {
        let pid_to_wid = mk_pid_map(&[(100, 42)]);
        let read = ppid_reader([(300, 1)].iter().cloned().collect());
        assert_eq!(walk_to_window_owner(300, &pid_to_wid, read, 10), None);
    }

    #[test]
    fn walk_to_window_owner_process_disappears() {
        // read_ppid returns None (e.g. the process was reaped mid-walk).
        let pid_to_wid = mk_pid_map(&[(100, 42)]);
        let read = ppid_reader(HashMap::new());
        assert_eq!(walk_to_window_owner(300, &pid_to_wid, read, 10), None);
    }

    #[test]
    fn walk_to_window_owner_respects_depth_limit() {
        let pid_to_wid = mk_pid_map(&[(100, 42)]);
        let read = ppid_reader(
            [(500, 400), (400, 300), (300, 200), (200, 100)].iter().cloned().collect(),
        );
        // depth 3 stops before reaching 100
        assert_eq!(walk_to_window_owner(500, &pid_to_wid, read, 3), None);
        // depth 5 reaches it
        let read2 = ppid_reader(
            [(500, 400), (400, 300), (300, 200), (200, 100)].iter().cloned().collect(),
        );
        assert_eq!(walk_to_window_owner(500, &pid_to_wid, read2, 5), Some(42));
    }

    #[test]
    fn walk_to_window_owner_empty_window_set() {
        let pid_to_wid: HashMap<u32, Vec<u32>> = HashMap::new();
        let read = ppid_reader([(300, 200), (200, 100)].iter().cloned().collect());
        assert_eq!(walk_to_window_owner(300, &pid_to_wid, read, 10), None);
    }

    #[test]
    fn walk_to_window_owner_returns_none_on_collision() {
        // gnome-terminal-server pid 2380 hosts two tracked windows. The walk
        // lands here and must refuse to pick one — wrong attribution is worse
        // than no attribution.
        let pid_to_wid = mk_pid_map(&[(2380, 42), (2380, 99)]);
        let read = ppid_reader([(605774, 2380), (2380, 1)].iter().cloned().collect());
        assert_eq!(walk_to_window_owner(605774, &pid_to_wid, read, 10), None);
    }

    #[test]
    fn walk_to_window_owner_collision_does_not_fall_through_to_ancestor() {
        // If the first hit is ambiguous, the walk gives up rather than
        // climbing further and matching an unrelated ancestor (e.g. systemd,
        // if it were a tracked window pid for some reason).
        let pid_to_wid = mk_pid_map(&[(2380, 42), (2380, 99), (1179, 7)]);
        let read = ppid_reader([(605774, 2380), (2380, 1179)].iter().cloned().collect());
        assert_eq!(walk_to_window_owner(605774, &pid_to_wid, read, 10), None);
    }

    // ── Pending-spawn claim ──

    fn mk_item(wid: u32) -> Item {
        Item {
            wid,
            label: format!("w{}", wid),
            wm_class: String::new(),
            accent_pixel: 0,
            custom_prefix: String::new(),
            session: None,
        }
    }

    fn pending_attach(name: &str, when: std::time::Instant) -> Option<super::PendingSpawn> {
        Some(super::PendingSpawn {
            kind: super::PendingSpawnKind::Attach(name.to_string()),
            spawned_at: when,
        })
    }

    fn pending_terminal(when: std::time::Instant) -> Option<super::PendingSpawn> {
        Some(super::PendingSpawn {
            kind: super::PendingSpawnKind::Terminal,
            spawned_at: when,
        })
    }

    #[test]
    fn pending_spawn_attach_claims_sole_new_wid_and_binds_session() {
        let now = std::time::Instant::now();
        let mut pending = pending_attach("demo", now);
        let prior: HashSet<u32> = [1, 2].iter().copied().collect();
        let mut items = vec![mk_item(1), mk_item(2), mk_item(3)];
        let claim = claim_pending_spawn(
            &mut pending,
            &prior,
            &mut items,
            std::time::Duration::from_secs(5),
            now,
        );
        let claim = claim.expect("expected claim");
        assert_eq!(claim.wid, 3);
        assert_eq!(claim.attach_session.as_deref(), Some("demo"));
        assert_eq!(items[2].session.as_deref(), Some("demo"));
        assert!(items[0].session.is_none());
        assert!(items[1].session.is_none());
        assert!(pending.is_none(), "claim should clear pending");
    }

    #[test]
    fn pending_spawn_terminal_claims_sole_new_wid_without_session_binding() {
        let now = std::time::Instant::now();
        let mut pending = pending_terminal(now);
        let prior: HashSet<u32> = [1].iter().copied().collect();
        let mut items = vec![mk_item(1), mk_item(7)];
        let claim = claim_pending_spawn(
            &mut pending,
            &prior,
            &mut items,
            std::time::Duration::from_secs(5),
            now,
        );
        let claim = claim.expect("expected claim");
        assert_eq!(claim.wid, 7);
        assert!(claim.attach_session.is_none(), "Terminal kind binds no session");
        assert!(items[1].session.is_none(), "Terminal kind must not set item.session");
        assert!(pending.is_none(), "claim should clear pending");
    }

    #[test]
    fn pending_spawn_defers_when_no_new_wid() {
        let now = std::time::Instant::now();
        let mut pending = pending_attach("demo", now);
        let prior: HashSet<u32> = [1, 2].iter().copied().collect();
        let mut items = vec![mk_item(1), mk_item(2)];
        let claim = claim_pending_spawn(
            &mut pending,
            &prior,
            &mut items,
            std::time::Duration::from_secs(5),
            now,
        );
        assert!(claim.is_none());
        assert!(pending.is_some(), "pending stays until window appears or timeout");
    }

    #[test]
    fn pending_spawn_defers_when_multiple_new_wids() {
        // User triggered a spawn but also happened to open an unrelated window
        // in the same refresh window. We can't tell which is which — wait.
        let now = std::time::Instant::now();
        let mut pending = pending_attach("demo", now);
        let prior: HashSet<u32> = [1].iter().copied().collect();
        let mut items = vec![mk_item(1), mk_item(2), mk_item(3)];
        let claim = claim_pending_spawn(
            &mut pending,
            &prior,
            &mut items,
            std::time::Duration::from_secs(5),
            now,
        );
        assert!(claim.is_none());
        assert!(items[1].session.is_none());
        assert!(items[2].session.is_none());
        assert!(pending.is_some());
    }

    #[test]
    fn pending_spawn_times_out() {
        let spawn = std::time::Instant::now();
        let later = spawn + std::time::Duration::from_secs(10);
        let mut pending = pending_attach("demo", spawn);
        let prior: HashSet<u32> = [1].iter().copied().collect();
        let mut items = vec![mk_item(1), mk_item(2)];
        let claim = claim_pending_spawn(
            &mut pending,
            &prior,
            &mut items,
            std::time::Duration::from_secs(5),
            later,
        );
        assert!(claim.is_none());
        assert!(items[1].session.is_none());
        assert!(pending.is_none(), "timed-out pending should be cleared");
    }

    #[test]
    fn is_attach_pending_for_matches_same_session() {
        let pending = pending_attach("demo", std::time::Instant::now());
        assert!(is_attach_pending_for(&pending, "demo"));
        assert!(!is_attach_pending_for(&pending, "other"));
    }

    #[test]
    fn is_attach_pending_for_terminal_kind_does_not_match_any_session() {
        // Terminal pending is for "+ New terminal" — it never represents a
        // session attach, so the orphan-row debounce should treat it as not
        // pending (the user can still click an orphan to attach).
        let pending = pending_terminal(std::time::Instant::now());
        assert!(!is_attach_pending_for(&pending, "demo"));
    }

    #[test]
    fn is_attach_pending_for_none() {
        let pending: Option<super::PendingSpawn> = None;
        assert!(!is_attach_pending_for(&pending, "demo"));
    }

    #[test]
    fn pending_spawn_none_is_noop() {
        let now = std::time::Instant::now();
        let mut pending: Option<super::PendingSpawn> = None;
        let prior: HashSet<u32> = HashSet::new();
        let mut items = vec![mk_item(1)];
        let claim = claim_pending_spawn(
            &mut pending,
            &prior,
            &mut items,
            std::time::Duration::from_secs(5),
            now,
        );
        assert!(claim.is_none());
        assert!(items[0].session.is_none());
        assert!(pending.is_none());
    }

    // ── Session-binding carry-over ──
    //
    // Under gnome-terminal the steady-state attribution path
    // (walk_to_window_owner) returns None because every terminal window
    // shares gnome-terminal-server's PID. claim_pending_spawn binds the
    // session once on the spawn refresh, then clears. Without carry-over,
    // the next refresh strands the marker. carry_over_session_bindings is
    // the path that preserves the binding while still pruning stale ones.

    fn mk_item_with_session(wid: u32, session: Option<&str>) -> Item {
        Item {
            wid,
            label: format!("w{}", wid),
            wm_class: String::new(),
            accent_pixel: 0,
            custom_prefix: String::new(),
            session: session.map(String::from),
        }
    }

    fn live_session_set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn carry_over_preserves_live_session_binding() {
        let prior = vec![mk_item_with_session(100, Some("0"))];
        let mut new_items = vec![mk_item_with_session(100, None)];
        let live = live_session_set(&["0"]);
        super::carry_over_session_bindings(&prior, &mut new_items, &live);
        assert_eq!(new_items[0].session.as_deref(), Some("0"));
    }

    #[test]
    fn carry_over_drops_session_no_longer_in_tmux() {
        let prior = vec![mk_item_with_session(100, Some("0"))];
        let mut new_items = vec![mk_item_with_session(100, None)];
        let live = live_session_set(&[]);
        super::carry_over_session_bindings(&prior, &mut new_items, &live);
        assert!(
            new_items[0].session.is_none(),
            "killed session should not ghost forward"
        );
    }

    #[test]
    fn carry_over_does_not_overwrite_claim_pending_spawn() {
        let prior = vec![mk_item_with_session(100, Some("0"))];
        let mut new_items = vec![mk_item_with_session(100, Some("X"))];
        let live = live_session_set(&["0", "X"]);
        super::carry_over_session_bindings(&prior, &mut new_items, &live);
        assert_eq!(
            new_items[0].session.as_deref(),
            Some("X"),
            "claim_pending_spawn's authoritative binding must win"
        );
    }

    #[test]
    fn carry_over_skips_unmatched_wid() {
        let prior = vec![mk_item_with_session(100, Some("0"))];
        let mut new_items = vec![mk_item_with_session(200, None)];
        let live = live_session_set(&["0"]);
        super::carry_over_session_bindings(&prior, &mut new_items, &live);
        assert!(
            new_items[0].session.is_none(),
            "different wid must not inherit prior wid's session"
        );
    }

    // ── Terminal detection ──

    #[test]
    fn detect_terminal_prefers_env_terminal() {
        let argv = detect_terminal_command(None, Some("urxvt"), |_| true);
        assert_eq!(argv, vec!["urxvt".to_string()]);
    }

    #[test]
    fn detect_terminal_env_terminal_splits_whitespace() {
        // Some users set TERMINAL with args.
        let argv = detect_terminal_command(None, Some("alacritty -T MyTerm"), |_| true);
        assert_eq!(argv, vec!["alacritty", "-T", "MyTerm"]);
    }

    #[test]
    fn detect_terminal_empty_env_falls_through() {
        // Empty string isn't a valid override.
        let argv = detect_terminal_command(None, Some(""), |name| name == "x-terminal-emulator");
        assert_eq!(argv, vec!["x-terminal-emulator".to_string()]);
    }

    #[test]
    fn detect_terminal_uses_x_terminal_emulator_when_env_unset() {
        let argv = detect_terminal_command(None, None, |name| name == "x-terminal-emulator");
        assert_eq!(argv, vec!["x-terminal-emulator".to_string()]);
    }

    #[test]
    fn detect_terminal_falls_back_to_xdg_terminal_exec() {
        let argv = detect_terminal_command(None, None, |name| name == "xdg-terminal-exec");
        assert_eq!(argv, vec!["xdg-terminal-exec".to_string()]);
    }

    #[test]
    fn detect_terminal_falls_back_to_xterm() {
        // No env, no optional binaries on PATH.
        let argv = detect_terminal_command(None, None, |_| false);
        assert_eq!(argv, vec!["xterm".to_string()]);
    }

    #[test]
    fn detect_terminal_prefers_x_terminal_emulator_over_xdg() {
        let argv = detect_terminal_command(None, None, |_| true);
        assert_eq!(argv, vec!["x-terminal-emulator".to_string()]);
    }

    #[test]
    fn detect_terminal_prefers_ptm_terminal_cmd_over_env_terminal() {
        // PTM-specific override must win even when $TERMINAL is set.
        let argv = detect_terminal_command(
            Some("gnome-terminal --profile=MyProfile"),
            Some("xterm"),
            |_| true,
        );
        assert_eq!(argv, vec!["gnome-terminal", "--profile=MyProfile"]);
    }

    #[test]
    fn detect_terminal_empty_ptm_cmd_falls_through_to_env_terminal() {
        // Empty/whitespace PTM_TERMINAL_CMD shouldn't shadow $TERMINAL.
        let argv = detect_terminal_command(Some("   "), Some("alacritty"), |_| true);
        assert_eq!(argv, vec!["alacritty".to_string()]);
    }

    // ── Attach argv builder ──

    #[test]
    fn terminal_argv_for_attach_xterm_uses_dash_e() {
        let term = vec!["xterm".to_string()];
        let argv = terminal_argv_for_attach(&term, "demo");
        assert_eq!(
            argv,
            vec!["xterm", "-e", "tmux", "attach-session", "-t", "demo"]
        );
    }

    #[test]
    fn terminal_argv_for_attach_gnome_terminal_uses_double_dash() {
        let term = vec!["gnome-terminal".to_string()];
        let argv = terminal_argv_for_attach(&term, "demo");
        assert_eq!(
            argv,
            vec!["gnome-terminal", "--", "tmux", "attach-session", "-t", "demo"]
        );
    }

    #[test]
    fn terminal_argv_for_attach_ptyxis_uses_double_dash() {
        let term = vec!["ptyxis".to_string()];
        let argv = terminal_argv_for_attach(&term, "demo");
        assert_eq!(
            argv,
            vec!["ptyxis", "--", "tmux", "attach-session", "-t", "demo"]
        );
    }

    #[test]
    fn terminal_argv_for_attach_unknown_falls_back_to_dash_e() {
        let term = vec!["some-obscure-terminal".to_string()];
        let argv = terminal_argv_for_attach(&term, "dev");
        assert_eq!(
            argv,
            vec![
                "some-obscure-terminal",
                "-e",
                "tmux",
                "attach-session",
                "-t",
                "dev"
            ]
        );
    }

    #[test]
    fn terminal_argv_for_attach_preserves_leading_args() {
        // $TERMINAL="alacritty -T MyTerm" case.
        let term = vec![
            "alacritty".to_string(),
            "-T".to_string(),
            "MyTerm".to_string(),
        ];
        let argv = terminal_argv_for_attach(&term, "demo");
        assert_eq!(
            argv,
            vec![
                "alacritty",
                "-T",
                "MyTerm",
                "-e",
                "tmux",
                "attach-session",
                "-t",
                "demo"
            ]
        );
    }

    #[test]
    fn terminal_argv_for_attach_uses_basename_for_match() {
        // Absolute path still recognised as gnome-terminal.
        let term = vec!["/usr/bin/gnome-terminal".to_string()];
        let argv = terminal_argv_for_attach(&term, "demo");
        assert_eq!(
            argv,
            vec![
                "/usr/bin/gnome-terminal",
                "--",
                "tmux",
                "attach-session",
                "-t",
                "demo"
            ]
        );
    }

    #[test]
    fn terminal_argv_for_attach_empty_term_returns_empty() {
        let argv = terminal_argv_for_attach(&[], "demo");
        assert!(argv.is_empty());
    }

    // ── Session context menu + inline rename ──

    /// Test helper: ensure a TmuxSystem group exists, append a session
    /// member with the given name, and rebuild rows. Sessions live only
    /// inside that group from T4.4 onward, so this replaces the old
    /// "push DisplaySlot::Session" pattern.
    fn push_session(app: &mut App, name: &str) {
        let gid = match app.groups.iter().find(|g| g.kind == super::GroupKind::TmuxSystem) {
            Some(g) => g.id,
            None => {
                let gid = app.next_group_id;
                app.next_group_id += 1;
                app.groups.push(super::Group {
                    id: gid,
                    name: "Tmux Sessions".to_string(),
                    collapsed: false,
                    kind: super::GroupKind::TmuxSystem,
                    members: Vec::new(),
                });
                app.display_order.push(DisplaySlot::Group(gid));
                gid
            }
        };
        let group = app.groups.iter_mut().find(|g| g.id == gid).unwrap();
        group.members.push(super::GroupMember {
            label: name.to_string(),
            wm_class: String::new(),
            custom_prefix: String::new(),
            live_wid: None,
        });
        app.build_display_rows();
    }

    #[test]
    fn menu_for_session_has_attach_rename_kill() {
        let mut app = make_app();
        push_session(&mut app, "demo");
        // Row 0 is now the TmuxSystem GroupHeader; row 1 is the session.
        let entries = build_menu_entries(&app, 1);
        assert_eq!(entries.len(), 3);
        assert!(matches!(entries[0].action, MenuAction::AttachSession));
        assert!(matches!(entries[1].action, MenuAction::RenameSession));
        assert!(matches!(entries[2].action, MenuAction::KillSession));
    }

    #[test]
    fn start_session_rename_initializes_with_current_name() {
        let mut app = make_app();
        app.start_session_rename("my-session");
        let rs = app.rename.as_ref().expect("rename state should be set");
        assert_eq!(rs.text, "my-session");
        assert_eq!(rs.cursor, "my-session".len());
        assert!(matches!(&rs.target, RenameTarget::Session(n) if n == "my-session"));
    }

    #[test]
    fn cancel_session_rename_preserves_display_order() {
        let mut app = make_app();
        push_session(&mut app, "demo");
        app.start_session_rename("demo");
        // User types then presses escape.
        if let Some(ref mut rs) = app.rename {
            rs.text = "renamed".to_string();
        }
        app.cancel_rename();
        // The TmuxSystem group is still in display_order; the session
        // member's name is preserved.
        assert_eq!(app.display_order.len(), 1);
        assert!(matches!(&app.display_order[0], DisplaySlot::Group(_)));
        let group = app.groups.iter().find(|g| g.kind == super::GroupKind::TmuxSystem).unwrap();
        assert_eq!(group.members.len(), 1);
        assert_eq!(group.members[0].label, "demo");
    }

    // ── Header button hit-test ──

    #[test]
    fn header_button_hit_test_top_region() {
        let app = make_app();
        assert!(app.hit_test_header_button(0));
        assert!(app.hit_test_header_button((HEADER_H - 1) as i16));
    }

    #[test]
    fn header_button_hit_test_below_header_is_false() {
        let app = make_app();
        assert!(!app.hit_test_header_button(HEADER_H as i16));
        assert!(!app.hit_test_header_button(HEADER_H as i16 + 10));
    }

    #[test]
    fn header_button_hit_test_negative_is_false() {
        let app = make_app();
        assert!(!app.hit_test_header_button(-1));
    }

    // ── Stage H: rename selection (T1.1 — selection_anchor data + helpers) ──

    fn make_rename_state(text: &str, cursor: usize, anchor: Option<usize>) -> RenameState {
        RenameState {
            target: RenameTarget::Group(0),
            text: text.to_string(),
            cursor,
            selection_anchor: anchor,
        }
    }

    #[test]
    fn rename_selection_range_none_when_anchor_unset() {
        let rs = make_rename_state("hello", 3, None);
        assert_eq!(rs.selection_range(), None);
        assert!(!rs.has_selection());
    }

    #[test]
    fn rename_selection_range_normalized_when_anchor_before_cursor() {
        let rs = make_rename_state("hello world", 7, Some(2));
        assert_eq!(rs.selection_range(), Some((2, 7)));
        assert!(rs.has_selection());
    }

    #[test]
    fn rename_selection_range_normalized_when_anchor_after_cursor() {
        let rs = make_rename_state("hello world", 2, Some(7));
        assert_eq!(rs.selection_range(), Some((2, 7)));
        assert!(rs.has_selection());
    }

    #[test]
    fn rename_selection_range_none_when_anchor_equals_cursor() {
        let rs = make_rename_state("hello", 3, Some(3));
        assert_eq!(rs.selection_range(), None);
        assert!(!rs.has_selection());
    }

    #[test]
    fn rename_clear_selection_unsets_anchor() {
        let mut rs = make_rename_state("hello", 3, Some(0));
        rs.clear_selection();
        assert_eq!(rs.selection_anchor, None);
        assert_eq!(rs.cursor, 3); // cursor unchanged
    }

    #[test]
    fn rename_anchor_if_none_sets_anchor_to_cursor_when_unset() {
        let mut rs = make_rename_state("hello", 3, None);
        rs.anchor_if_none();
        assert_eq!(rs.selection_anchor, Some(3));
    }

    #[test]
    fn rename_anchor_if_none_preserves_existing_anchor() {
        let mut rs = make_rename_state("hello", 3, Some(0));
        rs.anchor_if_none();
        assert_eq!(rs.selection_anchor, Some(0)); // not overwritten
    }

    // ── Stage H: motion (T1.2 — Shift+Left/Right/Home/End) ──

    #[test]
    fn rename_move_left_char_no_shift_no_selection_moves_one() {
        let mut rs = make_rename_state("hello", 3, None);
        rs.move_left_char(false);
        assert_eq!(rs.cursor, 2);
        assert_eq!(rs.selection_anchor, None);
    }

    #[test]
    fn rename_move_left_char_no_shift_with_selection_collapses_to_start() {
        let mut rs = make_rename_state("hello world", 7, Some(2));
        rs.move_left_char(false);
        assert_eq!(rs.cursor, 2); // collapse to start, no further motion
        assert_eq!(rs.selection_anchor, None);
    }

    #[test]
    fn rename_move_right_char_no_shift_with_selection_collapses_to_end() {
        let mut rs = make_rename_state("hello world", 2, Some(7));
        rs.move_right_char(false);
        assert_eq!(rs.cursor, 7);
        assert_eq!(rs.selection_anchor, None);
    }

    #[test]
    fn rename_shift_right_from_no_selection_anchors_then_moves() {
        let mut rs = make_rename_state("hello", 2, None);
        rs.move_right_char(true);
        assert_eq!(rs.cursor, 3);
        assert_eq!(rs.selection_anchor, Some(2));
        assert_eq!(rs.selection_range(), Some((2, 3)));
    }

    #[test]
    fn rename_shift_right_extends_existing_selection() {
        let mut rs = make_rename_state("hello world", 3, Some(1));
        rs.move_right_char(true);
        assert_eq!(rs.cursor, 4);
        assert_eq!(rs.selection_anchor, Some(1)); // preserved
        assert_eq!(rs.selection_range(), Some((1, 4)));
    }

    #[test]
    fn rename_shift_left_shrinks_selection_when_cursor_past_anchor() {
        let mut rs = make_rename_state("hello", 3, Some(1));
        rs.move_left_char(true);
        assert_eq!(rs.cursor, 2);
        assert_eq!(rs.selection_range(), Some((1, 2)));
    }

    #[test]
    fn rename_shift_left_collapses_selection_when_cursor_meets_anchor() {
        let mut rs = make_rename_state("hello", 2, Some(1));
        rs.move_left_char(true);
        assert_eq!(rs.cursor, 1);
        assert_eq!(rs.selection_anchor, Some(1)); // anchor still set...
        assert_eq!(rs.selection_range(), None); // ...but collapsed
    }

    #[test]
    fn rename_move_home_clears_selection_when_no_shift() {
        let mut rs = make_rename_state("hello", 3, Some(0));
        rs.move_home(false);
        assert_eq!(rs.cursor, 0);
        assert_eq!(rs.selection_anchor, None);
    }

    #[test]
    fn rename_move_end_clears_selection_when_no_shift() {
        let mut rs = make_rename_state("hello", 2, Some(0));
        rs.move_end(false);
        assert_eq!(rs.cursor, 5);
        assert_eq!(rs.selection_anchor, None);
    }

    #[test]
    fn rename_shift_home_anchors_and_jumps_to_zero() {
        let mut rs = make_rename_state("hello", 3, None);
        rs.move_home(true);
        assert_eq!(rs.cursor, 0);
        assert_eq!(rs.selection_anchor, Some(3));
        assert_eq!(rs.selection_range(), Some((0, 3)));
    }

    #[test]
    fn rename_shift_end_anchors_and_jumps_to_len() {
        let mut rs = make_rename_state("hello", 2, None);
        rs.move_end(true);
        assert_eq!(rs.cursor, 5);
        assert_eq!(rs.selection_anchor, Some(2));
        assert_eq!(rs.selection_range(), Some((2, 5)));
    }

    #[test]
    fn rename_move_left_at_zero_is_clamped() {
        let mut rs = make_rename_state("hello", 0, None);
        rs.move_left_char(false);
        assert_eq!(rs.cursor, 0);
    }

    #[test]
    fn rename_move_right_at_end_is_clamped() {
        let mut rs = make_rename_state("hello", 5, None);
        rs.move_right_char(false);
        assert_eq!(rs.cursor, 5);
    }

    #[test]
    fn rename_motion_is_char_aware_for_multibyte() {
        // "café" is 5 bytes (c, a, f, é=2 bytes). Cursor at 5 (end).
        let mut rs = make_rename_state("café", 5, None);
        rs.move_left_char(false);
        assert_eq!(rs.cursor, 3); // back to start of é
        rs.move_left_char(false);
        assert_eq!(rs.cursor, 2);
    }

    // ── Stage H: T1.3 — Ctrl+A select-all, Ctrl+Backspace/Delete word delete,
    //                    Ctrl+Left/Right word motion ──

    #[test]
    fn rename_select_all_anchors_zero_cursor_len() {
        let mut rs = make_rename_state("hello world", 4, None);
        rs.select_all();
        assert_eq!(rs.selection_anchor, Some(0));
        assert_eq!(rs.cursor, 11);
        assert_eq!(rs.selection_range(), Some((0, 11)));
    }

    #[test]
    fn rename_select_all_on_empty_text_is_no_op_collapse() {
        let mut rs = make_rename_state("", 0, None);
        rs.select_all();
        // anchor at 0 and cursor at 0 → range is None (collapsed)
        assert_eq!(rs.cursor, 0);
        assert_eq!(rs.selection_range(), None);
    }

    // Word-boundary helpers (alnum vs non-alnum transitions, readline-style).
    // Forward = "skip non-alnum, then skip alnum" → lands at end of (next) word.
    // Backward = mirror → lands at start of (current/previous) word.

    #[test]
    fn rename_next_word_boundary_basic_word() {
        let s = "hello world";
        // From start of "hello": skip alnum to end → 5.
        assert_eq!(super::next_word_boundary(s, 0), 5);
        // From end of "hello": skip space, skip "world" → 11.
        assert_eq!(super::next_word_boundary(s, 5), 11);
        // At end of string: stays at end.
        assert_eq!(super::next_word_boundary(s, 11), 11);
    }

    #[test]
    fn rename_next_word_boundary_skips_run_of_non_alnum() {
        let s = "a   b";
        assert_eq!(super::next_word_boundary(s, 1), 5); // skip "   ", skip "b"
    }

    #[test]
    fn rename_next_word_boundary_punctuation_treated_as_non_alnum() {
        let s = "foo,bar";
        assert_eq!(super::next_word_boundary(s, 3), 7); // ',' then "bar"
    }

    #[test]
    fn rename_prev_word_boundary_basic_word() {
        let s = "hello world";
        // From end-of-string: skip nothing (alnum), skip "world" → 6.
        assert_eq!(super::prev_word_boundary(s, 11), 6);
        // From 6 (start of "world"): skip ' ', skip "hello" → 0.
        assert_eq!(super::prev_word_boundary(s, 6), 0);
        // From 0: stays at 0.
        assert_eq!(super::prev_word_boundary(s, 0), 0);
    }

    #[test]
    fn rename_prev_word_boundary_skips_run_of_non_alnum() {
        let s = "a   b";
        // From end: skip nothing (alnum 'b'), skip "b" → 4. Then from 4: skip "   " → 1. Then skip "a" → 0.
        assert_eq!(super::prev_word_boundary(s, 5), 4);
        assert_eq!(super::prev_word_boundary(s, 4), 0);
    }

    #[test]
    fn rename_word_boundaries_are_unicode_safe() {
        // "café world" — é = 2 bytes. Total length 10 bytes.
        // alnum positions: 0,1,2,3 (caf), 3..5 (é), 6..10 (world)
        let s = "café world";
        // From end: skip nothing (alnum), skip "world" → 6.
        assert_eq!(super::next_word_boundary(s, 0), 5); // "café" ends at byte 5
        assert_eq!(super::prev_word_boundary(s, 10), 6);
    }

    // Word-motion methods (Ctrl+Left/Right with optional Shift)

    #[test]
    fn rename_move_left_word_no_shift_jumps_word() {
        let mut rs = make_rename_state("hello world", 11, None);
        rs.move_left_word(false);
        assert_eq!(rs.cursor, 6);
        assert_eq!(rs.selection_anchor, None);
    }

    #[test]
    fn rename_move_right_word_no_shift_jumps_word() {
        let mut rs = make_rename_state("hello world", 0, None);
        rs.move_right_word(false);
        assert_eq!(rs.cursor, 5);
        assert_eq!(rs.selection_anchor, None);
    }

    #[test]
    fn rename_shift_ctrl_right_extends_selection_by_word() {
        let mut rs = make_rename_state("hello world", 0, None);
        rs.move_right_word(true);
        assert_eq!(rs.cursor, 5);
        assert_eq!(rs.selection_anchor, Some(0));
        assert_eq!(rs.selection_range(), Some((0, 5)));
    }

    #[test]
    fn rename_shift_ctrl_left_extends_selection_by_word() {
        let mut rs = make_rename_state("hello world", 11, None);
        rs.move_left_word(true);
        assert_eq!(rs.cursor, 6);
        assert_eq!(rs.selection_anchor, Some(11));
        assert_eq!(rs.selection_range(), Some((6, 11)));
    }

    #[test]
    fn rename_no_shift_ctrl_right_with_selection_clears_anchor() {
        let mut rs = make_rename_state("hello world", 0, Some(2));
        rs.move_right_word(false);
        // No-shift word motion clears the selection (per plan table).
        assert_eq!(rs.selection_anchor, None);
        assert_eq!(rs.cursor, 5);
    }

    // Word-delete methods

    #[test]
    fn rename_delete_word_left_removes_prev_word() {
        let mut rs = make_rename_state("hello world", 11, None);
        rs.delete_word_left();
        assert_eq!(rs.text, "hello ");
        assert_eq!(rs.cursor, 6);
    }

    #[test]
    fn rename_delete_word_left_removes_only_whitespace_when_in_whitespace_run() {
        let mut rs = make_rename_state("hello   ", 8, None);
        rs.delete_word_left();
        // From end: skip "   " backward (5), skip "hello" backward (0). Delete bytes 0..8.
        assert_eq!(rs.text, "");
        assert_eq!(rs.cursor, 0);
    }

    #[test]
    fn rename_delete_word_left_at_start_is_noop() {
        let mut rs = make_rename_state("hello", 0, None);
        rs.delete_word_left();
        assert_eq!(rs.text, "hello");
        assert_eq!(rs.cursor, 0);
    }

    #[test]
    fn rename_delete_word_right_removes_next_word() {
        let mut rs = make_rename_state("hello world", 0, None);
        rs.delete_word_right();
        assert_eq!(rs.text, " world");
        assert_eq!(rs.cursor, 0);
    }

    #[test]
    fn rename_delete_word_right_at_end_is_noop() {
        let mut rs = make_rename_state("hello", 5, None);
        rs.delete_word_right();
        assert_eq!(rs.text, "hello");
        assert_eq!(rs.cursor, 5);
    }

    // ── Stage H: T1.4 — printable input replaces selection;
    //                    selection-aware backspace/delete ──

    #[test]
    fn rename_insert_char_no_selection_inserts_at_cursor() {
        let mut rs = make_rename_state("hllo", 1, None);
        rs.insert_char('e');
        assert_eq!(rs.text, "hello");
        assert_eq!(rs.cursor, 2);
        assert_eq!(rs.selection_anchor, None);
    }

    #[test]
    fn rename_insert_char_replaces_selection() {
        let mut rs = make_rename_state("hello world", 5, Some(0));
        rs.insert_char('X');
        assert_eq!(rs.text, "X world");
        assert_eq!(rs.cursor, 1); // after the inserted X
        assert_eq!(rs.selection_anchor, None);
    }

    #[test]
    fn rename_insert_char_replaces_selection_with_reversed_anchor() {
        let mut rs = make_rename_state("hello world", 0, Some(5));
        rs.insert_char('Y');
        assert_eq!(rs.text, "Y world");
        assert_eq!(rs.cursor, 1);
        assert_eq!(rs.selection_anchor, None);
    }

    #[test]
    fn rename_insert_multibyte_char_advances_cursor_correctly() {
        let mut rs = make_rename_state("caf", 3, None);
        rs.insert_char('é');
        assert_eq!(rs.text, "café");
        assert_eq!(rs.cursor, 5); // é is 2 bytes
    }

    #[test]
    fn rename_delete_back_char_with_selection_only_deletes_selection() {
        let mut rs = make_rename_state("hello world", 5, Some(0));
        rs.delete_back_char();
        assert_eq!(rs.text, " world");
        assert_eq!(rs.cursor, 0);
        assert_eq!(rs.selection_anchor, None);
    }

    #[test]
    fn rename_delete_back_char_without_selection_deletes_char_before() {
        let mut rs = make_rename_state("hello", 5, None);
        rs.delete_back_char();
        assert_eq!(rs.text, "hell");
        assert_eq!(rs.cursor, 4);
    }

    #[test]
    fn rename_delete_back_char_at_zero_is_noop_when_no_selection() {
        let mut rs = make_rename_state("hello", 0, None);
        rs.delete_back_char();
        assert_eq!(rs.text, "hello");
        assert_eq!(rs.cursor, 0);
    }

    #[test]
    fn rename_delete_forward_char_with_selection_only_deletes_selection() {
        let mut rs = make_rename_state("hello world", 5, Some(0));
        rs.delete_forward_char();
        assert_eq!(rs.text, " world");
        assert_eq!(rs.cursor, 0);
        assert_eq!(rs.selection_anchor, None);
    }

    #[test]
    fn rename_delete_forward_char_without_selection_deletes_char_after() {
        let mut rs = make_rename_state("hello", 0, None);
        rs.delete_forward_char();
        assert_eq!(rs.text, "ello");
        assert_eq!(rs.cursor, 0);
    }

    #[test]
    fn rename_delete_forward_char_at_end_is_noop_when_no_selection() {
        let mut rs = make_rename_state("hello", 5, None);
        rs.delete_forward_char();
        assert_eq!(rs.text, "hello");
        assert_eq!(rs.cursor, 5);
    }

    #[test]
    fn rename_delete_word_left_with_selection_only_deletes_selection() {
        // Word delete with active selection should NOT also delete a word —
        // standard UX: selection takes precedence.
        let mut rs = make_rename_state("hello world", 5, Some(0));
        rs.delete_word_left();
        assert_eq!(rs.text, " world");
        assert_eq!(rs.cursor, 0);
        assert_eq!(rs.selection_anchor, None);
    }

    #[test]
    fn rename_delete_word_right_with_selection_only_deletes_selection() {
        let mut rs = make_rename_state("hello world", 5, Some(0));
        rs.delete_word_right();
        assert_eq!(rs.text, " world");
        assert_eq!(rs.cursor, 0);
        assert_eq!(rs.selection_anchor, None);
    }

    // ── Stage H: T1.5 — Pre-select all text on rename open ──

    #[test]
    fn start_rename_preselects_existing_name() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1); // group named "Group 1"

        app.start_rename(0);
        let rs = app.rename.as_ref().expect("rename state set");
        assert_eq!(rs.text, "Group 1");
        assert_eq!(rs.cursor, "Group 1".len());
        assert_eq!(rs.selection_anchor, Some(0));
        assert_eq!(rs.selection_range(), Some((0, "Group 1".len())));
    }

    #[test]
    fn start_session_rename_preselects() {
        let mut app = make_app();
        app.start_session_rename("my-session");
        let rs = app.rename.as_ref().unwrap();
        assert_eq!(rs.cursor, "my-session".len());
        assert_eq!(rs.selection_anchor, Some(0));
    }

    #[test]
    fn start_tab_rename_with_existing_prefix_preselects() {
        let mut app = make_app();
        add_item(&mut app, 1, "Firefox");
        app.items[0].custom_prefix = "Browser".to_string();
        app.start_tab_rename(1);
        let rs = app.rename.as_ref().unwrap();
        assert_eq!(rs.text, "Browser");
        assert_eq!(rs.selection_anchor, Some(0));
        assert_eq!(rs.selection_range(), Some((0, "Browser".len())));
    }

    #[test]
    fn start_tab_rename_with_empty_prefix_no_selection() {
        // Empty initial text → no selection (anchor stays None) so the very
        // first typed char doesn't create a phantom selection from cursor 0
        // to cursor 1.
        let mut app = make_app();
        add_item(&mut app, 1, "Firefox");
        app.start_tab_rename(1);
        let rs = app.rename.as_ref().unwrap();
        assert_eq!(rs.text, "");
        assert_eq!(rs.selection_anchor, None);
    }

    #[test]
    fn typing_after_preselect_replaces_text() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1); // "Group 1"
        app.start_rename(0);

        // Simulate typing 'X' into the pre-selected field.
        let rs = app.rename.as_mut().unwrap();
        rs.insert_char('X');

        assert_eq!(rs.text, "X");
        assert_eq!(rs.cursor, 1);
        // Important: no phantom selection of the just-typed char.
        assert_eq!(rs.selection_anchor, None);
    }

    // ── Stage H: keysym-column fallback (regression for shift+arrow / Home / End) ──

    #[test]
    fn select_keysym_no_shift_returns_col_zero() {
        // keycode for 'a': col0=0x61 ('a'), col1=0x41 ('A')
        assert_eq!(super::select_keysym(&[0x61, 0x41], 0x00), 0x61);
    }

    #[test]
    fn select_keysym_shift_returns_col_one_when_distinct() {
        assert_eq!(super::select_keysym(&[0x61, 0x41], 0x01), 0x41);
    }

    #[test]
    fn select_keysym_shift_falls_back_to_col_zero_when_col_one_is_no_symbol() {
        // Arrow key: col0=0xff51 (Left), col1=0 (NoSymbol). Shift+Left should
        // still return Left so the rename handler's match arm fires.
        assert_eq!(super::select_keysym(&[0xff51, 0x0], 0x01), 0xff51);
    }

    #[test]
    fn select_keysym_shift_falls_back_when_only_one_column() {
        // Some keycodes have only one symbol — must still return it under shift.
        assert_eq!(super::select_keysym(&[0xff50], 0x01), 0xff50);
    }

    #[test]
    fn select_keysym_empty_row_returns_zero() {
        assert_eq!(super::select_keysym(&[], 0x00), 0);
    }

    // ── Stage G / T3.1: DropTarget classifier ──

    fn setup_classifier_app() -> App {
        // [Header(g0), Window(1,g0), Window(2,g0), Window(3,None)]
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        add_item(&mut app, 3, "C");
        app.create_group(1);
        app.add_to_group(0, 2);
        app
    }

    #[test]
    fn classify_drop_on_group_header_joins_at_zero() {
        let app = setup_classifier_app();
        // Drag window C (row 3, ungrouped) onto group header (row 0)
        let header_y = app.row_y(0) + 5;
        assert_eq!(
            super::classify_drop(&app, 3, header_y),
            super::DropTarget::JoinGroup { gid: 0, at: 0 }
        );
    }

    #[test]
    fn classify_drop_into_group_body_top_half_joins_at_member_pos() {
        // G-1 fix: drop into the upper half of a member row joins ABOVE it.
        let app = setup_classifier_app();
        let row1_y = app.row_y(1) + 2; // upper half of member row 1 (window 1)
        assert_eq!(
            super::classify_drop(&app, 3, row1_y),
            super::DropTarget::JoinGroup { gid: 0, at: 0 }
        );
    }

    #[test]
    fn classify_drop_into_group_body_bottom_half_joins_at_next_pos() {
        let app = setup_classifier_app();
        let row1_bottom_y = app.row_y(1) + ITEM_H as i16 - 1;
        assert_eq!(
            super::classify_drop(&app, 3, row1_bottom_y),
            super::DropTarget::JoinGroup { gid: 0, at: 1 }
        );
    }

    #[test]
    fn classify_drop_in_spacing_below_last_member_joins_at_end() {
        // G-2 fix: dropping in the small spacing right after the last member
        // of a group still counts as in-group (joins at end), not eject.
        let app = setup_classifier_app();
        // Last member is row 2; spacing below extends to row_y(3).
        let in_spacing = app.row_y(2) + ITEM_H as i16 + 1;
        assert_eq!(
            super::classify_drop(&app, 3, in_spacing),
            super::DropTarget::JoinGroup { gid: 0, at: 2 }
        );
    }

    #[test]
    fn classify_drop_within_same_group_reorders() {
        // Drop window B (row 2, in group) onto window A (row 1, in group).
        let app = setup_classifier_app();
        let row1_y = app.row_y(1) + 2;
        assert_eq!(
            super::classify_drop(&app, 2, row1_y),
            super::DropTarget::ReorderInGroup { gid: 0, to: 0 }
        );
    }

    #[test]
    fn classify_drop_on_member_from_outside_joins_at_pos() {
        // Drop window C (ungrouped, row 3) onto member row 2 — bottom half.
        let app = setup_classifier_app();
        let row2_bottom = app.row_y(2) + ITEM_H as i16 - 1;
        assert_eq!(
            super::classify_drop(&app, 3, row2_bottom),
            super::DropTarget::JoinGroup { gid: 0, at: 2 }
        );
    }

    #[test]
    fn classify_drop_clearly_outside_group_extracts() {
        // Drop window B (in-group, row 2) ONTO row 3 (an ungrouped window).
        // This is "clearly outside" the group → InsertBefore (extracts).
        let app = setup_classifier_app();
        let row3_top = app.row_y(3) + 2;
        assert_eq!(
            super::classify_drop(&app, 2, row3_top),
            super::DropTarget::InsertBefore(3)
        );
    }

    #[test]
    fn classify_drop_above_first_row_inserts_at_zero() {
        let app = setup_classifier_app();
        let above = app.row_y(0) - 5;
        assert_eq!(
            super::classify_drop(&app, 3, above),
            super::DropTarget::InsertBefore(0)
        );
    }

    #[test]
    fn classify_drop_past_last_row_is_insert_at_end() {
        let app = setup_classifier_app();
        let below = app.row_y(3) + (ITEM_H as i16) * 5;
        assert_eq!(
            super::classify_drop(&app, 1, below),
            super::DropTarget::InsertAtEnd
        );
    }

    #[test]
    fn classify_drop_invalid_source_row_is_noop() {
        let app = setup_classifier_app();
        assert_eq!(
            super::classify_drop(&app, 99, 100),
            super::DropTarget::NoOp
        );
    }

    // Session-source drag-reorder no longer applies — sessions live as
    // members of the auto-managed TmuxSystem group from T4.4 onward and
    // are not user-reorderable. T4.6 adds the explicit NoOp test.

    #[test]
    fn classify_drop_collapsed_group_header_still_joins() {
        let app = {
            let mut a = setup_classifier_app();
            a.toggle_collapse(0);
            a
        };
        // Now display_rows = [Header(0), Window(3,None)]
        let header_y = a_row_y(&app, 0) + 5;
        assert_eq!(
            super::classify_drop(&app, 1, header_y),
            super::DropTarget::JoinGroup { gid: 0, at: 0 }
        );
    }

    fn a_row_y(app: &App, i: usize) -> i16 {
        app.row_y(i)
    }

    // ── Stage F / Phase 2a: profile-aware paths + atomic writes ──

    #[test]
    fn data_dir_under_profile_default() {
        // The new persistence root should live under ~/.config/ptm/profiles/default/.
        // We can't assume HOME — just check the trailing path components.
        let p = super::data_dir_in(std::path::Path::new("/some/home"));
        let parts: Vec<&str> = p
            .components()
            .map(|c: std::path::Component| c.as_os_str().to_str().unwrap())
            .collect();
        // Expect […, ".config", "ptm", "profiles", "default"]
        assert_eq!(&parts[parts.len() - 4..], &[".config", "ptm", "profiles", "default"]);
    }

    #[test]
    fn legacy_data_dir_is_one_level_up() {
        let p = super::legacy_data_dir_in(std::path::Path::new("/some/home"));
        let parts: Vec<&str> = p
            .components()
            .map(|c: std::path::Component| c.as_os_str().to_str().unwrap())
            .collect();
        assert_eq!(&parts[parts.len() - 2..], &[".config", "ptm"]);
    }

    #[test]
    fn write_atomic_creates_file_with_content() {
        let dir = std::env::temp_dir().join("ptm_test_atom_create");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("groups");

        super::write_atomic(&path, b"hello world").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"hello world");
        assert!(!path.with_extension("tmp").exists(), "tmp should be cleaned up");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_overwrites_existing_file() {
        let dir = std::env::temp_dir().join("ptm_test_atom_overwrite");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("groups");

        std::fs::write(&path, b"old").unwrap();
        super::write_atomic(&path, b"new").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_overwrites_stale_tmp_file() {
        // If a previous mid-write crash left .tmp behind, write_atomic must
        // overwrite it cleanly rather than fail.
        let dir = std::env::temp_dir().join("ptm_test_atom_stale_tmp");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("groups");
        std::fs::write(&path, b"existing real content").unwrap();
        std::fs::write(path.with_extension("tmp"), b"junk from prior crash").unwrap();

        super::write_atomic(&path, b"fresh").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"fresh");
        assert!(!path.with_extension("tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_real_file_intact_after_simulated_partial_write() {
        // Simulate a partial write by manually creating a .tmp with junk
        // (as if the previous PTM died mid-write). The REAL file should
        // never be observed in a corrupt state — it stays as last-good
        // until the rename succeeds.
        let dir = std::env::temp_dir().join("ptm_test_atom_partial");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("groups");
        std::fs::write(&path, b"last-good content").unwrap();
        std::fs::write(path.with_extension("tmp"), b"partial garbage").unwrap();

        // At this snapshot in time (before any new save), the real file is intact.
        assert_eq!(std::fs::read(&path).unwrap(), b"last-good content");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_moves_legacy_files_to_new_dir() {
        let root = std::env::temp_dir().join("ptm_test_migrate_basic");
        let _ = std::fs::remove_dir_all(&root);
        let legacy = root.join("legacy");
        let new = root.join("new");
        std::fs::create_dir_all(&legacy).unwrap();

        // Pre-populate legacy files
        std::fs::write(legacy.join("groups"), b"v1\nGROUP\tFoo\t0\n").unwrap();
        std::fs::write(legacy.join("geometry"), b"100 200 300 400\n").unwrap();

        super::migrate_legacy_files(&legacy, &new);

        // Legacy files moved
        assert!(!legacy.join("groups").exists());
        assert!(!legacy.join("geometry").exists());
        // New files exist with expected content
        assert_eq!(
            std::fs::read(new.join("groups")).unwrap(),
            b"v1\nGROUP\tFoo\t0\n"
        );
        assert_eq!(
            std::fs::read(new.join("geometry")).unwrap(),
            b"100 200 300 400\n"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_does_not_clobber_existing_new_file() {
        let root = std::env::temp_dir().join("ptm_test_migrate_no_clobber");
        let _ = std::fs::remove_dir_all(&root);
        let legacy = root.join("legacy");
        let new = root.join("new");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&new).unwrap();

        std::fs::write(legacy.join("groups"), b"OLD").unwrap();
        std::fs::write(new.join("groups"), b"NEW").unwrap();

        super::migrate_legacy_files(&legacy, &new);

        // New file untouched; legacy file left alone (avoid silent data loss)
        assert_eq!(std::fs::read(new.join("groups")).unwrap(), b"NEW");
        assert_eq!(std::fs::read(legacy.join("groups")).unwrap(), b"OLD");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_is_idempotent() {
        let root = std::env::temp_dir().join("ptm_test_migrate_idempotent");
        let _ = std::fs::remove_dir_all(&root);
        let legacy = root.join("legacy");
        let new = root.join("new");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("groups"), b"v1\nGROUP\tFoo\t0\n").unwrap();

        super::migrate_legacy_files(&legacy, &new);
        super::migrate_legacy_files(&legacy, &new); // second run

        // After 2 runs the result is the same as after 1.
        assert!(!legacy.join("groups").exists());
        assert_eq!(
            std::fs::read(new.join("groups")).unwrap(),
            b"v1\nGROUP\tFoo\t0\n"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_with_nothing_to_do_is_noop() {
        let root = std::env::temp_dir().join("ptm_test_migrate_noop");
        let _ = std::fs::remove_dir_all(&root);
        let legacy = root.join("legacy");
        let new = root.join("new");
        // Neither directory contains anything.

        super::migrate_legacy_files(&legacy, &new);

        // Doesn't crash; doesn't leave junk.
        assert!(!new.join("groups").exists());
        assert!(!new.join("geometry").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    // ── Stage F / Phase 2b: dirty-flag + debounced save ──

    #[test]
    fn app_starts_not_dirty() {
        let app = make_app();
        assert!(!app.is_dirty());
    }

    #[test]
    fn create_group_marks_dirty() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);
        assert!(app.is_dirty());
    }

    #[test]
    fn delete_group_marks_dirty() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);
        app.clear_dirty();
        app.delete_group(0);
        assert!(app.is_dirty());
    }

    #[test]
    fn add_to_group_marks_dirty() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        add_item(&mut app, 2, "B");
        app.create_group(1);
        app.clear_dirty();
        app.add_to_group(0, 2);
        assert!(app.is_dirty());
    }

    #[test]
    fn remove_from_group_marks_dirty() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);
        app.clear_dirty();
        app.remove_from_group(1);
        assert!(app.is_dirty());
    }

    #[test]
    fn toggle_collapse_marks_dirty() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);
        app.clear_dirty();
        app.toggle_collapse(0);
        assert!(app.is_dirty());
    }

    #[test]
    fn commit_rename_with_change_marks_dirty() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1); // "Group 1"
        app.clear_dirty();
        app.start_rename(0);
        if let Some(ref mut rs) = app.rename {
            rs.text = "Renamed".to_string();
        }
        app.commit_rename();
        assert!(app.is_dirty());
    }

    #[test]
    fn cancel_rename_does_not_mark_dirty() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);
        app.clear_dirty();
        app.start_rename(0);
        if let Some(ref mut rs) = app.rename {
            rs.text = "Renamed".to_string();
        }
        app.cancel_rename();
        assert!(!app.is_dirty());
    }

    #[test]
    fn restore_groups_does_not_leave_app_dirty() {
        let mut app = make_app();
        add_item_with_class(&mut app, 1, "Foo", "FooClass");
        let saved = vec![SavedGroup {
            name: "G".to_string(),
            collapsed: false,
            kind: GroupKind::Normal,
            members: vec![SavedMember {
                label: "Foo".to_string(),
                wm_class: "FooClass".to_string(),
                custom_prefix: String::new(),
            }],
        }];
        super::restore_groups(&mut app, &saved);
        // Restoration mirrors on-disk state; should not request a save.
        assert!(!app.is_dirty());
    }

    // Pure debounce/backstop logic (testable without Instant juggling):
    use std::time::{Duration, Instant};

    #[test]
    fn should_save_now_returns_false_when_clean() {
        let now = Instant::now();
        assert!(!super::should_save_now(None, None, now));
    }

    #[test]
    fn should_save_now_false_within_debounce_window() {
        let now = Instant::now();
        let last = now - Duration::from_millis(100);
        let first = last;
        assert!(!super::should_save_now(Some(first), Some(last), now));
    }

    #[test]
    fn should_save_now_true_after_debounce_idle() {
        let now = Instant::now();
        let last = now - Duration::from_millis(300);
        let first = last;
        assert!(super::should_save_now(Some(first), Some(last), now));
    }

    #[test]
    fn should_save_now_true_via_backstop_even_when_still_mutating() {
        // User keeps mutating: last is recent (within debounce) but first is
        // long ago — the backstop must fire so we don't lose >30s of work.
        let now = Instant::now();
        let last = now - Duration::from_millis(50); // still actively typing
        let first = now - Duration::from_secs(31); // started 31s ago
        assert!(super::should_save_now(Some(first), Some(last), now));
    }

    #[test]
    fn mark_dirty_sets_first_and_last_timestamps() {
        let mut app = make_app();
        assert!(app.first_dirty_at.is_none());
        app.mark_dirty();
        let first = app.first_dirty_at.expect("first set");
        let last = app.last_dirty_at.expect("last set");
        assert!(first <= last);
    }

    #[test]
    fn mark_dirty_twice_preserves_first_updates_last() {
        let mut app = make_app();
        app.mark_dirty();
        let first1 = app.first_dirty_at.unwrap();
        let last1 = app.last_dirty_at.unwrap();
        // Force some time to pass for the second mark.
        std::thread::sleep(Duration::from_millis(2));
        app.mark_dirty();
        let first2 = app.first_dirty_at.unwrap();
        let last2 = app.last_dirty_at.unwrap();
        assert_eq!(first1, first2, "first preserved");
        assert!(last2 > last1, "last advances");
    }

    #[test]
    fn clear_dirty_resets_both_timestamps() {
        let mut app = make_app();
        app.mark_dirty();
        app.clear_dirty();
        assert!(app.first_dirty_at.is_none());
        assert!(app.last_dirty_at.is_none());
        assert!(!app.is_dirty());
    }

    // ── T1.7: auto-rename on new group create ──

    #[test]
    fn create_group_via_menu_immediately_starts_rename_with_text_preselected() {
        // UX: when the user makes a new group via right-click → "New Group",
        // they almost always want to name it. Skip the second right-click +
        // "Rename Group" by entering rename mode immediately, with the
        // default name pre-selected so a single keystroke replaces it.
        // Pressing Enter without typing accepts "Group N".
        let mut app = make_app();
        add_item(&mut app, 1, "term");
        app.build_display_rows();

        let _ = super::execute_menu_action(&mut app, super::MenuAction::CreateGroup, 0);

        let rs = app.rename.as_ref().expect("rename should be active after CreateGroup");
        let gid = match rs.target {
            super::RenameTarget::Group(g) => g,
            _ => panic!("expected Group rename target, got {:?}", "non-group"),
        };
        // Should target the just-created group
        assert!(app.groups.iter().any(|g| g.id == gid), "rename targets a real group");
        // Default name pre-populated, cursor at end, full text selected
        assert_eq!(rs.text, "Group 1");
        assert_eq!(rs.cursor, rs.text.len());
        assert_eq!(rs.selection_anchor, Some(0));
    }

    #[test]
    fn save_groups_to_uses_atomic_write() {
        // After save, the on-disk file matches expected content and no .tmp
        // sibling is left behind — proves the rename happened.
        let dir = std::env::temp_dir().join("ptm_test_save_atomic");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("groups");

        let saved = vec![SavedGroup {
            name: "Foo".to_string(),
            collapsed: false,
            kind: GroupKind::Normal,
            members: vec![],
        }];
        super::save_groups_to(&path, &saved);

        assert!(path.exists());
        assert!(!path.with_extension("tmp").exists(), "no tmp leftover");

        // Loaded round-trip works
        let loaded = super::load_groups_from(&path).expect("loads");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Foo");

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn add_attached_item(app: &mut App, wid: u32, label: &str, session: &str) {
        app.items.push(Item {
            wid,
            label: label.to_string(),
            wm_class: "test".to_string(),
            accent_pixel: 0,
            custom_prefix: String::new(),
            session: Some(session.to_string()),
        });
        app.display_order.push(DisplaySlot::Window(wid));
        app.build_display_rows();
    }

    // ── T4.1: confirmation popup ──

    fn make_test_confirm(action: super::ConfirmAction) -> super::ConfirmPopup {
        super::ConfirmPopup {
            window: 0,
            pixmap: 0,
            message: "Kill session?".to_string(),
            action,
            width: 240,
            height: 80,
            yes_rect: super::Rectangle { x: 0, y: 0, width: 0, height: 0 },
            no_rect: super::Rectangle { x: 0, y: 0, width: 0, height: 0 },
            hover_button: None,
        }
    }

    #[test]
    fn confirm_popup_with_yes_returns_action() {
        let mut app = make_app();
        app.confirm = Some(make_test_confirm(
            super::ConfirmAction::KillSession("demo".to_string()),
        ));
        let action = super::dispatch_confirm(&app, true);
        assert!(
            matches!(action, Some(super::ConfirmAction::KillSession(ref n)) if n == "demo"),
            "expected KillSession(demo), got {:?}",
            action
        );
        assert!(
            app.confirm.is_some(),
            "dispatch must not consume the popup; close_confirm_popup is responsible for that"
        );
    }

    #[test]
    fn confirm_popup_with_no_returns_none_without_consuming() {
        let mut app = make_app();
        app.confirm = Some(make_test_confirm(
            super::ConfirmAction::KillSession("demo".to_string()),
        ));
        let action = super::dispatch_confirm(&app, false);
        assert!(action.is_none(), "rejected dispatch returns no action");
        assert!(
            app.confirm.is_some(),
            "dispatch must not consume the popup; close_confirm_popup is responsible for that"
        );
    }

    #[test]
    fn dispatch_confirm_with_no_popup_returns_none() {
        let app = make_app();
        assert!(app.confirm.is_none());
        let action = super::dispatch_confirm(&app, true);
        assert!(action.is_none());
    }

    #[test]
    fn dispatch_confirm_does_not_consume_popup() {
        let mut app = make_app();
        app.confirm = Some(make_test_confirm(
            super::ConfirmAction::KillSession("demo".to_string()),
        ));
        let _ = super::dispatch_confirm(&app, true);
        let _ = super::dispatch_confirm(&app, false);
        let _ = super::dispatch_confirm(&app, true);
        assert!(
            app.confirm.is_some(),
            "repeated dispatch_confirm calls must be idempotent on app.confirm"
        );
    }

    // ── T4.2: kill tmux session from attached terminal row ──

    #[test]
    fn menu_for_attached_terminal_includes_kill_session() {
        let mut app = make_app();
        add_attached_item(&mut app, 1, "term", "demo");
        let entries = super::build_menu_entries(&app, 0);
        let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
        assert!(
            labels.contains(&"Kill tmux session"),
            "expected 'Kill tmux session' in {:?}",
            labels
        );
        assert!(
            entries
                .iter()
                .any(|e| matches!(e.action, super::MenuAction::KillSession)),
            "expected KillSession action"
        );
    }

    #[test]
    fn menu_for_unattached_terminal_excludes_kill_session() {
        let mut app = make_app();
        add_item(&mut app, 1, "term"); // session: None
        app.build_display_rows();
        let entries = super::build_menu_entries(&app, 0);
        assert!(
            !entries
                .iter()
                .any(|e| matches!(e.action, super::MenuAction::KillSession)),
            "unattached window must not show KillSession"
        );
    }

    #[test]
    fn menu_for_grouped_attached_terminal_includes_kill_session() {
        let mut app = make_app();
        add_attached_item(&mut app, 1, "term", "demo");
        app.create_group(1);
        // Row 0 = header, row 1 = the grouped attached window.
        let entries = super::build_menu_entries(&app, 1);
        assert!(
            entries
                .iter()
                .any(|e| matches!(e.action, super::MenuAction::KillSession)),
            "grouped attached terminal must show KillSession"
        );
    }

    #[test]
    fn kill_session_from_window_dispatch_returns_confirm_request() {
        let mut app = make_app();
        add_attached_item(&mut app, 1, "term", "demo");
        let req = super::execute_menu_action(&mut app, super::MenuAction::KillSession, 0)
            .expect("expected ConfirmRequest follow-up");
        assert!(
            matches!(req.action, super::ConfirmAction::KillSession(ref n) if n == "demo"),
            "expected KillSession(demo), got {:?}",
            req.action
        );
        assert!(
            req.message.contains("demo"),
            "expected message to mention session name, got {:?}",
            req.message
        );
        // App state is not yet mutated — popup hasn't materialized.
        assert!(app.confirm.is_none());
    }

    // ── T4.3: GroupKind ──

    #[test]
    fn group_default_kind_is_normal() {
        let mut app = make_app();
        add_item(&mut app, 1, "A");
        app.create_group(1);
        assert_eq!(app.groups[0].kind, super::GroupKind::Normal);
    }

    // ── T4.4a: loader tolerance for 4-field GROUP line ──

    fn write_groups_file(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join("groups");
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn load_groups_3_field_group_line_treated_as_normal() {
        let dir = std::env::temp_dir().join("ptm_test_load_3field");
        let path = write_groups_file(&dir, "v1\nGROUP\tWork\t0\nMEMBER\tA\tcls\t\n");
        let loaded = super::load_groups_from(&path).expect("loader accepts 3-field");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Work");
        assert!(!loaded[0].collapsed);
        assert_eq!(loaded[0].kind, super::GroupKind::Normal);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_groups_4_field_group_line_with_tmux_system_kind() {
        let dir = std::env::temp_dir().join("ptm_test_load_4field_tmux");
        let path = write_groups_file(&dir, "v1\nGROUP\tTmux Sessions\t1\ttmux_system\n");
        let loaded = super::load_groups_from(&path).expect("loader accepts 4-field");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Tmux Sessions");
        assert!(loaded[0].collapsed);
        assert_eq!(loaded[0].kind, super::GroupKind::TmuxSystem);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_groups_4_field_group_line_with_normal_kind() {
        let dir = std::env::temp_dir().join("ptm_test_load_4field_normal");
        let path = write_groups_file(&dir, "v1\nGROUP\tWork\t0\tnormal\n");
        let loaded = super::load_groups_from(&path).expect("loader accepts 4-field normal");
        assert_eq!(loaded[0].kind, super::GroupKind::Normal);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_groups_4_field_with_unknown_kind_rejects() {
        let dir = std::env::temp_dir().join("ptm_test_load_4field_bad");
        let path = write_groups_file(&dir, "v1\nGROUP\tFoo\t0\tweird\n");
        let loaded = super::load_groups_from(&path);
        assert!(loaded.is_none(), "unknown kind must reject the file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_groups_5_field_rejects() {
        let dir = std::env::temp_dir().join("ptm_test_load_5field");
        let path = write_groups_file(&dir, "v1\nGROUP\tFoo\t0\tnormal\textra\n");
        let loaded = super::load_groups_from(&path);
        assert!(loaded.is_none(), "5-field GROUP must reject");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── T4.4: TmuxSystem group derivation + rendering ──

    fn make_system_group(app: &mut App, sessions: &[&str]) -> u32 {
        let gid = app.next_group_id;
        app.next_group_id += 1;
        let members = sessions
            .iter()
            .map(|s| super::GroupMember {
                label: s.to_string(),
                wm_class: String::new(),
                custom_prefix: String::new(),
                live_wid: None,
            })
            .collect();
        app.groups.push(super::Group {
            id: gid,
            name: "Tmux Sessions".to_string(),
            collapsed: false,
            kind: super::GroupKind::TmuxSystem,
            members,
        });
        app.display_order.push(DisplaySlot::Group(gid));
        app.build_display_rows();
        gid
    }

    #[test]
    fn sync_system_group_appends_new_sessions() {
        let mut group = super::Group {
            id: 0,
            name: "T".into(),
            collapsed: false,
            kind: super::GroupKind::TmuxSystem,
            members: Vec::new(),
        };
        super::sync_system_group_members(
            &mut group,
            &["a".into(), "b".into()],
        );
        assert_eq!(group.members.len(), 2);
        assert_eq!(group.members[0].label, "a");
        assert_eq!(group.members[0].wm_class, "");
        assert_eq!(group.members[0].live_wid, None);
        assert_eq!(group.members[1].label, "b");
    }

    #[test]
    fn sync_system_group_drops_vanished_sessions() {
        let mut group = super::Group {
            id: 0,
            name: "T".into(),
            collapsed: false,
            kind: super::GroupKind::TmuxSystem,
            members: vec![
                super::GroupMember {
                    label: "a".into(),
                    wm_class: String::new(),
                    custom_prefix: String::new(),
                    live_wid: None,
                },
                super::GroupMember {
                    label: "b".into(),
                    wm_class: String::new(),
                    custom_prefix: String::new(),
                    live_wid: None,
                },
            ],
        };
        super::sync_system_group_members(&mut group, &["a".into()]);
        assert_eq!(group.members.len(), 1);
        assert_eq!(group.members[0].label, "a");
    }

    #[test]
    fn sync_system_group_preserves_order_when_appending() {
        let mut group = super::Group {
            id: 0,
            name: "T".into(),
            collapsed: false,
            kind: super::GroupKind::TmuxSystem,
            members: vec![
                super::GroupMember {
                    label: "a".into(),
                    wm_class: String::new(),
                    custom_prefix: String::new(),
                    live_wid: None,
                },
                super::GroupMember {
                    label: "b".into(),
                    wm_class: String::new(),
                    custom_prefix: String::new(),
                    live_wid: None,
                },
            ],
        };
        super::sync_system_group_members(
            &mut group,
            &["a".into(), "b".into(), "c".into()],
        );
        assert_eq!(
            group.members.iter().map(|m| m.label.clone()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn build_display_rows_renders_system_group_as_session_rows() {
        let mut app = make_app();
        make_system_group(&mut app, &["a", "b"]);
        add_item(&mut app, 1, "win"); // ungrouped window
        app.build_display_rows();

        assert_eq!(app.display_rows.len(), 4);
        assert!(matches!(&app.display_rows[0], DisplayRow::GroupHeader { .. }));
        assert!(
            matches!(&app.display_rows[1], DisplayRow::Session { name, group_id: Some(_) } if name == "a"),
            "row 1 should be Session(a) inside system group"
        );
        assert!(
            matches!(&app.display_rows[2], DisplayRow::Session { name, group_id: Some(_) } if name == "b"),
            "row 2 should be Session(b) inside system group"
        );
        assert!(matches!(&app.display_rows[3], DisplayRow::Window { wid: 1, group_id: None }));
    }

    #[test]
    fn build_display_rows_collapsed_system_group_hides_sessions() {
        let mut app = make_app();
        let gid = make_system_group(&mut app, &["a", "b"]);
        // Collapse the system group.
        app.groups.iter_mut().find(|g| g.id == gid).unwrap().collapsed = true;
        app.build_display_rows();
        assert_eq!(app.display_rows.len(), 1);
        assert!(matches!(&app.display_rows[0], DisplayRow::GroupHeader { .. }));
    }

    /// Collapsed group header label is rendered as
    /// `format!("({})", group.display_count())`. For TmuxSystem groups the
    /// count must reflect the number of session members (sessions aren't
    /// windows, so `live_count()` would always be 0 here).
    #[test]
    fn collapsed_tmux_system_group_count_equals_session_count() {
        let mut app = make_app();
        make_system_group(&mut app, &["alpha", "beta", "gamma"]);
        let group = app
            .groups
            .iter()
            .find(|g| g.kind == super::GroupKind::TmuxSystem)
            .expect("system group present");
        assert_eq!(
            group.display_count(),
            3,
            "collapsed Tmux Sessions header should report 3 sessions, got {}",
            group.display_count()
        );
    }

    /// Sanity: Normal groups still report the live (non-ghost) count, so
    /// the existing `live_count()` semantics for windows-in-group is
    /// preserved through the helper.
    #[test]
    fn display_count_for_normal_group_equals_live_count() {
        let mut app = make_app();
        add_item(&mut app, 1, "win-a");
        add_item(&mut app, 2, "win-b");
        // create_group seeds the group with one wid; add_to_group adds the
        // second so we end up with a Normal group of two live members.
        let gid = app.create_group(1);
        app.add_to_group(gid, 2);
        let group = app.groups.iter().find(|g| g.id == gid).expect("group");
        assert_eq!(group.live_count(), 2);
        assert_eq!(group.display_count(), 2);
    }

    #[test]
    fn is_session_attached_returns_true_for_attached_item() {
        let mut app = make_app();
        add_attached_item(&mut app, 1, "term", "demo");
        assert!(super::is_session_attached(&app, "demo"));
    }

    #[test]
    fn is_session_attached_returns_false_for_orphan() {
        let app = make_app();
        assert!(!super::is_session_attached(&app, "ghost"));
    }

    // ── T4.5: ensure_tmux_system_group + writer extension ──

    #[test]
    fn ensure_tmux_system_group_creates_when_absent() {
        let mut app = make_app();
        super::ensure_tmux_system_group(&mut app);
        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].kind, super::GroupKind::TmuxSystem);
        assert!(app.groups[0].collapsed, "default collapsed");
        assert!(app.groups[0].members.is_empty());
    }

    #[test]
    fn ensure_tmux_system_group_idempotent() {
        let mut app = make_app();
        super::ensure_tmux_system_group(&mut app);
        super::ensure_tmux_system_group(&mut app);
        assert_eq!(
            app.groups
                .iter()
                .filter(|g| g.kind == super::GroupKind::TmuxSystem)
                .count(),
            1
        );
    }

    #[test]
    fn ensure_tmux_system_group_appends_to_display_order() {
        let mut app = make_app();
        add_item(&mut app, 1, "win");
        super::ensure_tmux_system_group(&mut app);
        // Order: ungrouped window first, system group appended.
        assert!(matches!(app.display_order.last(), Some(DisplaySlot::Group(_))));
    }

    #[test]
    fn ensure_tmux_system_group_does_not_mark_dirty() {
        let mut app = make_app();
        app.clear_dirty();
        super::ensure_tmux_system_group(&mut app);
        assert!(!app.is_dirty(), "auto-create must be invisible to persistence");
    }

    #[test]
    fn save_load_roundtrip_preserves_system_group_kind() {
        let dir = std::env::temp_dir().join("ptm_test_rt_kind");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("groups");

        let saved = vec![
            SavedGroup {
                name: "Work".into(),
                collapsed: false,
                kind: super::GroupKind::Normal,
                members: vec![],
            },
            SavedGroup {
                name: "Tmux Sessions".into(),
                collapsed: true,
                kind: super::GroupKind::TmuxSystem,
                members: vec![],
            },
        ];
        super::save_groups_to(&path, &saved);
        let loaded = super::load_groups_from(&path).expect("loads");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].kind, super::GroupKind::Normal);
        assert_eq!(loaded[1].kind, super::GroupKind::TmuxSystem);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_load_roundtrip_preserves_system_group_collapse_state() {
        let dir = std::env::temp_dir().join("ptm_test_rt_collapse");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("groups");

        let saved = vec![SavedGroup {
            name: "Tmux Sessions".into(),
            collapsed: false,
            kind: super::GroupKind::TmuxSystem,
            members: vec![],
        }];
        super::save_groups_to(&path, &saved);
        let loaded = super::load_groups_from(&path).expect("loads");
        assert_eq!(loaded.len(), 1);
        assert!(!loaded[0].collapsed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── T4.5b: + New tmux header button ──

    #[test]
    fn top_buttons_layout_one_button_when_tmux_unavailable() {
        let (left, right_opt) = super::top_buttons_layout(false, 250);
        assert!(right_opt.is_none(), "no right button when tmux unavailable");
        assert_eq!(left.x, ITEM_MARGIN);
        // Full width minus margins.
        assert_eq!(left.width as i16, 250 - ITEM_MARGIN * 2);
    }

    #[test]
    fn top_buttons_layout_two_buttons_when_tmux_available() {
        let (left, right_opt) = super::top_buttons_layout(true, 250);
        let right = right_opt.expect("expected right button when tmux available");
        // Side-by-side, separated by TOP_BUTTON_GAP.
        assert_eq!(right.x, left.x + left.width as i16 + super::TOP_BUTTON_GAP);
        // Widths within 1px (rounding lands in right rect).
        let diff = (left.width as i32 - right.width as i32).abs();
        assert!(diff <= 1, "widths must differ by at most 1px (got {} vs {})", left.width, right.width);
        // Both inside the available area.
        assert!(
            left.x + left.width as i16 + super::TOP_BUTTON_GAP + right.width as i16
                <= 250 - ITEM_MARGIN
        );
    }

    #[test]
    fn hit_test_top_buttons_routes_left_to_new_terminal() {
        let mut app = make_app();
        app.tmux_available = true;
        app.width = 250;
        let (left, _) = super::top_buttons_layout(true, 250);
        let center_x = left.x + (left.width as i16 / 2);
        let center_y = left.y + (left.height as i16 / 2);
        assert_eq!(
            app.hit_test_top_buttons(center_x, center_y),
            Some(super::TopButton::NewTerminal)
        );
    }

    #[test]
    fn hit_test_top_buttons_routes_right_to_new_tmux() {
        let mut app = make_app();
        app.tmux_available = true;
        app.width = 250;
        let (_, right_opt) = super::top_buttons_layout(true, 250);
        let right = right_opt.unwrap();
        let center_x = right.x + (right.width as i16 / 2);
        let center_y = right.y + (right.height as i16 / 2);
        assert_eq!(
            app.hit_test_top_buttons(center_x, center_y),
            Some(super::TopButton::NewTmux)
        );
    }

    #[test]
    fn hit_test_top_buttons_unavailable_falls_back_to_full_width() {
        let mut app = make_app();
        app.tmux_available = false;
        app.width = 250;
        // Click anywhere in the header band — should always be NewTerminal,
        // never NewTmux.
        for x in [ITEM_MARGIN + 5, 100, 200, 250 - ITEM_MARGIN - 1] {
            let result = app.hit_test_top_buttons(x, 5);
            assert_eq!(
                result,
                Some(super::TopButton::NewTerminal),
                "x={} should hit NewTerminal",
                x
            );
        }
    }

    #[test]
    fn hit_test_top_buttons_outside_row_returns_none() {
        let mut app = make_app();
        app.tmux_available = true;
        app.width = 250;
        // Below the header row.
        assert!(app.hit_test_top_buttons(100, HEADER_H as i16 + 5).is_none());
        // Above the row.
        assert!(app.hit_test_top_buttons(100, -1).is_none());
    }

    // ── T4.6: drag classifier denies system group as drop target ──

    #[test]
    fn classify_drop_window_into_system_group_header_is_noop() {
        let mut app = make_app();
        add_item(&mut app, 1, "win"); // ungrouped window source (row 0)
        make_system_group(&mut app, &[]); // empty system group (row 1)
        app.build_display_rows();
        // [Window(1), GroupHeader(system)]
        let header_y = app.row_y(1) + 2;
        assert_eq!(
            super::classify_drop(&app, 0, header_y),
            super::DropTarget::NoOp
        );
    }

    #[test]
    fn classify_drop_window_into_system_group_body_is_noop() {
        let mut app = make_app();
        add_item(&mut app, 1, "win"); // row 0
        make_system_group(&mut app, &["a", "b"]); // header + 2 sessions = rows 1..3
        app.build_display_rows();
        // [Window(1), GroupHeader, Session(a), Session(b)]
        // Drop window onto session row 'a'.
        let body_y = app.row_y(2) + 2;
        assert_eq!(
            super::classify_drop(&app, 0, body_y),
            super::DropTarget::NoOp
        );
        // And onto 'b' (lower half too).
        let body_y_b = app.row_y(3) + (ITEM_H as i16) - 2;
        assert_eq!(
            super::classify_drop(&app, 0, body_y_b),
            super::DropTarget::NoOp
        );
    }

    #[test]
    fn classify_drop_session_source_is_noop() {
        let mut app = make_app();
        make_system_group(&mut app, &["a", "b"]);
        add_item(&mut app, 1, "win");
        app.build_display_rows();
        // [GroupHeader, Session(a), Session(b), Window(1)]
        // Try dragging session 'a' (row 1) anywhere — should NoOp.
        let target_y = app.row_y(3) + 2;
        assert_eq!(
            super::classify_drop(&app, 1, target_y),
            super::DropTarget::NoOp
        );
    }

    #[test]
    fn classify_drop_system_group_header_as_source_reorders_normally() {
        let mut app = make_app();
        add_item(&mut app, 1, "win"); // row 0
        make_system_group(&mut app, &["a"]); // rows 1, 2
        app.build_display_rows();
        // [Window(1), GroupHeader, Session(a)]
        // Drag header (row 1) above row 0 → InsertBefore(0).
        let above = app.row_y(0) - 5;
        assert_eq!(
            super::classify_drop(&app, 1, above),
            super::DropTarget::InsertBefore(0)
        );
    }

    #[test]
    fn is_target_system_group_returns_true_for_system() {
        let mut app = make_app();
        let gid = make_system_group(&mut app, &[]);
        assert!(super::is_target_system_group(&app, gid));
    }

    #[test]
    fn is_target_system_group_returns_false_for_normal() {
        let mut app = make_app();
        add_item(&mut app, 1, "win");
        let gid = app.create_group(1);
        assert!(!super::is_target_system_group(&app, gid));
    }

    #[test]
    fn is_target_system_group_returns_false_for_unknown_gid() {
        let app = make_app();
        assert!(!super::is_target_system_group(&app, 9999));
    }

    #[test]
    fn menu_for_system_group_header_excludes_delete_group() {
        let mut app = make_app();
        make_system_group(&mut app, &[]);
        let entries = super::build_menu_entries(&app, 0);
        assert!(
            entries.iter().any(|e| matches!(e.action, super::MenuAction::RenameGroup)),
            "Rename Group must remain"
        );
        assert!(
            !entries.iter().any(|e| matches!(e.action, super::MenuAction::DeleteGroup)),
            "Delete Group must be suppressed for the system group"
        );
    }

    // ── T4.7: [x] glyph hit-test ──

    #[test]
    fn hit_test_session_close_button_inside_band() {
        let row_w: i16 = 220;
        // Just inside the right edge.
        assert!(super::hit_test_session_close_button(row_w - 1, row_w));
        // Middle of band.
        assert!(super::hit_test_session_close_button(
            row_w - super::SESSION_CLOSE_BAND_WIDTH / 2,
            row_w,
        ));
    }

    #[test]
    fn hit_test_session_close_button_outside_band() {
        let row_w: i16 = 220;
        // Far left.
        assert!(!super::hit_test_session_close_button(8, row_w));
        // Just left of the band.
        assert!(!super::hit_test_session_close_button(
            row_w - super::SESSION_CLOSE_BAND_WIDTH - 1,
            row_w,
        ));
    }

    #[test]
    fn hit_test_session_close_button_at_left_edge() {
        let row_w: i16 = 220;
        assert!(!super::hit_test_session_close_button(0, row_w));
    }

    // ── T4.8: single-click [x] dispatch ──

    #[test]
    fn click_close_band_on_grouped_session_returns_kill_request() {
        let row_w: i16 = 220;
        let local_x = row_w - 4; // inside the close band
        let req = super::dispatch_session_click("demo", Some(0), local_x, row_w)
            .expect("expected kill request");
        assert!(
            matches!(req.action, super::ConfirmAction::KillSession(ref n) if n == "demo"),
            "expected KillSession(demo), got {:?}",
            req.action
        );
        assert!(req.message.contains("demo"));
    }

    #[test]
    fn click_session_body_returns_no_request() {
        let row_w: i16 = 220;
        let local_x = 50; // far from the close band
        assert!(super::dispatch_session_click("demo", Some(0), local_x, row_w).is_none());
    }

    #[test]
    fn click_close_band_on_ungrouped_session_returns_no_request() {
        // Defensive: sessions live inside the system group from T4.4 onward.
        // If a stray Session row ever appears outside a group, the close band
        // must not trigger — fall through to the normal click path.
        let row_w: i16 = 220;
        let local_x = row_w - 4;
        assert!(super::dispatch_session_click("demo", None, local_x, row_w).is_none());
    }

    #[test]
    fn hit_test_session_close_button_past_right_edge() {
        let row_w: i16 = 220;
        // Just past the row — out of bounds, not a click.
        assert!(!super::hit_test_session_close_button(row_w, row_w));
        assert!(!super::hit_test_session_close_button(row_w + 5, row_w));
    }

    #[test]
    fn menu_for_normal_group_header_includes_delete_group() {
        let mut app = make_app();
        add_item(&mut app, 1, "win");
        app.create_group(1);
        let entries = super::build_menu_entries(&app, 0);
        assert!(
            entries.iter().any(|e| matches!(e.action, super::MenuAction::DeleteGroup)),
            "normal group keeps Delete Group entry"
        );
    }

    #[test]
    fn hit_test_top_buttons_in_gap_returns_none() {
        let mut app = make_app();
        app.tmux_available = true;
        app.width = 250;
        let (left, _) = super::top_buttons_layout(true, 250);
        // The gap is between the two buttons.
        let gap_x = left.x + left.width as i16 + 1;
        let gap_y = left.y + (left.height as i16 / 2);
        assert!(app.hit_test_top_buttons(gap_x, gap_y).is_none());
    }

    #[test]
    fn writer_emits_4_field_group_line() {
        // Belt-and-braces: verify the on-disk byte sequence so we catch
        // accidental field reorder / format drift.
        let dir = std::env::temp_dir().join("ptm_test_writer_format");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("groups");

        let saved = vec![SavedGroup {
            name: "Foo".into(),
            collapsed: true,
            kind: super::GroupKind::TmuxSystem,
            members: vec![],
        }];
        super::save_groups_to(&path, &saved);
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body, "v1\nGROUP\tFoo\t1\ttmux_system\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn kill_session_from_orphan_session_dispatch_returns_no_confirm() {
        let mut app = make_app();
        push_session(&mut app, "ghost");
        // Row 0 is the system group header; row 1 is the session.
        let req = super::execute_menu_action(&mut app, super::MenuAction::KillSession, 1);
        // Orphan path keeps direct invocation (no popup) — but in tests the
        // tmux command may or may not actually exist; what we verify is that
        // no ConfirmRequest comes back and the member is dropped optimistically.
        assert!(req.is_none(), "orphan session path returns no confirm");
        let group = app
            .groups
            .iter()
            .find(|g| g.kind == super::GroupKind::TmuxSystem)
            .expect("system group present");
        assert!(
            !group.members.iter().any(|m| m.label == "ghost"),
            "expected member 'ghost' to be optimistically removed"
        );
    }

    #[test]
    fn execute_confirm_kill_optimistically_removes_session_member() {
        let mut app = make_app();
        push_session(&mut app, "ghost");
        assert!(
            app.display_rows.iter().any(|r| matches!(
                r,
                super::DisplayRow::Session { name, .. } if name == "ghost"
            )),
            "pre-condition: 'ghost' row should exist before kill"
        );
        super::execute_confirm_action(&mut app, super::ConfirmAction::KillSession("ghost".to_string()));
        let group = app
            .groups
            .iter()
            .find(|g| g.kind == super::GroupKind::TmuxSystem)
            .expect("system group present");
        assert!(
            !group.members.iter().any(|m| m.label == "ghost"),
            "expected member 'ghost' to be optimistically removed after popup-accept kill"
        );
        assert!(
            !app.display_rows.iter().any(|r| matches!(
                r,
                super::DisplayRow::Session { name, .. } if name == "ghost"
            )),
            "expected display row for 'ghost' to be gone after popup-accept kill"
        );
    }

    // ── Session origin tracking + glyph helpers (RED-first per plan) ──

    #[test]
    fn session_origin_recorded_on_first_sighting() {
        let mut origins: HashMap<String, String> = HashMap::new();
        let sessions = vec![("$0".to_string(), "0".to_string(), false)];
        super::update_session_origins(&mut origins, &sessions);
        assert_eq!(origins.get("$0").map(String::as_str), Some("0"));
    }

    #[test]
    fn session_origin_preserved_across_rename() {
        // Session $0 was originally "0"; the user has since renamed it to
        // "myproject". A subsequent refresh must NOT overwrite the origin.
        let mut origins: HashMap<String, String> = HashMap::new();
        origins.insert("$0".to_string(), "0".to_string());
        let sessions = vec![("$0".to_string(), "myproject".to_string(), false)];
        super::update_session_origins(&mut origins, &sessions);
        assert_eq!(
            origins.get("$0").map(String::as_str),
            Some("0"),
            "origin must remain '0' after a rename observation"
        );
    }

    #[test]
    fn session_origin_dropped_when_session_disappears() {
        // Origin entries for sessions absent from the live list get pruned
        // so the map size stays bounded over long PTM lifetimes.
        let mut origins: HashMap<String, String> = HashMap::new();
        origins.insert("$0".to_string(), "0".to_string());
        origins.insert("$1".to_string(), "1".to_string());
        let sessions = vec![("$0".to_string(), "0".to_string(), false)];
        super::update_session_origins(&mut origins, &sessions);
        assert!(origins.contains_key("$0"));
        assert!(
            !origins.contains_key("$1"),
            "origin for vanished session $1 should have been pruned"
        );
    }

    #[test]
    fn format_session_row_label_unrenamed() {
        assert_eq!(super::format_session_row_label("0", "0"), "0");
        assert_eq!(super::format_session_row_label("mywork", "mywork"), "mywork");
    }

    #[test]
    fn format_session_row_label_renamed() {
        assert_eq!(
            super::format_session_row_label("myproject", "0"),
            "myproject (0)"
        );
    }

    #[test]
    fn marker_glyph_truncates_origin_to_two_chars() {
        assert_eq!(super::marker_glyph_for_origin("0"), "0");
        assert_eq!(super::marker_glyph_for_origin("10"), "10");
        assert_eq!(super::marker_glyph_for_origin("mywork"), "my");
        // 3-digit ids are rare but possible if the user keeps the same tmux
        // server alive across hundreds of session creates; truncation is
        // documented and accepted.
        assert_eq!(super::marker_glyph_for_origin("100"), "10");
    }

    #[test]
    fn session_origin_for_name_returns_current_when_unmapped() {
        // Defensive lookup: when a session we just saw has no origin record
        // yet (refresh hasn't called update_session_origins), fall back to
        // the current name so the renderer doesn't render an empty glyph.
        let origins: HashMap<String, String> = HashMap::new();
        let sessions = vec![("$0".to_string(), "0".to_string(), false)];
        assert_eq!(
            super::session_origin_for_name("0", &sessions, &origins),
            "0"
        );
    }
}
