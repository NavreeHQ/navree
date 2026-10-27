//! [`SettingsChanged`]: emitted once a change to stored settings is
//! committed.

use navree_events::Event;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::ScopeKind;

/// Settings changed at one level (`settings.changed`): emitted through
/// navree-events' `EventBus`, when `EventsModule` is in the app, once the
/// change commits (never for a rolled-back change).
///
/// `old` and `new` hold the changed fields as stored at that level before
/// and after; a field missing from `new` inherits again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingsChanged {
    /// The settings key.
    pub key: String,
    /// The level of the change.
    pub scope_type: ScopeKind,
    /// The organization's, workspace's or user's id; `None` at the global
    /// level.
    pub scope_id: Option<Uuid>,
    /// The fields that changed, sorted.
    pub fields: Vec<String>,
    /// Their stored values before (fields that inherited are missing).
    pub old: Map<String, Value>,
    /// Their stored values after (fields that inherit now are missing).
    pub new: Map<String, Value>,
    /// Who made the change, when a user did.
    pub actor_id: Option<Uuid>,
}

impl Event for SettingsChanged {
    const NAME: &'static str = "settings.changed";
}
