//! [`Settings`]: resolves and changes settings.

use std::fmt;
use std::sync::Arc;

use chrono::Utc;
use navree_audit::AuditLog;
use navree_cache::Cache;
use navree_contracts::{AuditEntry, AuditKind, Clock};
use navree_db::Uow;
use navree_events::EventBus;
use navree_kernel::WeakAppContext;
use navree_validation::Messages;
use navree_workspace::Workspaces;
use serde::Serialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::fake::FakeState;
use crate::registry::RegisteredSettings;
use crate::resolve::{Layer, merge};
use crate::{
    Resolved, ResolvedSettings, Scope, ScopeKind, ScopeTarget, SettingsChanged,
    SettingsDefinition, SettingsError, SettingsGroup, SettingsOptions, SettingsRegistry,
    SettingsStore, StoredSetting,
};

/// Resolves and changes settings. Inject `Arc<Settings>` (SettingsModule is
/// global) and pass a scope:
///
/// ```ignore
/// let portal = settings.resolve::<PortalSettings>(Scope::workspace(acme.id)).await?;
/// portal.show_leaderboard;                    // the value
/// portal.source("show_leaderboard");         // Some(Source::Organization)
///
/// settings
///     .update::<PortalSettings>(&uow, Scope::workspace(acme.id), json!({"welcome_text": "Hi"}))
///     .await?;
/// settings
///     .inherit::<PortalSettings>(&uow, Scope::workspace(acme.id), "welcome_text")
///     .await?;
/// ```
///
/// Reads go through the app's cache (`CacheModule`, for
/// `settings.cache_ttl_secs`), except inside a unit of work with an open
/// transaction, whose own writes they see. Writes take the unit of work
/// they belong to: they commit or roll back with it, clear the cached
/// values they affect, write a `settings.updated` audit entry in the same
/// transaction (with `AuditModule`) and emit [`SettingsChanged`] after the
/// commit (with `EventsModule`).
///
/// In tests, a [`SettingsFake`](crate::SettingsFake) gives a `Settings` on
/// an in-memory store that records every change.
#[derive(Clone)]
pub struct Settings {
    engine: Arc<Engine>,
}

impl fmt::Debug for Settings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Settings")
            .field("fake", &self.engine.fake.is_some())
            .finish_non_exhaustive()
    }
}

/// The real settings: registry, storage, cache, audit and events.
pub(crate) struct Engine {
    pub(crate) registry: Arc<SettingsRegistry>,
    pub(crate) store: Arc<dyn SettingsStore>,
    pub(crate) options: Arc<SettingsOptions>,
    /// Whether reads may run through the task's open transaction (a
    /// database store).
    pub(crate) transactional: bool,
    pub(crate) cache: Option<Arc<Cache>>,
    pub(crate) clock: Option<Arc<dyn Clock>>,
    pub(crate) app: Option<WeakAppContext>,
    /// The fake's state: its record of changes and the organizations of
    /// workspaces (looked up through `Workspaces` otherwise).
    pub(crate) fake: Option<Arc<FakeState>>,
}

/// One change to stored values, before it is recorded.
struct Change<'a> {
    entry: &'a RegisteredSettings,
    target: ScopeTarget,
    fields: Vec<String>,
    old: Map<String, Value>,
    new: Map<String, Value>,
    actor: Option<Uuid>,
}

impl Settings {
    pub(crate) fn engine(engine: Engine) -> Settings {
        Settings {
            engine: Arc::new(engine),
        }
    }

    /// `T`, resolved for `scope` (a [`Scope`], a `&Workspace`, a
    /// `&CurrentWorkspace`, a `&Principal`, ...): every field from the most
    /// specific level that stores it, else `T::default()`, with its source.
    ///
    /// # Errors
    ///
    /// [`SettingsError::Unregistered`] when no module registered `T`;
    /// storage errors.
    pub async fn resolve<T: SettingsGroup>(
        &self,
        scope: impl Into<Scope>,
    ) -> Result<Resolved<T>, SettingsError> {
        let entry = self.engine.entry::<T>()?;
        let (resolved, scope) = self.engine.resolve(&entry, scope.into(), None).await?;
        Resolved::from_json(resolved, scope)
    }

