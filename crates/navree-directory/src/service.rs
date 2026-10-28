//! [`Directory`]: the service behind the routes, the SCIM server and the
//! commands.

use std::fmt;
use std::sync::{Arc, OnceLock};

use chrono::TimeDelta;
use navree_contracts::{Dispatcher, Job as _, JobEnvelope, Secret};
use navree_crypto::{random_token, sha256_hex};
use navree_db::Uow;
use navree_integrations::{Level, Owner};
use navree_kernel::{AppContext, BoxError, BoxFuture, Lifecycle};
use navree_orm::{Model as _, Record};
use navree_queue::{Operation, Operations};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::apply::{group_with_members, groups_of};
use crate::error::DirectoryError;
use crate::fake::{DirectoryFake, FakeState};
use crate::models;
use crate::runs::{SyncDirectory, change_view, run_view};
use crate::runtime::{Runtime, owner_columns, owner_of};
use crate::types::{
    ConnectionStatus, DirectoryConnection, RunStatus, RunTrigger, SCIM, SyncChange, SyncOptions,
    SyncRun, SyncedGroup, SyncedUser,
};
use crate::user::DirectoryUser;

/// The prefix of SCIM tokens.
pub const SCIM_TOKEN_PREFIX: &str = "nvscim_";

/// A new connection ([`Directory::create_connection`]).
///
/// ```
/// use navree_directory::NewConnection;
/// use serde_json::json;
///
/// let ldap = NewConnection::new("Acme AD", "ldap")
///     .settings(json!({
///         "url": "ldaps://dc1.acme.test",
///         "base_dn": "DC=acme,DC=test",
///         "bind_dn": "CN=navree-sync,OU=Service,DC=acme,DC=test"
///     }))
///     .secret("bind password")
///     .interval(30);
/// assert_eq!(ldap.sync_interval_mins, Some(30));
/// ```
#[derive(Clone, Default, PartialEq, Deserialize, ToSchema)]
#[schema(as = DirectoryNewConnection)]
pub struct NewConnection {
    /// Its name, for people.
    pub name: String,
    /// The source: `scim`, `microsoft`, `google`, `okta`, `ldap` or a
    /// registered one.
    pub source: String,
    /// The source's settings.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub settings: Value,
    /// The secret (the LDAP bind password), encrypted at rest.
    #[serde(default)]
    pub secret: Option<String>,
    /// Minutes between scheduled syncs (default
    /// `directory.sync.default_interval_mins`; 0: on demand only).
    #[serde(default)]
    pub sync_interval_mins: Option<u32>,
    /// The user creating it.
    #[serde(skip)]
    pub created_by: Option<Uuid>,
}

impl NewConnection {
    /// A connection named `name` to `source`.
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: source.into(),
            settings: json!({}),
            ..Self::default()
        }
    }

    /// Sets the settings.
    #[must_use]
    pub fn settings(mut self, settings: Value) -> Self {
        self.settings = settings;
        self
    }

    /// Sets the secret.
    #[must_use]
    pub fn secret(mut self, secret: impl Into<String>) -> Self {
        self.secret = Some(secret.into());
        self
    }

    /// Sets the minutes between scheduled syncs.
    #[must_use]
    pub fn interval(mut self, minutes: u32) -> Self {
        self.sync_interval_mins = Some(minutes);
        self
    }

    /// Sets who created it.
    #[must_use]
    pub fn created_by(mut self, user: Uuid) -> Self {
        self.created_by = Some(user);
        self
    }
}

impl fmt::Debug for NewConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewConnection")
            .field("name", &self.name)
            .field("source", &self.source)
            .field("settings", &self.settings)
            .field("secret", &self.secret.as_ref().map(|_| "***"))
            .finish_non_exhaustive()
    }
}

/// A change to a connection ([`Directory::update_connection`]); `None`
/// leaves a field as it is.
#[derive(Clone, Default, PartialEq, Deserialize, ToSchema)]
#[schema(as = DirectoryConnectionUpdate)]
pub struct ConnectionUpdate {
    /// A new name.
    pub name: Option<String>,
    /// New settings (replacing the old ones).
    #[schema(value_type = Option<Object>)]
    pub settings: Option<Value>,
    /// A new secret; an empty string removes it.
    pub secret: Option<String>,
    /// New minutes between scheduled syncs.
    pub sync_interval_mins: Option<u32>,
    /// `active` or `paused`.
    pub status: Option<ConnectionStatus>,
}

