---
name: navree-controllers-and-routing
description: "Adds HTTP endpoints to a Navree app: `#[controller]` and route attributes, the `/api` prefix and versions, path and query parameters, extractors, response types, access attributes, throttles, guards, hand-written routes, custom extractors and route tests. Use when adding an endpoint, route, controller or route parameter, reading headers or the client IP, or debugging a 401, 403, 404, 405 or 415 on a route."
---

# Controllers and routing

Routes live in controllers: a struct marked `#[injectable]` whose `impl`
block is marked `#[controller("/prefix", version = "1")]`. Each `async fn
(&self, ...)` with `#[get]`, `#[post]`, `#[put]`, `#[patch]` or `#[delete]`
is a route. Controllers are singleton providers, so their fields are
injected services.

## Steps

1. In an existing module: `navree make:controller posts admin` writes
   `src/modules/posts/controllers/admin.rs` and registers it at the
   module's `// navree:wire` marker. For a whole resource, use
   `navree make:module posts --crud` instead (five routes, DTOs, tests).
   By hand: add the type to the module's `controllers = [..]`.
2. Give every route an access attribute (deny by default; see below).
3. Take input through extractors; put the body extractor last.
4. Return `Result<T>` where `T` is `Json<_>`, `Created<_>`, `NoContent`,
   `Page<_>` or any `IntoResponse`.
5. Test it with `TestApp` (template at the end); check `navree routes`.

## Template

```rust,ignore
use std::sync::Arc;

use navree::access::{Accessible, Authorized, Create, Delete, Read, Update};
use navree::db::{Page, PageQuery, Uow};
use navree::http::{Created, Json, NoContent, Result};
use navree::orm::Record;
use navree::validation::Valid;
use navree::{controller, injectable};

use super::dto::{CreatePost, UpdatePost};
use super::entity::Post;
use super::service::PostService;

#[injectable]
pub struct PostController {
    posts: Arc<PostService>,
}

#[controller("/posts", version = "1")]           // routes start with /api/v1/posts
impl PostController {
    #[get("/")]
    #[can(Read, Post)]
    async fn index(&self, uow: Uow, scope: Accessible<Post, Read>, page: PageQuery) -> Result<Page<Record<Post>>> {
        Ok(self.posts.list(&uow, scope, page).await?)
    }

    #[get("/{id}")]
    #[can(Read, Post, from = "id")]              // loads the record: 404 if missing, 403 if not allowed
    async fn show(&self, post: Authorized<Post, Read>) -> Json<Record<Post>> {
        Json(post.into_inner())
    }

    #[post("/")]
    #[can(Create, Post)]
    #[throttle(limit = 30, per = "1m")]
    async fn store(&self, uow: Uow, Valid(Json(input)): Valid<Json<CreatePost>>) -> Result<Created<Record<Post>>> {
        let post = self.posts.create(&uow, input).await?;
        Ok(Created(format!("/api/v1/posts/{}", post.id), post)) // 201 + Location
    }

    #[patch("/{id}")]
    #[can(Update, Post, from = "id")]
    async fn update(
        &self,
        uow: Uow,
        post: Authorized<Post, Update>,
        Valid(Json(input)): Valid<Json<UpdatePost>>,
    ) -> Result<Json<Record<Post>>> {
        post.check_fields(&input.changed_fields())?; // 403 for fields the rules don't allow
        Ok(Json(self.posts.update(&uow, post.into_inner(), input).await?))
    }

    #[delete("/{id}")]
    #[can(Delete, Post, from = "id")]
    async fn destroy(&self, uow: Uow, post: Authorized<Post, Delete>) -> Result<NoContent> {
        self.posts.delete(&uow, post.into_inner()).await?;
        Ok(NoContent)                            // 204
    }
}
```

## Paths

- Full path = `http.api_prefix` (`/api`) + `/v{version}` + controller
  prefix + route path. Without `version`, there is no `/vN` segment.
  `#[controller]` with no arguments has an empty prefix.