    /// `T`'s values for `scope`, without the sources.
    pub async fn get<T: SettingsGroup>(&self, scope: impl Into<Scope>) -> Result<T, SettingsError> {
        Ok(self.resolve::<T>(scope).await?.value)
    }

    /// Sets the fields of `patch` (anything that serializes to a JSON
    /// object: `json!({"welcome_text": "Hi"})`, a struct of options) at the
    /// level of `scope`'s target, in `uow`, and returns the result.
    ///
    /// The values after the patch, inherited ones included, are checked
    /// with `T`'s garde rules first. A field set to the value it inherits
    /// is still stored: it no longer follows the level above.
    ///
    /// # Errors
    ///
    /// [`SettingsError::ScopeNotAllowed`] when `T` may not be stored at the
    /// target's level; [`SettingsError::UnknownFields`];
    /// [`SettingsError::Invalid`] with the failures by field;
    /// [`SettingsError::InvalidPatch`] for a patch that is not an object.
    pub async fn update<T: SettingsGroup>(
        &self,
        uow: &Uow,
        scope: impl Into<Scope>,
        patch: impl Serialize,
    ) -> Result<Resolved<T>, SettingsError> {
        let entry = self.engine.entry::<T>()?;
        let patch = to_patch(patch)?;
        let (resolved, scope) = self
            .engine
            .update(Some(uow), &entry, scope.into(), patch)
            .await?;
        Resolved::from_json(resolved, scope)
    }

    /// [`update`](Settings::update) outside a unit of work (the fake's
    /// presets).
    pub(crate) async fn preset<T: SettingsGroup>(
        &self,
        scope: Scope,
        patch: impl Serialize,
    ) -> Result<Resolved<T>, SettingsError> {
        let entry = self.engine.entry::<T>()?;
        let patch = to_patch(patch)?;
        let (resolved, scope) = self.engine.update(None, &entry, scope, patch).await?;
        Resolved::from_json(resolved, scope)
    }

    /// Removes `field` from the level of `scope`'s target, so it inherits
    /// again, and returns the result. Nothing changes when the field was
    /// not set there.
    pub async fn inherit<T: SettingsGroup>(
        &self,
        uow: &Uow,
        scope: impl Into<Scope>,
        field: &str,
    ) -> Result<Resolved<T>, SettingsError> {
        let entry = self.engine.entry::<T>()?;
        let (resolved, scope) = self
            .engine
            .inherit(Some(uow), &entry, scope.into(), Some(&[field.to_owned()]))
            .await?;
        Resolved::from_json(resolved, scope)
    }

    /// Removes every field stored at the level of `scope`'s target, so all
    /// of them inherit again.
    pub async fn reset<T: SettingsGroup>(
        &self,
        uow: &Uow,
        scope: impl Into<Scope>,
    ) -> Result<Resolved<T>, SettingsError> {
        let entry = self.engine.entry::<T>()?;
        let (resolved, scope) = self
            .engine
            .inherit(Some(uow), &entry, scope.into(), None)
            .await?;
        Resolved::from_json(resolved, scope)
    }

    /// The fields stored at the level of `scope`'s target (empty when it
    /// inherits everything).
    pub async fn stored<T: SettingsGroup>(
        &self,
        scope: impl Into<Scope>,
    ) -> Result<Map<String, Value>, SettingsError> {
        let entry = self.engine.entry::<T>()?;
        self.engine.stored(&entry, scope.into()).await
    }

    // ------------------------------------------------------------ by key

