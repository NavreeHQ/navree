//! [`LongWaitDetected`].

use navree_events::Event;
use serde::{Deserialize, Serialize};

/// A queue fell behind: its oldest available job has waited longer than
/// the queue's `fleet.long_wait` threshold. A running master emits it
/// (event name `fleet.long_wait`) at most once per queue every
/// `fleet.long_wait_cooldown_minutes`, across every server, when the app
/// has `EventsModule`. [`Fleet::long_waits`](crate::Fleet::long_waits)
/// returns the queues over their threshold right now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LongWaitDetected {
    /// The queue.
    pub queue: String,
    /// How long its oldest job has waited, in seconds.
    pub wait_secs: u64,
    /// The queue's threshold, in seconds.
    pub threshold_secs: u64,
}

impl Event for LongWaitDetected {
    const NAME: &'static str = "fleet.long_wait";
}
