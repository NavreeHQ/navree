//! [`ZonesFake`]: an in-memory zone source for tests and demos.

use std::collections::BTreeMap;
use std::sync::{Arc, PoisonError, RwLock};

use http::request::Parts;
use navree_contracts::{ZoneLevel, ZoneOwner, ZoneRequest, ZoneSource};
use navree_kernel::{BoxError, BoxFuture, Factory, Module, ModuleBuilder, ModuleId};
use uuid::Uuid;

/// An in-memory [`ZoneSource`] at one level, for tests (and demos without
/// a database). Clones share their owners, so a test can change zones
/// after boot.
///
/// At [`ZoneLevel::User`] it answers for the signed-in user; at the other
/// levels for the owner whose id the request sends in
/// [`header`](ZonesFake::header).
///
/// ```
/// use navree_contracts::{ZoneLevel, ZoneOwner};
/// use navree_time::ZonesFake;
/// use uuid::Uuid;
///
/// let workspace = Uuid::now_v7();
/// let workspaces = ZonesFake::new(ZoneLevel::Workspace)
///     .header("X-Workspace")
///     .owner(ZoneOwner::new(workspace).zone("Asia/Kolkata"));
/// // TestApp::builder().module(TimeModule::from_config()).module(workspaces.module())
/// ```
#[derive(Clone, Debug)]
pub struct ZonesFake {
    level: ZoneLevel,
    header: Option<String>,
    owners: Arc<RwLock<BTreeMap<Uuid, ZoneOwner>>>,
}

impl ZonesFake {
    /// An empty source at `level`.
    pub fn new(level: ZoneLevel) -> Self {
        ZonesFake {
            level,
            header: None,
            owners: Arc::default(),
        }
    }

    /// Reads the request's owner id from header `name` (levels other than
    /// [`ZoneLevel::User`]).
    pub fn header(mut self, name: &str) -> Self {
        self.header = Some(name.to_owned());
        self
    }

    /// Adds (or replaces) an owner.
    pub fn owner(self, owner: ZoneOwner) -> Self {
        self.set(owner);
        self
    }

    /// Adds or replaces an owner (also after boot).
    pub fn set(&self, owner: ZoneOwner) {
        self.owners
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(owner.id, owner);
    }

    /// Removes an owner.
    pub fn remove(&self, owner: Uuid) {
        self.owners
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&owner);
    }

    /// A module registering this source. One fake per level: a second fake
    /// module at the same level fails the boot (configured twice).
    pub fn module(&self) -> impl Module + use<> {
        ZonesFakeModule(self.clone())
    }

    fn get(&self, owner: Uuid) -> Option<ZoneOwner> {
        self.owners
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&owner)
            .cloned()
    }
}

impl ZoneSource for ZonesFake {
    fn level(&self) -> ZoneLevel {
        self.level
    }

    fn zone_of_request<'a>(
        &'a self,
        request: &'a mut ZoneRequest<'_>,
    ) -> BoxFuture<'a, Result<Option<ZoneOwner>, BoxError>> {
        Box::pin(async move {
            let id = match (self.level, &self.header) {
                (ZoneLevel::User, _) => request.user_id(),
                (_, Some(header)) => request
                    .head::<Parts>()
                    .and_then(|parts| parts.headers.get(header.as_str()))
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| Uuid::parse_str(value.trim()).ok()),
                (_, None) => None,
            };
            Ok(id.and_then(|id| self.get(id)))
        })
    }

    fn zone_of(&self, owner: Uuid) -> BoxFuture<'_, Result<Option<ZoneOwner>, BoxError>> {
        Box::pin(async move { Ok(self.get(owner)) })
    }

    fn owners(&self) -> BoxFuture<'_, Result<Vec<ZoneOwner>, BoxError>> {
        Box::pin(async move {
            Ok(self
                .owners
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .values()
                .cloned()
                .collect())
        })
    }
}

/// Registers a [`ZonesFake`]; one module id per level.
struct ZonesFakeModule(ZonesFake);

struct UserZonesFake;
struct WorkspaceZonesFake;
struct OrganizationZonesFake;

impl Module for ZonesFakeModule {
    fn id(&self) -> ModuleId {
        match self.0.level {
            ZoneLevel::User => ModuleId::of::<UserZonesFake>(),
            ZoneLevel::Workspace => ModuleId::of::<WorkspaceZonesFake>(),
            ZoneLevel::Organization => ModuleId::of::<OrganizationZonesFake>(),
        }
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        let source: Arc<dyn ZoneSource> = Arc::new(self.0.clone());
        m.multi(Factory::<dyn ZoneSource>::new(move |_| {
            let source = source.clone();
            async move { Ok(source) }
        }));
    }
}