- `{id}` captures a segment, `{*rest}` the rest of the path; a parameter
  is a whole segment (`:id` and `/v{n}` are compile errors). The
  controller prefix may hold parameters too:
  `#[controller("/deals/{deal_id}/notes")]`.
- A path starting with `!` ignores the prefix, version and controller
  prefix: `#[get("!/up")]` is `GET /up` (webhooks, probes, and the pages
  of Navree UI: `#[get("!/contacts")]` returning an `AppPage`; see
  `navree-ui`). `#[page]` and `#[json_only]` say whether a route's errors
  are pages or JSON (`navree-errors-and-responses`).
- Paths match exactly, never rewritten: `GET /api/v1/posts/` (trailing
  slash) and `//` are 404s; a `.`/`..` segment (also `%2e`) or a backslash
  is a 400; a `{id}` value with `%2F`, `%5C` or `%00`, or not UTF-8 once
  decoded, is a 400 before any guard runs. Static segments beat `{param}`,
  which beats `{*rest}`; two routes of one method differing only in
  parameter names fail the boot. A known path with another method is a 405
  with `Allow`; `OPTIONS` gets 204 with `Allow`; `HEAD` runs the `GET` route.
- `#[name("posts.show")]` names a route (shown by `navree routes`, used as
  the OpenAPI `operationId`).

## Access attributes (required)

| Attribute | Who may call |
|---|---|
| `#[public]` | anyone; can't be combined with another access attribute |
| `#[authenticated]` | any signed-in user |
| `#[roles("admin", "manager")]` | a user with any of the roles |
| `#[permissions("reports.export")]` | a user with every listed permission |
| `#[can(Update, Post)]` / `#[can(Update, Post, from = "id")]` | abilities allow the action (on the record) |

Put them on a route or on the `impl` block (then they apply to every
route; roles and permissions add up, different kinds are ANDed). A guest
gets 401, a signed-in user without the grant 403. Roles, permissions and
`#[can]` need `AccessModule` in the app; non-public routes need an
authenticator (`AuthModule`), or the boot fails. Rules themselves:
`navree-access-control` skill.

## Extractors

| Extractor | Gives |
|---|---|
| `Path(id): Path<Uuid>`, `Path((a, b)): Path<(Uuid, u32)>` | path parameters (bad value: 400) |
| `Query(f): Query<T>`, `Valid(Query(f)): Valid<Query<T>>` | the query string (`Option` or serde defaults for optional keys) |
| `Json(b): Json<T>`, `Valid(Json(b)): Valid<Json<T>>` | a JSON body (`Content-Type: application/json` or `+json`, else 415) |
| `Form(f): Form<T>` (`navree::http`) | an urlencoded form (the query string for `GET`) |
| `uow: Uow` | the request's unit of work (pass `&uow` to services) |
| `CurrentPrincipal(me)` (`navree::http`) | the signed-in user's id (`me.user_id`), no query |
| `CurrentUser(user)` (prelude) | the `users` row, loaded once per request |
| `page: PageQuery`, `cursor: CursorQuery` | `?page=&per_page=`, `?cursor=&limit=` |
| `Authorized<E, A>`, `Accessible<E, A>`, `Abilities(ability)` | the checked record, the SQL list filter, the user's rules |
| `ClientIp(ip)` (`navree::http`) | the client address (honours `http.trusted_proxies`) |
| `Extension(id): Extension<RequestId>` | the request id (`navree::http::Extension`, `navree::contracts::RequestId`) |
| `headers: HeaderMap` (`http::HeaderMap`) | every header |
| `String`, `navree::http::body::Bytes`, `navree::http::extract::Multipart` | raw bodies, uploads; `Multipart` streams each value as it arrives, has limits on parts, fields, files and part headers (`http.multipart`, `form.with_limits(..)`), prefers `filename*` (RFC 8187) to `filename`, and answers 400 to a part with two `Content-Disposition`/`Content-Type` headers, a repeated parameter, a `filename` that isn't UTF-8, a head with a bare line feed or bytes after its header lines, or a `Content-Disposition` two readers could take differently; a delimiter counts only at the start of the body or of a line |
| `navree::http::ws::WebSocketUpgrade` | a WebSocket: return `upgrade.on_upgrade(handler)`; frames and messages stop at 1 MiB unless you set `max_frame_size` and `max_message_size`; a handler that trusts a cookie checks `upgrade.origin_allowed(&allowed)` |
| `Scoped<T>` | a request-scoped provider |

