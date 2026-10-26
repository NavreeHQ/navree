//! [`Flags`]: checks and changes flags.

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::sync::Arc;

use navree_audit::AuditLog;
use navree_cache::Cache;
use navree_contracts::{AuditEntry, AuditKind, Clock};
use navree_db::{Db, Uow};
use navree_events::EventBus;
use navree_kernel::WeakAppContext;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::fake::FakeState;
use crate::resolve::Facts;
use crate::{
    Flag, FlagChange, FlagChanged, FlagDefinition, FlagOverride, FlagRegistry, FlagScope,
    FlagState, FlagStore, FlagsError, FlagsOptions, PlanResolver, Rollout, ScopeType,
};

/// Checks and changes feature flags. Inject `Arc<Flags>` (FlagsModule is
/// global) and pass a scope:
///
/// ```ignore
/// let scope = FlagScope::user(deal.owner_id).workspace(workspace_id);
/// if flags.active::<AiSummaries>(&scope).await? {
///     // ...
/// }
/// flags.activate::<NewPipeline>(FlagScope::workspace(acme.id)).await?;
/// flags.rollout::<NewPipeline>(Rollout::percent(10).of_workspaces()).await?;
/// ```
///
/// Checks read the stored state through the app's cache (`CacheModule`,
/// for `flags.cache_ttl_secs`). Writes join the task's unit of work when
/// there is one (`Uow::current`, a request's while its handler runs), so
/// they commit or roll back with it; otherwise they run in their own. Each
/// change clears the cached state it touches, emits [`FlagChanged`] after
/// the commit (with `EventsModule`) and writes a `flag.changed` audit entry
/// in the same transaction (with `AuditModule`).
///
/// In tests, a [`FlagsFake`](crate::FlagsFake) gives a `Flags` that answers
/// what it is told.
#[derive(Clone)]
pub struct Flags {
    inner: Inner,
}

#[derive(Clone)]
enum Inner {
    Engine(Arc<Engine>),
    Fake(Arc<FakeState>),
}

impl fmt::Debug for Flags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match &self.inner {
            Inner::Engine(_) => "engine",
            Inner::Fake(_) => "fake",
        };
        f.debug_struct("Flags").field("kind", &kind).finish()
    }
}

/// A change to one flag.
#[derive(Clone, Debug)]
pub(crate) enum Change {
    /// The global state: on, off or unset.
    Everyone(Option<bool>),
    /// An override: on, off or removed.
    Override(ScopeType, Uuid, Option<bool>),
    /// The rollout: set or removed.
    Rollout(Option<Rollout>),
    /// The kill switch.
    Killed(bool),
    /// Every stored state of the flag.
    Purge,
}

impl Change {
    /// The change for setting `scope` to `value` (`None`: forget).
    fn for_scope(scope: &FlagScope, value: Option<bool>) -> Result<Change, FlagsError> {
        match scope.target() {
            Ok(None) => Ok(Change::Everyone(value)),
            Ok(Some((scope_type, id))) => Ok(Change::Override(scope_type, id, value)),
            Err(()) => Err(FlagsError::InvalidScope(
                "an override is for one user or one workspace, not both: use \
                 FlagScope::user(id) or FlagScope::workspace(id)"
                    .to_owned(),
            )),
        }
    }

    fn kind(&self) -> FlagChange {
        let toggled = |value: &Option<bool>| match value {
            Some(true) => FlagChange::Activated,
            Some(false) => FlagChange::Deactivated,
            None => FlagChange::Forgotten,
        };
        match self {
            Change::Everyone(value) | Change::Override(_, _, value) => toggled(value),
            Change::Rollout(Some(_)) => FlagChange::RolloutSet,
            Change::Rollout(None) => FlagChange::RolloutStopped,
            Change::Killed(true) => FlagChange::Killed,
            Change::Killed(false) => FlagChange::Revived,
            Change::Purge => FlagChange::Purged,
        }
    }
}

impl Flags {
    pub(crate) fn engine(engine: Engine) -> Flags {
        Flags {
            inner: Inner::Engine(Arc::new(engine)),
        }
    }

    pub(crate) fn fake(state: Arc<FakeState>) -> Flags {
        Flags {
            inner: Inner::Fake(state),
        }
    }

