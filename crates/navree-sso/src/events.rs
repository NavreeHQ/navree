//! The module's events, emitted through navree-events' `EventBus` once the
//! unit of work that made the change commits, when `EventsModule` is in the
//! app.

use navree_events::Event;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::SsoOwner;

/// Single sign-on created a user, or made a user a member of the
/// connection's organization or workspace, or changed their role there
/// (just-in-time provisioning).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsoUserProvisioned {
    /// The connection.
    pub connection_id: Uuid,
    /// The user.
    pub user_id: Uuid,
    /// The connection's owner.
    pub owner: SsoOwner,
    /// The role given (none for an app-wide connection).
    pub role: Option<String>,
    /// Whether the user was created by this sign-in.
    pub created: bool,
}

impl Event for SsoUserProvisioned {
    const NAME: &'static str = "sso.user_provisioned";
}

/// A domain's TXT record was found (or an operator verified it): its
/// emails now route to the connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsoDomainVerified {
    /// The connection.
    pub connection_id: Uuid,
    /// The domain.
    pub domain: String,
}

impl Event for SsoDomainVerified {
    const NAME: &'static str = "sso.domain_verified";
}
