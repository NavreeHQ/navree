//! [`CallTickets`]: sealed, one-time tickets for results a client posts
//! back.
//!
//! When a page asks a device to do something and post the result back (a
//! location, a scanned code, a confirmation), the server hands the client a
//! ticket with the call. The ticket is sealed by the server and names who it
//! was issued to, the call, the route the result goes to and when it
//! expires; the route that receives the result redeems it, once. A client
//! cannot make one, change one, use one twice, use one from another session
//! or send one to another route.
//!
//! The page code that builds the call issues the ticket (synchronously, and
//! without storing anything); the handler of the route redeems it. Both reach
//! the app's `dyn CallTickets` (navree-auth registers one), so neither needs
//! the other's crate.

use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{BoxFuture, Principal};

/// Who a ticket is for: the request's user and session, as the
/// authenticator set them on the request's [`Principal`]. Both are `None`
/// for a guest. A ticket redeems only for the same holder.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct TicketHolder {
    /// The signed-in user.
    pub user: Option<Uuid>,
    /// The signed-in session ([`Principal::session_id`]).
    pub session: Option<String>,
}

impl TicketHolder {
    /// The holder of a request whose principal is `principal` (`None` for a
    /// guest).
    pub fn of(principal: Option<&Principal>) -> Self {
        match principal {
            Some(principal) => Self {
                user: Some(principal.user_id),
                session: principal.session_id.clone(),
            },
            None => Self::default(),
        }
    }

    /// A guest: no user, no session.
    pub fn guest() -> Self {
        Self::default()
    }
}

/// The session is a credential of some strategies: never in debug output.
impl fmt::Debug for TicketHolder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TicketHolder")
            .field("user", &self.user)
            .field("session", &self.session.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

/// What a ticket is issued for (see [`CallTickets::issue`]).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TicketRequest {
    /// Who may redeem it.
    pub holder: TicketHolder,
    /// The call it goes with (`"confirm"`, `"locate_once"`, `"scan_code"`).
    pub feature: String,
    /// The method of the route the result goes to (upper case).
    pub method: String,
    /// The path of the route the result goes to (no query, no trailing
    /// slash).
    pub path: String,
    /// A digest of what the call is about (its arguments, or the details of
    /// the action a confirmation approves), returned on redemption.
    pub binding: Option<String>,
    /// How long it may be redeemed: [`TicketRequest::DEFAULT_TTL`] unless
    /// set, at most [`TicketRequest::MAX_TTL`].
    pub ttl: Duration,
}

impl TicketRequest {
    /// The default life of a ticket: 5 minutes.
    pub const DEFAULT_TTL: Duration = Duration::from_secs(300);

    /// The longest life of a ticket: an hour.
    pub const MAX_TTL: Duration = Duration::from_secs(3600);

    /// A ticket for `holder`, for the call `feature`, whose result is posted
    /// with `method` to `path` (a query or fragment is dropped).
    pub fn new(holder: TicketHolder, feature: impl Into<String>, method: &str, path: &str) -> Self {
        Self {
            holder,
            feature: feature.into(),
            method: method.trim().to_ascii_uppercase(),
            path: route_path(path),
            binding: None,
            ttl: Self::DEFAULT_TTL,
        }
    }

    /// Binds the ticket to `digest` (what the call is about).
    #[must_use]
    pub fn binding(mut self, digest: impl Into<String>) -> Self {
        self.binding = Some(digest.into());
        self
    }

    /// The ticket's life, between a second and [`TicketRequest::MAX_TTL`].
    #[must_use]
    pub fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl.clamp(Duration::from_secs(1), Self::MAX_TTL);
        self
    }
}

/// What the redeeming route expects of a ticket (see
/// [`CallTickets::redeem`]).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TicketCheck {
    /// The request's holder.
    pub holder: TicketHolder,
    /// The call the route takes the result of.
    pub feature: String,
    /// The route's method, when the ticket must have been issued for it.
    pub method: Option<String>,
    /// The route's path, when the ticket must have been issued for it.
    pub path: Option<String>,
}

impl TicketCheck {
    /// A ticket of `holder` for the call `feature`, at any route. Use
    /// [`at`](TicketCheck::at) in the handler that receives the result.
    pub fn new(holder: TicketHolder, feature: impl Into<String>) -> Self {
        Self {
            holder,
            feature: feature.into(),
            method: None,
            path: None,
        }
    }

    /// The ticket must have been issued for the route `method` `path` (the
    /// request's own, a query or fragment dropped).
    #[must_use]
    pub fn at(mut self, method: &str, path: &str) -> Self {
        self.method = Some(method.trim().to_ascii_uppercase());
        self.path = Some(route_path(path));
        self
    }
}

/// An issued ticket: the opaque value the client posts back, and when it
/// expires (so a client showing a stored screen asks for a new one instead
/// of posting a dead one).
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct IssuedTicket {
    /// The sealed ticket (URL- and form-safe).
    pub ticket: String,
    /// When it stops being accepted.
    pub expires_at: DateTime<Utc>,
}

impl IssuedTicket {
    /// A ticket `ticket` that expires at `expires_at` (for implementations).
    pub fn new(ticket: String, expires_at: DateTime<Utc>) -> Self {
        Self { ticket, expires_at }
    }
}

