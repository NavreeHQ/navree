//! What the [`Directory`](crate::Directory) service works with once it is
//! bound to an app: the options, the sources, the database and the app's
//! services.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_config::Config;
use navree_contracts::{AuditEntry, AuditKind, AuditSink, Clock, Secret, SystemClock};
use navree_crypto::Encrypter;
use navree_db::sea_orm::DbBackend;
use navree_db::{Db, Uow};
use navree_events::{Event, EventBus};
use navree_integrations::{Level, Owner, OwnerType};
use navree_kernel::{AppContext, BoxError, WeakAppContext};
use navree_workspace::WorkspaceConfig;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::error::DirectoryError;
use crate::fake::FakeState;
use crate::models;
use crate::options::DirectoryOptions;
use crate::sink::DirectorySink;
use crate::source::{self, DirectorySource, SourceRegistration, is_source_name};
use crate::types::{ConnectionStatus, DirectoryConnection, SCIM};

/// The bound service's state.
pub(crate) struct Runtime {
    pub(crate) options: Arc<DirectoryOptions>,
    pub(crate) db: Arc<Db>,
    pub(crate) encrypter: Option<Arc<Encrypter>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) app: WeakAppContext,
    /// The pull sources by name (built-in and registered).
    pub(crate) sources: BTreeMap<String, Arc<dyn DirectorySource>>,
    /// A fake's directory, which answers every pull.
    pub(crate) fake: Option<Arc<FakeState>>,
    /// Whose connections the routes manage (`workspace` turns into `app`
    /// without `WorkspaceModule`).
    pub(crate) level: Level,
    /// The public URL of the SCIM endpoints (or their path).
    pub(crate) scim_base: String,
}

impl Runtime {
    /// Resolves everything from `app`.
    pub(crate) async fn build(
        app: &AppContext,
        fake: Option<Arc<FakeState>>,
    ) -> Result<Runtime, BoxError> {
        let options = app.get::<DirectoryOptions>().await?;
        let mut sources: BTreeMap<String, Arc<dyn DirectorySource>> = BTreeMap::new();
        for (name, source) in source::built_in() {
            sources.insert(name.to_owned(), source);
        }
        for contributed in app.contributions::<SourceRegistration>() {
            let registration = &*contributed.value;
            if !is_source_name(&registration.name) || registration.name == SCIM {
                return Err(format!(
                    "directory: source `{}` (module {}) must be lower-case letters, digits, `-` and `_`, and not `scim`",
                    registration.name, contributed.module.name
                )
                .into());
            }
            sources.insert(registration.name.clone(), registration.source.clone());
        }
        let clock = match app.try_get::<dyn Clock>().await {
            Some(clock) => clock,
            None => Arc::new(SystemClock),
        };
        let workspaces = app.try_get::<WorkspaceConfig>().await;
        let level = match (options.level, &workspaces) {
            (Level::Workspace, None) => Level::App,
            (Level::Organization, None) => {
                return Err(
                    "directory.level = \"organization\" needs WorkspaceModule in the app".into(),
                );
            }
            (Level::Organization, Some(config)) if !config.organizations.enabled => {
                return Err(
                    "directory.level = \"organization\" needs workspace.organizations.enabled"
                        .into(),
                );
            }
            (level, _) => level,
        };
        let scim_base = match &options.scim.base_url {
            Some(base) => base.trim_end_matches('/').to_owned(),
            None => {
                let app_url = match app.try_get::<Config>().await {
                    Some(config) => config.get_string("app.url"),
                    None => None,
                };
                match app_url {
                    Some(url) if !url.trim().is_empty() => {
                        format!("{}{}", url.trim().trim_end_matches('/'), options.scim.path)
                    }
                    _ => options.scim.path.clone(),
                }
            }
        };
        Ok(Runtime {
            db: app.get::<Db>().await?,
            encrypter: app.try_get::<Encrypter>().await,
            clock,
            app: app.downgrade(),
            sources,
            fake,
            level,
            scim_base,
            options,
        })
    }

