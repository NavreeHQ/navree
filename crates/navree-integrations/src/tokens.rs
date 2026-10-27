//! Access tokens: the cached token while it is fresh, else a new one from
//! the provider under a lock, so concurrent requests and instances refresh
//! a connection once. Also the health bookkeeping (failures, status and
//! the events of a change).

use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use http::Method;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::Value;
use uuid::Uuid;

use crate::entity::integration_connection::{self, Column, Entity};
use crate::entry::{Probe, ProviderEntry};
use crate::events::{ConnectionRestored, ConnectionUnhealthy};
use crate::oauth::{Exchange, Grant};
use crate::runtime::Runtime;
use crate::types::StoredCredentials;
use crate::{ConnectionStatus, Health, IntegrationsError, Owner};

/// A token is refreshed when it expires within this many seconds.
const FRESH_MARGIN_SECS: i64 = 60;

/// What a caller accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Want {
    /// The cached token while it is fresh.
    Cached,
    /// A new token, unless the current one differs from this rejected one
    /// (someone else already replaced it).
    Replace(String),
    /// A new token in any case (health checks).
    Fresh,
}

impl Runtime {
    /// The row `id`.
    pub(crate) async fn load(
        &self,
        id: Uuid,
    ) -> Result<integration_connection::Model, IntegrationsError> {
        Entity::find_by_id(id)
            .one(&self.db.reader().await)
            .await?
            .ok_or(IntegrationsError::ConnectionNotFound)
    }

    /// The cached token of `model`, when `want` accepts it.
    fn cached(
        &self,
        model: &integration_connection::Model,
        want: &Want,
    ) -> Result<Option<String>, IntegrationsError> {
        let Some(encrypted) = &model.access_token else {
            return Ok(None);
        };
        let fresh = model
            .token_expires_at
            .is_none_or(|at| at > self.now() + TimeDelta::seconds(FRESH_MARGIN_SECS));
        if !fresh || *want == Want::Fresh {
            return Ok(None);
        }
        let token = self.encrypter.decrypt_string(encrypted)?;
        match want {
            Want::Replace(stale) if *stale == token => Ok(None),
            _ => Ok(Some(token)),
        }
    }

    /// A token for connection `id` (acting as `subject` for a service
    /// account). A refusal turns the connection unhealthy and answers
    /// [`IntegrationsError::Reauthorize`].
    pub(crate) async fn token(
        &self,
        id: Uuid,
        subject: Option<&str>,
        want: Want,
    ) -> Result<String, IntegrationsError> {
        match self.token_raw(id, subject, want).await {
            Ok(token) => Ok(token),
            Err(TokenFailure::Connection(model, error)) => {
                let message = failure_message(&error);
                self.record(&model, Some((message.clone(), true)), None)
                    .await?;
                Err(IntegrationsError::Reauthorize {
                    provider: self.provider_name(&model.provider),
                    error: message,
                })
            }
            Err(TokenFailure::Other(error)) => Err(error),
        }
    }

    /// [`token`](Self::token) without the bookkeeping: a refusal of the
    /// connection's own credentials comes back with its row.
    pub(crate) async fn token_raw(
        &self,
        id: Uuid,
        subject: Option<&str>,
        want: Want,
    ) -> Result<String, TokenFailure> {
        let model = self.load(id).await?;
        let entry = self.provider(&model.provider)?.clone();
        let credentials = self.credentials(&model)?;
        match &credentials {
            StoredCredentials::ApiKey { key, .. } => return Ok(key.clone()),
            StoredCredentials::ServiceAccount {
                subject: stored, ..
            } => {
                if let Some(subject) = subject
                    && Some(subject) != stored.as_deref()
                {
                    return Ok(self
                        .subject_token(&entry, &model, &credentials, subject, &want)
                        .await?);
                }
            }
            _ if subject.is_some() => {
                return Err(TokenFailure::Other(IntegrationsError::Unsupported {
                    provider: entry.name.clone(),
                    kind: crate::CredentialKind::ServiceAccount.phrase(),
                }));
            }
            _ => {}
        }
        if let Some(token) = self.cached(&model, &want)? {
            return Ok(token);
        }
        let wait = Duration::from_secs(self.options.lock_timeout_secs);
        let _guard = self
            .locks
            .lock(&format!("integrations:token:{id}"), wait + Duration::from_secs(15))
            .block(wait)
            .await
            .map_err(IntegrationsError::from)?;
        // Another request or instance may have refreshed it meanwhile.
        let model = self.load(id).await?;
        if let Some(token) = self.cached(&model, &want)? {
            return Ok(token);
        }
        let credentials = self.credentials(&model)?;
        match self.mint(&entry, &model, &credentials, None).await {
            Ok(grant) => {
                self.save_grant(&model, &credentials, &grant).await?;
                Ok(grant.access_token)
            }
            Err(error) if error.is_refusal() => Err(TokenFailure::Connection(Box::new(model), error)),
            Err(error) => Err(TokenFailure::Other(error)),
        }
    }

