//! [`Operation`]: the state of one executing GraphQL operation (a query, a
//! mutation or a subscription), shared by its fields through the
//! async-graphql context.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

use http::request::Parts;
use http::{HeaderMap, Method, Uri};
use navree_access::{CurrentAccess, FieldCheck, PrincipalAccess};
use navree_auth::{CurrentUser, User};
use navree_contracts::{AuthState, Principal, RequestId};
use navree_db::Uow;
use navree_http::{AccessSpec, AppError, CanSpec, RouteMeta};
use navree_kernel::AppContext;
use navree_workspace::{CurrentWorkspace, DemoIntent};
use tokio::sync::OnceCell;

use crate::__private::FieldAccess;
use crate::schema::Shared;

/// Runs one extractor on an operation's request parts: a Navree extractor
/// (`navree_http::extract::FromRequestParts`) or one on the previous
/// framework's trait (`CurrentWorkspace`); `M` is inferred, and a type
/// implements one of the two, never both. It returns the extractor's own
/// future (no async wrapper), so a field's future nests no deeper than with
/// one trait.
pub(crate) fn extract<T, M>(parts: &mut Parts) -> impl Future<Output = Result<T, AppError>> + Send
where
    T: Extract<M>,
{
    T::extract(parts)
}

/// An extractor [`extract`] runs; `M` tells the two traits apart.
pub(crate) trait Extract<M>: Sized {
    /// Runs the extractor.
    fn extract(parts: &mut Parts) -> impl Future<Output = Result<Self, AppError>> + Send;
}

/// Navree's extractors.
pub(crate) enum ViaNavree {}

/// The previous framework's extractors.
pub(crate) enum ViaPrevious {}

impl<T> Extract<ViaNavree> for T
where
    T: navree_http::extract::FromRequestParts<Rejection = AppError>,
{
    fn extract(parts: &mut Parts) -> impl Future<Output = Result<Self, AppError>> + Send {
        T::from_request_parts(parts)
    }
}

impl<T> Extract<ViaPrevious> for T
where
    T: axum::extract::FromRequestParts<(), Rejection = AppError> + Send,
{
    fn extract(parts: &mut Parts) -> impl Future<Output = Result<Self, AppError>> + Send {
        T::from_request_parts(parts, &())
    }
}

/// Where an operation comes from: the HTTP request (or WebSocket upgrade)
/// that carried it, and who sent it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Origin {
    /// The request's extensions (`AppContext`, `RequestId`, `ClientIp`, ...).
    pub(crate) extensions: http::Extensions,
    /// The request's headers (`X-Workspace`, `User-Agent`, ...).
    pub(crate) headers: HeaderMap,
    /// The request's URI.
    pub(crate) uri: Uri,
    /// The viewer.
    pub(crate) principal: Option<Principal>,
}

