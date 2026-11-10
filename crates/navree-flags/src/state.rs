//! [`FlagState`] and [`FlagOverride`]: what is stored about a flag.

use chrono::{DateTime, Utc};
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Rollout, ScopeType};

/// The stored global state of a flag (a row of `feature_flags`; a flag
/// without a row has the [`unset`](FlagState::unset) state).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FlagState {
    /// The flag's name.
    pub name: String,
    /// On or off for everyone; `None` when unset.
    pub everyone: Option<bool>,
    /// The rollout, if any.
    pub rollout: Option<Rollout>,
    /// Whether the kill switch is on: off for everyone, overrides included.
    pub killed: bool,
    /// When the state last changed (`None` for a flag never changed).
    pub updated_at: Option<DateTime<Utc>>,
    /// Who made the last change, when a user did.
    pub updated_by: Option<Uuid>,
}

impl FlagState {
    /// The state of a flag nothing was stored for.
    pub fn unset(name: &str) -> FlagState {
        FlagState {
            name: name.to_owned(),
            everyone: None,
            rollout: None,
            killed: false,
            updated_at: None,
            updated_by: None,
        }
    }
}

/// An override: flag `flag` turned on or off for one user or one workspace
/// (a row of `feature_flag_overrides`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FlagOverride {
    /// The flag's name.
    pub flag: String,
    /// A user or a workspace.
    pub scope_type: ScopeType,
    /// The user's or the workspace's id.
    pub scope_id: Uuid,
    /// On or off.
    pub active: bool,
}