impl fmt::Debug for ConnectionUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionUpdate")
            .field("name", &self.name)
            .field("settings", &self.settings)
            .field("secret", &self.secret.as_ref().map(|_| "***"))
            .field("sync_interval_mins", &self.sync_interval_mins)
            .field("status", &self.status)
            .finish()
    }
}

/// A created connection, with its SCIM token (shown once).
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CreatedConnection {
    /// The connection.
    pub connection: DirectoryConnection,
    /// For a SCIM connection: the bearer token to give the identity
    /// provider. It is stored only as a hash.
    #[serde(serialize_with = "expose_token")]
    #[schema(value_type = Option<String>)]
    pub scim_token: Option<Secret<String>>,
    /// For a SCIM connection: the URL to give the identity provider.
    pub scim_url: Option<String>,
}

fn expose_token<S: serde::Serializer>(
    token: &Option<Secret<String>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match token {
        Some(token) => serializer.serialize_some(token.expose()),
        None => serializer.serialize_none(),
    }
}

/// Directory connections, SCIM, sync runs and the synced users and groups.
///
/// Resolve it from the app (`ctx.get::<Directory>()`, or inject
/// `Arc<Directory>`). [`Directory::fake`] gives one whose pull sources are
/// a [`DirectoryFake`].
pub struct Directory {
    fake: Option<Arc<FakeState>>,
    runtime: OnceLock<Arc<Runtime>>,
}

impl Directory {
    /// A service whose pull sources all read the [`DirectoryFake`]'s
    /// directory: pass it to `override_value(Arc::new(directory))`.
    pub fn fake() -> (Directory, DirectoryFake) {
        let (fake, state) = DirectoryFake::new();
        (
            Directory {
                fake: Some(state),
                runtime: OnceLock::new(),
            },
            fake,
        )
    }

    pub(crate) fn real(runtime: Runtime) -> Directory {
        let bound = OnceLock::new();
        let _ = bound.set(Arc::new(runtime));
        Directory {
            fake: None,
            runtime: bound,
        }
    }

    /// Whether this is a [`fake`](Directory::fake).
    pub fn is_fake(&self) -> bool {
        self.fake.is_some()
    }

    pub(crate) fn rt(&self) -> Result<&Arc<Runtime>, DirectoryError> {
        self.runtime.get().ok_or(DirectoryError::NotBound)
    }

    /// Whose connections the routes manage.
    pub fn level(&self) -> Result<Level, DirectoryError> {
        Ok(self.rt()?.level)
    }

    /// The sources connections can use: `scim`, then the pull sources.
    pub fn sources(&self) -> Result<Vec<String>, DirectoryError> {
        let rt = self.rt()?;
        Ok(std::iter::once(SCIM.to_owned())
            .chain(rt.sources.keys().cloned())
            .collect())
    }

    /// The SCIM base URL to give identity providers
    /// (`https://app.acme.test/scim/v2`).
    pub fn scim_url(&self) -> Result<String, DirectoryError> {
        Ok(self.rt()?.scim_base.clone())
    }

    // -- connections ----------------------------------------------------------

