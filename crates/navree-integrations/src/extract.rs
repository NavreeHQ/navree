//! Extractors: [`Connection<P, S>`] and [`IntegrationOwner`].

use std::fmt;
use std::marker::PhantomData;
use std::ops::Deref;

use axum::extract::FromRequestParts;
use http::request::Parts;
use navree_http::AppError;
use navree_kernel::AppContext;
use navree_workspace::{CurrentOrganization, CurrentWorkspace};

use crate::provider::DefaultScopes;
use crate::{ConnectionInfo, Integrations, IntegrationsError, Level, Owner, Provider, ScopeSet};

/// Who the request acts for, by `integrations.level`: the request's
/// workspace (resolved and checked like `CurrentWorkspace`), its
/// organization, or the app. Answers like those extractors: 400 when the
/// request names no workspace, 404 for an unknown one, 403 for a
/// non-member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntegrationOwner(pub Owner);

impl<S: Send + Sync> FromRequestParts<S> for IntegrationOwner {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        if let Some(owner) = parts.extensions.get::<IntegrationOwner>() {
            return Ok(*owner);
        }
        let integrations = integrations(parts).await?;
        let level = integrations
            .level()
            .map_err(IntegrationsError::into_app_error)?;
        let owner = match level {
            Level::App => Owner::App,
            Level::Workspace => Owner::Workspace(
                CurrentWorkspace::from_request_parts(parts, state)
                    .await?
                    .id(),
            ),
            Level::Organization => Owner::Organization(
                CurrentOrganization::from_request_parts(parts, state)
                    .await?
                    .id(),
            ),
        };
        parts.extensions.insert(IntegrationOwner(owner));
        Ok(IntegrationOwner(owner))
    }
}

/// The app's `Integrations` service, from the request.
pub(crate) async fn integrations(parts: &Parts) -> Result<std::sync::Arc<Integrations>, AppError> {
    let ctx = parts
        .extensions
        .get::<AppContext>()
        .ok_or_else(|| AppError::internal("Connection needs the navree request pipeline"))?;
    ctx.get::<Integrations>()
        .await
        .map_err(|_| AppError::internal("Connection needs IntegrationsModule in the app"))
}

/// The request owner's connection to provider `P` in scope set `S`
/// (default: the `default` scope set), with its typed client: a handler
/// that takes `graph: Connection<MicrosoftGraph>` calls
/// `graph.get("/users").send().await?`.
///
/// The owner comes from `integrations.level` (see [`IntegrationOwner`]);
/// a workspace without its own connection uses its organization's. A
/// missing connection is a 404 `integration_not_connected`.
///
/// ```no_run
/// use navree_integrations::{Connection, MicrosoftGraph};
/// use navree_http::{AppError, Json};
///
/// async fn users(graph: Connection<MicrosoftGraph>) -> Result<Json<serde_json::Value>, AppError> {
///     let users = graph.get("/users").send().await?.error_for_status()?.json()?;
///     Ok(Json(users))
/// }
/// ```
pub struct Connection<P: Provider, S: ScopeSet = DefaultScopes> {
    connection: ConnectionInfo,
    client: P::Client,
    _scopes: PhantomData<fn() -> S>,
}

impl<P: Provider, S: ScopeSet> Connection<P, S> {
    /// The connection.
    pub fn info(&self) -> &ConnectionInfo {
        &self.connection
    }

    /// The typed client.
    pub fn client(&self) -> &P::Client {
        &self.client
    }

    /// The typed client, taken out.
    pub fn into_client(self) -> P::Client {
        self.client
    }
}

impl<P: Provider, S: ScopeSet> Deref for Connection<P, S> {
    type Target = P::Client;

    fn deref(&self) -> &P::Client {
        &self.client
    }
}

impl<P: Provider, S: ScopeSet> fmt::Debug for Connection<P, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("provider", &P::KEY)
            .field("scope_set", &S::NAME)
            .field("connection", &self.connection.id)
            .finish_non_exhaustive()
    }
}

impl<St, P, S> FromRequestParts<St> for Connection<P, S>
where
    St: Send + Sync,
    P: Provider,
    S: ScopeSet,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &St) -> Result<Self, Self::Rejection> {
        let IntegrationOwner(owner) = IntegrationOwner::from_request_parts(parts, state).await?;
        let integrations = integrations(parts).await?;
        let client = integrations
            .client(owner, P::KEY, S::NAME)
            .await
            .map_err(IntegrationsError::into_app_error)?;
        let connection = client.connection().clone();
        Ok(Connection {
            connection,
            client: client.into(),
            _scopes: PhantomData,
        })
    }
}