    /// The settings registered under `key`, resolved for `scope`, as JSON
    /// (what the routes and commands use).
    ///
    /// # Errors
    ///
    /// [`SettingsError::UnknownKey`] when no settings use `key`.
    pub async fn resolve_key(
        &self,
        key: &str,
        scope: impl Into<Scope>,
    ) -> Result<ResolvedSettings, SettingsError> {
        let entry = self.engine.registry.by_key(key)?;
        Ok(self.engine.resolve(&entry, scope.into(), None).await?.0)
    }

    /// [`update`](Settings::update) for the settings registered under `key`.
    pub async fn update_key(
        &self,
        uow: &Uow,
        key: &str,
        scope: impl Into<Scope>,
        patch: Map<String, Value>,
    ) -> Result<ResolvedSettings, SettingsError> {
        let entry = self.engine.registry.by_key(key)?;
        Ok(self
            .engine
            .update(Some(uow), &entry, scope.into(), patch)
            .await?
            .0)
    }

    /// [`inherit`](Settings::inherit) for the settings registered under
    /// `key`, for every field in `fields`.
    pub async fn inherit_key(
        &self,
        uow: &Uow,
        key: &str,
        scope: impl Into<Scope>,
        fields: &[String],
    ) -> Result<ResolvedSettings, SettingsError> {
        let entry = self.engine.registry.by_key(key)?;
        Ok(self
            .engine
            .inherit(Some(uow), &entry, scope.into(), Some(fields))
            .await?
            .0)
    }

    /// [`reset`](Settings::reset) for the settings registered under `key`.
    pub async fn reset_key(
        &self,
        uow: &Uow,
        key: &str,
        scope: impl Into<Scope>,
    ) -> Result<ResolvedSettings, SettingsError> {
        let entry = self.engine.registry.by_key(key)?;
        Ok(self
            .engine
            .inherit(Some(uow), &entry, scope.into(), None)
            .await?
            .0)
    }

    /// Every registered settings type, by key.
    pub fn registry(&self) -> &SettingsRegistry {
        &self.engine.registry
    }

    /// Every stored row, or every row of `key`: for admin screens and
    /// commands.
    pub async fn rows(&self, key: Option<&str>) -> Result<Vec<StoredSetting>, SettingsError> {
        self.engine.store.list(None, key).await
    }

    /// `scope` with its organization filled in (looked up for a workspace
    /// whose organization was not named).
    pub async fn complete(&self, scope: impl Into<Scope>) -> Result<Scope, SettingsError> {
        self.engine.complete(scope.into()).await
    }

    // ------------------------------------------------------ maintenance

    /// Deletes every value stored at `target` (a deleted workspace,
    /// organization or user), without audit entries or events; the keys it
    /// held.
    pub async fn forget_scope(&self, target: ScopeTarget) -> Result<Vec<String>, SettingsError> {
        let keys = self.engine.store.delete_scope(None, target).await?;
        for key in &keys {
            self.engine.flush_tags(key, target).await;
        }
        if target.kind == ScopeKind::Workspace
            && let Some(id) = target.id
        {
            self.engine.forget_organization_of(id).await;
        }
        Ok(keys)
    }

    /// Forgets the cached organization of `workspace` (it was attached to
    /// or detached from one).
    pub async fn forget_organization_of(&self, workspace: Uuid) {
        self.engine.forget_organization_of(workspace).await;
    }
}

/// `patch` as a JSON object.
fn to_patch(patch: impl Serialize) -> Result<Map<String, Value>, SettingsError> {
    match serde_json::to_value(patch) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(other) => Err(SettingsError::InvalidPatch(format!(
            "a settings patch is a JSON object of fields, not `{other}`"
        ))),
        Err(error) => Err(SettingsError::InvalidPatch(error.to_string())),
    }
}

/// The cache tag of `key` at `target`: `settings:<key>` for the global
/// level (every entry of the key carries it), else
/// `settings:<key>:<level>:<id>`.
fn tag(key: &str, target: ScopeTarget) -> String {
    match target.id {
        None => format!("settings:{key}"),
        Some(id) => format!("settings:{key}:{}:{id}", target.kind),
    }
}

