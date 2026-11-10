//! The social module's events. They are emitted through navree-events'
//! `EventBus` once the unit of work that made the change commits, when
//! `EventsModule` is in the app. Provider tokens are never part of them.

use navree_events::Event;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A provider account was linked to a user, at sign-in (a new user, or a
/// matching verified email) or from the user's settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialAccountLinked {
    /// The user.
    pub user_id: Uuid,
    /// The provider key.
    pub provider: String,
    /// The provider's user id.
    pub provider_user_id: String,
}

impl Event for SocialAccountLinked {
    const NAME: &'static str = "social.linked";
}

/// A provider account was disconnected from a user.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialAccountUnlinked {
    /// The user.
    pub user_id: Uuid,
    /// The provider key.
    pub provider: String,
    /// The provider's user id.
    pub provider_user_id: String,
}

impl Event for SocialAccountUnlinked {
    const NAME: &'static str = "social.unlinked";
}