/// One executing operation. Fields reach it with `ctx.data::<Arc<Operation>>()`.
pub(crate) struct Operation {
    pub(crate) shared: Arc<Shared>,
    pub(crate) app: AppContext,
    pub(crate) principal: Option<Principal>,
    pub(crate) request_id: String,
    origin: Origin,
    /// The unit of work query fields share (lazy: it begins a transaction
    /// only when a field uses it). Replaced once finished.
    reads: Mutex<Option<Uow>>,
    access: OnceCell<Arc<PrincipalAccess>>,
    user: OnceCell<CurrentUser>,
    workspace: OnceCell<CurrentWorkspace>,
    loaders: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Operation {
    /// The state of a new operation from `origin`.
    pub(crate) async fn start(shared: Arc<Shared>, app: AppContext, mut origin: Origin) -> Self {
        let request_id = match origin.extensions.get::<RequestId>() {
            Some(id) => id.clone(),
            None => {
                let id = RequestId::generate();
                origin.extensions.insert(id.clone());
                id
            }
        };
        origin.extensions.insert(app.clone());
        let principal = origin.principal.clone();
        let request_id = request_id.as_str().to_owned();
        let reads = match &shared.db {
            Some(db) => Some(
                db.uow(
                    principal.as_ref().map(|p| p.user_id),
                    Some(request_id.clone()),
                )
                .await,
            ),
            None => None,
        };
        let loaders = shared
            .loaders
            .iter()
            .map(|loader| (loader.type_id, (loader.make)()))
            .collect();
        Self {
            shared,
            app,
            principal,
            request_id,
            origin,
            reads: Mutex::new(reads),
            access: OnceCell::new(),
            user: OnceCell::new(),
            workspace: OnceCell::new(),
            loaders,
        }
    }

    /// The unit of work query fields share; `None` without a database.
    pub(crate) fn reads(&self) -> Option<Uow> {
        self.reads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// A new unit of work for a mutation field; `None` without a database.
    pub(crate) async fn new_uow(&self) -> Option<Uow> {
        match &self.shared.db {
            Some(db) => Some(
                db.uow(
                    self.principal.as_ref().map(|p| p.user_id),
                    Some(self.request_id.clone()),
                )
                .await,
            ),
            None => None,
        }
    }

    /// Ends the shared reads when they began a transaction, so a mutation
    /// (or the next subscription event) does not wait for its lock; later
    /// fields get a new one.
    pub(crate) async fn release_reads(&self) {
        let Some(current) = self.reads() else {
            return;
        };
        if !current.has_open_transaction().await {
            return;
        }
        let fresh = self.new_uow().await;
        *self.reads.lock().unwrap_or_else(PoisonError::into_inner) = fresh;
        if let Err(error) = current.rollback().await {
            tracing::warn!(target: "navree::graphql", %error, "ending the read unit of work failed");
        }
    }

    /// Ends the operation: its reads commit nothing.
    pub(crate) async fn finish(&self) {
        let current = self
            .reads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(uow) = current
            && let Err(error) = uow.rollback().await
        {
            tracing::warn!(target: "navree::graphql", %error, "ending the read unit of work failed");
        }
    }

    /// Request parts for navree's extractors and the route authorizer: the
    /// origin's headers and extensions, the viewer as `AuthState`.
    pub(crate) fn parts(&self) -> Parts {
        let (mut parts, ()) = http::Request::new(()).into_parts();
        parts.method = Method::POST;
        parts.uri = self.origin.uri.clone();
        parts.headers = self.origin.headers.clone();
        parts.extensions = self.origin.extensions.clone();
        parts.extensions.insert(AuthState(self.principal.clone()));
        if let Some(access) = self.access.get() {
            parts.extensions.insert(access.clone());
        }
        parts
    }

    /// The viewer's roles, permissions and abilities (computed once per
    /// operation).
    pub(crate) async fn access(&self) -> Result<Arc<PrincipalAccess>, AppError> {
        self.access
            .get_or_try_init(|| async {
                let mut parts = self.parts();
                let CurrentAccess(access) = extract::<CurrentAccess, _>(&mut parts).await?;
                Ok(access)
            })
            .await
            .cloned()
    }

    /// The viewer's user record (navree-auth).
    pub(crate) async fn current_user(&self) -> Result<User, AppError> {
        self.user
            .get_or_try_init(|| async { extract::<CurrentUser, _>(&mut self.parts()).await })
            .await
            .map(|user| user.0.clone())
    }

    /// The workspace of the operation (navree-workspace). `intent` tells a
    /// read-only demo workspace whether the operation reads (a query or a
    /// subscription) or writes (a mutation): every operation is a `POST`.
    pub(crate) async fn workspace(&self, intent: DemoIntent) -> Result<CurrentWorkspace, AppError> {
        self.workspace
            .get_or_try_init(|| async {
                let mut parts = self.parts();
                if let Some(uow) = self.reads() {
                    parts.extensions.insert(uow);
                }
                parts.extensions.insert(intent);
                extract::<CurrentWorkspace, _>(&mut parts).await
            })
            .await
            .cloned()
    }

    /// This operation's dataloader of type `L`.
    pub(crate) fn loader<L: 'static>(&self) -> Option<&(dyn Any + Send + Sync)> {
        self.loaders.get(&TypeId::of::<L>()).map(|loader| &**loader)
    }

    /// Runs the route authorizer for a field declaring roles, permissions or
    /// an ability, with the record id of `can(.., from = "x")` read from
    /// `args`. Returns the request extensions after the check (with the
    /// record `Authorized<E, A>` reads).
    pub(crate) async fn authorize(
        &self,
        path: String,
        access: &FieldAccess,
        args: &[(&str, String)],
        uow: Option<&Uow>,
    ) -> Result<http::Extensions, AppError> {
        let authorizer = self.shared.authorizer.as_ref().ok_or_else(|| {
            AppError::internal(
                "a GraphQL field declares roles, permissions or can, but no RouteAuthorizer is \
                 registered: add AccessModule to the app",
            )
        })?;
        let mut parts = self.parts();
        match uow {
            Some(uow) => {
                parts.extensions.insert(uow.clone());
            }
            None => {
                parts.extensions.remove::<Uow>();
            }
        }
        parts.extensions.insert(FieldCheck {
            path: path.clone(),
            args: args
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.clone()))
                .collect(),
        });
        let meta = RouteMeta {
            name: Some(path),
            access: AccessSpec {
                public: false,
                authenticated: true,
                roles: access.roles.iter().map(|r| (*r).to_owned()).collect(),
                permissions: access.permissions.iter().map(|p| (*p).to_owned()).collect(),
                can: access.can.as_ref().map(|can| CanSpec {
                    action: can.action.to_owned(),
                    subject: can.subject.to_owned(),
                    from: can.from.map(str::to_owned),
                }),
            },
            method: Method::POST.to_string(),
            full_path: self.shared.options.path.clone(),
            ..RouteMeta::default()
        };
        authorizer.authorize(&mut parts, &meta).await?;
        if let Some(access) = parts.extensions.get::<Arc<PrincipalAccess>>() {
            let _ = self.access.set(access.clone());
        }
        Ok(parts.extensions)
    }
}
