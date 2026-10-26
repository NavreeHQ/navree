//! [`GraphqlSchema`]: the app's schema, built at boot from every
//! registered resolver, and how operations execute against it.

use std::any::TypeId;
use std::collections::HashMap;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use async_graphql::futures_util::stream::{self, BoxStream, StreamExt};
use async_graphql::registry::Registry;
use async_graphql::{BatchRequest, BatchResponse, Data, Executor, Request, Response, Schema};
use navree_contracts::Principal;
use navree_db::Db;
use navree_events::EventBus;
use navree_http::{Authenticator, ErrorMapper, RouteAuthorizer};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, ModuleId, Resolver, WeakAppContext,
};
use navree_validation::Messages;

use crate::__private::{FieldKind, FieldSpec, ResolverParts};
use crate::error::render_response;
use crate::loader::{LoaderEntry, LoaderRegistration};
use crate::operation::{Operation, Origin};
use crate::options::GraphqlOptions;
use crate::root::{
    Building, ErasedObject, ErasedSubscription, Parts, RootMutation, RootQuery, RootSubscription,
    field_names, with_building,
};

/// A type whose impl block carries `#[resolver]`: its `#[query]`,
/// `#[mutation]` and `#[subscription]` methods become root fields of the
/// app's schema. Implemented by the `#[resolver]` macro (not by hand);
/// register the type with
/// [`ModuleBuilderGraphqlExt::resolver`](crate::ModuleBuilderGraphqlExt::resolver).
pub trait GraphqlResolver: Send + Sync + 'static {
    /// The objects holding the resolver's fields (generated).
    #[doc(hidden)]
    fn parts(self: Arc<Self>) -> ResolverParts;
}

type ResolveParts =
    Arc<dyn Fn(Resolver) -> BoxFuture<'static, Result<ResolverParts, KernelError>> + Send + Sync>;

/// A resolver registered by a module (a contribution).
#[derive(Clone)]
pub(crate) struct ResolverRegistration {
    pub(crate) module: ModuleId,
    pub(crate) type_name: &'static str,
    pub(crate) resolve: ResolveParts,
}

impl ResolverRegistration {
    pub(crate) fn of<R: GraphqlResolver + Injectable>(module: ModuleId) -> Self {
        Self {
            module,
            type_name: std::any::type_name::<R>(),
            resolve: Arc::new(|r: Resolver| {
                Box::pin(async move { Ok(r.get::<R>().await?.parts()) })
            }),
        }
    }
}

/// What every operation of the schema shares.
pub(crate) struct Shared {
    pub(crate) app: WeakAppContext,
    pub(crate) options: Arc<GraphqlOptions>,
    pub(crate) db: Option<Arc<Db>>,
    pub(crate) events: Option<Arc<EventBus>>,
    pub(crate) authorizer: Option<Arc<dyn RouteAuthorizer>>,
    pub(crate) authenticator: Option<Arc<dyn Authenticator>>,
    pub(crate) messages: Arc<Messages>,
    pub(crate) mappers: Vec<ErrorMapper>,
    pub(crate) loaders: Vec<LoaderEntry>,
    pub(crate) allowed_origins: Vec<String>,
}

type Inner = Schema<RootQuery, RootMutation, RootSubscription>;

/// The app's GraphQL schema: one `Query`, `Mutation` and `Subscription`
/// type holding the root fields of every registered resolver, built when
/// the app boots. Provided and exported by `GraphqlModule`.
///
/// The HTTP endpoint executes operations through it; tests and other
/// transports can too. `execute` reads the viewer from the request data
/// (`request.data(principal)`), runs the operation and renders its errors
/// as the endpoint does.
///
/// ```ignore
/// let schema = app.get::<GraphqlSchema>().await;
/// let response = schema
///     .execute(async_graphql::Request::new("{ deals { title } }").data(principal(rep)))
///     .await;
/// assert!(response.errors.is_empty());
/// ```
#[derive(Clone)]
pub struct GraphqlSchema(Arc<SchemaInner>);