Only one extractor may read the body, and it must be the **last**
argument; otherwise the handler doesn't implement `Handler` (a confusing
compile error).

## Other route attributes

`#[throttle(limit = 10, per = "1m")]` (per user, per IP for guests; route
or impl), `#[timeout("5s")]` (route only), `#[guard(Arc::new(MyGuard))]`
(an `Arc<impl navree::http::Guard>`, runs before authorization; does not
count as an access declaration), `#[audit(action = "posts.exported")]`
(writes an audit entry after a 2xx), `#[api(summary = "..", tag = "..",
request = T, response = T, status = 201, responses(400, 404))]` (OpenAPI;
see `navree-openapi`), `#[page]` (route or impl: its errors are pages
in an app with Navree UI) and `#[json_only]` (route or impl: its errors
are always JSON). `#[no_compression]` (route) leaves its responses
uncompressed, as does a response with `Cache-Control: no-transform` and
`http.compression = false`; the UI toolkit's brotli for pages and ops answers
follows the same three. Durations: `"500ms"`, `"5s"`, `"1m"`, `"1h"`.

## Responses

- `Json(value)`: 200 JSON. `Created(location, value)`: 201 + `Location`.
  `NoContent`: 204. `(StatusCode::ACCEPTED, Json(v))`: any status.
- `Page<T>` serializes as `{ "data": [..], "meta": { current_page, per_page, total, last_page } }`.
- Headers or files: return `navree::http::response::Response` built with
  `([(CONTENT_TYPE, "text/csv"), (CONTENT_DISPOSITION, "attachment; filename=\"x.csv\"")], body).into_response()`.
  Don't return `impl IntoResponse` from a controller method (the `&self`
  borrow is captured); return `Response`.
- Errors: `Err(AppError::not_found())` etc.; see `navree-errors-and-responses`.

## A custom extractor

```rust
use http::request::Parts;
use navree::http::extract::FromRequestParts;
use navree::prelude::*;

/// The `X-Tenant` header, required.
pub struct Tenant(pub String);

impl FromRequestParts for Tenant {
    type Rejection = AppError; // rendered in Navree's error format

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get("x-tenant")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .map(|value| Tenant(value.to_owned()))
            .ok_or_else(|| AppError::bad_request("The X-Tenant header is required."))
    }
}
```

To resolve a service inside an extractor, read the `AppContext` from
`parts.extensions.get::<AppContext>()` and call `ctx.get::<T>().await`;
the route's decoded path parameters are `parts.extensions.get::<PathParams>()`.
The trait takes no state parameter: the app is in the request's extensions,
as above.

## Hand-written routes

Implement `navree::http::Controller` with `RouteDef::on(Method::POST, "/", handler)`
when a route needs something attributes can't express: a per-route body
limit (`.body_limit(20 * 1024 * 1024)`) or a Tower layer on one route.
Declare access with `.public()`, `.authenticated()`, `.roles(&[..])`,
`.permissions(&[..])` or `.can("update", "Post", Some("id"))`; the app
refuses to boot with an undeclared hand-written route.

## A service under a path

A tower service another crate gives answers under a path prefix with
`m.mount(Mount::new(prefix, service))`, behind the same pipeline as a route.
Its access declaration is required, as a route's:

```rust
use navree::plugin::prelude::*; // brings `m.mount`

/// Another crate's ledger service under `/khata`, for signed-in users.
pub struct LedgerModule;

impl Module for LedgerModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let khata = navree::http::AppRouter::from_handler(|| async { "the khata" });
        m.mount(navree::http::Mount::new("/khata", khata).authenticated());
    }
}
```

- The prefix is absolute (not under `/api`); the service answers it and
  every path below, for every method unless `.methods(&[..])` names some.
