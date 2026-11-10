//! [`ZoneSource`]: where the time zone of a user, a workspace or an
//! organization comes from.
//!
//! navree-time resolves a request's zone through every `dyn ZoneSource`
//! multi-binding, level by level (user, then workspace, then organization,
//! then the app's default zone), and navree-schedule runs tasks per owner in
//! each owner's local time. Modules that store zones (navree-workspace, the
//! app's users) register a source, so neither needs the other.

use std::any::Any;
use std::fmt;

use navree_kernel::{BoxError, BoxFuture};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Principal;

/// Who a zone belongs to, in resolution order: a user's own zone wins over
/// their workspace's, which wins over its organization's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneLevel {
    /// The signed-in user.
    User,
    /// The workspace (the tenant) of the request.
    Workspace,
    /// The organization above the workspace.
    Organization,
}

impl ZoneLevel {
    /// Every level, in resolution order.
    pub const ALL: [ZoneLevel; 3] = [
        ZoneLevel::User,
        ZoneLevel::Workspace,
        ZoneLevel::Organization,
    ];

    /// `user`, `workspace` or `organization`.
    pub fn as_str(self) -> &'static str {
        match self {
            ZoneLevel::User => "user",
            ZoneLevel::Workspace => "workspace",
            ZoneLevel::Organization => "organization",
        }
    }
}

impl fmt::Display for ZoneLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An owner of a zone (a user, a workspace or an organization) with what
/// is stored for it: an IANA zone name and an ISO 3166-1 alpha-2 country,
/// either of which may be unset. A source returns the raw stored values;
/// navree-time validates them and derives a zone from the country when only
/// the country is set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneOwner {
    /// The user, workspace or organization id.
    pub id: Uuid,
    /// The stored zone name (`Asia/Kolkata`), if any.
    pub zone: Option<String>,
    /// The stored country code (`IN`), if any.
    pub country: Option<String>,
}

impl ZoneOwner {
    /// The owner `id` with nothing stored.
    pub fn new(id: Uuid) -> Self {
        ZoneOwner {
            id,
            zone: None,
            country: None,
        }
    }

    /// Sets the stored zone name.
    pub fn zone(mut self, zone: impl Into<String>) -> Self {
        self.zone = Some(zone.into());
        self
    }

    /// Sets the stored country code.
    pub fn country(mut self, country: impl Into<String>) -> Self {
        self.country = Some(country.into());
        self
    }
}

/// What a [`ZoneSource`] sees of a request: the signed-in user and the
/// request's head. In an HTTP request the head is `http::request::Parts`
/// (`request.head::<http::request::Parts>()`); jobs and commands resolve
/// zones [`detached`](ZoneRequest::detached) from any request.
pub struct ZoneRequest<'a> {
    principal: Option<&'a Principal>,
    head: Option<&'a mut (dyn Any + Send)>,
}

impl<'a> ZoneRequest<'a> {
    /// A request made by `principal` (`None` for guests) with `head`.
    pub fn new<H: Any + Send>(principal: Option<&'a Principal>, head: &'a mut H) -> Self {
        ZoneRequest {
            principal,
            head: Some(head),
        }
    }

    /// A request with a principal and no head (outside HTTP).
    pub fn detached(principal: Option<&'a Principal>) -> Self {
        ZoneRequest {
            principal,
            head: None,
        }
    }

    /// The signed-in user, if any.
    pub fn principal(&self) -> Option<&Principal> {
        self.principal
    }

    /// The signed-in user's id, if any.
    pub fn user_id(&self) -> Option<Uuid> {
        self.principal.map(|principal| principal.user_id)
    }

    /// The head as an `H`, when it is one.
    pub fn head<H: Any>(&mut self) -> Option<&mut H> {
        self.head.as_deref_mut()?.downcast_mut::<H>()
    }
}

impl fmt::Debug for ZoneRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZoneRequest")
            .field("user_id", &self.user_id())
            .field("has_head", &self.head.is_some())
            .finish()
    }
}

/// A source of zones at one [`ZoneLevel`]: a module that stores a zone (and
/// a country) per user, workspace or organization. Register it as a
/// multi-binding:
///
/// ```ignore
/// m.multi(Factory::<dyn ZoneSource>::new(|r| async move {
///     Ok(Arc::new(UserZones::new(r.get::<Db>().await?)) as Arc<dyn ZoneSource>)
/// }));
/// ```
///
/// Errors fail the lookup (a 500 for a request); a source that has nothing
/// for an owner returns `Ok(None)`, or an owner with neither a zone nor a
/// country.
pub trait ZoneSource: Send + Sync + 'static {
    /// The level this source answers for.
    fn level(&self) -> ZoneLevel;

    /// The owner at this level the request belongs to (the signed-in user,
    /// the request's workspace), with what is stored for it; `None` when
    /// the request has no owner at this level.
    fn zone_of_request<'a>(
        &'a self,
        request: &'a mut ZoneRequest<'_>,
    ) -> BoxFuture<'a, Result<Option<ZoneOwner>, BoxError>>;

    /// What is stored for `owner`; `None` for an unknown owner.
    fn zone_of(&self, owner: Uuid) -> BoxFuture<'_, Result<Option<ZoneOwner>, BoxError>>;

    /// Every owner at this level, for work that runs per owner in local
    /// time (navree-schedule's `per_workspace_local`). The default lists
    /// none.
    fn owners(&self) -> BoxFuture<'_, Result<Vec<ZoneOwner>, BoxError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}
