//! `navree_time`: the time zone names, local days and send-window times
//! that users and clients type.
//!
//! Invariants: a zone's name parses to the same zone; a local day prints
//! as the text it was read from; a send window that builds opens before it
//! closes.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_time::{LocalDay, SendWindow, Zone};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(zone) = Zone::parse(text) {
        assert_eq!(Zone::parse(zone.name()), Ok(zone));
        assert_eq!(Zone::canonical_name(text), Some(zone.name()));
    }
    if let Ok(day) = LocalDay::parse(text) {
        assert_eq!(day.to_string(), text);
        assert_eq!(LocalDay::parse(&day.to_string()), Ok(day));
        assert_eq!(day.succ().pred(), day);
    }
    let (start, end) = text.split_once('-').unwrap_or((text, "24:00"));
    if let Ok(window) = SendWindow::weekdays().try_between(start, end) {
        if let Some(end) = window.end() {
            assert!(window.start() < end);
        }
    }
});