    /// Creates a connection for `owner`. A SCIM connection gets its bearer
    /// token, returned once; a pull connection's settings are checked by
    /// its source, and its first scheduled sync is due at once.
    pub async fn create_connection(
        &self,
        owner: Owner,
        input: NewConnection,
    ) -> Result<CreatedConnection, DirectoryError> {
        let rt = self.rt()?.clone();
        let name = input.name.trim().to_owned();
        if name.is_empty() || name.len() > 255 {
            return Err(DirectoryError::InvalidSettings(
                "name must be 1 to 255 characters".to_owned(),
            ));
        }
        if !rt.knows_source(&input.source) {
            return Err(DirectoryError::UnknownSource(input.source));
        }
        let settings = if input.settings.is_null() {
            json!({})
        } else {
            input.settings
        };
        let secret = input.secret.filter(|s| !s.is_empty());
        rt.check_settings(&input.source, &settings, secret.is_some())?;
        let sealed = secret.as_deref().map(|s| rt.seal(s)).transpose()?;
        let is_scim = input.source == SCIM;
        let token = is_scim.then(new_token);
        let now = rt.now();
        let interval = if is_scim {
            0
        } else {
            input
                .sync_interval_mins
                .unwrap_or(rt.options.sync.default_interval_mins)
        };
        let (owner_type, owner_id) = owner_columns(owner);
        let row = models::Connection {
            id: Uuid::now_v7(),
            owner_type: owner_type.to_owned(),
            owner_id,
            name,
            source: input.source.clone(),
            settings: settings.to_string(),
            secret: sealed,
            scim_token_hash: token.as_deref().map(|t| sha256_hex(t.as_bytes())),
            scim_token_hint: token.as_deref().map(hint),
            cursor: None,
            status: ConnectionStatus::Active.as_str().to_owned(),
            sync_interval_mins: i32::try_from(interval).unwrap_or(i32::MAX),
            last_run_id: None,
            last_synced_at: None,
            next_sync_at: (interval > 0).then_some(now),
            created_by: input.created_by,
            created_at: now,
            updated_at: now,
        };
        let runtime = rt.clone();
        let row = rt
            .write(input.created_by, |uow| async move {
                {
                    let tx = uow.tx().await?;
                    let mut record = Record::new(row);
                    record.save(&*tx).await?;
                    uow.mark_modified();
                    let row = record.into_inner();
                    drop(tx);
                    runtime
                        .audit_after_commit(&uow, "directory.connection_created", &row, json!({}))
                        .await;
                    Ok(row)
                }
            })
            .await?;
        Ok(CreatedConnection {
            connection: rt.info(&row),
            scim_token: token.map(Secret::new),
            scim_url: is_scim.then(|| rt.scim_base.clone()),
        })
    }

    /// Changes connection `id`. New settings are checked by its source.
    pub async fn update_connection(
        &self,
        id: Uuid,
        update: ConnectionUpdate,
    ) -> Result<DirectoryConnection, DirectoryError> {
        let rt = self.rt()?.clone();
        let mut row = self.row(id).await?;
        if let Some(name) = &update.name {
            let name = name.trim();
            if name.is_empty() || name.len() > 255 {
                return Err(DirectoryError::InvalidSettings(
                    "name must be 1 to 255 characters".to_owned(),
                ));
            }
            row.name = name.to_owned();
        }
        let mut changed = Vec::new();
        match update.secret.as_deref() {
            Some("") => {
                row.secret = None;
                changed.push("secret");
            }
            Some(secret) => {
                row.secret = Some(rt.seal(secret)?);
                changed.push("secret");
            }
            None => {}
        }
        if let Some(settings) = &update.settings {
            rt.check_settings(&row.source, settings, row.secret.is_some())?;
            // A source pointed elsewhere starts over.
            row.cursor = None;
            row.settings = settings.to_string();
            changed.push("settings");
        } else if changed.contains(&"secret") {
            let settings: Value = serde_json::from_str(&row.settings).unwrap_or_else(|_| json!({}));
            rt.check_settings(&row.source, &settings, row.secret.is_some())?;
        }
        if let Some(minutes) = update.sync_interval_mins
            && row.source != SCIM
        {
            row.sync_interval_mins = i32::try_from(minutes).unwrap_or(i32::MAX);
            if minutes == 0 {
                row.next_sync_at = None;
            } else if row.next_sync_at.is_none() {
                row.next_sync_at = Some(rt.now());
            }
        }
        if let Some(status) = update.status {
            row.status = status.as_str().to_owned();
        }
        row.updated_at = rt.now();
        let runtime = rt.clone();
        let row = rt
            .write(None, |uow| async move {
                let tx = uow.tx().await?;
                let mut record = models::Connection::query()
                    .where_(models::Connection::ID.eq(row.id))
                    .first(&*tx)
                    .await?
                    .ok_or(DirectoryError::ConnectionNotFound)?;
                *record = row;
                record.save(&*tx).await?;
                uow.mark_modified();
                drop(tx);
                let row = record.into_inner();
                runtime
                    .audit_after_commit(
                        &uow,
                        "directory.connection_updated",
                        &row,
                        json!({ "changed": changed }),
                    )
                    .await;
                Ok(row)
            })
            .await?;
        Ok(rt.info(&row))
    }