    /// A token of a service account acting as `subject`, kept in memory.
    async fn subject_token(
        &self,
        entry: &ProviderEntry,
        model: &integration_connection::Model,
        credentials: &StoredCredentials,
        subject: &str,
        want: &Want,
    ) -> Result<String, IntegrationsError> {
        let key = (model.id, subject.to_owned());
        let usable = |(token, expires): &(String, Option<DateTime<Utc>>)| {
            let fresh = expires
                .is_none_or(|at| at > self.now() + TimeDelta::seconds(FRESH_MARGIN_SECS));
            let rejected = matches!(want, Want::Replace(stale) if stale == token);
            (fresh && !rejected && *want != Want::Fresh).then(|| token.clone())
        };
        if let Some(token) = self.subject_tokens.lock().await.get(&key).and_then(usable) {
            return Ok(token);
        }
        let grant = self.mint(entry, model, credentials, Some(subject)).await?;
        let expires = grant
            .expires_in
            .map(|secs| self.now() + TimeDelta::seconds(secs));
        self.subject_tokens
            .lock()
            .await
            .insert(key, (grant.access_token.clone(), expires));
        Ok(grant.access_token)
    }

    /// Asks the provider for a new token.
    pub(crate) async fn mint(
        &self,
        entry: &ProviderEntry,
        model: &integration_connection::Model,
        credentials: &StoredCredentials,
        subject: Option<&str>,
    ) -> Result<Grant, IntegrationsError> {
        let scopes = entry
            .scopes_for(&model.scope_set, &self.scope_sets)
            .unwrap_or_else(|_| {
                model
                    .scopes
                    .as_deref()
                    .unwrap_or_default()
                    .split(' ')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            });
        let exchange = match credentials {
            StoredCredentials::OAuth {
                refresh_token: Some(refresh_token),
            } => Exchange::Refresh {
                refresh_token,
                scopes: &scopes,
            },
            StoredCredentials::OAuth {
                refresh_token: None,
            } => {
                return Err(IntegrationsError::Refused {
                    provider: entry.name.clone(),
                    error: "token_expired".to_owned(),
                    description: Some("no refresh token was issued".to_owned()),
                });
            }
            StoredCredentials::AppOnly { tenant } => Exchange::ClientCredentials { tenant },
            StoredCredentials::ServiceAccount {
                key,
                subject: stored,
            } => Exchange::JwtBearer {
                key,
                subject: subject.or(stored.as_deref()),
                scopes: &scopes,
                now: self.now(),
            },
            StoredCredentials::ApiKey { key, .. } => {
                return Ok(Grant {
                    access_token: key.clone(),
                    ..Grant::default()
                });
            }
        };
        self.backend.grant(entry, &model.scope_set, exchange).await
    }