/// The cache key of the organization of `workspace`.
fn organization_key(workspace: Uuid) -> String {
    format!("settings:organization-of:{workspace}")
}

impl Engine {
    /// The registration of `T`.
    fn entry<T: SettingsGroup>(&self) -> Result<Arc<RegisteredSettings>, SettingsError> {
        self.registry.of(&SettingsDefinition::of::<T>())
    }

    // --------------------------------------------------------- reading

    /// Whether reads now run through the task's open transaction, whose
    /// uncommitted state must not be cached.
    async fn reads_in_transaction(&self) -> bool {
        if !self.transactional {
            return false;
        }
        match Uow::current() {
            Some(uow) => uow.open_tx().await.is_some(),
            None => false,
        }
    }

    /// The organization of `workspace`, if it has one (cached).
    async fn organization_of(&self, workspace: Uuid) -> Result<Option<Uuid>, SettingsError> {
        match &self.fake {
            Some(fake) => Ok(fake.organization_of(workspace)),
            None => {
                let cache = self.cache_with_ttl();
                if let Some((cache, _)) = &cache {
                    match cache.get::<Option<Uuid>>(&organization_key(workspace)).await {
                        Ok(Some(found)) => return Ok(found),
                        Ok(None) => {}
                        Err(error) => {
                            tracing::warn!(target: "navree::settings", %error, "reading a cached organization failed");
                        }
                    }
                }
                let Some(app) = self.app.as_ref().and_then(WeakAppContext::upgrade) else {
                    return Ok(None);
                };
                let Some(workspaces) = app.try_get::<Workspaces>().await else {
                    return Ok(None);
                };
                let organization = workspaces
                    .find(workspace)
                    .await
                    .map_err(|error| SettingsError::Other(error.to_string()))?
                    .and_then(|found| found.organization_id);
                if let Some((cache, ttl)) = cache
                    && !self.reads_in_transaction().await
                    && let Err(error) = cache
                        .put(&organization_key(workspace), &organization, ttl)
                        .await
                {
                    tracing::warn!(target: "navree::settings", %error, "caching an organization failed");
                }
                Ok(organization)
            }
        }
    }

    /// `scope` with its workspace's organization filled in.
    async fn complete(&self, scope: Scope) -> Result<Scope, SettingsError> {
        if scope.organization_known() {
            return Ok(scope);
        }
        let Some(workspace) = scope.workspace_id() else {
            return Ok(scope);
        };
        Ok(scope.in_organization(self.organization_of(workspace).await?))
    }

    fn cache_with_ttl(&self) -> Option<(Arc<Cache>, std::time::Duration)> {
        Some((self.cache.clone()?, self.options.cache_ttl()?))
    }

    /// The levels of `scope` that `entry` may be stored at, most specific
    /// first.
    fn chain(entry: &RegisteredSettings, scope: &Scope) -> Vec<ScopeTarget> {
        scope
            .chain()
            .into_iter()
            .filter(|target| entry.definition.allows(target.kind))
            .collect()
    }

