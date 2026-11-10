//! [`SettingsFake`]: settings in memory that record every change.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use uuid::Uuid;

use crate::service::Engine;
use crate::{
    MemorySettingsStore, Resolved, Scope, Settings, SettingsChanged, SettingsDefinition,
    SettingsError, SettingsGroup, SettingsOptions, SettingsRegistry, SettingsStoreKind,
};

/// Settings for tests: the real resolution and validation on an in-memory
/// store, with no registration, cache, audit log or events, recording every
/// change made through its [`Settings`](struct@crate::Settings).
///
/// ```
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// use navree_validation::Validate;
/// use navree_settings::{Scope, SettingsFake, SettingsGroup, Source};
/// use serde::{Deserialize, Serialize};
/// use serde_json::json;
/// use uuid::Uuid;
///
/// #[derive(Serialize, Deserialize, Validate, Default)]
/// #[validate(skip_unmarked)]
/// struct PortalSettings {
///     show_leaderboard: bool,
/// }
///
/// impl SettingsGroup for PortalSettings {
///     const KEY: &'static str = "portal";
/// }
///
/// let (agra, holding) = (Uuid::now_v7(), Uuid::now_v7());
/// let fake = SettingsFake::new().attach(agra, holding);
/// fake.set::<PortalSettings>(Scope::organization(holding), json!({"show_leaderboard": true}))
///     .await
///     .unwrap();
///
/// // Install with TestApp::builder().override_value(Arc::new(fake.settings())).
/// let settings = fake.settings();
/// let portal = settings.resolve::<PortalSettings>(Scope::workspace(agra)).await.unwrap();
/// assert!(portal.show_leaderboard);
/// assert_eq!(portal.source("show_leaderboard"), Some(Source::Organization));
/// fake.assert_not_changed::<PortalSettings>(); // presets are not changes
/// # });
/// ```
///
/// Writes through [`settings`](SettingsFake::settings) (the service your
/// code uses) are recorded: [`changes`](SettingsFake::changes),
/// [`assert_changed`](SettingsFake::assert_changed). They take a unit of
/// work like the real service, but nothing joins it: the in-memory store
/// keeps a write even when the unit of work rolls back.
#[derive(Clone)]
pub struct SettingsFake {
    state: Arc<FakeState>,
    settings: Settings,
}

/// What the fake knows: its record of changes and the organizations of
/// workspaces.
#[derive(Debug, Default)]
pub(crate) struct FakeState {
    changes: Mutex<Vec<SettingsChanged>>,
    organizations: Mutex<HashMap<Uuid, Uuid>>,
    /// Presets in progress (not recorded).
    presetting: AtomicUsize,
}

impl FakeState {
    fn changes(&self) -> MutexGuard<'_, Vec<SettingsChanged>> {
        self.changes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records `event`, unless a preset is being written.
    pub(crate) fn record(&self, event: &SettingsChanged) {
        if self.presetting.load(Ordering::SeqCst) == 0 {
            self.changes().push(event.clone());
        }
    }

    /// The organization `workspace` is attached to.
    pub(crate) fn organization_of(&self, workspace: Uuid) -> Option<Uuid> {
        self.organizations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&workspace)
            .copied()
    }
}

impl Default for SettingsFake {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SettingsFake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsFake")
            .field("changes", &self.state.changes().len())
            .finish_non_exhaustive()
    }
}

impl SettingsFake {
    /// A fake with nothing stored: every setting is its type's default.
    pub fn new() -> Self {
        let state = Arc::new(FakeState::default());
        let settings = Settings::engine(Engine {
            registry: Arc::new(SettingsRegistry::open()),
            store: Arc::new(MemorySettingsStore::new()),
            options: Arc::new(SettingsOptions {
                store: SettingsStoreKind::Memory,
                cache_ttl_secs: 0,
                routes: false,
            }),
            transactional: false,
            cache: None,
            clock: None,
            app: None,
            fake: Some(state.clone()),
        });
        SettingsFake { state, settings }
    }

    /// The `Settings` service on the fake (typed calls register their type
    /// on first use).
    pub fn settings(&self) -> Settings {
        self.settings.clone()
    }

    /// Registers `T` up front, so calls by key (`resolve_key`, the routes,
    /// the commands) know it before a typed call names it.
    ///
    /// # Panics
    ///
    /// When `T` is not a valid settings type, or another type uses its key.
    pub fn register<T: SettingsGroup>(self) -> Self {
        if let Err(error) = self.settings.registry().of(&SettingsDefinition::of::<T>()) {
            panic!("SettingsFake::register: {error}");
        }
        self
    }

    /// Attaches `workspace` to `organization`: a workspace scope then
    /// inherits the organization's values.
    pub fn attach(self, workspace: Uuid, organization: Uuid) -> Self {
        self.state
            .organizations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(workspace, organization);
        self
    }

    /// Stores `patch` at `scope`'s target, validated like a real write but
    /// not recorded as a change (test setup).
    pub async fn set<T: SettingsGroup>(
        &self,
        scope: impl Into<Scope>,
        patch: impl Serialize,
    ) -> Result<Resolved<T>, SettingsError> {
        self.state.presetting.fetch_add(1, Ordering::SeqCst);
        let result = self.settings.preset::<T>(scope.into(), patch).await;
        self.state.presetting.fetch_sub(1, Ordering::SeqCst);
        result
    }

    /// Every change written through [`settings`](SettingsFake::settings),
    /// in order.
    pub fn changes(&self) -> Vec<SettingsChanged> {
        self.state.changes().clone()
    }

    /// Forgets the recorded changes.
    pub fn clear(&self) {
        self.state.changes().clear();
    }

    /// The recorded changes of `T` at `scope`'s target.
    pub fn changes_of<T: SettingsGroup>(&self, scope: impl Into<Scope>) -> Vec<SettingsChanged> {
        let target = scope.into().target();
        self.state
            .changes()
            .iter()
            .filter(|change| {
                change.key == T::KEY
                    && change.scope_type == target.kind
                    && change.scope_id == target.id
            })
            .cloned()
            .collect()
    }

    /// Panics unless `T` was changed at `scope`'s target.
    pub fn assert_changed<T: SettingsGroup>(&self, scope: impl Into<Scope>) {
        let scope = scope.into();
        assert!(
            !self.changes_of::<T>(scope).is_empty(),
            "expected a change of the `{}` settings at {}, got {:?}",
            T::KEY,
            scope.target(),
            self.changes()
        );
    }

    /// Panics if `T` was changed anywhere.
    pub fn assert_not_changed<T: SettingsGroup>(&self) {
        let changes: Vec<SettingsChanged> = self
            .changes()
            .into_iter()
            .filter(|change| change.key == T::KEY)
            .collect();
        assert!(
            changes.is_empty(),
            "expected no change of the `{}` settings, got {changes:?}",
            T::KEY
        );
    }
}