    /// Stores a new token (and a rotated refresh token); a connection that
    /// was unhealthy is healthy again.
    async fn save_grant(
        &self,
        model: &integration_connection::Model,
        credentials: &StoredCredentials,
        grant: &Grant,
    ) -> Result<(), IntegrationsError> {
        let now = self.now();
        let token = self.encrypter.encrypt_string(&grant.access_token)?;
        let expires = grant
            .expires_in
            .map(|secs| now + TimeDelta::seconds(secs));
        let rotated = match (credentials, &grant.refresh_token) {
            (StoredCredentials::OAuth { refresh_token }, Some(new))
                if refresh_token.as_deref() != Some(new.as_str()) =>
            {
                Some(self.encrypter.encrypt_json(&StoredCredentials::OAuth {
                    refresh_token: Some(new.clone()),
                })?)
            }
            _ => None,
        };
        let id = model.id;
        let scopes = grant.scopes.as_ref().map(|scopes| scopes.join(" "));
        let restored = ConnectionStatus::parse(&model.status) == ConnectionStatus::Unhealthy;
        let event = restored.then(|| ConnectionRestored {
            connection_id: id,
            owner: Owner::from_columns(&model.owner_type, model.owner_id),
            provider: model.provider.clone(),
            scope_set: model.scope_set.clone(),
        });
        self.housekeeping(|uow| async move {
            let mut update = Entity::update_many()
                .col_expr(Column::AccessToken, Expr::value(Some(token)))
                .col_expr(Column::TokenExpiresAt, Expr::value(expires))
                .filter(Column::Id.eq(id));
            if let Some(credentials) = rotated {
                update = update.col_expr(Column::Credentials, Expr::value(credentials));
            }
            if let Some(scopes) = scopes {
                update = update.col_expr(Column::Scopes, Expr::value(Some(scopes)));
            }
            if restored {
                update = update
                    .col_expr(Column::Status, Expr::value(ConnectionStatus::Healthy.as_str()))
                    .col_expr(Column::Failures, Expr::value(0))
                    .col_expr(Column::LastError, Expr::value(None::<String>))
                    .col_expr(Column::UpdatedAt, Expr::value(now));
            }
            update.exec(&*uow.tx().await?).await?;
            if let Some(event) = event {
                self.emit_after_commit(&uow, event).await;
            }
            Ok(())
        })
        .await
    }

    /// Records the outcome of a check or token request: `failure` is the
    /// message and whether it was a refusal; `checked_at` is set by checks.
    pub(crate) async fn record(
        &self,
        model: &integration_connection::Model,
        failure: Option<(String, bool)>,
        checked_at: Option<DateTime<Utc>>,
    ) -> Result<Health, IntegrationsError> {
        let now = self.now();
        let before = ConnectionStatus::parse(&model.status);
        let (status, failures, error) = match &failure {
            None => (ConnectionStatus::Healthy, 0, None),
            Some((message, refusal)) => {
                let failures = model.failures.saturating_add(1);
                let threshold = i32::try_from(self.options.unhealthy_after).unwrap_or(i32::MAX);
                let status = if *refusal || failures >= threshold {
                    ConnectionStatus::Unhealthy
                } else {
                    before
                };
                (status, failures, Some(message.clone()))
            }
        };
        let owner = Owner::from_columns(&model.owner_type, model.owner_id);
        let id = model.id;
        let failures_u32 = u32::try_from(failures.max(0)).unwrap_or(u32::MAX);
        let unhealthy = (before == ConnectionStatus::Healthy && status == ConnectionStatus::Unhealthy)
            .then(|| ConnectionUnhealthy {
                connection_id: id,
                owner,
                provider: model.provider.clone(),
                scope_set: model.scope_set.clone(),
                error: error.clone().unwrap_or_default(),
                failures: failures_u32,
            });
        let restored = (before == ConnectionStatus::Unhealthy && status == ConnectionStatus::Healthy)
            .then(|| ConnectionRestored {
                connection_id: id,
                owner,
                provider: model.provider.clone(),
                scope_set: model.scope_set.clone(),
            });
        let changed = before != status;
        let last_error = error.clone();
        self.housekeeping(|uow| async move {
            let mut update = Entity::update_many()
                .col_expr(Column::Status, Expr::value(status.as_str()))
                .col_expr(Column::Failures, Expr::value(failures))
                .col_expr(Column::LastError, Expr::value(last_error))
                .filter(Column::Id.eq(id));
            if let Some(checked_at) = checked_at {
                update = update.col_expr(Column::LastCheckedAt, Expr::value(Some(checked_at)));
            }
            if changed {
                update = update.col_expr(Column::UpdatedAt, Expr::value(now));
            }
            update.exec(&*uow.tx().await?).await?;
            if let Some(event) = unhealthy {
                self.emit_after_commit(&uow, event).await;
            }
            if let Some(event) = restored {
                self.emit_after_commit(&uow, event).await;
            }
            Ok(())
        })
        .await?;
        if let (Some(event_status), Some(message)) = (changed.then_some(status), &error) {
            tracing::warn!(
                target: "navree::integrations",
                connection = %id,
                provider = %model.provider,
                status = event_status.as_str(),
                error = %message,
                "an integration connection changed status"
            );
        }
        Ok(Health {
            connection_id: id,
            status,
            failures: failures_u32,
            error,
            checked_at: checked_at.unwrap_or(now),
        })
    }

