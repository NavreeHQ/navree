//! [`ContextExt`]: Navree's request state from an async-graphql [`Context`].

use std::future::Future;
use std::sync::Arc;

use async_graphql::parser::types::OperationType;
use async_graphql::{Context, Error};
use futures_util::FutureExt;
use navree_access::{Ability, Accessible, ActionMarker, Authorized, Subject, SubjectRecords};
use navree_auth::User;
use navree_contracts::Principal;
use navree_db::Uow;
use navree_http::AppError;
use navree_kernel::AppContext;
use navree_workspace::{CurrentWorkspace, DemoIntent};

use crate::__private::FIELD;
use crate::loader::RequestLoader;
use crate::operation::{Operation, extract};

/// The state of the operation `ctx` belongs to.
pub(crate) fn operation<'a>(ctx: &Context<'a>) -> Result<&'a Arc<Operation>, Error> {
    ctx.data_opt::<Arc<Operation>>().ok_or_else(|| {
        AppError::internal(
            "the GraphQL operation runs outside navree-graphql: execute it with GraphqlSchema",
        )
        .into()
    })
}

/// Navree's request state inside a resolver: `use navree::graphql::ContextExt`
/// and call these on the `ctx: &Context<'_>` of a field.
///
/// | Method | Gives you |
/// |---|---|
/// | [`uow`](ContextExt::uow) | the field's unit of work |
/// | [`principal`](ContextExt::principal) | the viewer; `UNAUTHENTICATED` for guests |
/// | [`current_user`](ContextExt::current_user) | the viewer's user record |
/// | [`abilities`](ContextExt::abilities) | the viewer's abilities |
/// | [`accessible`](ContextExt::accessible) | a SQL filter for the records the viewer may act on |
/// | [`authorized`](ContextExt::authorized) | the record a `#[can(.., from = "arg")]` check loaded |
/// | [`workspace`](ContextExt::workspace) | the current workspace |
/// | [`loader`](ContextExt::loader) | this operation's dataloader of a type |
/// | [`app`](ContextExt::app) | the `AppContext` |
pub trait ContextExt {
    /// The `AppContext`, to resolve any provider.
    fn app(&self) -> Result<&AppContext, Error>;

    /// The field's unit of work: a mutation's own (committed when the
    /// mutation returns `Ok`), else the unit of work the operation's query
    /// fields share (it commits nothing). An error without `DatabaseModule`.
    fn uow(&self) -> Result<Uow, Error>;

    /// The authenticated viewer; an `UNAUTHENTICATED` error for guests.
    fn principal(&self) -> Result<&Principal, Error>;

    /// The viewer's user record (navree-auth's `AuthModule`);
    /// `UNAUTHENTICATED` for guests and deleted users.
    fn current_user(&self) -> impl Future<Output = Result<User, Error>> + Send;

    /// The viewer's abilities (a guest's for guests).
    fn abilities(&self) -> impl Future<Output = Result<Arc<Ability>, Error>> + Send;

    /// A filter for the records of `E` the viewer may `A`
    /// (with navree-orm: `Deal::query().accessible(&ability, Read)`).
    fn accessible<E, A>(&self) -> impl Future<Output = Result<Accessible<E, A>, Error>> + Send
    where
        E: Subject,
        A: ActionMarker;

    /// The record the field's `#[can(A, E, from = "arg")]` check loaded; an
    /// internal error when the field declares no such check.
    fn authorized<E, A>(&self) -> Result<Authorized<E, A>, Error>
    where
        E: SubjectRecords,
        A: ActionMarker;

    /// The current workspace (navree-workspace's `WorkspaceModule`), resolved
    /// from the request (the `X-Workspace` header, a subdomain or a token
    /// claim) and checked like the `CurrentWorkspace` extractor.
    fn workspace(&self) -> impl Future<Output = Result<CurrentWorkspace, Error>> + Send;

    /// This operation's dataloader of type `L`, registered with
    /// `m.dataloader::<L>()`.
    ///
    /// # Panics
    ///
    /// When `L` is not registered (a programming error).
    fn loader<L: Send + Sync + 'static>(&self) -> &RequestLoader<L>;
}

impl ContextExt for Context<'_> {
    fn app(&self) -> Result<&AppContext, Error> {
        Ok(&operation(self)?.app)
    }

    fn uow(&self) -> Result<Uow, Error> {
        if let Ok(Some(uow)) = FIELD.try_with(|field| field.uow.clone()) {
            return Ok(uow);
        }
        operation(self)?
            .reads()
            .ok_or_else(|| AppError::internal("ctx.uow() needs DatabaseModule in the app").into())
    }

    fn principal(&self) -> Result<&Principal, Error> {
        operation(self)?
            .principal
            .as_ref()
            .ok_or_else(|| AppError::unauthenticated().into())
    }

    async fn current_user(&self) -> Result<User, Error> {
        Ok(operation(self)?.current_user().await?)
    }

    async fn abilities(&self) -> Result<Arc<Ability>, Error> {
        Ok(operation(self)?.access().await?.ability.clone())
    }

    async fn accessible<E, A>(&self) -> Result<Accessible<E, A>, Error>
    where
        E: Subject,
        A: ActionMarker,
    {
        let access = operation(self)?.access().await?;
        Ok(Accessible::new(access.ability.clone()))
    }

    fn authorized<E, A>(&self) -> Result<Authorized<E, A>, Error>
    where
        E: SubjectRecords,
        A: ActionMarker,
    {
        let missing = || -> Error {
            AppError::internal(format!(
                "ctx.authorized::<{}, {}>() needs the field to declare #[can({}, {}, from = ..)]",
                E::SUBJECT,
                A::NAME,
                A::NAME,
                E::SUBJECT
            ))
            .into()
        };
        let extensions = FIELD
            .try_with(|field| field.authorized.clone())
            .ok()
            .flatten()
            .ok_or_else(missing)?;
        let (mut parts, ()) = http::Request::new(()).into_parts();
        parts.extensions = (*extensions).clone();
        // The extractor only reads the extensions: it is ready at once.
        match extract::<Authorized<E, A>>(&mut parts).now_or_never() {
            Some(Ok(authorized)) => Ok(authorized),
            Some(Err(error)) => Err(error.into()),
            None => Err(missing()),
        }
    }

    async fn workspace(&self) -> Result<CurrentWorkspace, Error> {
        // Queries and subscriptions read a demo workspace; mutations write.
        let intent = match self.query_env.operation.node.ty {
            OperationType::Mutation => DemoIntent::Write,
            OperationType::Query | OperationType::Subscription => DemoIntent::Read,
        };
        Ok(operation(self)?.workspace(intent).await?)
    }

    fn loader<L: Send + Sync + 'static>(&self) -> &RequestLoader<L> {
        let loader = operation(self)
            .ok()
            .and_then(|op| op.loader::<L>())
            .and_then(|loader| loader.downcast_ref::<RequestLoader<L>>());
        match loader {
            Some(loader) => loader,
            None => panic!(
                "no dataloader {} in this operation: register it with m.dataloader::<{}>()",
                std::any::type_name::<L>(),
                std::any::type_name::<L>()
            ),
        }
    }
}
