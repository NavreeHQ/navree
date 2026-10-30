---
name: navree-graphql
description: "GraphQL with navree-graphql (facade feature `graphql`, opt-in): `GraphqlModule` and the `graphql` config section, `#[resolver]` impl blocks with `#[query]`, `#[mutation]` and `#[subscription]` fields, access attributes on fields (`#[public]`, `#[authenticated]`, `#[roles]`, `#[permissions]`, `#[can(.., from = \"id\")]`), `ContextExt` (`ctx.uow()`, `ctx.principal()`, `ctx.authorized`, `ctx.accessible`, `ctx.loader`), `Valid<T>` arguments, per-request dataloaders, subscriptions over WebSockets with `EventStream`, errors with `extensions.code`, GraphiQL, depth and complexity limits and `graphql:schema --check`. Use when adding a GraphQL API or field to a Navree app, writing a resolver, dataloader or subscription, testing GraphQL, or debugging a GraphQL error code, a 400/415 from /graphql, a WebSocket close code (4401, 4403, 4408) or a boot error about resolvers."
---

# GraphQL

`navree::graphql` (crate `navree-graphql`, facade feature `graphql`, **not**
in `full`) serves a GraphQL API from the app's modules on async-graphql 7.
Resolvers are providers: an `#[injectable]` struct whose impl block carries
`#[resolver]` contributes root fields, and `GraphqlSchema` merges every
module's fields at boot. When in doubt, read `crates/navree-graphql/src`
and its tests (`tests/graphql/`, `tests/docs_examples.rs`).

## Steps: add a GraphQL API

1. Dependencies. async-graphql's derives name their own crate, so depend on
   it too:

```toml
[dependencies]
navree = { git = "https://github.com/NavreeHQ/navree", features = ["graphql"] }
async-graphql = { version = "7.2", default-features = false, features = ["chrono", "uuid"] }
```

2. Add the module to the app root. It needs `ConfigModule` and `HttpModule`;
   fields with `#[roles]`/`#[permissions]`/`#[can]` need `AccessModule`;
   non-public fields need an authenticator (`AuthModule`); units of work
   need `DatabaseModule`; `EventStream` needs `EventsModule`. A missing one
   fails the boot naming the field.

```rust
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(HttpModule::from_config())
    .module(DatabaseModule::from_config())
    .module(EventsModule::for_root())
    .module(AuthModule::from_config())
    .module(AccessModule::for_root(access_options()))
    .module(GraphqlModule::from_config()) // section `graphql`
    .module(DealsModule)
```

3. Types are plain async-graphql types:

```rust
use async_graphql::{ComplexObject, Enum, InputObject, SimpleObject};
use navree::validation::Validate;

#[derive(Clone, Debug, SimpleObject)]
#[graphql(name = "Deal", complex)]
pub struct DealObject {
    pub id: Uuid,
    pub title: String,
    #[graphql(skip)]
    pub company_id: Uuid,
}

#[derive(Debug, InputObject, Validate)]
pub struct MoveDealInput {
    #[validate(skip)]
    pub stage: Stage,
    #[validate(range(min = 0))]
    pub position: Option<i64>,
}
```

4. Write the resolver. Every field declares access (deny by default: no
   declaration does not compile):

```rust
use async_graphql::Context;
use navree::graphql::{ContextExt, Result, resolver};
use navree::prelude::*;

#[injectable]
pub struct DealResolver {
    deals: Arc<DealService>,
}

#[resolver]
impl DealResolver {
    /// One deal (doc comments become descriptions).
    #[query]
    #[can(Read, Deal, from = "id")]
    async fn deal(&self, ctx: &Context<'_>, id: Uuid) -> Result<DealObject> {
        let deal = ctx.authorized::<Deal, Read>()?.into_inner();
        Ok(self.deals.view(&ctx.uow()?, deal).await?.into())
    }

    #[query]
    #[can(Read, Deal)]
    async fn deals(
        &self,
        ctx: &Context<'_>,
        #[graphql(default = 50, validator(maximum = 100))] limit: u64,
    ) -> Result<Vec<DealObject>> {
        let scope = ctx.accessible::<Deal, Read>().await?;
        let deals = self.deals.find(&ctx.uow()?, scope, limit).await?;
        Ok(deals.into_iter().map(Into::into).collect())
    }

    #[mutation]
    #[can(Move, Deal, from = "id")]
    async fn move_deal(&self, ctx: &Context<'_>, id: Uuid, input: Valid<MoveDealInput>) -> Result<DealObject> {
        let Valid(MoveDealInput { stage, position }) = input;
        let deal = ctx.authorized::<Deal, Move>()?.into_inner();
        Ok(self.deals.move_to(&ctx.uow()?, deal, stage.into(), position).await?.into())
    }
}
```

