//! [`Integrations`]: the service behind the routes, the extractor and the
//! commands.

use std::fmt;
use std::sync::{Arc, OnceLock};

use chrono::TimeDelta;
use navree_contracts::{AuditEntry, AuditKind, Secret};
use navree_kernel::{AppContext, BoxError, BoxFuture, Lifecycle};
use navree_orm::{Model as _, Record};
use serde_json::json;
use url::Url;
use uuid::Uuid;

use crate::client::IntegrationClient;
use crate::entry::ProviderEntry;
use crate::events::{IntegrationConnected, IntegrationDisconnected};
use crate::fake::{FakeState, IntegrationsFake};
use crate::model::IntegrationConnection as Row;
use crate::oauth::{Exchange, Grant, authorize_url};
use crate::provider::{DEFAULT_SCOPE_SET, ProviderInfo};
use crate::runtime::Runtime;
use crate::state::{StatePayload, check_return_to};
use crate::tokens::{TokenFailure, Want, failure_message};
use crate::types::StoredCredentials;
use crate::{
    ApiKeyCredentials, CheckSummary, CompletedConnection, ConnectCallback, ConnectOptions,
    ConnectRedirect, ConnectionInfo, ConnectionStatus, CredentialKind, Health, IntegrationsError,
    Level, NewApiKey, NewServiceAccount, Owner, Provider, ScopeSet,
};

/// Connections to third-party APIs: the connect flows, the encrypted
/// credentials, tokens, typed clients and health checks.
///
/// Resolve it from the app (`ctx.get::<Integrations>()`, or inject
/// `Arc<Integrations>`). [`Integrations::fake`] gives one whose providers
/// are an [`IntegrationsFake`].
pub struct Integrations {
    fake: Option<Arc<FakeState>>,
    runtime: OnceLock<Arc<Runtime>>,
}

impl Integrations {
    /// A service whose providers are faked: pass it to
    /// `override_value(Arc::new(integrations))` and drive the providers
    /// with the [`IntegrationsFake`].
    pub fn fake() -> (Integrations, IntegrationsFake) {
        let (fake, state) = IntegrationsFake::new();
        (
            Integrations {
                fake: Some(state),
                runtime: OnceLock::new(),
            },
            fake,
        )
    }

    /// The real service, bound to `runtime`.
    pub(crate) fn real(runtime: Runtime) -> Integrations {
        let bound = OnceLock::new();
        let _ = bound.set(Arc::new(runtime));
        Integrations {
            fake: None,
            runtime: bound,
        }
    }

    /// Whether this is a [`fake`](Integrations::fake).
    pub fn is_fake(&self) -> bool {
        self.fake.is_some()
    }

    pub(crate) fn rt(&self) -> Result<&Arc<Runtime>, IntegrationsError> {
        self.runtime.get().ok_or(IntegrationsError::NotBound)
    }

    /// The level the routes and the [`Connection`](crate::Connection)
    /// extractor use.
    pub fn level(&self) -> Result<Level, IntegrationsError> {
        Ok(self.rt()?.level)
    }

    /// The configured providers, with how they connect and their scope
    /// sets.
    pub fn providers(&self) -> Result<Vec<ProviderInfo>, IntegrationsError> {
        let rt = self.rt()?;
        Ok(rt
            .providers
            .values()
            .map(|entry| ProviderInfo {
                key: entry.key.clone(),
                name: entry.name.clone(),
                kind: entry.kind,
                kinds: entry.kinds(),
                scope_sets: entry.scope_sets(&rt.scope_sets),
            })
            .collect())
    }

    // -- connecting ---------------------------------------------------------

