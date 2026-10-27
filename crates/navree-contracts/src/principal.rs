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

/// The user acting as the request's [`Principal`] while impersonating them
/// (the `act` claim of an impersonation token, RFC 8693 §4.1): a request
/// extension the authenticator sets next to [`AuthState`]. The principal is
/// the impersonated user; this is who is really at the keyboard. The unit
/// of work carries it and the audit trail records it on every entry of the
/// request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Impersonator(pub Uuid);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impersonator_is_a_plain_id() {
        let id = Uuid::nil();
        let impersonator = Impersonator(id);
        assert_eq!(impersonator.0, id);
        assert_eq!(
            serde_json::to_value(impersonator).unwrap(),
            serde_json::json!(id)
        );
    }
}
