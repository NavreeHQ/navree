//! [`FlagChanged`]: emitted once a change to a flag is committed.

use navree_events::Event;
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::ScopeType;

/// What changed about a flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FlagChange {
    /// Turned on (for a user, a workspace or everyone).
    Activated,
    /// Turned off (for a user, a workspace or everyone).
    Deactivated,
    /// An override, or the global state, removed.
    Forgotten,
    /// A rollout set or changed.
    RolloutSet,
    /// The rollout removed.
    RolloutStopped,
    /// The kill switch turned on.
    Killed,
    /// The kill switch turned off.
    Revived,
    /// Every stored state of the flag deleted (`flags:purge`).
    Purged,
}

impl FlagChange {
    /// The snake-case name (`activated`, `rollout_set`, ...).
    pub fn as_str(self) -> &'static str {
        match self {
            FlagChange::Activated => "activated",
            FlagChange::Deactivated => "deactivated",
            FlagChange::Forgotten => "forgotten",
            FlagChange::RolloutSet => "rollout_set",
            FlagChange::RolloutStopped => "rollout_stopped",
            FlagChange::Killed => "killed",
            FlagChange::Revived => "revived",
            FlagChange::Purged => "purged",
        }
    }
}

/// A flag changed (`flag.changed`): emitted through navree-events'
/// `EventBus`, when `EventsModule` is in the app, once the change commits
/// (never for a rolled-back change).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlagChanged {
    /// The flag's name.
    pub flag: String,
    /// What changed.
    pub change: FlagChange,
    /// For an override: `user` or `workspace`; `None` for the global state.
    pub scope_type: Option<ScopeType>,
    /// For an override: the user's or workspace's id.
    pub scope_id: Option<Uuid>,
    /// The changed values before (`{"everyone": null}`, `{"active": true}`,
    /// `{"rollout": {..}}`, `{"killed": false}`).
    pub old: Value,
    /// The changed values after.
    pub new: Value,
    /// Who made the change, when a user did.
    pub actor_id: Option<Uuid>,
}

impl Event for FlagChanged {
    const NAME: &'static str = "flag.changed";
}