    pub(crate) fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    /// The app, while it runs.
    pub(crate) fn app(&self) -> Result<AppContext, DirectoryError> {
        self.app.upgrade().ok_or(DirectoryError::NotBound)
    }

    /// The source a pull connection reads from (the fake's directory when
    /// the service is a fake).
    pub(crate) fn source(&self, name: &str) -> Result<Arc<dyn DirectorySource>, DirectoryError> {
        if name == SCIM {
            return Err(DirectoryError::PushOnly);
        }
        if let Some(fake) = &self.fake {
            return Ok(fake.clone() as Arc<dyn DirectorySource>);
        }
        self.sources
            .get(name)
            .cloned()
            .ok_or_else(|| DirectoryError::UnknownSource(name.to_owned()))
    }

    /// Whether `name` is a source connections can use.
    pub(crate) fn knows_source(&self, name: &str) -> bool {
        name == SCIM || self.sources.contains_key(name) || (self.fake.is_some() && is_source_name(name))
    }

    /// Checks a pull connection's settings with its source.
    pub(crate) fn check_settings(
        &self,
        source: &str,
        settings: &Value,
        has_secret: bool,
    ) -> Result<(), DirectoryError> {
        if !settings.is_object() {
            return Err(DirectoryError::InvalidSettings(
                "settings must be a JSON object".to_owned(),
            ));
        }
        if source == SCIM || self.fake.is_some() {
            return Ok(());
        }
        let source = self
            .sources
            .get(source)
            .ok_or_else(|| DirectoryError::UnknownSource(source.to_owned()))?;
        source
            .check_settings(settings, has_secret)
            .map_err(DirectoryError::InvalidSettings)
    }

    /// The registered sinks, resolved when needed (a sink may depend on
    /// the `Directory` service).
    pub(crate) async fn sinks(&self) -> Result<Vec<Arc<dyn DirectorySink>>, DirectoryError> {
        let app = self.app()?;
        Ok(app.get_all::<dyn DirectorySink>().await?)
    }

    /// The public view of a connection row.
    pub(crate) fn info(&self, row: &models::Connection) -> DirectoryConnection {
        let owner = owner_of(&row.owner_type, row.owner_id);
        DirectoryConnection {
            id: row.id,
            owner_type: owner_type(owner),
            owner_id: owner.id(),
            name: row.name.clone(),
            source: row.source.clone(),
            settings: serde_json::from_str(&row.settings).unwrap_or_else(|_| json!({})),
            has_secret: row.secret.is_some(),
            status: ConnectionStatus::parse(&row.status),
            scim_token_hint: row.scim_token_hint.clone(),
            sync_interval_mins: u32::try_from(row.sync_interval_mins.max(0)).unwrap_or(0),
            last_run_id: row.last_run_id,
            last_synced_at: row.last_synced_at,
            next_sync_at: row.next_sync_at,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }

    /// Encrypts a secret with the application key.
    pub(crate) fn seal(&self, secret: &str) -> Result<String, DirectoryError> {
        let encrypter = self.encrypter.as_ref().ok_or_else(|| {
            DirectoryError::Config(
                "storing a connection secret needs CryptoModule with an application key".to_owned(),
            )
        })?;
        Ok(encrypter.encrypt_string(secret)?)
    }

    /// The decrypted secret of a row.
    pub(crate) fn secret(
        &self,
        row: &models::Connection,
    ) -> Result<Option<Secret<String>>, DirectoryError> {
        let Some(sealed) = &row.secret else {
            return Ok(None);
        };
        let encrypter = self.encrypter.as_ref().ok_or_else(|| {
            DirectoryError::Config(
                "reading a connection secret needs CryptoModule with an application key".to_owned(),
            )
        })?;
        Ok(Some(Secret::new(encrypter.decrypt_string(sealed)?)))
    }

    /// Emits `event` once `uow` commits, when the app has an `EventBus`.
    pub(crate) async fn emit_after_commit<E: Event>(&self, uow: &Uow, event: E) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let Some(bus) = app.try_get::<EventBus>().await else {
            return;
        };
        uow.after_commit(move || {
            Box::pin(async move {
                if let Err(error) = bus.emit(event).await {
                    tracing::warn!(target: "navree::directory", event = E::NAME, %error, "a directory event listener failed");
                }
            })
        });
    }

