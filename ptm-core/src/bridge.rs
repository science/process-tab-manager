use x11rb_protocol::protocol::Event;

/// Atom IDs needed for event translation.
/// Constructed from the x11 connection's AtomCache.
pub struct AtomIds {
    pub net_client_list: u32,
    pub net_active_window: u32,
    pub net_wm_name: u32,
    pub net_current_desktop: u32,
    pub net_wm_state: u32,
}

/// Domain events — no x11rb types leak out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtmEvent {
    WindowListChanged,
    ActiveWindowChanged,
    WindowTitleChanged(u32),
    WindowStateChanged(u32),
    DesktopChanged,
    WindowDestroyed(u32),
}

/// Pure function: translate an x11rb Event into a PtmEvent (if relevant).
pub fn translate_event(event: &Event, atoms: &AtomIds, root: u32) -> Option<PtmEvent> {
    match event {
        Event::PropertyNotify(pn) if pn.window == root => {
            if pn.atom == atoms.net_client_list {
                Some(PtmEvent::WindowListChanged)
            } else if pn.atom == atoms.net_active_window {
                Some(PtmEvent::ActiveWindowChanged)
            } else if pn.atom == atoms.net_current_desktop {
                Some(PtmEvent::DesktopChanged)
            } else {
                None
            }
        }
        Event::PropertyNotify(pn) => {
            if pn.atom == atoms.net_wm_name {
                Some(PtmEvent::WindowTitleChanged(pn.window))
            } else if pn.atom == atoms.net_wm_state {
                Some(PtmEvent::WindowStateChanged(pn.window))
            } else {
                None
            }
        }
        Event::DestroyNotify(dn) => Some(PtmEvent::WindowDestroyed(dn.window)),
        _ => None,
    }
}