5. Register it (and loaders) in the module's `register` hook:

```rust
#[module(register = Self::wire)]
pub struct DealsModule;

impl DealsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<Deal>()) // so `can(.., from)` can load deals
            .resolver::<DealResolver>()
            .dataloader::<CompanyLoader>();
    }
}
```

## Field rules

- Signature: non-generic `async fn(&self, [ctx: &Context<'_>,] args..)`;
  arguments are plain names (`id: Uuid`) with optional `#[graphql(..)]`
  argument attributes; return a value or `Result<T>` (any error that
  converts into `async_graphql::Error`). `#[graphql(name = .., deprecation
  = .., complexity = ..)]` on the method applies to the field.
- Access on the method or on the impl block (below `#[resolver]`):
  `#[public]`, `#[authenticated]`, `#[roles("a", ..)]`,
  `#[permissions("deals.update")]`, `#[can(Action, Subject[, from = "arg"])]`
  (marker types or string literals). `from` must name an argument.
  `#[public]` plus a restriction does not compile.
- Guests on a non-public field get `UNAUTHENTICATED`; refusals `FORBIDDEN`
  (logged on `navree::security`, audited as `access.denied` with metadata
  `field`); a missing `from` record `NOT_FOUND`.
- Nested fields (`#[ComplexObject]`) are not checked: check there with
  `ctx.abilities().await?.can("read_margin", "Deal", None)`.

## ContextExt

| Call | Returns |
|---|---|
| `ctx.uow()?` | `Uow`: a mutation's own, else the operation's shared reads (commit nothing) |
| `ctx.principal()?` | `&Principal`; `UNAUTHENTICATED` for guests |
| `ctx.current_user().await?` | `navree::auth::User` (needs `AuthModule`) |
| `ctx.abilities().await?` | `Arc<Ability>` |
| `ctx.accessible::<E, A>().await?` | `Accessible<E, A>` (`.query()`, `.apply(E::query())`) |
| `ctx.authorized::<E, A>()?` | `Authorized<E, A>` of the field's `#[can(A, E, from = ..)]` |
| `ctx.workspace().await?` | `CurrentWorkspace` (from `X-Workspace`, subdomain or claim) |
| `ctx.loader::<L>()` | `&RequestLoader<L>`; panics if `L` is not registered |
| `ctx.app()?` | `&AppContext` |

## Dataloaders

```rust
#[injectable]
pub struct CompanyLoader {
    db: Arc<Db>,
}

impl Loader<Uuid> for CompanyLoader {          // navree::graphql::Loader
    type Value = CompanyObject;
    type Error = Arc<OrmError>;

    async fn load(&self, ids: &[Uuid]) -> Result<HashMap<Uuid, CompanyObject>, Self::Error> {
        let rows = Company::query()
            .where_key_in(ids.iter().copied())
            .get(&*self.db)
            .await
            .map_err(Arc::new)?;
        Ok(rows.into_iter().map(|c| (c.id, CompanyObject::from(&*c))).collect())
    }
}

#[ComplexObject]
impl DealObject {
    async fn company(&self, ctx: &Context<'_>) -> Result<Option<CompanyObject>> {
        Ok(ctx.loader::<CompanyLoader>().load_one(self.company_id).await?)
    }
}
```

One dataloader per operation (cache never shared between viewers); loads
of the same tick batch into one `load`; loaders read through the pool and
do **not** check access.

## Subscriptions

```rust
#[subscription]
#[can(Read, Deal)]
async fn deal_updated(&self, ctx: &Context<'_>) -> Result<impl Stream<Item = DealUpdatedObject>> {
    Ok(EventStream::<DealUpdated>::new(ctx)?.map(DealUpdatedObject::from))
}
```

- Endpoint `GET /graphql/ws`, protocols `graphql-transport-ws` and
  `graphql-ws`. The client sends `{"type":"connection_init","payload":
  {"authorization":"Bearer <token>"}}`.