    /// Starts a connect flow for `owner`: the provider's consent screen
    /// (Microsoft's admin consent, or an OAuth authorization) and the
    /// sealed state that comes back on the callback.
    pub async fn connect_url(
        &self,
        owner: Owner,
        provider: &str,
        options: ConnectOptions,
    ) -> Result<ConnectRedirect, IntegrationsError> {
        let rt = self.rt()?;
        let entry = rt.provider(provider)?;
        if entry.consent_kind().is_none() {
            return Err(IntegrationsError::Unsupported {
                provider: entry.name.clone(),
                kind: CredentialKind::OAuth.phrase(),
            });
        }
        let scope_set = options
            .scope_set
            .unwrap_or_else(|| DEFAULT_SCOPE_SET.to_owned());
        let scopes = entry.scopes_for(&scope_set, &rt.scope_sets)?;
        if let Some(return_to) = &options.return_to {
            check_return_to(return_to)?;
        }
        let ttl = i64::try_from(rt.options.state_ttl_secs).unwrap_or(i64::MAX);
        let state = StatePayload::new(
            &entry.key,
            &scope_set,
            owner,
            options.user_id,
            options.return_to,
            entry.pkce(),
            rt.now().timestamp().saturating_add(ttl),
        );
        let sealed = state.seal(&rt.encrypter)?;
        let mut url = authorize_url(
            entry,
            &scope_set,
            &scopes,
            &rt.redirect_uri(&entry.key)?,
            &sealed,
            state.challenge().as_deref(),
        )?;
        if !options.params.is_empty() {
            let mut parsed = Url::parse(&url).map_err(|_| {
                IntegrationsError::Config("the authorization URL is invalid".to_owned())
            })?;
            for (name, value) in &options.params {
                parsed.query_pairs_mut().append_pair(name, value);
            }
            url = parsed.into();
        }
        Ok(ConnectRedirect { url, state: sealed })
    }

