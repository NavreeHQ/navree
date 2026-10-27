//! The integrations module's events, emitted after their change commits
//! when the app has `EventsModule`.

use navree_events::Event;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{CredentialKind, Owner};

/// A connection was made (or made again, replacing its credentials).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationConnected {
    /// The connection.
    pub connection_id: Uuid,
    /// Who it belongs to.
    pub owner: Owner,
    /// The provider key.
    pub provider: String,
    /// The scope set.
    pub scope_set: String,
    /// How it authenticates.
    pub kind: CredentialKind,
    /// The user who connected it.
    pub connected_by: Option<Uuid>,
}

impl Event for IntegrationConnected {
    const NAME: &'static str = "integrations.connected";
}

/// A connection was removed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationDisconnected {
    /// The connection.
    pub connection_id: Uuid,
    /// Who it belonged to.
    pub owner: Owner,
    /// The provider key.
    pub provider: String,
    /// The scope set.
    pub scope_set: String,
}

impl Event for IntegrationDisconnected {
    const NAME: &'static str = "integrations.disconnected";
}

/// A connection stopped working: the provider refused its credentials, or
/// could not be reached `integrations.unhealthy_after` times in a row.
/// Emitted once, when a healthy connection turns unhealthy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionUnhealthy {
    /// The connection.
    pub connection_id: Uuid,
    /// Who it belongs to.
    pub owner: Owner,
    /// The provider key.
    pub provider: String,
    /// The scope set.
    pub scope_set: String,
    /// What the provider answered (never a credential).
    pub error: String,
    /// Consecutive failures.
    pub failures: u32,
}

impl Event for ConnectionUnhealthy {
    const NAME: &'static str = "integrations.unhealthy";
}

/// An unhealthy connection works again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionRestored {
    /// The connection.
    pub connection_id: Uuid,
    /// Who it belongs to.
    pub owner: Owner,
    /// The provider key.
    pub provider: String,
    /// The scope set.
    pub scope_set: String,
}

impl Event for ConnectionRestored {
    const NAME: &'static str = "integrations.restored";
}
