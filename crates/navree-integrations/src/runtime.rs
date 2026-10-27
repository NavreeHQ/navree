//! What the [`Integrations`](crate::Integrations) service works with once
//! it is bound to an app: the options, the providers, the scope sets and
//! the app's services.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_cache::{Cache, Locks};
use navree_contracts::{AuditEntry, AuditSink, Clock, SystemClock};
use navree_crypto::Encrypter;
use navree_db::sea_orm::DbBackend;
use navree_db::{Db, Uow};
use navree_events::{Event, EventBus};
use navree_http_client::{HttpClient, HttpClientOptions};
use navree_kernel::{AppContext, BoxError, WeakAppContext};
use navree_workspace::{WorkspaceConfig, Workspaces};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::entry::ProviderEntry;
use crate::fake::FakeState;
use crate::model::IntegrationConnection;
use crate::oauth::{HttpBackend, TokenBackend};
use crate::provider::{ScopeSetRegistration, is_scope_set_name};
use crate::types::StoredCredentials;
use crate::{
    ConnectionInfo, ConnectionStatus, CredentialKind, IntegrationsError, IntegrationsOptions,
    Level, Owner,
};

/// A token minted for a service account acting as another user, kept in
/// memory: (token, expiry).
pub(crate) type SubjectToken = (String, Option<DateTime<Utc>>);

/// The bound service's state.
pub(crate) struct Runtime {
    pub(crate) options: Arc<IntegrationsOptions>,
    pub(crate) providers: BTreeMap<String, Arc<ProviderEntry>>,
    pub(crate) scope_sets: BTreeMap<&'static str, ScopeSetRegistration>,
    pub(crate) encrypter: Arc<Encrypter>,
    pub(crate) db: Arc<Db>,
    pub(crate) cache: Cache,
    pub(crate) locks: Locks,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) http: HttpClient,
    pub(crate) backend: Arc<dyn TokenBackend>,
    pub(crate) app: WeakAppContext,
    /// The level the routes and extractors use (`workspace` turns into
    /// `app` without `WorkspaceModule`).
    pub(crate) level: Level,
    /// Tokens of service accounts acting as other users, by (connection,
    /// user).
    pub(crate) subject_tokens: Mutex<HashMap<(Uuid, String), SubjectToken>>,
}

impl Runtime {
    /// Resolves everything from `app`. A fake brings its own HTTP client
    /// (the API stubs) and token backend.
    pub(crate) async fn build(
        app: &AppContext,
        fake: Option<Arc<FakeState>>,
    ) -> Result<Runtime, BoxError> {
        let options = app.get::<IntegrationsOptions>().await?;
        let mut scope_sets = BTreeMap::new();
        for contributed in app.contributions::<ScopeSetRegistration>() {
            let registration = *contributed.value;
            if !is_scope_set_name(registration.name) {
                return Err(format!(
                    "integrations: scope set `{}` (module {}) must be lower-case letters, digits and `_`",
                    registration.name, contributed.module.name
                )
                .into());
            }
            scope_sets.entry(registration.name).or_insert(registration);
        }
        let mut providers = BTreeMap::new();
        for (key, config) in &options.providers {
            providers.insert(key.clone(), Arc::new(ProviderEntry::new(key, config)?));
        }
        let cache = match app.try_get::<Cache>().await {
            Some(cache) => (*cache).clone(),
            None => Cache::memory(),
        };
        let locks = match app.try_get::<Locks>().await {
            Some(locks) => (*locks).clone(),
            None => Locks::memory(),
        };
        let clock = match app.try_get::<dyn Clock>().await {
            Some(clock) => clock,
            None => Arc::new(SystemClock),
        };
        let (http, backend): (HttpClient, Arc<dyn TokenBackend>) = match fake {
            Some(fake) => (fake.http.clone(), fake),
            None => {
                let http = match app.try_get::<HttpClient>().await {
                    Some(http) => (*http).clone(),
                    None => HttpClient::new(HttpClientOptions::default())?,
                };
                (http.clone(), Arc::new(HttpBackend { http }))
            }
        };
        let workspaces = app.try_get::<WorkspaceConfig>().await;
        let level = match (options.level, &workspaces) {
            (Level::Workspace, None) => Level::App,
            (Level::Organization, None) => {
                return Err(
                    "integrations.level = \"organization\" needs WorkspaceModule in the app".into(),
                );
            }
            (Level::Organization, Some(config)) if !config.organizations.enabled => {
                return Err(
                    "integrations.level = \"organization\" needs workspace.organizations.enabled"
                        .into(),
                );
            }
            (level, _) => level,
        };
        Ok(Runtime {
            providers,
            scope_sets,
            encrypter: app.get::<Encrypter>().await?,
            db: app.get::<Db>().await?,
            cache,
            locks,
            clock,
            http,
            backend,
            app: app.downgrade(),
            level,
            subject_tokens: Mutex::new(HashMap::new()),
            options,
        })
    }

    /// The provider under `key`.
    pub(crate) fn provider(&self, key: &str) -> Result<&Arc<ProviderEntry>, IntegrationsError> {
        self.providers
            .get(key)
            .ok_or_else(|| IntegrationsError::UnknownProvider(key.to_owned()))
    }