    /// Finishes a connect flow with what the provider sent back: checks
    /// the state (same provider, owner and admin; not expired; used once),
    /// exchanges the code or records the tenant's consent, and stores the
    /// connection (replacing an earlier one of the same scope set).
    pub async fn complete(
        &self,
        owner: Owner,
        provider: &str,
        callback: &ConnectCallback,
        user_id: Option<Uuid>,
    ) -> Result<CompletedConnection, IntegrationsError> {
        let rt = self.rt()?;
        let entry = rt.provider(provider)?.clone();
        let state = StatePayload::open(&rt.encrypter, &callback.state)?;
        let now = rt.now().timestamp();
        if state.provider != entry.key
            || state.owner != owner
            || state.exp <= now
            || state.user.is_some_and(|user| Some(user) != user_id)
        {
            return Err(IntegrationsError::InvalidState);
        }
        let ttl = u64::try_from(state.exp.saturating_sub(now))
            .unwrap_or(1)
            .max(1);
        let fresh = rt
            .cache
            .add(
                &format!("integrations:state:{}", state.id),
                &true,
                std::time::Duration::from_secs(ttl),
            )
            .await?;
        if !fresh {
            return Err(IntegrationsError::InvalidState);
        }
        if let Some(error) = &callback.error {
            return Err(IntegrationsError::ConsentDenied {
                provider: entry.name.clone(),
                error: error.clone(),
            });
        }
        let scopes = entry.scopes_for(&state.scope_set, &rt.scope_sets)?;
        let kind = entry.consent_kind().ok_or(IntegrationsError::Unsupported {
            provider: entry.name.clone(),
            kind: CredentialKind::OAuth.phrase(),
        })?;
        let (credentials, grant) = match kind {
            CredentialKind::AppOnly => {
                let granted = callback
                    .admin_consent
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case("true"));
                let tenant = callback
                    .tenant
                    .as_deref()
                    .filter(|tenant| is_tenant(tenant));
                let (true, Some(tenant)) = (granted, tenant) else {
                    return Err(IntegrationsError::ConsentDenied {
                        provider: entry.name.clone(),
                        error: "admin_consent_missing".to_owned(),
                    });
                };
                let grant = Grant {
                    external_id: Some(tenant.to_owned()),
                    scopes: Some(scopes.clone()),
                    ..Grant::default()
                };
                (
                    StoredCredentials::AppOnly {
                        tenant: tenant.to_owned(),
                    },
                    grant,
                )
            }
            _ => {
                let code = callback
                    .code
                    .as_deref()
                    .filter(|code| !code.is_empty())
                    .ok_or(IntegrationsError::ConsentDenied {
                        provider: entry.name.clone(),
                        error: "missing_code".to_owned(),
                    })?;
                let redirect_uri = rt.redirect_uri(&entry.key)?;
                let grant = rt
                    .backend
                    .grant(
                        &entry,
                        &state.scope_set,
                        Exchange::Code {
                            code,
                            verifier: state.verifier.as_deref(),
                            redirect_uri: &redirect_uri,
                            scopes: &scopes,
                        },
                    )
                    .await?;
                (
                    StoredCredentials::OAuth {
                        refresh_token: grant.refresh_token.clone(),
                    },
                    grant,
                )
            }
        };
        let has_token = !grant.access_token.is_empty();
        let connection = self
            .store(
                owner,
                &entry,
                &state.scope_set,
                credentials,
                has_token.then_some(&grant),
                &grant,
                scopes,
                user_id,
            )
            .await?;
        Ok(CompletedConnection {
            connection,
            return_to: state.return_to,
        })
    }

    /// Connects `owner` with a Google service-account key. The key is
    /// tried first (a token for the scope set, acting as `subject`), so a
    /// key without domain-wide delegation for those scopes is refused
    /// before it is stored.
    pub async fn connect_service_account(
        &self,
        owner: Owner,
        provider: &str,
        input: NewServiceAccount,
    ) -> Result<ConnectionInfo, IntegrationsError> {
        let rt = self.rt()?;
        let entry = rt.provider(provider)?.clone();
        entry.require(CredentialKind::ServiceAccount)?;
        let scopes = entry.scopes_for(&input.scope_set, &rt.scope_sets)?;
        if scopes.is_empty() {
            return Err(IntegrationsError::UnknownScopeSet {
                provider: entry.name.clone(),
                scope_set: input.scope_set,
            });
        }
        if let Some(subject) = &input.subject
            && !is_email(subject)
        {
            return Err(IntegrationsError::InvalidCredentials(
                "subject must be the email address of a user to act as".to_owned(),
            ));
        }
        input.key.signing_key()?;
        let grant = rt
            .backend
            .grant(
                &entry,
                &input.scope_set,
                Exchange::JwtBearer {
                    key: &input.key,
                    subject: input.subject.as_deref(),
                    scopes: &scopes,
                    now: rt.now(),
                },
            )
            .await?;
        self.store(
            owner,
            &entry,
            &input.scope_set,
            StoredCredentials::ServiceAccount {
                key: input.key,
                subject: input.subject,
            },
            Some(&grant),
            &grant,
            scopes,
            input.connected_by,
        )
        .await
    }

    /// Connects `owner` with an API key. With a health check configured
    /// (`health_url`), the key is tried first and a refused key is not
    /// stored.
    pub async fn connect_api_key(
        &self,
        owner: Owner,
        provider: &str,
        input: NewApiKey,
    ) -> Result<ConnectionInfo, IntegrationsError> {
        let rt = self.rt()?;
        let entry = rt.provider(provider)?.clone();
        entry.require(CredentialKind::ApiKey)?;
        if input.scope_set != DEFAULT_SCOPE_SET {
            entry.scopes_for(&input.scope_set, &rt.scope_sets)?;
        }
        let key = input.key.trim().to_owned();
        if key.is_empty() {
            return Err(IntegrationsError::InvalidCredentials(
                "key must not be empty".to_owned(),
            ));
        }
        if let Some(url) = &input.api_url
            && !Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
        {
            return Err(IntegrationsError::InvalidCredentials(
                "api_url must be an absolute http(s) URL".to_owned(),
            ));
        }
        if entry.api_url.is_none() && input.api_url.is_none() {
            return Err(IntegrationsError::InvalidCredentials(format!(
                "api_url is required for {}",
                entry.name
            )));
        }
        let credentials = StoredCredentials::ApiKey {
            key: key.clone(),
            api_url: input.api_url,
            fields: input.fields,
        };
        if let Some(probe) = entry.probe() {
            rt.probe(&entry, &credentials, &key, &probe).await?;
        }
        let none = Grant::default();
        self.store(
            owner,
            &entry,
            &input.scope_set,
            credentials,
            None,
            &none,
            Vec::new(),
            input.connected_by,
        )
        .await
    }

    /// Inserts or replaces the connection of `owner` to `entry` in
    /// `scope_set` (a data change: audited, committed with the request).
    #[allow(clippy::too_many_arguments)]
    async fn store(
        &self,
        owner: Owner,
        entry: &ProviderEntry,
        scope_set: &str,
        credentials: StoredCredentials,
        token: Option<&Grant>,
        grant: &Grant,
        requested: Vec<String>,
        connected_by: Option<Uuid>,
    ) -> Result<ConnectionInfo, IntegrationsError> {
        let rt = self.rt()?.clone();
        let now = rt.now();
        let kind = credentials.kind();
        let encrypted = rt.encrypter.encrypt_json(&credentials)?;
        let access_token = token
            .map(|grant| rt.encrypter.encrypt_string(&grant.access_token))
            .transpose()?;
        let expires = token
            .and_then(|grant| grant.expires_in)
            .map(|secs| now + TimeDelta::seconds(secs));
        let scopes = grant.scopes.clone().unwrap_or(requested).join(" ");
        let scopes = (!scopes.is_empty()).then_some(scopes);
        let (owner_type, owner_id) = owner.columns();
        let provider = entry.key.clone();
        let scope_set = scope_set.to_owned();
        let external_id = grant.external_id.clone();
        let display_name = grant.display_name.clone();
        let runtime = rt.clone();
        rt.write(connected_by, |uow| async move {
            let model = {
                // Written with set-based statements, not `Record::save`: a
                // change event would carry the encrypted credentials and
                // token. The audit entry below records the connection
                // instead.
                let existing = Row::query()
                    .where_(Row::OWNER_TYPE.eq(owner_type))
                    .where_(Row::OWNER_ID.eq(owner_id))
                    .where_(Row::PROVIDER.eq(provider.as_str()))
                    .where_(Row::SCOPE_SET.eq(scope_set.as_str()))
                    .first(&uow)
                    .await?
                    .map(Record::into_inner);
                let is_new = existing.is_none();
                let mut model = existing.unwrap_or_else(|| Row {
                    id: Uuid::now_v7(),
                    owner_type: owner_type.to_owned(),
                    owner_id,
                    provider: provider.clone(),
                    scope_set: scope_set.clone(),
                    created_at: now,
                    ..Row::default()
                });
                model.kind = kind.as_str().to_owned();
                model.external_id = external_id;
                model.display_name = display_name;
                model.credentials = encrypted;
                model.access_token = access_token;
                model.token_expires_at = expires;
                model.scopes = scopes;
                model.status = ConnectionStatus::Healthy.as_str().to_owned();
                model.failures = 0;
                model.last_error = None;
                model.connected_by = connected_by;
                model.updated_at = now;
                if is_new {
                    Row::insert(&uow, [model.clone()]).await?;
                } else {
                    Row::query()
                        .where_key(model.id)
                        .update(
                            &uow,
                            [
                                Row::KIND.set(model.kind.as_str()),
                                Row::EXTERNAL_ID.set(model.external_id.clone()),
                                Row::DISPLAY_NAME.set(model.display_name.clone()),
                                Row::CREDENTIALS.set(model.credentials.as_str()),
                                Row::ACCESS_TOKEN.set(model.access_token.clone()),
                                Row::TOKEN_EXPIRES_AT.set(model.token_expires_at),
                                Row::SCOPES.set(model.scopes.clone()),
                                Row::STATUS.set(model.status.as_str()),
                                Row::FAILURES.set(model.failures),
                                Row::LAST_ERROR.set(model.last_error.clone()),
                                Row::CONNECTED_BY.set(model.connected_by),
                                Row::UPDATED_AT.set(now),
                            ],
                        )
                        .await?;
                }
                uow.mark_modified();
                model
            };
            runtime
                .emit_after_commit(
                    &uow,
                    IntegrationConnected {
                        connection_id: model.id,
                        owner,
                        provider: model.provider.clone(),
                        scope_set: model.scope_set.clone(),
                        kind,
                        connected_by,
                    },
                )
                .await;
            runtime
                .audit_after_commit(
                    &uow,
                    audit_entry(&uow, "integration.connected", &model, owner),
                )
                .await;
            Ok(runtime.info(&model))
        })
        .await
    }

    // -- reading ------------------------------------------------------------

    /// The connections `owner` can use: its own, then (for a workspace)
    /// its organization's.
    pub async fn connections(
        &self,
        owner: Owner,
    ) -> Result<Vec<ConnectionInfo>, IntegrationsError> {
        let rt = self.rt()?;
        let chain = rt.owner_chain(owner).await;
        let rows = Row::query()
            .where_group(|group| {
                chain.iter().fold(group, |group, owner| {
                    let (kind, id) = owner.columns();
                    group.or_where_group(|pair| {
                        pair.where_(Row::OWNER_TYPE.eq(kind))
                            .where_(Row::OWNER_ID.eq(id))
                    })
                })
            })
            .order_by(Row::PROVIDER)
            .order_by(Row::SCOPE_SET)
            .get(&rt.db)
            .await?;
        let mut infos: Vec<ConnectionInfo> = rows.iter().map(|row| rt.info(row)).collect();
        infos.sort_by_key(|info| {
            chain
                .iter()
                .position(|owner| *owner == info.owner())
                .unwrap_or(usize::MAX)
        });
        Ok(infos)
    }

    /// The connection `owner` uses for `provider` in `scope_set`: its own,
    /// else its organization's. [`IntegrationsError::NotConnected`] when
    /// there is none.
    pub async fn connection(
        &self,
        owner: Owner,
        provider: &str,
        scope_set: &str,
    ) -> Result<ConnectionInfo, IntegrationsError> {
        let rt = self.rt()?;
        Ok(rt.info(&self.connection_row(owner, provider, scope_set).await?))
    }

    async fn connection_row(
        &self,
        owner: Owner,
        provider: &str,
        scope_set: &str,
    ) -> Result<Row, IntegrationsError> {
        let rt = self.rt()?;
        let entry = rt.provider(provider)?;
        for candidate in rt.owner_chain(owner).await {
            let (kind, id) = candidate.columns();
            let row = Row::query()
                .where_(Row::OWNER_TYPE.eq(kind))
                .where_(Row::OWNER_ID.eq(id))
                .where_(Row::PROVIDER.eq(provider))
                .where_(Row::SCOPE_SET.eq(scope_set))
                .first(&rt.db)
                .await?;
            if let Some(row) = row {
                return Ok(row.into_inner());
            }
        }
        Err(IntegrationsError::NotConnected {
            provider: entry.name.clone(),
            scope_set: scope_set.to_owned(),
        })
    }

    /// The connection `id`.
    pub async fn find(&self, id: Uuid) -> Result<ConnectionInfo, IntegrationsError> {
        let rt = self.rt()?;
        Ok(rt.info(&rt.load(id).await?))
    }

    /// The connection `id` when `owner` can use it (its own or its
    /// organization's); [`IntegrationsError::ConnectionNotFound`] otherwise.
    pub async fn find_for(
        &self,
        owner: Owner,
        id: Uuid,
    ) -> Result<ConnectionInfo, IntegrationsError> {
        let rt = self.rt()?;
        let row = rt.load(id).await?;
        let row_owner = Owner::from_columns(&row.owner_type, row.owner_id);
        if !rt.owner_chain(owner).await.contains(&row_owner) {
            return Err(IntegrationsError::ConnectionNotFound);
        }
        Ok(rt.info(&row))
    }

    /// Every connection of every owner (for operators and commands).
    pub async fn all(&self) -> Result<Vec<ConnectionInfo>, IntegrationsError> {
        let rt = self.rt()?;
        let rows = Row::query()
            .order_by(Row::OWNER_TYPE)
            .order_by(Row::OWNER_ID)
            .order_by(Row::PROVIDER)
            .order_by(Row::SCOPE_SET)
            .get(&rt.db)
            .await?;
        Ok(rows.iter().map(|row| rt.info(row)).collect())
    }

    // -- disconnecting ------------------------------------------------------

    /// Removes connection `id`, revoking its OAuth tokens at the provider
    /// first where the provider supports it (best effort).
    pub async fn disconnect(&self, id: Uuid) -> Result<(), IntegrationsError> {
        let rt = self.rt()?.clone();
        let row = rt.load(id).await?;
        if let Some(entry) = rt.providers.get(&row.provider)
            && let Ok(StoredCredentials::OAuth { refresh_token }) = rt.credentials(&row)
        {
            let token = match refresh_token {
                Some(token) => Some(token),
                None => row
                    .access_token
                    .as_deref()
                    .and_then(|token| rt.encrypter.decrypt_string(token).ok()),
            };
            if let Some(token) = token
                && let Err(error) = rt.backend.revoke(entry, &row.scope_set, &token).await
            {
                tracing::warn!(target: "navree::integrations", connection = %id, %error, "could not revoke the provider's token");
            }
        }
        let owner = Owner::from_columns(&row.owner_type, row.owner_id);
        let runtime = rt.clone();
        rt.write(None, |uow| async move {
            let event = IntegrationDisconnected {
                connection_id: row.id,
                owner,
                provider: row.provider.clone(),
                scope_set: row.scope_set.clone(),
            };
            let entry = audit_entry(&uow, "integration.disconnected", &row, owner);
            Row::query().where_(Row::ID.eq(row.id)).delete(&uow).await?;
            runtime.emit_after_commit(&uow, event).await;
            runtime.audit_after_commit(&uow, entry).await;
            Ok(())
        })
        .await?;
        rt.subject_tokens
            .lock()
            .await
            .retain(|(conn, _), _| *conn != id);
        Ok(())
    }

    /// Removes connection `id` when it is `owner`'s own (not inherited from
    /// an organization).
    pub async fn disconnect_for(&self, owner: Owner, id: Uuid) -> Result<(), IntegrationsError> {
        let rt = self.rt()?;
        let row = rt.load(id).await?;
        if Owner::from_columns(&row.owner_type, row.owner_id) != owner {
            return Err(IntegrationsError::ConnectionNotFound);
        }
        self.disconnect(id).await
    }

    /// Deletes every connection of `owner` without calling the providers
    /// (the workspace or organization is gone). Returns how many.
    pub async fn forget(&self, owner: Owner) -> Result<u64, IntegrationsError> {
        let rt = self.rt()?;
        let (kind, id) = owner.columns();
        Ok(Row::query()
            .where_(Row::OWNER_TYPE.eq(kind))
            .where_(Row::OWNER_ID.eq(id))
            .delete(&rt.db)
            .await?)
    }

    // -- tokens and clients -------------------------------------------------

    /// A current access token of connection `id`: the cached one while it
    /// is fresh, else a new one from the provider (refreshed, or minted for
    /// app-only and service-account connections) under a lock. An API-key
    /// connection gives its key. When the provider refuses,
    /// [`IntegrationsError::Reauthorize`] and the connection turns
    /// unhealthy.
    pub async fn token(&self, id: Uuid) -> Result<Secret<String>, IntegrationsError> {
        Ok(Secret::new(self.rt()?.token(id, None, Want::Cached).await?))
    }

    /// The key and fields of an API-key connection.
    pub async fn api_key(&self, id: Uuid) -> Result<ApiKeyCredentials, IntegrationsError> {
        let rt = self.rt()?;
        let row = rt.load(id).await?;
        match rt.credentials(&row)? {
            StoredCredentials::ApiKey {
                key,
                api_url,
                fields,
            } => Ok(ApiKeyCredentials {
                key: Secret::new(key),
                api_url,
                fields,
            }),
            _ => Err(IntegrationsError::Unsupported {
                provider: rt.provider_name(&row.provider),
                kind: CredentialKind::ApiKey.phrase(),
            }),
        }
    }

    /// A client for `owner`'s connection to `provider` in `scope_set`.
    pub async fn client(
        &self,
        owner: Owner,
        provider: &str,
        scope_set: &str,
    ) -> Result<IntegrationClient, IntegrationsError> {
        let row = self.connection_row(owner, provider, scope_set).await?;
        self.client_from_row(&row)
    }

    /// The typed client of `owner`'s connection to `P` (default scope set).
    pub async fn client_for<P: Provider>(
        &self,
        owner: Owner,
    ) -> Result<P::Client, IntegrationsError> {
        Ok(self.client(owner, P::KEY, DEFAULT_SCOPE_SET).await?.into())
    }

    /// The typed client of `owner`'s connection to `P` in scope set `S`.
    pub async fn client_in<P: Provider, S: ScopeSet>(
        &self,
        owner: Owner,
    ) -> Result<P::Client, IntegrationsError> {
        Ok(self.client(owner, P::KEY, S::NAME).await?.into())
    }

    /// A client for the connection `connection`.
    pub async fn client_of(
        &self,
        connection: &ConnectionInfo,
    ) -> Result<IntegrationClient, IntegrationsError> {
        let row = self.rt()?.load(connection.id).await?;
        self.client_from_row(&row)
    }

    fn client_from_row(&self, row: &Row) -> Result<IntegrationClient, IntegrationsError> {
        let rt = self.rt()?;
        let entry = rt.provider(&row.provider)?.clone();
        let api_url = match rt.credentials(row)? {
            StoredCredentials::ApiKey {
                api_url: Some(url), ..
            } => Some(Url::parse(&url).map_err(|_| {
                IntegrationsError::InvalidCredentials("the stored api_url is invalid".to_owned())
            })?),
            _ => None,
        };
        Ok(IntegrationClient::new(
            rt.clone(),
            entry,
            rt.info(row),
            api_url,
        ))
    }

    // -- health -------------------------------------------------------------

    /// Checks connection `id` now: a new token from the provider (for
    /// OAuth, app-only and service-account connections), then the
    /// provider's health probe (`health_url`, Slack's `auth.test`). A
    /// refusal turns it unhealthy at once; an unreachable provider after
    /// `integrations.unhealthy_after` failures in a row. Emits
    /// [`ConnectionUnhealthy`](crate::ConnectionUnhealthy) and
    /// [`ConnectionRestored`](crate::ConnectionRestored) on a change.
    pub async fn check(&self, id: Uuid) -> Result<Health, IntegrationsError> {
        let rt = self.rt()?;
        let row = rt.load(id).await?;
        let entry = rt.provider(&row.provider)?.clone();
        let credentials = rt.credentials(&row)?;
        let outcome = async {
            let want = match &credentials {
                // Without a refresh token the stored token is all there is.
                StoredCredentials::OAuth {
                    refresh_token: None,
                } => Want::Cached,
                _ => Want::Fresh,
            };
            let token = match rt.token_raw(id, None, want).await {
                Ok(token) => token,
                Err(TokenFailure::Connection(_, error) | TokenFailure::Other(error)) => {
                    return Err(error);
                }
            };
            if let Some(probe) = entry.probe() {
                rt.probe(&entry, &credentials, &token, &probe).await?;
            }
            Ok(())
        }
        .await;
        let now = rt.now();
        let row = rt.load(id).await?;
        match outcome {
            Ok(()) => rt.record(&row, None, Some(now)).await,
            Err(error) if error.is_refusal() => {
                rt.record(&row, Some((failure_message(&error), true)), Some(now))
                    .await
            }
            Err(error @ (IntegrationsError::Unavailable { .. } | IntegrationsError::Http(_))) => {
                rt.record(&row, Some((failure_message(&error), false)), Some(now))
                    .await
            }
            Err(error) => Err(error),
        }
    }

    /// Checks every connection whose last check is older than
    /// `integrations.check_interval_mins` (nothing when it is 0).
    pub async fn check_due(&self) -> Result<CheckSummary, IntegrationsError> {
        let rt = self.rt()?;
        let interval = rt.options.check_interval_mins;
        if interval == 0 {
            return Ok(CheckSummary::default());
        }
        let cutoff =
            rt.now() - TimeDelta::minutes(i64::try_from(interval).unwrap_or(i64::MAX / 120));
        let ids = Row::query()
            .where_group(|due| {
                due.where_(Row::LAST_CHECKED_AT.is_null())
                    .or_where(Row::LAST_CHECKED_AT.lt(cutoff))
            })
            .order_by(Row::ID)
            .pluck(&rt.db, Row::ID)
            .await?;
        Ok(self.check_many(ids).await)
    }

    /// Checks every connection.
    pub async fn check_all(&self) -> Result<CheckSummary, IntegrationsError> {
        let rt = self.rt()?;
        let ids = Row::query()
            .order_by(Row::ID)
            .pluck(&rt.db, Row::ID)
            .await?;
        Ok(self.check_many(ids).await)
    }

    async fn check_many(&self, ids: Vec<Uuid>) -> CheckSummary {
        let mut summary = CheckSummary::default();
        for id in ids {
            match self.check(id).await {
                Ok(health) => {
                    summary.checked += 1;
                    match health.status {
                        ConnectionStatus::Healthy => summary.healthy += 1,
                        ConnectionStatus::Unhealthy => summary.unhealthy += 1,
                    }
                }
                Err(error) => {
                    tracing::warn!(target: "navree::integrations", connection = %id, %error, "could not check a connection");
                }
            }
        }
        summary
    }
}