- Close codes: `4403` token refused, `4408` no `connection_init` within
  `graphql.connection_init_timeout_secs`, `4401` the token's `exp` passed
  (reconnect with a fresh token). Pings every `graphql.keep_alive_secs`.
- Without `authorization` in the payload, the upgrade request's own
  authentication counts (an `Authorization` header); a session cookie
  counts only when `Origin` is in `http.cors.allowed_origins`.
- Access is checked when the subscription starts: a guest or a refused
  viewer gets one `next` with the error, then `complete`.
- `EventStream` sees this instance's bus; 256 events of lag max.

## Configuration (`[graphql]`, env `APP_GRAPHQL__<KEY>`)

`path` (`/graphql`, absolute), `graphiql` and `introspection` (on in debug
builds), `depth_limit` (16), `complexity_limit` (1000), `batching` (false),
`subscriptions` (true), `connection_init_timeout_secs` (10),
`keep_alive_secs` (15, 0 = off). Introspection lists only the public schema (the
per-resolver part types are removed at build, nothing starts with `__` but the
spec's own types). GraphiQL, React and React DOM are embedded and served at
`<path>/assets/` with `script-src 'self'`; no CDN. The page's CSP allows
WebSockets only to the request's own host: `connect-src 'self'
ws://<Host> wss://<Host>` (never bare `ws:`/`wss:`; a `Host` that isn't a
plain name or `name:port` leaves `'self'`), so a proxy must forward `Host`. In code:
`GraphqlModule::for_root(GraphqlOptions { depth_limit: 12, ..GraphqlOptions::default() })`.

## Errors

`extensions` carries `code` (`BAD_REQUEST`, `UNAUTHENTICATED`, `FORBIDDEN`,
`NOT_FOUND`, `CONFLICT`, `INTERNAL_SERVER_ERROR`, ...), `statusCode`,
`error` and, for `Valid<T>` failures, `errors` (`{"input.position": [..]}`).
`AppError` keeps its status, `OrmError` and `DbError` map like on routes, `m.map_error::<E>`
mappings apply, plain messages are `BAD_REQUEST`, anything else is a 500
with message `Internal server error` (logged on `navree::error`).
HTTP status: 200 when the operation ran (even with field errors); 400 for a
document that does not parse, breaks the schema or a limit (no `data`);
415 for a non-JSON body; 405 for GET (GraphiQL only to HTML clients when on).

## Testing

```rust
let app = TestApp::builder()
    .temp_database()
    .module(DatabaseModule)
    .module(AccessModule::for_root(access_options()))
    .module(GraphqlModule::from_config())
    .module(DealsModule)
    .build()
    .await;
app.acting_as(principal(rep))
    .post("/graphql")
    .json(&json!({ "query": "{ deals { title } }" }))
    .send()
    .await
    .assert_status(200)
    .assert_json_path("data.deals.0.title", json!("Acme renewal"));

// Without HTTP:
let schema = app.get::<GraphqlSchema>().await;
let response = schema.execute(async_graphql::Request::new("{ deals { title } }").data(principal(rep))).await;
assert!(schema.sdl().contains("deals("));
```

Schema file: `cargo run -- graphql:schema schema.graphql`, and in CI
`cargo run -- graphql:schema schema.graphql --check` (exit 1 with a diff).

## Checks

- `cargo test` (resolvers need no extra setup in `TestApp`: it installs a
  test authenticator when the app has none).
- `cargo run -- graphql:schema schema.graphql --check` after schema changes.
- `cargo clippy --all-targets -- -D warnings`.

## Gotchas

- Forgetting `features = ["graphql"]`: `navree::graphql` does not exist.
- `async-graphql` must be a direct dependency for `SimpleObject` and friends.
- Two resolvers with the same root field name fail the boot, naming both.
- `#[can(.., from = "id")]` needs `AccessModule::for_feature::<E>()` imported
  by some module, or the check is a 500 ("no record loader for subject").
- `Valid<T>` works on `#[resolver]` field arguments only, not on
  `#[ComplexObject]` arguments. Error keys use Rust field names
  (`input.lost_reason`).
- In async-graphql, a field that errors is left out of `data` (a query whose
  only field fails has `"data": null`).
- `ctx.uow()` in a query is shared by the operation's query fields and rolled
  back at the end: write only in mutations.
- Subscriptions with `graphql.subscriptions = false` 404 on `/graphql/ws`.