/// The ticket is a bearer credential: never in debug output.
impl fmt::Debug for IssuedTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IssuedTicket")
            .field("ticket", &"[redacted]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// What a redeemed ticket was issued for.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RedeemedTicket {
    /// The call.
    pub feature: String,
    /// The method of the route the result was meant for.
    pub method: String,
    /// The path of the route the result was meant for.
    pub path: String,
    /// The digest it was bound to, if any.
    pub binding: Option<String>,
    /// When it was issued.
    pub issued_at: DateTime<Utc>,
    /// When it would have expired.
    pub expires_at: DateTime<Utc>,
}

impl RedeemedTicket {
    /// A redeemed ticket (for implementations).
    pub fn new(
        feature: String,
        method: String,
        path: String,
        binding: Option<String>,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Self {
        Self {
            feature,
            method,
            path,
            binding,
            issued_at,
            expires_at,
        }
    }
}

/// Why a ticket was not issued or not accepted. The messages say what was
/// wrong, never the ticket.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TicketError {
    /// Not a ticket this server sealed (forged, changed, truncated).
    #[error("not a valid ticket")]
    Malformed,
    /// Past its life.
    #[error("the ticket has expired")]
    Expired,
    /// Issued to another user or session.
    #[error("the ticket was issued to another session")]
    WrongHolder,
    /// Issued for another call.
    #[error("the ticket was issued for another call")]
    WrongFeature,
    /// Issued for another route.
    #[error("the ticket was issued for another route")]
    WrongRoute,
    /// Already redeemed.
    #[error("the ticket was already used")]
    Used,
    /// The tickets cannot be issued or redeemed now (no application key, the
    /// store of used tickets failed).
    #[error("tickets are unavailable: {0}")]
    Unavailable(String),
}

impl TicketError {
    /// A short code for logs and audit entries (`"expired"`, `"used"`, ...).
    pub fn code(&self) -> &'static str {
        match self {
            TicketError::Malformed => "malformed",
            TicketError::Expired => "expired",
            TicketError::WrongHolder => "wrong_holder",
            TicketError::WrongFeature => "wrong_feature",
            TicketError::WrongRoute => "wrong_route",
            TicketError::Used => "used",
            TicketError::Unavailable(_) => "unavailable",
        }
    }
}

/// Issues and redeems sealed one-time tickets. Resolve the app's `dyn
/// CallTickets` (navree-auth's `AuthModule` registers one).
pub trait CallTickets: Send + Sync + 'static {
    /// Seals a ticket for `request`. Nothing is stored: issuing is cheap
    /// enough for every call a page carries.
    fn issue(&self, request: &TicketRequest) -> Result<IssuedTicket, TicketError>;

    /// Opens `ticket`, checks it against `check` (the holder, the call, and
    /// the route when the check names one) and its expiry, and marks it
    /// used: of two redemptions of one ticket, even at the same moment, one
    /// succeeds.
    fn redeem<'a>(
        &'a self,
        ticket: &'a str,
        check: &'a TicketCheck,
    ) -> BoxFuture<'a, Result<RedeemedTicket, TicketError>>;
}

/// `path` without its query and fragment, and without a trailing slash
/// (except `/`).
fn route_path(path: &str) -> String {
    let path = path.trim();
    let end = path.find(['?', '#']).unwrap_or(path.len());
    let path = &path[..end];
    match path.trim_end_matches('/') {
        "" => "/".to_owned(),
        trimmed => trimmed.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AuthVia;

    #[test]
    fn holders_compare_user_and_session() {
        let user = Uuid::now_v7();
        let principal = |session: Option<&str>| Principal {
            user_id: user,
            via: AuthVia::Session,
            token_abilities: None,
            session_id: session.map(str::to_owned),
        };
        let phone = TicketHolder::of(Some(&principal(Some("s-1"))));
        assert_eq!(phone.user, Some(user));
        assert_eq!(phone, TicketHolder::of(Some(&principal(Some("s-1")))));
        assert_ne!(phone, TicketHolder::of(Some(&principal(Some("s-2")))));
        assert_ne!(phone, TicketHolder::of(Some(&principal(None))));
        assert_eq!(TicketHolder::of(None), TicketHolder::guest());
        assert!(!format!("{phone:?}").contains("s-1"));
    }

    #[test]
    fn requests_clamp_their_life_and_name_their_route() {
        let request = TicketRequest::new(TicketHolder::guest(), "share", "post", "/orders/7/?a=1");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/orders/7");
        assert_eq!(request.ttl, TicketRequest::DEFAULT_TTL);
        assert_eq!(
            request.clone().ttl(Duration::from_secs(86_400)).ttl,
            TicketRequest::MAX_TTL
        );
        assert_eq!(request.ttl(Duration::ZERO).ttl, Duration::from_secs(1));
        let check = TicketCheck::new(TicketHolder::guest(), "share").at("post", "/orders/7#x");
        assert_eq!(check.path.as_deref(), Some("/orders/7"));
        assert_eq!(route_path("/"), "/");
        assert_eq!(route_path(""), "/");
        let issued = IssuedTicket::new("v1.secret".to_owned(), Utc::now());
        assert!(!format!("{issued:?}").contains("secret"));
        assert_eq!(TicketError::Used.code(), "used");
    }
}