    /// The display name of `key` (the key itself for a provider that is no
    /// longer configured).
    pub(crate) fn provider_name(&self, key: &str) -> String {
        self.providers
            .get(key)
            .map_or_else(|| key.to_owned(), |entry| entry.name.clone())
    }

    /// The callback URL of `key`: `{redirect_base}/{key}`.
    pub(crate) fn redirect_uri(&self, key: &str) -> Result<String, IntegrationsError> {
        let base = self.options.redirect_base.as_deref().ok_or_else(|| {
            IntegrationsError::Config("integrations.redirect_base is not set".to_owned())
        })?;
        Ok(format!("{}/{key}", base.trim_end_matches('/')))
    }

    pub(crate) fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    /// The organization of workspace `id`, when it has one (and the app
    /// has workspaces).
    pub(crate) async fn organization_of(&self, workspace: Uuid) -> Option<Uuid> {
        let app = self.app.upgrade()?;
        let workspaces = app.try_get::<Workspaces>().await?;
        workspaces
            .find(workspace)
            .await
            .ok()
            .flatten()?
            .organization_id
    }

    /// The owners whose connections `owner` may use, its own first: a
    /// workspace also uses its organization's.
    pub(crate) async fn owner_chain(&self, owner: Owner) -> Vec<Owner> {
        let mut chain = vec![owner];
        if let Owner::Workspace(id) = owner
            && let Some(organization) = self.organization_of(id).await
        {
            chain.push(Owner::Organization(organization));
        }
        chain
    }

    /// The public view of a row.
    pub(crate) fn info(&self, model: &IntegrationConnection) -> ConnectionInfo {
        let owner = Owner::from_columns(&model.owner_type, model.owner_id);
        ConnectionInfo {
            id: model.id,
            owner_type: owner.kind(),
            owner_id: owner.id(),
            provider: model.provider.clone(),
            provider_name: self.provider_name(&model.provider),
            scope_set: model.scope_set.clone(),
            kind: CredentialKind::parse(&model.kind),
            external_id: model.external_id.clone(),
            display_name: model.display_name.clone(),
            scopes: model
                .scopes
                .as_deref()
                .unwrap_or_default()
                .split(' ')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
            status: ConnectionStatus::parse(&model.status),
            failures: u32::try_from(model.failures.max(0)).unwrap_or(u32::MAX),
            last_error: model.last_error.clone(),
            last_checked_at: model.last_checked_at,
            connected_by: model.connected_by,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }

    /// The decrypted credentials of a row.
    pub(crate) fn credentials(
        &self,
        model: &IntegrationConnection,
    ) -> Result<StoredCredentials, IntegrationsError> {
        Ok(self.encrypter.decrypt_json(&model.credentials)?)
    }

    /// The app's audit sink, when one is registered.
    async fn audit_sink(&self) -> Option<Arc<dyn AuditSink>> {
        self.app.upgrade()?.try_get::<dyn AuditSink>().await
    }

    /// Records `entry` once `uow` commits.
    pub(crate) async fn audit_after_commit(&self, uow: &Uow, entry: AuditEntry) {
        if let Some(sink) = self.audit_sink().await {
            uow.after_commit(move || {
                Box::pin(async move {
                    let action = entry.action.clone();
                    if let Err(error) = sink.record(entry).await {
                        tracing::warn!(target: "navree::integrations", %error, action, "could not write an audit entry");
                    }
                })
            });
        }
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
                    tracing::warn!(target: "navree::integrations", event = E::NAME, %error, "an integrations event listener failed");
                }
            })
        });
    }

    /// Runs `f` in the task's unfinished unit of work (a request's), or else
    /// in a new one committed when `f` succeeds. For changes people make:
    /// they commit with the request.
    pub(crate) async fn write<T, F, Fut>(
        &self,
        actor: Option<Uuid>,
        f: F,
    ) -> Result<T, IntegrationsError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, IntegrationsError>>,
    {
        if let Some(uow) = Uow::current()
            && !uow.is_finished().await
        {
            return f(uow).await;
        }
        self.own_unit(actor, f).await
    }

    /// Runs `f` in a unit of work of its own, committed when `f` succeeds:
    /// tokens and health results survive a request that fails afterwards.
    /// On SQLite, inside a request that already holds the write lock, it
    /// joins that request instead (a second writer would wait for the
    /// first forever).
    pub(crate) async fn housekeeping<T, F, Fut>(&self, f: F) -> Result<T, IntegrationsError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, IntegrationsError>>,
    {
        if self.db.backend() == DbBackend::Sqlite
            && let Some(uow) = Uow::current()
            && uow.open_tx().await.is_some()
        {
            return f(uow).await;
        }
        self.own_unit(None, f).await
    }

    async fn own_unit<T, F, Fut>(&self, actor: Option<Uuid>, f: F) -> Result<T, IntegrationsError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, IntegrationsError>>,
    {
        let uow = self.db.uow(actor, None).await;
        match uow.scope(f(uow.clone())).await {
            Ok(value) => {
                uow.commit().await?;
                Ok(value)
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "navree::integrations", error = %rollback, "rollback failed");
                }
                Err(error)
            }
        }
    }
}