    /// Deletes connection `id` with its synced users, groups and runs. Your
    /// records stay as your sink left them.
    pub async fn delete_connection(&self, id: Uuid) -> Result<(), DirectoryError> {
        let rt = self.rt()?.clone();
        let row = self.row(id).await?;
        let runtime = rt.clone();
        rt.write(None, |uow| async move {
            delete_rows(&uow, &row).await?;
            runtime
                .audit_after_commit(&uow, "directory.connection_deleted", &row, json!({}))
                .await;
            Ok(())
        })
        .await
    }

    /// Deletes every connection of `owner` (when the workspace or
    /// organization is deleted); returns how many.
    pub async fn forget(&self, owner: Owner) -> Result<u64, DirectoryError> {
        let rt = self.rt()?.clone();
        let (owner_type, owner_id) = owner_columns(owner);
        let rows = models::Connection::query()
            .where_(models::Connection::OWNER_TYPE.eq(owner_type))
            .where_(models::Connection::OWNER_ID.eq(owner_id))
            .get(&*rt.db)
            .await?
            .into_models();
        let count = rows.len() as u64;
        rt.write(None, |uow| async move {
            for row in &rows {
                delete_rows(&uow, row).await?;
            }
            Ok(())
        })
        .await?;
        Ok(count)
    }

    /// Connection `id`.
    pub async fn connection(&self, id: Uuid) -> Result<DirectoryConnection, DirectoryError> {
        let row = self.row(id).await?;
        Ok(self.rt()?.info(&row))
    }

    /// Connection `id` when it belongs to `owner` (otherwise not found).
    pub async fn connection_for(
        &self,
        owner: Owner,
        id: Uuid,
    ) -> Result<DirectoryConnection, DirectoryError> {
        let connection = self.connection(id).await?;
        if connection.owner() == owner {
            Ok(connection)
        } else {
            Err(DirectoryError::ConnectionNotFound)
        }
    }

    /// The connections of `owner`, by name.
    pub async fn connections(
        &self,
        owner: Owner,
    ) -> Result<Vec<DirectoryConnection>, DirectoryError> {
        let rt = self.rt()?;
        let (owner_type, owner_id) = owner_columns(owner);
        let rows = models::Connection::query()
            .where_(models::Connection::OWNER_TYPE.eq(owner_type))
            .where_(models::Connection::OWNER_ID.eq(owner_id))
            .order_by(models::Connection::NAME)
            .get(&*rt.db)
            .await?
            .into_models();
        Ok(rows.iter().map(|row| rt.info(row)).collect())
    }

    /// Every connection, by name.
    pub async fn all_connections(&self) -> Result<Vec<DirectoryConnection>, DirectoryError> {
        let rt = self.rt()?;
        let rows = models::Connection::query()
            .order_by(models::Connection::NAME)
            .get(&*rt.db)
            .await?
            .into_models();
        Ok(rows.iter().map(|row| rt.info(row)).collect())
    }

    /// Gives SCIM connection `id` a new bearer token (the old one stops
    /// working) and returns it; only its hash is kept.
    pub async fn rotate_scim_token(&self, id: Uuid) -> Result<Secret<String>, DirectoryError> {
        let rt = self.rt()?.clone();
        let row = self.row(id).await?;
        if row.source != SCIM {
            return Err(DirectoryError::NotScim);
        }
        let token = new_token();
        let hash = sha256_hex(token.as_bytes());
        let token_hint = hint(&token);
        let now = rt.now();
        let runtime = rt.clone();
        rt.write(None, |uow| async move {
            let tx = uow.tx().await?;
            models::Connection::query()
                .where_(models::Connection::ID.eq(row.id))
                .update(
                    &*tx,
                    [
                        models::Connection::SCIM_TOKEN_HASH.set(Some(hash)),
                        models::Connection::SCIM_TOKEN_HINT.set(Some(token_hint)),
                        models::Connection::UPDATED_AT.set(now),
                    ],
                )
                .await?;
            uow.mark_modified();
            drop(tx);
            runtime
                .audit_after_commit(&uow, "directory.scim_token_rotated", &row, json!({}))
                .await;
            Ok(())
        })
        .await?;
        Ok(Secret::new(token))
    }