    /// Whether `F` is on for `scope` (a `&FlagScope`, `FlagScope::user(id)`,
    /// a `&Workspace`, a `&User`, ...).
    ///
    /// # Errors
    ///
    /// [`FlagsError::Unregistered`] when no module registered `F`; storage
    /// errors.
    pub async fn active<F: Flag>(&self, scope: impl Into<FlagScope>) -> Result<bool, FlagsError> {
        self.check(&FlagDefinition::of::<F>(), &scope.into()).await
    }

    /// Every registered flag's value for `scope`, by name (a fake: every
    /// flag it was told about or asked).
    pub async fn all(
        &self,
        scope: impl Into<FlagScope>,
    ) -> Result<BTreeMap<String, bool>, FlagsError> {
        let scope = scope.into();
        match &self.inner {
            Inner::Engine(engine) => {
                let definitions: Vec<FlagDefinition> = engine.registry.iter().copied().collect();
                engine.check_many(&definitions, &scope).await
            }
            Inner::Fake(fake) => Ok(fake.all(&scope)),
        }
    }

    /// Turns `F` on for one user or one workspace (an override), or for
    /// everyone with `FlagScope::everyone()` (the global state).
    pub async fn activate<F: Flag>(&self, scope: impl Into<FlagScope>) -> Result<(), FlagsError> {
        let change = Change::for_scope(&scope.into(), Some(true))?;
        self.change(&FlagDefinition::of::<F>(), change).await
    }

    /// Turns `F` off for one user, one workspace, or everyone.
    pub async fn deactivate<F: Flag>(&self, scope: impl Into<FlagScope>) -> Result<(), FlagsError> {
        let change = Change::for_scope(&scope.into(), Some(false))?;
        self.change(&FlagDefinition::of::<F>(), change).await
    }

    /// Removes the override of one user or workspace, or (with
    /// `FlagScope::everyone()`) the global state, so the next rule decides.
    pub async fn forget<F: Flag>(&self, scope: impl Into<FlagScope>) -> Result<(), FlagsError> {
        let change = Change::for_scope(&scope.into(), None)?;
        self.change(&FlagDefinition::of::<F>(), change).await
    }

    /// Sets the rollout of `F` (replacing any).
    ///
    /// # Errors
    ///
    /// [`FlagsError::InvalidRollout`] for a percentage above 100 or a blank
    /// plan; [`FlagsError::NoPlanResolver`] for a rollout by plan in an app
    /// without a `dyn PlanResolver`.
    pub async fn rollout<F: Flag>(&self, rollout: Rollout) -> Result<(), FlagsError> {
        self.set_rollout(&FlagDefinition::of::<F>(), Some(rollout))
            .await
    }

    /// Removes the rollout of `F`.
    pub async fn stop_rollout<F: Flag>(&self) -> Result<(), FlagsError> {
        self.set_rollout(&FlagDefinition::of::<F>(), None).await
    }

    /// The kill switch: `F` is off for everyone, overrides included, until
    /// [`revive`](Flags::revive).
    pub async fn kill<F: Flag>(&self) -> Result<(), FlagsError> {
        self.change(&FlagDefinition::of::<F>(), Change::Killed(true))
            .await
    }

    /// Turns the kill switch off: `F` is back to the overrides, global state
    /// and rollout it had.
    pub async fn revive<F: Flag>(&self) -> Result<(), FlagsError> {
        self.change(&FlagDefinition::of::<F>(), Change::Killed(false))
            .await
    }

    /// The stored state of `F` (unset when nothing was stored), for admin
    /// screens.
    pub async fn state<F: Flag>(&self) -> Result<FlagState, FlagsError> {
        self.state_of(&FlagDefinition::of::<F>()).await
    }

    /// Every registered flag (empty for a fake).
    pub fn registered(&self) -> Vec<FlagDefinition> {
        match &self.inner {
            Inner::Engine(engine) => engine.registry.iter().copied().collect(),
            Inner::Fake(_) => Vec::new(),
        }
    }

    // ----------------------------------------------------- by definition