- The service sees `/` plus the rest of the path as sent (`/khata/a%2Fb` is
  `/a%2Fb`), nothing decoded, the query kept. `//` below the prefix is a 400.
- Boot fails for a mount without access, a prefix under `/_` or not a plain
  path, and a route or another mount at, under or above the prefix.
- A request with `Upgrade` or `CONNECT` never reaches it: the mount answers
  501 after the access check. With `.methods(..)`, a method left out
  (`CONNECT` among them) is the router's 405 first, as for a route; `HEAD`
  runs the `GET` answer when `GET` is listed, `OPTIONS` is a 204 with
  `Allow`.
- A 4xx the service writes goes out as written; a 5xx with no body or a
  short plain-text one takes the app's error shape, its text logged, never
  sent.
- Details: https://navree.com/docs/plugins/existing-crates/

## A layer whose service can fail

`router.layer(..)` and `Endpoint::layer(..)` take layers whose service
can't fail. Wrap one whose service can (a timeout, a load shedder) in
`FallibleLayer`: its error answers as a 500 `AppError` with the error as its
source, which `m.map_error` can turn into another answer in any slot but
`LayerSlot::Edge` (outside error rendering: there it is neither mapped nor
logged).

```rust,ignore
use std::time::Duration;

use navree::http::{FallibleLayer, GlobalLayer, LayerSlot};

m.http_layer(GlobalLayer::new(LayerSlot::Context, "timeout", |_ctx, router| {
    Ok(router.layer(FallibleLayer::new(tower::timeout::TimeoutLayer::new(
        Duration::from_secs(10),
    ))))
}))
.map_error::<tower::timeout::error::Elapsed>(|_| {
    Some(AppError::service_unavailable("The khata took too long."))
});
```

Only the app adds a layer that runs on every request. A module of a plugin
that is not one of the framework's own packages stops the boot when it calls
`m.http_layer`: the plugin ships the layer as a function (`khata_timeout()`)
and the app adds it with `m.http_layer(khata_timeout())`. A service with the
message module and no HTTP module refuses it the same way. A plugin's own
routes and mounts may carry layers of their own. A plugin maps only its own
crate's error types with `m.map_error`: the mapping of a wrapped crate's
error, such as `Elapsed` above, is the app's to register.

Details: https://navree.com/docs/plugins/existing-crates/

## Test

```rust,ignore
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use serde_json::{Value, json};
use uuid::Uuid;

use super::PostsModule;

#[tokio::test]
async fn guests_must_sign_in_and_users_create_posts() {
    let app = TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(DatabaseModule)
        .module(AccessModule::for_root(AccessOptions {
            abilities: crate::abilities::define,
            super_admin_role: None,
        }))
        .module(PostsModule)
        .build()
        .await;
    app.client()
        .post("/api/v1/posts")
        .json(&json!({ "name": "x" }))
        .send()
        .await
        .assert_error(401, "Unauthorized");
    let created = app
        .acting_as(principal(Uuid::now_v7()))
        .post("/api/v1/posts")
        .json(&json!({ "name": "First" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    assert!(created["id"].is_string());
    app.shutdown().await.unwrap();
}
```

## Checks

`navree routes` (method, path, name, access, module; `--json` for more),
`cargo test`, `cargo clippy --all-targets -- -D warnings`.

## Common mistakes

- A route without an access attribute (compile error "has no access
  declaration"), or `#[public]` plus another attribute (compile error).
- Two `#[can]` on one route (one on the impl block counts).
- `from = "id"` without a matching `{id}` in the path; or an `Authorized<E, A>`
  extractor that doesn't match the route's `#[can]` (500).
- `#[can(.., from = "id")]` for a model whose module doesn't import
  `AccessModule::for_feature::<Post>()` (500 when the route runs).
- Sending a stale `Authorization` header to a public route: an invalid
  token is a 401 on every route.
- A generic or non-`async` handler, or one taking `self`/`&mut self`.

## Docs

- https://navree.com/docs/basics/routing/
- https://navree.com/docs/basics/requests/
- https://navree.com/docs/basics/responses/
- https://navree.com/docs/basics/access-control/
- https://navree.com/docs/basics/middleware/
