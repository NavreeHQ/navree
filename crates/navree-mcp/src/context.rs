//! [`McpContext`]: who is calling a tool, resource or prompt, and where.

use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_access::{
    Ability, Accessible, ActionMarker, Authorized, PrincipalAccess, Subject, SubjectRecords,
};
use navree_auth::{AuthService, User};
use navree_contracts::{Clock, Principal};
use navree_db::Uow;
use navree_http::AppError;
use navree_kernel::AppContext;
use rmcp::model::{ProgressNotificationParam, ProgressToken};
use rmcp::{Peer, RoleServer};
use uuid::Uuid;

/// The client that called: its name and version from the `initialize`
/// handshake.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpClient {
    /// The client's name (`claude-code`, `cursor`, ...).
    pub name: String,
    /// The client's version.
    pub version: String,
}

/// The caller of a tool, resource or prompt: the user and their access,
/// the call's unit of work, the session and the client.
///
/// It has the helpers services need to keep the app's security model:
/// list filters through [`accessible`](McpContext::accessible), record
/// checks with [`authorize`](McpContext::authorize), writes in
/// [`uow`](McpContext::uow) (committed when the handler returns `Ok`,
/// rolled back otherwise).
#[derive(Clone)]
pub struct McpContext {
    inner: Arc<Inner>,
}

struct Inner {
    app: AppContext,
    principal: Principal,
    access: Arc<PrincipalAccess>,
    uow: Option<Uow>,
    session_id: Option<String>,
    client: Option<McpClient>,
    request_id: String,
    progress: Option<(Peer<RoleServer>, ProgressToken)>,
    clock: Option<Arc<dyn Clock>>,
}

/// What a context is built from.
pub(crate) struct ContextParts {
    pub(crate) app: AppContext,
    pub(crate) principal: Principal,
    pub(crate) access: Arc<PrincipalAccess>,
    pub(crate) uow: Option<Uow>,
    pub(crate) session_id: Option<String>,
    pub(crate) client: Option<McpClient>,
    pub(crate) request_id: String,
    pub(crate) progress: Option<(Peer<RoleServer>, ProgressToken)>,
    pub(crate) clock: Option<Arc<dyn Clock>>,
}

impl McpContext {
    pub(crate) fn new(parts: ContextParts) -> Self {
        let ContextParts {
            app,
            principal,
            access,
            uow,
            session_id,
            client,
            request_id,
            progress,
            clock,
        } = parts;
        Self {
            inner: Arc::new(Inner {
                app,
                principal,
                access,
                uow,
                session_id,
                client,
                request_id,
                progress,
                clock,
            }),
        }
    }

    /// The app.
    pub fn app(&self) -> &AppContext {
        &self.inner.app
    }

    /// The authenticated principal: the user id, how they authenticated and
    /// the abilities their token is limited to.
    pub fn principal(&self) -> &Principal {
        &self.inner.principal
    }

    /// The user's id.
    pub fn user_id(&self) -> Uuid {
        self.inner.principal.user_id
    }

    /// The signed-in user, loaded through navree-auth's `AuthService`: a 401
    /// when the account no longer exists, a 500 when the app has no
    /// `AuthModule`.
    pub async fn user(&self) -> Result<User, AppError> {
        let auth = self.inner.app.get::<AuthService>().await.map_err(|error| {
            AppError::internal(format!("cx.user() needs AuthModule in the app: {error}"))
        })?;
        auth.find_user(self.user_id())
            .await
            .map_err(AppError::internal)?
            .ok_or_else(AppError::unauthenticated)
    }

    /// The user's roles, permissions and compiled abilities, restricted to
    /// their token's abilities.
    pub fn access(&self) -> &PrincipalAccess {
        &self.inner.access
    }

    /// The user's abilities, for checks in services.
    pub fn ability(&self) -> &Arc<Ability> {
        &self.inner.access.ability
    }

    /// A list filter: the records of `E` the user may do `A` on (a SeaORM
    /// entity's `apply(select)`, an ORM model's `apply(query)`).
    pub fn accessible<E, A>(&self) -> Accessible<E, A>
    where
        E: Subject,
        A: ActionMarker,
    {
        Accessible::new(self.inner.access.ability.clone())
    }

    /// Loads record `id` of `E` (a SeaORM entity, an ORM model or any other
    /// subject with records) in the call's unit of work and checks `A` on
    /// it: the record, or a 404 (no such record) or 403 (not allowed). The
    /// tool's equivalent of `#[can(A, E, from = "id")]`.
    pub async fn authorize<E, A>(&self, id: impl fmt::Display) -> Result<Authorized<E, A>, AppError>
    where
        E: SubjectRecords,
        A: ActionMarker,
    {
        Authorized::load(
            self.uow(),
            self.inner.access.ability.clone(),
            &id.to_string(),
        )
        .await
    }

    /// The call's unit of work: tools write here; it commits when the
    /// handler returns `Ok` and rolls back otherwise.
    ///
    /// # Panics
    ///
    /// When the app has no `DatabaseModule`.
    pub fn uow(&self) -> &Uow {
        self.inner
            .uow
            .as_ref()
            .expect("McpContext::uow needs DatabaseModule in the app")
    }

    /// The current time, from the app's `dyn Clock` when it has one.
    pub fn now(&self) -> DateTime<Utc> {
        match &self.inner.clock {
            Some(clock) => clock.now(),
            None => Utc::now(),
        }
    }

    /// The MCP session (the `Mcp-Session-Id` of the HTTP transport, or the
    /// stdio session's id); `None` for sessionless requests.
    pub fn session_id(&self) -> Option<&str> {
        self.inner.session_id.as_deref()
    }

    /// The client's name and version, when it introduced itself.
    pub fn client(&self) -> Option<&McpClient> {
        self.inner.client.as_ref()
    }

    /// The request id (the HTTP request's `X-Request-Id`).
    pub fn request_id(&self) -> &str {
        &self.inner.request_id
    }

    /// Whether the client asked for progress notifications.
    pub fn wants_progress(&self) -> bool {
        self.inner.progress.is_some()
    }

    /// Reports progress of a long-running call (`done` of `total`, with a
    /// message) when the client asked for it; does nothing otherwise. Over
    /// HTTP the response becomes an SSE stream carrying the notifications
    /// before the result.
    pub async fn progress(&self, done: f64, total: Option<f64>, message: impl Into<String>) {
        let Some((peer, token)) = &self.inner.progress else {
            return;
        };
        let mut param = ProgressNotificationParam::new(token.clone(), done);
        if let Some(total) = total {
            param = param.with_total(total);
        }
        let message = message.into();
        if !message.is_empty() {
            param = param.with_message(message);
        }
        if let Err(error) = peer.notify_progress(param).await {
            tracing::debug!(target: "navree::mcp", %error, "could not send a progress notification");
        }
    }
}

impl fmt::Debug for McpContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpContext")
            .field("user_id", &self.inner.principal.user_id)
            .field("session_id", &self.inner.session_id)
            .field("client", &self.inner.client)
            .field("request_id", &self.inner.request_id)
            .finish_non_exhaustive()
    }
}