    /// Whether `definition` is on for `scope`.
    pub(crate) async fn check(
        &self,
        definition: &FlagDefinition,
        scope: &FlagScope,
    ) -> Result<bool, FlagsError> {
        match &self.inner {
            Inner::Engine(engine) => {
                let values = engine
                    .check_many(std::slice::from_ref(definition), scope)
                    .await?;
                Ok(values.get(definition.name).copied().unwrap_or(false))
            }
            Inner::Fake(fake) => Ok(fake.check(definition, scope)),
        }
    }

    /// Sets or removes the rollout of `definition`.
    pub(crate) async fn set_rollout(
        &self,
        definition: &FlagDefinition,
        rollout: Option<Rollout>,
    ) -> Result<(), FlagsError> {
        if let (Some(rollout), Inner::Engine(engine)) = (&rollout, &self.inner) {
            rollout.check().map_err(FlagsError::InvalidRollout)?;
            if !rollout.plans.is_empty() && engine.plan_resolver().await.is_none() {
                return Err(FlagsError::NoPlanResolver);
            }
        }
        self.change(definition, Change::Rollout(rollout)).await
    }

    /// Applies `change` to `definition`.
    pub(crate) async fn change(
        &self,
        definition: &FlagDefinition,
        change: Change,
    ) -> Result<(), FlagsError> {
        match &self.inner {
            Inner::Engine(engine) => engine.change(definition, change).await,
            Inner::Fake(fake) => {
                fake.change(definition, &change);
                Ok(())
            }
        }
    }

    /// The stored state of `definition`.
    pub(crate) async fn state_of(
        &self,
        definition: &FlagDefinition,
    ) -> Result<FlagState, FlagsError> {
        match &self.inner {
            Inner::Engine(engine) => {
                engine.ensure_registered(definition)?;
                Ok(engine
                    .store
                    .state(None, definition.name)
                    .await?
                    .unwrap_or_else(|| FlagState::unset(definition.name)))
            }
            Inner::Fake(fake) => Ok(fake.state(definition)),
        }
    }

    /// Every stored state, by name (with flags no module registers).
    pub(crate) async fn stored_states(&self) -> Result<BTreeMap<String, FlagState>, FlagsError> {
        match &self.inner {
            Inner::Engine(engine) => Ok(engine
                .store
                .states(None)
                .await?
                .into_iter()
                .map(|state| (state.name.clone(), state))
                .collect()),
            Inner::Fake(fake) => Ok(fake.states()),
        }
    }

    /// Every override of `definition`.
    pub(crate) async fn overrides(
        &self,
        definition: &FlagDefinition,
    ) -> Result<Vec<FlagOverride>, FlagsError> {
        match &self.inner {
            Inner::Engine(engine) => {
                engine.ensure_registered(definition)?;
                engine.store.overrides(None, definition.name).await
            }
            Inner::Fake(_) => Ok(Vec::new()),
        }
    }

    /// How many overrides each flag has.
    pub(crate) async fn override_counts(&self) -> Result<BTreeMap<String, u64>, FlagsError> {
        match &self.inner {
            Inner::Engine(engine) => engine.store.override_counts(None).await,
            Inner::Fake(_) => Ok(BTreeMap::new()),
        }
    }

    /// The registered flag named `name`.
    pub(crate) fn definition(&self, name: &str) -> Result<FlagDefinition, FlagsError> {
        match &self.inner {
            Inner::Engine(engine) => engine
                .registry
                .get(name)
                .copied()
                .ok_or_else(|| FlagsError::Unknown(name.to_owned())),
            Inner::Fake(fake) => fake
                .known(name)
                .ok_or_else(|| FlagsError::Unknown(name.to_owned())),
        }
    }

    /// The names with stored state that no module registers.
    pub(crate) async fn unregistered_names(&self) -> Result<Vec<String>, FlagsError> {
        match &self.inner {
            Inner::Engine(engine) => Ok(engine
                .store
                .names(None)
                .await?
                .into_iter()
                .filter(|name| !engine.registry.contains(name))
                .collect()),
            Inner::Fake(_) => Ok(Vec::new()),
        }
    }

    /// Deletes every stored state of flag `name` (registered or not).
    pub(crate) async fn purge(&self, name: &str) -> Result<(), FlagsError> {
        match &self.inner {
            Inner::Engine(engine) => engine.purge(name).await,
            Inner::Fake(_) => Ok(()),
        }
    }
}