    /// The active SCIM connection a bearer token belongs to.
    pub(crate) async fn connection_by_token(
        &self,
        token: &str,
    ) -> Result<Option<models::Connection>, DirectoryError> {
        let rt = self.rt()?;
        if !token.starts_with(SCIM_TOKEN_PREFIX) {
            return Ok(None);
        }
        Ok(models::Connection::query()
            .where_(models::Connection::SCIM_TOKEN_HASH.eq(Some(sha256_hex(token.as_bytes()))))
            .where_(models::Connection::SOURCE.eq(SCIM))
            .first(&*rt.db)
            .await?
            .map(Record::into_inner))
    }

    pub(crate) async fn row(&self, id: Uuid) -> Result<models::Connection, DirectoryError> {
        let rt = self.rt()?;
        models::Connection::query()
            .where_(models::Connection::ID.eq(id))
            .first(&*rt.db)
            .await?
            .map(Record::into_inner)
            .ok_or(DirectoryError::ConnectionNotFound)
    }

    // -- syncing --------------------------------------------------------------

    /// Queues a sync of connection `id` and returns the queued run. With
    /// navree-queue's operations in the app, the run is the work of an
    /// operation (`directory.sync`) whose progress and cancel follow it;
    /// otherwise it goes through the job dispatcher, or runs in a task of
    /// its own after the commit when the app has no queue.
    pub async fn sync(&self, id: Uuid, options: SyncOptions) -> Result<SyncRun, DirectoryError> {
        let rt = self.rt()?.clone();
        let row = self.row(id).await?;
        self.check_can_sync(&row).await?;
        let app = rt.app()?;
        let actor = navree_db::Uow::current().and_then(|uow| uow.actor());
        let owner = owner_of(&row.owner_type, row.owner_id);
        let runtime = rt.clone();
        let run = rt
            .write(actor, |uow| async move {
                let mut run = runtime.new_run(&uow, &row, options, actor).await?;
                let job = SyncDirectory { run_id: run.id };
                if let Some(operations) = app.try_get::<Operations>().await {
                    let mut operation =
                        Operation::new(SyncDirectory::NAME).note("Waiting for a worker");
                    if let Some(actor) = actor {
                        operation = operation.owner(actor);
                    }
                    if let Owner::Workspace(workspace) = owner {
                        operation = operation.workspace(workspace);
                    }
                    let record = operations
                        .start(&uow, operation)
                        .await
                        .map_err(|e| DirectoryError::Queue(e.to_string()))?;
                    operations
                        .dispatch(&uow, &record, job)
                        .await
                        .map_err(|e| DirectoryError::Queue(e.to_string()))?;
                    let tx = uow.tx().await?;
                    models::Run::query()
                        .where_(models::Run::ID.eq(run.id))
                        .update(&*tx, [models::Run::OPERATION_ID.set(Some(record.id))])
                        .await?;
                    run.operation_id = Some(record.id);
                } else if let Some(dispatcher) = app.try_get::<dyn Dispatcher>().await {
                    let envelope = JobEnvelope::of(&job).map_err(|e| DirectoryError::Queue(e.to_string()))?;
                    dispatcher
                        .dispatch_envelope(envelope)
                        .await
                        .map_err(|e| DirectoryError::Queue(e.to_string()))?;
                } else {
                    // No queue: run in a task of its own once the run row
                    // is committed.
                    let task = runtime.clone();
                    let run_id = run.id;
                    uow.after_commit(move || {
                        Box::pin(async move {
                            tokio::spawn(async move {
                                if let Err(error) = task.execute(run_id, None).await {
                                    tracing::warn!(target: "navree::directory", %error, "a directory sync failed");
                                }
                            });
                        })
                    });
                }
                Ok(run)
            })
            .await?;
        Ok(run)
    }

    /// Runs a sync of connection `id` in this task and returns the
    /// finished run. For commands, jobs and tests: inside a request, use
    /// [`sync`](Directory::sync).
    pub async fn sync_now(
        &self,
        id: Uuid,
        options: SyncOptions,
    ) -> Result<SyncRun, DirectoryError> {
        let rt = self.rt()?.clone();
        let row = self.row(id).await?;
        self.check_can_sync(&row).await?;
        let runtime = rt.clone();
        let run = rt
            .own_unit(None, |uow| async move {
                runtime.new_run(&uow, &row, options, None).await
            })
            .await?;
        rt.execute(run.id, None).await
    }

