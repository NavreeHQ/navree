//! Live updates: how a change in the app reaches the pages that are open.
//!
//! The app's live hub (`RealtimeModule`) carries them. Code that changes a
//! customer calls `Live::nudge` with [`topic`] (the customers service does,
//! after the change is committed), and every browser tab listening to
//! [`stream`] fetches its table, or its figures, again. A nudge only says
//! "ask again", so nothing a stream sends can put markup in a page, and the
//! hub tells only the members of the workspace the change is in.

use navree::realtime::Topic;
use navree::ui::LiveStream;
use uuid::Uuid;

/// The name of the topic the table and the figures listen to.
pub const TOPIC: &str = "customers";

/// The stream a page listens on: `GET /broadcasting/stream` with the topic.
pub fn stream() -> LiveStream {
    LiveStream::topics([TOPIC])
}

/// The topic of `workspace`'s customers: only the members of that workspace
/// hear it.
pub fn topic(workspace: Uuid) -> Topic {
    Topic::workspace(workspace, TOPIC)
}
