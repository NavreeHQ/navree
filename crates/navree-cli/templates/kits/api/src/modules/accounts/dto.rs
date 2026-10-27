//! Response bodies of `/api/v1/account`.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

/// A signed-in device or browser: one sign-in, kept alive by its rotating
/// refresh token.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AccountSession {
    /// The session's id (the refresh-token family).
    pub id: Uuid,
    /// The client address it signed in from.
    pub ip: Option<String>,
    /// The client's user agent.
    pub user_agent: Option<String>,
    /// When its tokens were last refreshed.
    pub last_active_at: DateTime<Utc>,
    /// When it ends unless it is refreshed again.
    pub expires_at: DateTime<Utc>,
}

/// `{data: [AccountSession]}`.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AccountSessionList {
    /// The sessions, most recently active first.
    pub data: Vec<AccountSession>,
}

/// A social sign-in provider with a button on the sign-in page.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ProviderResource {
    /// The key in `social.providers` and the routes (`github`).
    pub key: String,
    /// The display name (`GitHub`).
    pub name: String,
}

/// `{data: [Provider]}`.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ProviderList {
    /// The configured providers, by key.
    pub data: Vec<ProviderResource>,
}