    /// Queues a sync of every active pull connection whose next sync is
    /// due (what the scheduled `directory:sync --due` runs). Connections
    /// with a run already going are skipped.
    pub async fn sync_due(&self) -> Result<Vec<SyncRun>, DirectoryError> {
        let rt = self.rt()?.clone();
        let now = rt.now();
        let rows = models::Connection::query()
            .where_(models::Connection::STATUS.eq(ConnectionStatus::Active.as_str()))
            .where_(models::Connection::SOURCE.ne(SCIM))
            .where_(models::Connection::SYNC_INTERVAL_MINS.gt(0))
            .where_(models::Connection::NEXT_SYNC_AT.lte(Some(now)))
            .get(&*rt.db)
            .await?
            .into_models();
        let mut runs = Vec::new();
        for row in rows {
            let next = now + TimeDelta::minutes(i64::from(row.sync_interval_mins.max(1)));
            models::Connection::query()
                .where_(models::Connection::ID.eq(row.id))
                .update(&*rt.db, [models::Connection::NEXT_SYNC_AT.set(Some(next))])
                .await?;
            let options = SyncOptions {
                trigger: RunTrigger::Schedule,
                ..SyncOptions::default()
            };
            match self.sync(row.id, options).await {
                Ok(run) => runs.push(run),
                Err(DirectoryError::SyncRunning) => {}
                Err(error) => {
                    tracing::warn!(target: "navree::directory", connection = %row.id, %error, "could not queue a scheduled sync");
                }
            }
        }
        Ok(runs)
    }

    /// Approves held run `run_id`: queues a new run of its connection that
    /// applies past the deactivation limit.
    pub async fn approve(&self, run_id: Uuid) -> Result<SyncRun, DirectoryError> {
        let run = self.run(run_id).await?;
        if run.status != RunStatus::Held {
            return Err(DirectoryError::RunNotHeld);
        }
        self.sync(
            run.connection_id,
            SyncOptions {
                force: true,
                trigger: run.trigger,
                ..SyncOptions::default()
            },
        )
        .await
    }

    async fn check_can_sync(&self, row: &models::Connection) -> Result<(), DirectoryError> {
        let rt = self.rt()?;
        if row.source == SCIM {
            return Err(DirectoryError::PushOnly);
        }
        if ConnectionStatus::parse(&row.status) == ConnectionStatus::Paused {
            return Err(DirectoryError::Paused);
        }
        let going = models::Run::query()
            .where_(models::Run::CONNECTION_ID.eq(row.id))
            .where_(
                models::Run::STATUS
                    .is_in([RunStatus::Queued.as_str(), RunStatus::Running.as_str()]),
            )
            .get(&*rt.db)
            .await?
            .into_models();
        let stale_before =
            rt.now() - TimeDelta::minutes(i64::from(rt.options.sync.stale_after_mins));
        for run in going {
            if run.created_at > stale_before {
                return Err(DirectoryError::SyncRunning);
            }
            // A crashed worker's run: close it.
            models::Run::query()
                .where_(models::Run::ID.eq(run.id))
                .update(
                    &*rt.db,
                    [
                        models::Run::STATUS.set(RunStatus::Failed.as_str()),
                        models::Run::ERROR.set(Some("the run stopped responding".to_owned())),
                        models::Run::FINISHED_AT.set(Some(rt.now())),
                    ],
                )
                .await?;
        }
        Ok(())
    }

    /// Run `id`.
    pub async fn run(&self, id: Uuid) -> Result<SyncRun, DirectoryError> {
        let rt = self.rt()?;
        models::Run::query()
            .where_(models::Run::ID.eq(id))
            .first(&*rt.db)
            .await?
            .map(|row| run_view(&row))
            .ok_or(DirectoryError::RunNotFound)
    }

    /// The runs of connection `id`, newest first, at most `limit`.
    pub async fn runs(
        &self,
        connection_id: Uuid,
        limit: u64,
    ) -> Result<Vec<SyncRun>, DirectoryError> {
        let rt = self.rt()?;
        let rows = models::Run::query()
            .where_(models::Run::CONNECTION_ID.eq(connection_id))
            .order_by_desc(models::Run::CREATED_AT)
            .order_by_desc(models::Run::ID)
            .limit(limit.max(1))
            .get(&*rt.db)
            .await?
            .into_models();
        Ok(rows.iter().map(run_view).collect())
    }

