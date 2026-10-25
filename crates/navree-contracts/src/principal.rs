//! [`Principal`]: who is making the request.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// An authenticated user, as established by the authentication layer.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Principal {
    /// The user's id.
    pub user_id: Uuid,
    /// How the user authenticated.
    pub via: AuthVia,
    /// For token authentication: the abilities granted to the token
    /// (`None` means unrestricted).
    pub token_abilities: Option<Vec<String>>,
    /// For session authentication: the session id. Redacted in `Debug`.
    pub session_id: Option<String>,
}

/// `session_id` is a bearer credential, so it never appears in debug output.
impl fmt::Debug for Principal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Principal")
            .field("user_id", &self.user_id)
            .field("via", &self.via)
            .field("token_abilities", &self.token_abilities)
            .field(
                "session_id",
                &self.session_id.as_ref().map(|_| "[redacted]"),
            )
            .finish()
    }
}

/// How a [`Principal`] authenticated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthVia {
    /// A session cookie.
    Session,
    /// An API token.
    Token,
    /// Set by tests (`acting_as`).
    Test,
}

/// The authentication state of the current request (a request extension):
/// `None` for guests.
#[derive(Clone, Debug, Default)]
pub struct AuthState(pub Option<Principal>);