    /// Asks the API whether the token works: `Ok` for 2xx (and Slack's
    /// `ok: true`).
    pub(crate) async fn probe(
        &self,
        entry: &ProviderEntry,
        credentials: &StoredCredentials,
        token: &str,
        probe: &Probe,
    ) -> Result<(), IntegrationsError> {
        let base = match credentials {
            StoredCredentials::ApiKey {
                api_url: Some(url), ..
            } => Some(url.as_str()),
            _ => None,
        };
        let url = if probe.path.contains("://") {
            probe.path.clone()
        } else {
            match base {
                Some(base) => format!(
                    "{}/{}",
                    base.trim_end_matches('/'),
                    probe.path.trim_start_matches('/')
                ),
                None => entry.api_path(&probe.path),
            }
        };
        let mut request = self.http.request(probe.method.clone(), &url).retries(0);
        request = match credentials {
            StoredCredentials::ApiKey { .. } => {
                let (name, value) = entry.api_key_header(token);
                request.header(name, value)
            }
            _ => request.bearer(token),
        };
        if probe.method == Method::POST {
            request = request.header("content-type", "application/x-www-form-urlencoded");
        }
        let response = request
            .send()
            .await
            .map_err(|error| IntegrationsError::Unavailable {
                provider: entry.name.clone(),
                message: error.to_string(),
            })?;
        let status = response.status();
        if status.is_server_error() || status.as_u16() == 429 {
            return Err(IntegrationsError::Unavailable {
                provider: entry.name.clone(),
                message: format!("the health check answered {}", status.as_u16()),
            });
        }
        if !status.is_success() {
            return Err(IntegrationsError::Refused {
                provider: entry.name.clone(),
                error: format!("http_{}", status.as_u16()),
                description: None,
            });
        }
        if entry.kind == crate::ProviderType::Slack {
            let body: Value = response.json().unwrap_or(Value::Null);
            if body.get("ok").and_then(Value::as_bool) != Some(true) {
                return Err(IntegrationsError::Refused {
                    provider: entry.name.clone(),
                    error: body
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("not_ok")
                        .to_owned(),
                    description: None,
                });
            }
        }
        Ok(())
    }
}

/// Why a token could not be had.
pub(crate) enum TokenFailure {
    /// The provider refused the connection's own credentials.
    Connection(Box<integration_connection::Model>, IntegrationsError),
    /// Anything else (unreachable provider, internal errors).
    Other(IntegrationsError),
}

impl<E: Into<IntegrationsError>> From<E> for TokenFailure {
    fn from(error: E) -> Self {
        TokenFailure::Other(error.into())
    }
}

/// The message kept for a failure: the provider's error code (and its
/// description), never a credential.
pub(crate) fn failure_message(error: &IntegrationsError) -> String {
    match error {
        IntegrationsError::Refused {
            error, description, ..
        } => match description {
            Some(description) => format!("{error}: {description}"),
            None => error.clone(),
        },
        IntegrationsError::Unavailable { message, .. } => message.clone(),
        other => other.to_string(),
    }
}