/// The audit entry of a connection change.
fn audit_entry(uow: &navree_db::Uow, action: &str, row: &Row, owner: Owner) -> AuditEntry {
    let mut entry = AuditEntry::new(AuditKind::Custom, action)
        .subject("IntegrationConnection", row.id.to_string())
        .metadata(json!({
            "provider": row.provider,
            "scope_set": row.scope_set,
            "kind": row.kind,
            "owner": owner,
        }));
    if let Some(actor) = uow.actor() {
        entry = entry.actor(actor);
    }
    if let Some(request_id) = uow.request_id() {
        entry = entry.request_id(request_id);
    }
    entry
}

/// A Microsoft tenant id or domain from a callback.
fn is_tenant(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

fn is_email(value: &str) -> bool {
    value
        .split_once('@')
        .is_some_and(|(user, domain)| !user.is_empty() && domain.contains('.'))
        && !value.chars().any(char::is_whitespace)
}

impl fmt::Debug for Integrations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Integrations")
            .field("fake", &self.is_fake())
            .field("bound", &self.runtime.get().is_some())
            .finish_non_exhaustive()
    }
}

/// Binds a [`Integrations::fake`] (or any `Integrations` given to
/// `override_value`) to the app at boot.
impl Lifecycle for Integrations {
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
        let (integrations, _fake) = Integrations::fake();
        assert!(integrations.is_fake());
        assert!(matches!(
            integrations.providers(),
            Err(IntegrationsError::NotBound)
        ));
        assert!(matches!(
            integrations.find(Uuid::nil()).await,
            Err(IntegrationsError::NotBound)
        ));
        assert!(format!("{integrations:?}").contains("bound: false"));
    }

    #[test]
    fn tenants_and_subjects_are_checked() {
        assert!(is_tenant("9188040d-6c67-4c5b-b112-36a304b66dad"));
        assert!(is_tenant("contoso.onmicrosoft.com"));
        assert!(!is_tenant("a/b"));
        assert!(!is_tenant(""));
        assert!(is_email("admin@acme.test"));
        assert!(!is_email("admin"));
        assert!(!is_email("a b@acme.test"));
    }
}