struct SchemaInner {
    schema: Inner,
    shared: Arc<Shared>,
}

/// Who registered a part, for boot errors.
#[derive(Clone)]
struct Owner {
    resolver: &'static str,
    module: &'static str,
}

impl fmt::Display for Owner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` (module `{}`)", short(self.resolver), self.module)
    }
}

/// `crate::deals::DealResolver` → `DealResolver`.
fn short(type_name: &str) -> &str {
    type_name.rsplit("::").next().unwrap_or(type_name)
}

impl GraphqlSchema {
    /// Builds the schema from the resolvers and dataloaders the app's
    /// modules registered. Fails when two resolvers define the same root
    /// field, when a field needs authentication or authorization the app
    /// cannot provide, or when two types share a GraphQL name.
    pub(crate) async fn build(
        app: AppContext,
        options: Arc<GraphqlOptions>,
    ) -> Result<Self, BoxError> {
        let authorizer = app.try_get::<dyn RouteAuthorizer>().await;
        let authenticator = app.try_get::<dyn Authenticator>().await;

        let mut query: Vec<(Arc<dyn ErasedObject>, Owner)> = Vec::new();
        let mut mutation: Vec<(Arc<dyn ErasedObject>, Owner)> = Vec::new();
        let mut subscription: Vec<(Arc<dyn ErasedSubscription>, Owner)> = Vec::new();
        for registration in app.contributions::<ResolverRegistration>() {
            let owner = Owner {
                resolver: registration.value.type_name,
                module: registration.module.name,
            };
            let parts =
                (registration.value.resolve)(app.resolver_for(registration.value.module)).await?;
            check_fields(
                &parts.fields,
                &owner,
                authorizer.is_some(),
                authenticator.is_some(),
            )?;
            if let Some(part) = parts.query {
                query.push((part, owner.clone()));
            }
            if let Some(part) = parts.mutation {
                mutation.push((part, owner.clone()));
            }
            if let Some(part) = parts.subscription {
                subscription.push((part, owner));
            }
        }

        let query_index = index(&query, FieldKind::Query, |part, registry| {
            part.meta(registry)
        })?;
        let mutation_index = index(&mutation, FieldKind::Mutation, |part, registry| {
            part.meta(registry)
        })?;
        let subscription_index =
            index(&subscription, FieldKind::Subscription, |part, registry| {
                part.meta(registry)
            })?;
        let building = Building {
            query: query.iter().map(|(part, _)| part.clone()).collect(),
            mutation: mutation.iter().map(|(part, _)| part.clone()).collect(),
            subscription: subscription.iter().map(|(part, _)| part.clone()).collect(),
        };
        let roots = (
            RootQuery(Arc::new(Parts::new(building.query.clone(), query_index))),
            RootMutation(Arc::new(Parts::new(
                building.mutation.clone(),
                mutation_index,
            ))),
            RootSubscription(Arc::new(Parts::new(
                building.subscription.clone(),
                subscription_index,
            ))),
        );
        let limits = (options.depth_limit, options.complexity_limit);
        let introspection = options.introspection;
        let schema = guarded(|| {
            with_building(building, || {
                let mut builder = Schema::build(roots.0, roots.1, roots.2)
                    .limit_depth(limits.0)
                    .limit_complexity(limits.1);
                if !introspection {
                    builder = builder.disable_introspection();
                }
                builder.finish()
            })
        })?;

        let mut loaders = Vec::new();
        for registration in app.contributions::<LoaderRegistration>() {
            let entry =
                (registration.value.resolve)(app.resolver_for(registration.value.module)).await?;
            if !loaders
                .iter()
                .any(|known: &LoaderEntry| known.type_id == entry.type_id)
            {
                loaders.push(entry);
            }
        }
        let allowed_origins = match app.try_get::<navree_http::HttpOptions>().await {
            Some(http) => http.cors.allowed_origins.clone(),
            None => Vec::new(),
        };
        let shared = Shared {
            app: app.downgrade(),
            options,
            db: app.try_get::<Db>().await,
            events: app.try_get::<EventBus>().await,
            authorizer,
            authenticator,
            messages: app
                .try_get::<Messages>()
                .await
                .unwrap_or_else(|| Arc::new(Messages::default())),
            mappers: app
                .contributions::<ErrorMapper>()
                .into_iter()
                .map(|mapper| (*mapper.value).clone())
                .collect(),
            loaders,
            allowed_origins,
        };
        Ok(Self(Arc::new(SchemaInner {
            schema,
            shared: Arc::new(shared),
        })))
    }

    /// The schema as SDL (what `graphql:schema` writes).
    pub fn sdl(&self) -> String {
        self.0.schema.sdl()
    }

    /// The options the endpoint is served with.
    pub fn options(&self) -> &GraphqlOptions {
        &self.0.shared.options
    }

    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.0.shared
    }

    /// Runs one operation. The viewer is the `Principal` in the request
    /// data (a guest without one); errors carry Navree's extensions.
    pub async fn execute(&self, request: impl Into<Request>) -> Response {
        let request = request.into();
        let origin = Origin {
            principal: data::<Principal>(&request.data).cloned(),
            ..Origin::default()
        };
        self.execute_from(request, origin).await
    }

    /// Runs a batch of operations, one after the other.
    pub async fn execute_batch(&self, batch: BatchRequest) -> BatchResponse {
        match batch {
            BatchRequest::Single(request) => BatchResponse::Single(self.execute(request).await),
            BatchRequest::Batch(requests) => {
                let mut responses = Vec::with_capacity(requests.len());
                for request in requests {
                    responses.push(self.execute(request).await);
                }
                BatchResponse::Batch(responses)
            }
        }
    }

    /// Runs one operation sent by `origin` (an HTTP request).
    pub(crate) async fn execute_from(&self, request: Request, origin: Origin) -> Response {
        let Some(app) = self.0.shared.app.upgrade() else {
            return Response::from_errors(vec![crate::error::request_error(
                "the app is shutting down",
            )]);
        };
        let op = Arc::new(Operation::start(self.0.shared.clone(), app, origin).await);
        let request = request.data(op.clone());
        let mut response = self.0.schema.execute(request).await;
        op.finish().await;
        render_response(&mut response, &self.0.shared.mappers, &op.request_id);
        response
    }

    /// Runs a subscription (or a single query or mutation) as a stream of
    /// responses. The viewer is the `Principal` in the request data.
    pub fn execute_stream(&self, request: impl Into<Request>) -> BoxStream<'static, Response> {
        let request = request.into();
        let origin = Origin {
            principal: data::<Principal>(&request.data).cloned(),
            ..Origin::default()
        };
        self.stream_from(request, origin, Arc::new(Data::default()))
    }

    /// Runs a subscription sent by `origin` (a WebSocket connection).
    pub(crate) fn stream_from(
        &self,
        request: Request,
        origin: Origin,
        session: Arc<Data>,
    ) -> BoxStream<'static, Response> {
        let this = self.clone();
        let setup = async move {
            let Some(app) = this.0.shared.app.upgrade() else {
                let error = crate::error::request_error("the app is shutting down");
                return stream::once(async move { Response::from_errors(vec![error]) }).boxed();
            };
            let op = Arc::new(Operation::start(this.0.shared.clone(), app, origin).await);
            let request = request.data(op.clone());
            let responses = this
                .0
                .schema
                .execute_stream_with_session_data(request, session);
            let mappers = this.0.shared.mappers.clone();
            let finish = op.clone();
            responses
                .then(move |mut response| {
                    let op = op.clone();
                    let mappers = mappers.clone();
                    async move {
                        render_response(&mut response, &mappers, &op.request_id);
                        op.release_reads().await;
                        response
                    }
                })
                .chain(
                    stream::once(async move {
                        finish.finish().await;
                        None
                    })
                    .filter_map(|nothing: Option<Response>| async move { nothing }),
                )
                .boxed()
        };
        stream::once(setup).flatten().boxed()
    }
}

impl fmt::Debug for GraphqlSchema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GraphqlSchema")
            .field("path", &self.0.shared.options.path)
            .finish_non_exhaustive()
    }
}

/// The WebSocket connection a subscription runs on (session data).
#[derive(Clone)]
pub(crate) struct Connection(pub(crate) Origin);

/// The viewer of a WebSocket connection, set by `connection_init`.
#[derive(Clone)]
pub(crate) struct ConnectionViewer(pub(crate) Option<Principal>);

impl Executor for GraphqlSchema {
    async fn execute(&self, request: Request) -> Response {
        GraphqlSchema::execute(self, request).await
    }

    fn execute_stream(
        &self,
        request: Request,
        session_data: Option<Arc<Data>>,
    ) -> BoxStream<'static, Response> {
        let session = session_data.unwrap_or_default();
        let mut origin = data::<Connection>(&session)
            .map(|connection| connection.0.clone())
            .unwrap_or_default();
        if let Some(viewer) = data::<ConnectionViewer>(&session) {
            origin.principal = viewer.0.clone();
        }
        self.stream_from(request, origin, session)
    }
}

/// A value of type `T` in async-graphql `Data`.
fn data<T: Send + Sync + 'static>(data: &Data) -> Option<&T> {
    data.get(&TypeId::of::<T>())
        .and_then(|value| value.downcast_ref::<T>())
}

/// Checks that the app can serve each field's access declaration.
fn check_fields(
    fields: &[FieldSpec],
    owner: &Owner,
    has_authorizer: bool,
    has_authenticator: bool,
) -> Result<(), BoxError> {
    for field in fields {
        if field.access.needs_authorizer() && !has_authorizer {
            return Err(format!(
                "GraphQL {} `{}` of {owner} declares roles, permissions or can, but no \
                 RouteAuthorizer is registered: add AccessModule to the app",
                field.kind.as_str(),
                field.name
            )
            .into());
        }
        if !field.access.public && !has_authenticator {
            return Err(format!(
                "GraphQL {} `{}` of {owner} needs a signed-in user, but no Authenticator is \
                 registered: add AuthModule to the app (or make the field #[public])",
                field.kind.as_str(),
                field.name
            )
            .into());
        }
    }
    Ok(())
}

/// Which part defines each root field of `kind`; two parts defining the
/// same field fail.
fn index<P: ?Sized>(
    parts: &[(Arc<P>, Owner)],
    kind: FieldKind,
    meta: impl Fn(&P, &mut Registry) -> async_graphql::registry::MetaType,
) -> Result<HashMap<String, usize>, BoxError> {
    let mut index: HashMap<String, usize> = HashMap::new();
    for (i, (part, owner)) in parts.iter().enumerate() {
        let names = guarded(|| {
            let mut registry = Registry::default();
            field_names(&meta(part, &mut registry))
        })?;
        for name in names {
            if let Some(&first) = index.get(&name) {
                return Err(format!(
                    "two resolvers define the GraphQL {} field `{name}`: {} and {}",
                    kind.as_str(),
                    parts[first].1,
                    owner
                )
                .into());
            }
            index.insert(name, i);
        }
    }
    Ok(index)
}

/// Runs `f`, turning a panic (async-graphql panics on two types with the
/// same GraphQL name) into an error.
fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, BoxError> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|panic| {
        let message = panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_else(|| "building the schema panicked".to_owned());
        format!("invalid GraphQL schema: {message}").into()
    })
}