    /// The recorded changes of run `id`, in the order they applied.
    pub async fn changes(
        &self,
        run_id: Uuid,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<SyncChange>, DirectoryError> {
        let rt = self.rt()?;
        let rows = models::RunChange::query()
            .where_(models::RunChange::RUN_ID.eq(run_id))
            .order_by(models::RunChange::ID)
            .offset(offset)
            .limit(limit.max(1))
            .get(&*rt.db)
            .await?
            .into_models();
        Ok(rows.iter().map(change_view).collect())
    }

    /// Deletes finished runs (and their changes) older than
    /// `directory.sync.keep_runs_days`; returns how many runs.
    pub async fn prune_runs(&self) -> Result<u64, DirectoryError> {
        let rt = self.rt()?;
        let cutoff = rt.now() - TimeDelta::days(i64::from(rt.options.sync.keep_runs_days));
        let ids: Vec<Uuid> = models::Run::query()
            .where_(models::Run::CREATED_AT.lt(cutoff))
            .where_(
                models::Run::STATUS
                    .not_in([RunStatus::Queued.as_str(), RunStatus::Running.as_str()]),
            )
            .get(&*rt.db)
            .await?
            .into_models()
            .into_iter()
            .map(|run| run.id)
            .collect();
        for chunk in ids.chunks(500) {
            models::RunChange::query()
                .where_(models::RunChange::RUN_ID.is_in(chunk.iter().copied()))
                .delete(&*rt.db)
                .await?;
            models::Run::query()
                .where_(models::Run::ID.is_in(chunk.iter().copied()))
                .delete(&*rt.db)
                .await?;
        }
        Ok(ids.len() as u64)
    }

    // -- synced data ----------------------------------------------------------

    /// The users of connection `id`, by sign-in name.
    pub async fn users(&self, connection_id: Uuid) -> Result<Vec<SyncedUser>, DirectoryError> {
        let rt = self.rt()?;
        let rows = models::User::query()
            .where_(models::User::CONNECTION_ID.eq(connection_id))
            .order_by(models::User::USER_NAME)
            .get(&*rt.db)
            .await?
            .into_models();
        Ok(rows.iter().map(synced_user).collect())
    }

    /// The user of connection `id` with this external id.
    pub async fn user(
        &self,
        connection_id: Uuid,
        external_id: &str,
    ) -> Result<Option<SyncedUser>, DirectoryError> {
        let rt = self.rt()?;
        Ok(models::User::query()
            .where_(models::User::CONNECTION_ID.eq(connection_id))
            .where_(models::User::EXTERNAL_ID.eq(external_id))
            .first(&*rt.db)
            .await?
            .map(|row| synced_user(&row)))
    }

    /// The groups of connection `id`, by name, with their members.
    pub async fn groups(&self, connection_id: Uuid) -> Result<Vec<SyncedGroup>, DirectoryError> {
        let rt = self.rt()?;
        let rows = models::Group::query()
            .where_(models::Group::CONNECTION_ID.eq(connection_id))
            .order_by(models::Group::DISPLAY_NAME)
            .get(&*rt.db)
            .await?
            .into_models();
        let mut groups = Vec::with_capacity(rows.len());
        for row in rows {
            groups.push(synced_group(&row, group_with_members(&*rt.db, &row).await?));
        }
        Ok(groups)
    }

    /// The groups user `directory_user_id` belongs to.
    pub async fn groups_of(
        &self,
        directory_user_id: Uuid,
    ) -> Result<Vec<SyncedGroup>, DirectoryError> {
        let rt = self.rt()?;
        let rows = groups_of(&*rt.db, directory_user_id).await?;
        let mut groups = Vec::with_capacity(rows.len());
        for row in rows {
            groups.push(synced_group(&row, group_with_members(&*rt.db, &row).await?));
        }
        Ok(groups)
    }
}

impl Runtime {
    /// Stores a queued run of `row`.
    async fn new_run(
        &self,
        uow: &Uow,
        row: &models::Connection,
        options: SyncOptions,
        actor: Option<Uuid>,
    ) -> Result<SyncRun, DirectoryError> {
        let run = models::Run {
            id: Uuid::now_v7(),
            connection_id: row.id,
            status: RunStatus::Queued.as_str().to_owned(),
            triggered_by: options.trigger.as_str().to_owned(),
            dry_run: options.dry_run,
            force: options.force,
            full_sync: options.full,
            complete: None,
            summary: "{}".to_owned(),
            done: 0,
            total: 0,
            note: Some("Waiting for a worker".to_owned()),
            error: None,
            operation_id: None,
            created_by: actor,
            created_at: self.now(),
            started_at: None,
            finished_at: None,
        };
        let tx = uow.tx().await?;
        let mut record = Record::new(run);
        record.save(&*tx).await?;
        uow.mark_modified();
        Ok(run_view(&record.into_inner()))
    }
}

/// Deletes a connection's rows in `uow`.
async fn delete_rows(uow: &Uow, row: &models::Connection) -> Result<(), DirectoryError> {
    let tx = uow.tx().await?;
    models::Membership::query()
        .where_(models::Membership::CONNECTION_ID.eq(row.id))
        .delete(&*tx)
        .await?;
    models::User::query()
        .where_(models::User::CONNECTION_ID.eq(row.id))
        .delete(&*tx)
        .await?;
    models::Group::query()
        .where_(models::Group::CONNECTION_ID.eq(row.id))
        .delete(&*tx)
        .await?;
    let run_ids: Vec<Uuid> = models::Run::query()
        .where_(models::Run::CONNECTION_ID.eq(row.id))
        .get(&*tx)
        .await?
        .into_models()
        .into_iter()
        .map(|run| run.id)
        .collect();
    for chunk in run_ids.chunks(500) {
        models::RunChange::query()
            .where_(models::RunChange::RUN_ID.is_in(chunk.iter().copied()))
            .delete(&*tx)
            .await?;
    }
    models::Run::query()
        .where_(models::Run::CONNECTION_ID.eq(row.id))
        .delete(&*tx)
        .await?;
    models::Connection::query()
        .where_(models::Connection::ID.eq(row.id))
        .delete(&*tx)
        .await?;
    uow.mark_modified();
    Ok(())
}

/// A new SCIM token: `nvscim_` and 40 random base62 characters.
fn new_token() -> String {
    format!("{SCIM_TOKEN_PREFIX}{}", random_token(40))
}

/// What a token is recognized by: its prefix and four more characters.
fn hint(token: &str) -> String {
    token.chars().take(SCIM_TOKEN_PREFIX.len() + 4).collect()
}

pub(crate) fn synced_user(row: &models::User) -> SyncedUser {
    SyncedUser {
        id: row.id,
        connection_id: row.connection_id,
        user: serde_json::from_str::<DirectoryUser>(&row.data).unwrap_or_default(),
        local_id: row.local_id.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        deactivated_at: row.deactivated_at,
    }
}

pub(crate) fn synced_group(row: &models::Group, group: crate::DirectoryGroup) -> SyncedGroup {
    SyncedGroup {
        id: row.id,
        connection_id: row.connection_id,
        group,
        local_id: row.local_id.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

impl fmt::Debug for Directory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Directory")
            .field("fake", &self.is_fake())
            .field("bound", &self.runtime.get().is_some())
            .finish_non_exhaustive()
    }
}

/// Binds a [`Directory::fake`] (or any `Directory` given to
/// `override_value`) to the app at boot.
impl Lifecycle for Directory {
    fn on_init(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            if self.runtime.get().is_none() {
                let runtime = Runtime::build(&ctx, self.fake.clone()).await?;
                let _ = self.runtime.set(Arc::new(runtime));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_unbound_service_says_so() {
        let (directory, _fake) = Directory::fake();
        assert!(directory.is_fake());
        assert!(matches!(directory.sources(), Err(DirectoryError::NotBound)));
        assert!(matches!(
            directory.connection(Uuid::nil()).await,
            Err(DirectoryError::NotBound)
        ));
        assert!(format!("{directory:?}").contains("bound: false"));
    }

    #[test]
    fn tokens_are_prefixed_and_hinted() {
        let token = new_token();
        assert!(token.starts_with("nvscim_"));
        assert_eq!(token.len(), 47);
        assert_eq!(hint(&token).len(), 11);
        assert!(token.starts_with(&hint(&token)));
        let created = NewConnection::new("Acme", "scim").secret("p");
        assert!(!format!("{created:?}").contains("\"p\""));
    }
}
