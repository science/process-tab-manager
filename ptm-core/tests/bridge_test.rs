use ptm_core::bridge::{translate_event, AtomIds, PtmEvent};
use x11rb_protocol::protocol::xproto::{
    DestroyNotifyEvent, PropertyNotifyEvent, PROPERTY_NOTIFY_EVENT, DESTROY_NOTIFY_EVENT,
};
use x11rb_protocol::protocol::Event;

/// Helper to build an AtomIds for testing.
fn test_atoms() -> AtomIds {
    AtomIds {
        net_client_list: 100,
        net_active_window: 101,
        net_wm_name: 102,
        net_current_desktop: 103,
        net_wm_state: 104,
    }
}

const ROOT: u32 = 0x000001ee;

#[test]
fn client_list_changed_on_root() {
    let atoms = test_atoms();
    let event = Event::PropertyNotify(PropertyNotifyEvent {
        response_type: PROPERTY_NOTIFY_EVENT,
        sequence: 0,
        window: ROOT,
        atom: atoms.net_client_list,
        time: 0,
        state: x11rb_protocol::protocol::xproto::Property::NEW_VALUE,
    });
    let result = translate_event(&event, &atoms, ROOT);
    assert_eq!(result, Some(PtmEvent::WindowListChanged));
}

#[test]
fn active_window_changed_on_root() {
    let atoms = test_atoms();
    let event = Event::PropertyNotify(PropertyNotifyEvent {
        response_type: PROPERTY_NOTIFY_EVENT,
        sequence: 0,
        window: ROOT,
        atom: atoms.net_active_window,
        time: 0,
        state: x11rb_protocol::protocol::xproto::Property::NEW_VALUE,
    });
    let result = translate_event(&event, &atoms, ROOT);
    assert_eq!(result, Some(PtmEvent::ActiveWindowChanged));
}

#[test]
fn title_changed_on_window() {
    let atoms = test_atoms();
    let wid = 0x02600017;
    let event = Event::PropertyNotify(PropertyNotifyEvent {
        response_type: PROPERTY_NOTIFY_EVENT,
        sequence: 0,
        window: wid,
        atom: atoms.net_wm_name,
        time: 0,
        state: x11rb_protocol::protocol::xproto::Property::NEW_VALUE,
    });
    let result = translate_event(&event, &atoms, ROOT);
    assert_eq!(result, Some(PtmEvent::WindowTitleChanged(wid)));
}

#[test]
fn desktop_changed_on_root() {
    let atoms = test_atoms();
    let event = Event::PropertyNotify(PropertyNotifyEvent {
        response_type: PROPERTY_NOTIFY_EVENT,
        sequence: 0,
        window: ROOT,
        atom: atoms.net_current_desktop,
        time: 0,
        state: x11rb_protocol::protocol::xproto::Property::NEW_VALUE,
    });
    let result = translate_event(&event, &atoms, ROOT);
    assert_eq!(result, Some(PtmEvent::DesktopChanged));
}

#[test]
fn window_destroyed() {
    let atoms = test_atoms();
    let wid = 0x03400006;
    let event = Event::DestroyNotify(DestroyNotifyEvent {
        response_type: DESTROY_NOTIFY_EVENT,
        sequence: 0,
        event: wid,
        window: wid,
    });
    let result = translate_event(&event, &atoms, ROOT);
    assert_eq!(result, Some(PtmEvent::WindowDestroyed(wid)));
}

#[test]
fn unrelated_property_on_root_returns_none() {
    let atoms = test_atoms();
    let event = Event::PropertyNotify(PropertyNotifyEvent {
        response_type: PROPERTY_NOTIFY_EVENT,
        sequence: 0,
        window: ROOT,
        atom: 999, // some unrelated atom
        time: 0,
        state: x11rb_protocol::protocol::xproto::Property::NEW_VALUE,
    });
    let result = translate_event(&event, &atoms, ROOT);
    assert_eq!(result, None);
}

#[test]
fn unrelated_property_on_window_returns_none() {
    let atoms = test_atoms();
    let event = Event::PropertyNotify(PropertyNotifyEvent {
        response_type: PROPERTY_NOTIFY_EVENT,
        sequence: 0,
        window: 0x02600017,
        atom: 999,
        time: 0,
        state: x11rb_protocol::protocol::xproto::Property::NEW_VALUE,
    });
    let result = translate_event(&event, &atoms, ROOT);
    assert_eq!(result, None);
}

#[test]
fn wm_state_changed_on_window() {
    let atoms = test_atoms();
    let wid = 0x02600017;
    let event = Event::PropertyNotify(PropertyNotifyEvent {
        response_type: PROPERTY_NOTIFY_EVENT,
        sequence: 0,
        window: wid,
        atom: atoms.net_wm_state,
        time: 0,
        state: x11rb_protocol::protocol::xproto::Property::NEW_VALUE,
    });
    let result = translate_event(&event, &atoms, ROOT);
    assert_eq!(result, Some(PtmEvent::WindowStateChanged(wid)));
}

#[test]
fn title_change_on_root_is_not_window_title() {
    // _NET_WM_NAME on root should not be treated as a window title change
    let atoms = test_atoms();
    let event = Event::PropertyNotify(PropertyNotifyEvent {
        response_type: PROPERTY_NOTIFY_EVENT,
        sequence: 0,
        window: ROOT,
        atom: atoms.net_wm_name,
        time: 0,
        state: x11rb_protocol::protocol::xproto::Property::NEW_VALUE,
    });
    let result = translate_event(&event, &atoms, ROOT);
    assert_eq!(result, None);
}