/// The real flags: storage, cache, registry, plans, audit and events.
pub(crate) struct Engine {
    pub(crate) registry: Arc<FlagRegistry>,
    pub(crate) store: Arc<dyn FlagStore>,
    pub(crate) options: Arc<FlagsOptions>,
    /// The database of a database store (for units of work of its own).
    pub(crate) db: Option<Arc<Db>>,
    pub(crate) cache: Option<Arc<Cache>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) app: WeakAppContext,
}

/// The cache key of the global states.
const STATES_KEY: &str = "flags:states";

fn overrides_key(scope_type: ScopeType, id: Uuid) -> String {
    format!("flags:overrides:{scope_type}:{id}")
}

fn plan_key(workspace: Uuid) -> String {
    format!("flags:plan:{workspace}")
}

impl Engine {
    fn ensure_registered(&self, definition: &FlagDefinition) -> Result<(), FlagsError> {
        if self.registry.contains(definition.name) {
            Ok(())
        } else {
            Err(FlagsError::unregistered(
                definition.name,
                definition.type_name,
            ))
        }
    }

    // --------------------------------------------------------- reading

    /// Whether reads go through a transaction whose uncommitted state must
    /// not be cached.
    async fn reads_in_transaction(&self) -> bool {
        match (&self.db, Uow::current()) {
            (Some(_), Some(uow)) => uow.open_tx().await.is_some(),
            _ => false,
        }
    }