    /// Records an audit entry for `connection` once `uow` commits, when
    /// the app has an audit sink.
    pub(crate) async fn audit_after_commit(
        &self,
        uow: &Uow,
        action: &str,
        connection: &models::Connection,
        extra: Value,
    ) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let Some(sink) = app.try_get::<dyn AuditSink>().await else {
            return;
        };
        let mut metadata = json!({
            "name": connection.name,
            "source": connection.source,
            "owner": owner_of(&connection.owner_type, connection.owner_id),
        });
        if let (Value::Object(metadata), Value::Object(extra)) = (&mut metadata, extra) {
            metadata.extend(extra);
        }
        let mut entry = AuditEntry::new(AuditKind::Custom, action)
            .subject("DirectoryConnection", connection.id.to_string())
            .metadata(metadata);
        if let Some(actor) = uow.actor() {
            entry = entry.actor(actor);
        }
        if let Some(request_id) = uow.request_id() {
            entry = entry.request_id(request_id);
        }
        uow.after_commit(move || {
            Box::pin(async move {
                let action = entry.action.clone();
                if let Err(error) = sink.record(entry).await {
                    tracing::warn!(target: "navree::directory", %error, action, "could not write an audit entry");
                }
            })
        });
    }

    /// Runs `f` in the task's unfinished unit of work (a request's), or else
    /// in a new one committed when `f` succeeds.
    pub(crate) async fn write<T, F, Fut>(&self, actor: Option<Uuid>, f: F) -> Result<T, DirectoryError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, DirectoryError>>,
    {
        if let Some(uow) = Uow::current()
            && !uow.is_finished().await
        {
            return f(uow).await;
        }
        self.own_unit(actor, f).await
    }

    /// Runs `f` in a unit of work of its own, committed when `f` succeeds.
    /// On SQLite, inside a request that already holds the write lock, it
    /// joins that request instead (a second writer would wait for the
    /// first forever).
    pub(crate) async fn housekeeping<T, F, Fut>(&self, f: F) -> Result<T, DirectoryError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, DirectoryError>>,
    {
        if self.db.backend() == DbBackend::Sqlite
            && let Some(uow) = Uow::current()
            && uow.open_tx().await.is_some()
        {
            return f(uow).await;
        }
        self.own_unit(None, f).await
    }

    pub(crate) async fn own_unit<T, F, Fut>(
        &self,
        actor: Option<Uuid>,
        f: F,
    ) -> Result<T, DirectoryError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, DirectoryError>>,
    {
        let uow = self.db.uow(actor, None).await;
        match uow.scope(f(uow.clone())).await {
            Ok(value) => {
                uow.commit().await?;
                Ok(value)
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "navree::directory", error = %rollback, "rollback failed");
                }
                Err(error)
            }
        }
    }
}

/// The `owner_type` and `owner_id` columns of an owner (the nil UUID for
/// the app).
pub(crate) fn owner_columns(owner: Owner) -> (&'static str, Uuid) {
    (owner_type(owner).as_str(), owner.id().unwrap_or(Uuid::nil()))
}

/// The owner of a row.
pub(crate) fn owner_of(kind: &str, id: Uuid) -> Owner {
    match kind {
        "workspace" => Owner::Workspace(id),
        "organization" => Owner::Organization(id),
        _ => Owner::App,
    }
}

fn owner_type(owner: Owner) -> OwnerType {
    owner.kind()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners_round_trip_through_their_columns() {
        let id = Uuid::now_v7();
        for owner in [Owner::App, Owner::Workspace(id), Owner::Organization(id)] {
            let (kind, column) = owner_columns(owner);
            assert_eq!(owner_of(kind, column), owner);
        }
        assert_eq!(owner_columns(Owner::App), ("app", Uuid::nil()));
    }
}