    /// `entry` resolved for `scope`, reading through `via` when given (and
    /// then never through the cache); the scope with its organization.
    pub(crate) async fn resolve(
        &self,
        entry: &RegisteredSettings,
        scope: Scope,
        via: Option<&Uow>,
    ) -> Result<(ResolvedSettings, Scope), SettingsError> {
        let scope = self.complete(scope).await?;
        let key = entry.definition.key;
        let target = scope.target();
        let chain = Self::chain(entry, &scope);
        let cache = match self.cache_with_ttl() {
            Some(cache) if via.is_none() && !self.reads_in_transaction().await => Some(cache),
            _ => None,
        };
        let cache_key = format!(
            "settings:{key}:resolved:{target}|{}",
            chain
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("|")
        );
        let tags: Vec<String> = std::iter::once(format!("settings:{key}"))
            .chain(
                chain
                    .iter()
                    .filter(|level| level.id.is_some())
                    .map(|level| tag(key, *level)),
            )
            .collect();
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        if let Some((cache, _)) = &cache {
            match cache
                .tags(&tag_refs)
                .get::<ResolvedSettings>(&cache_key)
                .await
            {
                Ok(Some(hit)) => return Ok((hit, scope)),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(target: "navree::settings", key, %error, "reading cached settings failed");
                }
            }
        }
        let rows = self.store.load(via, key, &chain).await?;
        let resolved = Self::merge_rows(entry, target, &chain, &rows)?;
        if let Some((cache, ttl)) = cache
            && let Err(error) = cache.tags(&tag_refs).put(&cache_key, &resolved, ttl).await
        {
            tracing::warn!(target: "navree::settings", key, %error, "caching settings failed");
        }
        Ok((resolved, scope))
    }

    /// `rows` (of the levels in `chain`) merged over the defaults.
    fn merge_rows(
        entry: &RegisteredSettings,
        target: ScopeTarget,
        chain: &[ScopeTarget],
        rows: &[StoredSetting],
    ) -> Result<ResolvedSettings, SettingsError> {
        let layers: Vec<Layer<'_>> = chain
            .iter()
            .rev()
            .filter_map(|level| {
                rows.iter()
                    .find(|row| row.scope == *level)
                    .map(|row| Layer {
                        source: level.kind.source(),
                        values: &row.values,
                    })
            })
            .collect();
        let merged = merge(&entry.definition, &layers)?;
        for (field, source, error) in &merged.skipped {
            tracing::warn!(
                target: "navree::settings",
                key = entry.definition.key,
                field = %field,
                level = %source,
                %error,
                "a stored setting no longer fits its type and is ignored"
            );
        }
        // The target's fields that resolution used: the type's, minus those
        // whose stored value was skipped.
        let own = target.kind.source();
        let mut overridden: Vec<String> = rows
            .iter()
            .find(|row| row.scope == target)
            .map(|row| {
                row.values
                    .keys()
                    .filter(|field| entry.has_field(field))
                    .filter(|field| {
                        !merged
                            .skipped
                            .iter()
                            .any(|(skipped, level, _)| skipped == *field && *level == own)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        overridden.sort();
        Ok(ResolvedSettings {
            data: merged.data,
            sources: merged.sources,
            overridden,
        })
    }

    /// The fields stored at `scope`'s target.
    async fn stored(
        &self,
        entry: &RegisteredSettings,
        scope: Scope,
    ) -> Result<Map<String, Value>, SettingsError> {
        let target = scope.target();
        Ok(self
            .store
            .load(None, entry.definition.key, std::slice::from_ref(&target))
            .await?
            .into_iter()
            .next()
            .map(|row| row.values)
            .unwrap_or_default())
    }

    // --------------------------------------------------------- writing

    /// Refuses a write at a level `entry` may not be stored at.
    fn check_target(entry: &RegisteredSettings, target: ScopeTarget) -> Result<(), SettingsError> {
        if entry.definition.allows(target.kind) {
            return Ok(());
        }
        Err(SettingsError::ScopeNotAllowed {
            key: entry.definition.key.to_owned(),
            scope: target.kind,
            allowed: entry.definition.levels_list(),
        })
    }

    /// Refuses fields `entry` does not have.
    fn check_fields<'a>(
        entry: &RegisteredSettings,
        fields: impl IntoIterator<Item = &'a String>,
    ) -> Result<(), SettingsError> {
        let unknown: Vec<String> = fields
            .into_iter()
            .filter(|field| !entry.has_field(field))
            .cloned()
            .collect();
        if unknown.is_empty() {
            Ok(())
        } else {
            Err(SettingsError::UnknownFields {
                key: entry.definition.key.to_owned(),
                fields: unknown,
            })
        }
    }

    /// The app's validation messages, or the defaults.
    async fn messages(&self) -> Arc<Messages> {
        if let Some(app) = self.app.as_ref().and_then(WeakAppContext::upgrade)
            && let Some(messages) = app.try_get::<Messages>().await
        {
            return messages;
        }
        Arc::new(Messages::default())
    }

    /// The row of `entry` at `target`, locked for the write when the store
    /// can.
    async fn row_for_update(
        &self,
        uow: Option<&Uow>,
        entry: &RegisteredSettings,
        target: ScopeTarget,
    ) -> Result<Option<StoredSetting>, SettingsError> {
        match uow {
            Some(uow) => {
                self.store
                    .load_for_update(uow, entry.definition.key, target)
                    .await
            }
            None => Ok(self
                .store
                .load(None, entry.definition.key, std::slice::from_ref(&target))
                .await?
                .into_iter()
                .next()),
        }
    }

    pub(crate) async fn update(
        &self,
        uow: Option<&Uow>,
        entry: &RegisteredSettings,
        scope: Scope,
        patch: Map<String, Value>,
    ) -> Result<(ResolvedSettings, Scope), SettingsError> {
        let scope = self.complete(scope).await?;
        let target = scope.target();
        Self::check_target(entry, target)?;
        Self::check_fields(entry, patch.keys())?;
        let row = self.row_for_update(uow, entry, target).await?;
        let (current, _) = self.resolve(entry, scope, uow).await?;
        if patch.is_empty() {
            return Ok((current, scope));
        }
        let mut merged = current.data.clone();
        merged.extend(patch.clone());
        let messages = self.messages().await;
        let validated = entry
            .definition
            .validate(&Value::Object(merged), &messages)
            .map_err(SettingsError::Invalid)?;
        let mut values = row.as_ref().map(|row| row.values.clone()).unwrap_or_default();
        let (mut old, mut new) = (Map::new(), Map::new());
        let mut fields = Vec::new();
        for field in patch.keys() {
            let value = validated.get(field).cloned().unwrap_or(Value::Null);
            let before = values.insert(field.clone(), value.clone());
            if before.as_ref() != Some(&value) {
                if let Some(before) = before {
                    old.insert(field.clone(), before);
                }
                new.insert(field.clone(), value);
                fields.push(field.clone());
            }
        }
        if fields.is_empty() {
            return Ok((current, scope));
        }
        let actor = actor_of(uow);
        let setting = StoredSetting {
            key: entry.definition.key.to_owned(),
            scope: target,
            values,
            updated_at: self.now(),
            updated_by: actor,
        };
        self.store.save(uow, &setting).await?;
        fields.sort();
        self.changed(
            uow,
            Change {
                entry,
                target,
                fields,
                old,
                new,
                actor,
            },
        )
        .await?;
        self.resolve(entry, scope, uow).await
    }

    /// Removes `fields` (all with `None`) from `scope`'s target.
    pub(crate) async fn inherit(
        &self,
        uow: Option<&Uow>,
        entry: &RegisteredSettings,
        scope: Scope,
        fields: Option<&[String]>,
    ) -> Result<(ResolvedSettings, Scope), SettingsError> {
        let scope = self.complete(scope).await?;
        let target = scope.target();
        Self::check_target(entry, target)?;
        if let Some(fields) = fields {
            Self::check_fields(entry, fields)?;
        }
        let Some(row) = self.row_for_update(uow, entry, target).await? else {
            return self.resolve(entry, scope, uow).await;
        };
        let mut values = row.values.clone();
        let mut old = Map::new();
        let removing: Vec<String> = match fields {
            Some(fields) => fields.to_vec(),
            None => values.keys().cloned().collect(),
        };
        for field in &removing {
            if let Some(before) = values.remove(field) {
                old.insert(field.clone(), before);
            }
        }
        if old.is_empty() {
            return self.resolve(entry, scope, uow).await;
        }
        let actor = actor_of(uow);
        if values.is_empty() {
            self.store.delete(uow, entry.definition.key, target).await?;
        } else {
            let setting = StoredSetting {
                values,
                updated_at: self.now(),
                updated_by: actor,
                ..row
            };
            self.store.save(uow, &setting).await?;
        }
        let fields = old.keys().cloned().collect();
        self.changed(
            uow,
            Change {
                entry,
                target,
                fields,
                old,
                new: Map::new(),
                actor,
            },
        )
        .await?;
        self.resolve(entry, scope, uow).await
    }

    fn now(&self) -> chrono::DateTime<Utc> {
        match &self.clock {
            Some(clock) => clock.now(),
            None => Utc::now(),
        }
    }

    /// Clears the cache, records and announces a change.
    async fn changed(&self, uow: Option<&Uow>, change: Change<'_>) -> Result<(), SettingsError> {
        let key = change.entry.definition.key;
        self.flush_tags(key, change.target).await;
        if let (Some(uow), Some((cache, _))) = (uow, self.cache_with_ttl()) {
            let tag = tag(key, change.target);
            uow.after_commit(move || {
                Box::pin(async move {
                    if let Err(error) = cache.tags(&[&tag]).flush().await {
                        tracing::warn!(target: "navree::settings", %error, "clearing cached settings failed");
                    }
                })
            });
        }
        let event = SettingsChanged {
            key: key.to_owned(),
            scope_type: change.target.kind,
            scope_id: change.target.id,
            fields: change.fields,
            old: change.old,
            new: change.new,
            actor_id: change.actor,
        };
        if let Some(fake) = &self.fake {
            fake.record(&event);
        }
        self.audit(&event, uow).await?;
        self.emit(event, uow).await;
        Ok(())
    }

    /// Invalidates the cached values `key` at `target` affects.
    pub(crate) async fn flush_tags(&self, key: &str, target: ScopeTarget) {
        if let Some(cache) = &self.cache
            && let Err(error) = cache.tags(&[&tag(key, target)]).flush().await
        {
            tracing::warn!(target: "navree::settings", key, %error, "clearing cached settings failed");
        }
    }

    pub(crate) async fn forget_organization_of(&self, workspace: Uuid) {
        if let Some(cache) = &self.cache
            && let Err(error) = cache.forget(&organization_key(workspace)).await
        {
            tracing::warn!(target: "navree::settings", %error, "clearing a cached organization failed");
        }
    }

    /// Writes the `settings.updated` audit entry, in `uow` when there is
    /// one.
    async fn audit(&self, event: &SettingsChanged, uow: Option<&Uow>) -> Result<(), SettingsError> {
        let Some(app) = self.app.as_ref().and_then(WeakAppContext::upgrade) else {
            return Ok(());
        };
        let Some(log) = app.try_get::<AuditLog>().await else {
            return Ok(());
        };
        let mut entry = AuditEntry::new(AuditKind::Custom, "settings.updated")
            .subject("Settings", event.key.clone())
            .old_values(Value::Object(event.old.clone()))
            .new_values(Value::Object(event.new.clone()))
            .metadata(json!({
                "scope_type": event.scope_type,
                "scope_id": event.scope_id,
                "fields": event.fields,
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
                    tracing::warn!(target: "navree::settings", %error, "recording a settings change in the audit log failed");
                }
            }
        }
        Ok(())
    }

    /// Emits `event` once `uow` commits (at once without one), when the app
    /// has an `EventBus`.
    async fn emit(&self, event: SettingsChanged, uow: Option<&Uow>) {
        let Some(app) = self.app.as_ref().and_then(WeakAppContext::upgrade) else {
            return;
        };
        let Some(bus) = app.try_get::<EventBus>().await else {
            return;
        };
        let send = async move {
            if let Err(error) = bus.emit(event).await {
                tracing::warn!(target: "navree::settings", %error, "a settings.changed listener failed");
            }
        };
        match uow {
            Some(uow) => uow.after_commit(move || Box::pin(send)),
            None => send.await,
        }
    }
}

/// Who makes a change: the unit of work's actor, else the task's.
fn actor_of(uow: Option<&Uow>) -> Option<Uuid> {
    uow.and_then(Uow::actor)
        .or_else(|| Uow::current().and_then(|current| current.actor()))
}