    /// `load()`, through the cache under `key` when there is one.
    async fn cached<T, F, Fut>(&self, key: &str, load: F) -> Result<T, FlagsError>
    where
        T: Serialize + DeserializeOwned + Send + Sync,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, FlagsError>>,
    {
        let (Some(cache), Some(ttl)) = (&self.cache, self.options.cache_ttl()) else {
            return load().await;
        };
        match cache.get::<T>(key).await {
            Ok(Some(value)) => return Ok(value),
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(target: "navree::flags", key, %error, "reading cached flag state failed");
            }
        }
        let in_transaction = self.reads_in_transaction().await;
        let value = load().await?;
        if !in_transaction && let Err(error) = cache.put(key, &value, ttl).await {
            tracing::warn!(target: "navree::flags", key, %error, "caching flag state failed");
        }
        Ok(value)
    }

    async fn states(&self) -> Result<BTreeMap<String, FlagState>, FlagsError> {
        self.cached(STATES_KEY, || async {
            Ok(self
                .store
                .states(None)
                .await?
                .into_iter()
                .map(|state| (state.name.clone(), state))
                .collect())
        })
        .await
    }

    async fn overrides_of(
        &self,
        scope_type: ScopeType,
        id: Uuid,
    ) -> Result<BTreeMap<String, bool>, FlagsError> {
        self.cached(&overrides_key(scope_type, id), || {
            self.store.overrides_of(None, scope_type, id)
        })
        .await
    }

    /// The app's plan resolver, if it has one.
    pub(crate) async fn plan_resolver(&self) -> Option<Arc<dyn PlanResolver>> {
        self.app.upgrade()?.try_get::<dyn PlanResolver>().await
    }

    /// The plan of `workspace` (cached); `None` when there is no resolver
    /// or it fails (logged), so the workspace is outside plan rollouts.
    async fn plan_of(&self, workspace: Uuid) -> Option<String> {
        let resolver = match self.plan_resolver().await {
            Some(resolver) => resolver,
            None => {
                tracing::warn!(target: "navree::flags", %workspace, "a rollout by plan, but the app has no dyn PlanResolver: the workspace is outside it");
                return None;
            }
        };
        let plan = self
            .cached(&plan_key(workspace), || async {
                resolver
                    .plan(workspace)
                    .await
                    .map_err(|error| FlagsError::Other(error.to_string()))
            })
            .await;
        match plan {
            Ok(plan) => plan,
            Err(error) => {
                tracing::warn!(target: "navree::flags", %workspace, %error, "resolving a workspace's plan failed: the workspace is outside plan rollouts");
                None
            }
        }
    }

    /// The values of `definitions` for `scope`, by name.
    pub(crate) async fn check_many(
        &self,
        definitions: &[FlagDefinition],
        scope: &FlagScope,
    ) -> Result<BTreeMap<String, bool>, FlagsError> {
        for definition in definitions {
            self.ensure_registered(definition)?;
        }
        let states = self.states().await?;
        let user_overrides = match scope.user_id() {
            Some(id) => self.overrides_of(ScopeType::User, id).await?,
            None => BTreeMap::new(),
        };
        let workspace_overrides = match scope.workspace_id() {
            Some(id) => self.overrides_of(ScopeType::Workspace, id).await?,
            None => BTreeMap::new(),
        };
        let mut values = BTreeMap::new();
        for definition in definitions {
            let facts = Facts {
                flag: definition.name,
                default: definition.default,
                state: states.get(definition.name),
                user_override: user_overrides.get(definition.name).copied(),
                workspace_override: workspace_overrides.get(definition.name).copied(),
                scope,
                missing_scope: self.options.missing_scope,
            };
            let plan = match facts.plan_needed() {
                Some(workspace) => self.plan_of(workspace).await,
                None => None,
            };
            values.insert(definition.name.to_owned(), facts.decide(plan.as_deref()));
        }
        Ok(values)
    }

    // --------------------------------------------------------- writing

    /// The unit of work a write joins (the task's, when unfinished) or
    /// opens, and whether it opened it. `None` for a store without a
    /// database.
    async fn unit_of_work(&self) -> Option<(Uow, bool)> {
        let db = self.db.as_ref()?;
        if let Some(uow) = Uow::current()
            && !uow.is_finished().await
        {
            return Some((uow, false));
        }
        Some((db.uow(None, None).await, true))
    }

    /// Runs `work` in the unit of work of a write, committing it when the
    /// write opened it.
    async fn in_unit_of_work<T, F, Fut>(&self, work: F) -> Result<T, FlagsError>
    where
        F: FnOnce(Option<Uow>) -> Fut,
        Fut: Future<Output = Result<T, FlagsError>>,
    {
        match self.unit_of_work().await {
            None => work(None).await,
            Some((uow, false)) => work(Some(uow)).await,
            Some((uow, true)) => match uow.scope(work(Some(uow.clone()))).await {
                Ok(value) => {
                    uow.commit().await?;
                    Ok(value)
                }
                Err(error) => {
                    if let Err(rollback) = uow.rollback().await {
                        tracing::warn!(target: "navree::flags", error = %rollback, "rolling back a flag change failed");
                    }
                    Err(error)
                }
            },
        }
    }

    async fn change(&self, definition: &FlagDefinition, change: Change) -> Result<(), FlagsError> {
        self.ensure_registered(definition)?;
        self.in_unit_of_work(|uow| self.apply(definition.name, change, uow))
            .await
    }

    async fn purge(&self, name: &str) -> Result<(), FlagsError> {
        self.in_unit_of_work(|uow| self.apply(name, Change::Purge, uow))
            .await
    }

    /// Applies `change` to flag `name` in `uow`, then clears the cache and
    /// records the change.
    async fn apply(&self, name: &str, change: Change, uow: Option<Uow>) -> Result<(), FlagsError> {
        let uow = uow.as_ref();
        let actor = uow
            .and_then(Uow::actor)
            .or_else(|| Uow::current().and_then(|current| current.actor()));
        let mut forget = Vec::new();
        let (scope, old, new) = match &change {
            Change::Override(scope_type, id, value) => {
                let old = self
                    .store
                    .overrides_of(uow, *scope_type, *id)
                    .await?
                    .get(name)
                    .copied();
                match value {
                    Some(active) => {
                        let entry = FlagOverride {
                            flag: name.to_owned(),
                            scope_type: *scope_type,
                            scope_id: *id,
                            active: *active,
                        };
                        self.store.set_override(uow, &entry).await?;
                    }
                    None => {
                        self.store
                            .remove_override(uow, name, *scope_type, *id)
                            .await?;
                    }
                }
                forget.push(overrides_key(*scope_type, *id));
                (
                    Some((*scope_type, *id)),
                    json!({"active": old}),
                    json!({"active": value}),
                )
            }
            Change::Purge => {
                let state = self.store.state(uow, name).await?;
                let overrides = self.store.overrides(uow, name).await?;
                for entry in &overrides {
                    forget.push(overrides_key(entry.scope_type, entry.scope_id));
                }
                self.store.purge(uow, name).await?;
                forget.push(STATES_KEY.to_owned());
                let old = if state.is_none() && overrides.is_empty() {
                    Value::Null
                } else {
                    json!({"state": state, "overrides": overrides.len()})
                };
                (None, old, Value::Null)
            }
            Change::Everyone(_) | Change::Rollout(_) | Change::Killed(_) => {
                let mut state = self
                    .store
                    .state(uow, name)
                    .await?
                    .unwrap_or_else(|| FlagState::unset(name));
                let (old, new) = match &change {
                    Change::Everyone(value) => {
                        let old = json!({"everyone": state.everyone});
                        state.everyone = *value;
                        (old, json!({"everyone": value}))
                    }
                    Change::Rollout(rollout) => {
                        let old = json!({"rollout": state.rollout});
                        state.rollout.clone_from(rollout);
                        (old, json!({"rollout": rollout}))
                    }
                    Change::Killed(killed) => {
                        let old = json!({"killed": state.killed});
                        state.killed = *killed;
                        (old, json!({"killed": killed}))
                    }
                    Change::Override(..) | Change::Purge => unreachable!(),
                };
                state.updated_at = Some(self.clock.now());
                state.updated_by = actor;
                self.store.save_state(uow, &state).await?;
                forget.push(STATES_KEY.to_owned());
                (None, old, new)
            }
        };
        self.forget_cached(&forget, uow).await;
        if old == new {
            return Ok(());
        }
        let event = FlagChanged {
            flag: name.to_owned(),
            change: change.kind(),
            scope_type: scope.map(|(scope_type, _)| scope_type),
            scope_id: scope.map(|(_, id)| id),
            old,
            new,
            actor_id: actor,
        };
        self.audit(&event, uow).await?;
        self.emit(event, uow).await;
        Ok(())
    }

    /// Forgets the cache `keys` now and, in a unit of work, again once it
    /// commits (so a concurrent check cannot keep the state from before).
    async fn forget_cached(&self, keys: &[String], uow: Option<&Uow>) {
        let Some(cache) = &self.cache else {
            return;
        };
        forget_keys(cache, keys).await;
        if let Some(uow) = uow {
            let (cache, keys) = (cache.clone(), keys.to_vec());
            uow.after_commit(move || Box::pin(async move { forget_keys(&cache, &keys).await }));
        }
    }

    /// Writes the `flag.changed` audit entry, in `uow` when there is one.
    async fn audit(&self, event: &FlagChanged, uow: Option<&Uow>) -> Result<(), FlagsError> {
        let Some(app) = self.app.upgrade() else {
            return Ok(());
        };
        let Some(log) = app.try_get::<AuditLog>().await else {
            return Ok(());
        };
        let mut entry = AuditEntry::new(AuditKind::Custom, "flag.changed")
            .subject("Flag", event.flag.clone())
            .old_values(event.old.clone())
            .new_values(event.new.clone())
            .metadata(json!({
                "change": event.change.as_str(),
                "scope_type": event.scope_type,
                "scope_id": event.scope_id,
            }));
        if let Some(actor) = event.actor_id {
            entry = entry.actor(actor);
        }
        match uow {
            Some(uow) => {
                if let Some(request_id) = uow.request_id() {
                    entry = entry.request_id(request_id);
                }
                log.record_in(uow, entry).await?;
            }
            None => {
                if let Err(error) = log.record(entry).await {
                    tracing::warn!(target: "navree::flags", %error, "recording a flag change in the audit log failed");
                }
            }
        }
        Ok(())
    }

    /// Emits `event` once `uow` commits (at once without one), when the app
    /// has an `EventBus`.
    async fn emit(&self, event: FlagChanged, uow: Option<&Uow>) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let Some(bus) = app.try_get::<EventBus>().await else {
            return;
        };
        let send = async move {
            if let Err(error) = bus.emit(event).await {
                tracing::warn!(target: "navree::flags", %error, "a flag.changed listener failed");
            }
        };
        match uow {
            Some(uow) => uow.after_commit(move || Box::pin(send)),
            None => send.await,
        }
    }
}

async fn forget_keys(cache: &Cache, keys: &[String]) {
    for key in keys {
        if let Err(error) = cache.forget(key).await {
            tracing::warn!(target: "navree::flags", key = %key, %error, "clearing cached flag state failed");
        }
    }
}
