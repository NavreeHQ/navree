---
name: navree-grpc
description: "gRPC with navree-grpc: `GrpcModule` and the `grpc` config section, `.proto` code generation in `build.rs` without a system protoc (protox + tonic-prost-build `compile_fds`), `#[grpc_service(server = XServer)]` on a tonic service impl with Navree extractors (`Uow`, `CurrentPrincipal`, `Authorized`, `Accessible`, `CurrentWorkspace`) and `Valid<Request<T>>`, access attributes and guards on methods (deny by default), interceptors, `m.grpc_service::<T>()` and `m.grpc_descriptors(..)`, the AppError to gRPC status mapping (`status_from_error`, `error_from_status`), streaming, deadlines, gRPC-Web and Connect (JSON and protobuf) on the HTTP port or `grpc.addr`, health and reflection, `GrpcChannels` clients (`grpc.clients.<name>`), and `TestChannel` tests. Use when adding a gRPC API to a Navree app, exposing a service to other languages or to browsers over gRPC-Web or Connect, calling another gRPC service, testing gRPC methods, or debugging a call that fails with UNAUTHENTICATED, PERMISSION_DENIED, INVALID_ARGUMENT, UNIMPLEMENTED, DEADLINE_EXCEEDED, a 415, or a boot error about a gRPC method."
---

# gRPC

`navree::grpc` (crate `navree-grpc`, facade feature `grpc`, **opt-in**: not
in `full`) serves tonic services from Navree modules. A method is tonic's,
plus Navree's pipeline: request id, interceptor, deadline, authentication,
guards, the access check, then the extractors and the method in a unit of
work. The services answer gRPC, gRPC-Web and Connect on the HTTP port by
default. Everything below is in the code; when in doubt read
`crates/navree-grpc/src` and its tests in `crates/navree-grpc/tests/`
(`tests/support/mod.rs` has a complete service; `crates/navree/tests/grpc.rs`
is the docs page's example through the facade).

## Steps: add a gRPC service

1. Dependencies. The generated code names `tonic` and `prost` itself, so the
   crate depends on them at navree-grpc's versions (`cargo tree -p
   navree-grpc`). No system `protoc`: protox compiles `.proto` files in Rust.

```toml
[dependencies]
navree = { git = "https://github.com/NavreeHQ/navree", features = ["grpc"] }
tonic = "0.14"
tonic-prost = "0.14"
prost = "0.14"
garde = { version = "0.23", features = ["derive"] } # only for Valid<Request<T>>
tokio-stream = { version = "0.1", features = ["sync"] } # only for streams

[build-dependencies]
protox = "0.10"
tonic-prost-build = "0.14"
prost = "0.14"
```

2. Write the contract in `proto/<package path>/<name>.proto` (`package
   crm.deals.v1;`, `service Deals { rpc GetDeal(GetDealRequest) returns
   (Deal); ... }`). Give a message an `id` field when a method should check
   access to one record (`#[can(.., from = "id")]`).

3. Generate in `build.rs`. Write the descriptor set to `OUT_DIR` (reflection
   and Connect JSON read it):

```rust
// build.rs
use std::{env, fs, path::PathBuf};

use prost::Message;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto");
    let descriptors = protox::compile(["crm/deals/v1/deals.proto"], ["proto"])?;
    let out = PathBuf::from(env::var("OUT_DIR")?);
    fs::write(out.join("deals_descriptor.bin"), descriptors.encode_to_vec())?;

    tonic_prost_build::configure()
        // garde rules for Valid<Request<MoveDealRequest>>: every field needs one, or skip
        .type_attribute(".crm.deals.v1.MoveDealRequest", "#[derive(garde::Validate)]")
        .field_attribute(".crm.deals.v1.MoveDealRequest.id", "#[garde(skip)]")
        .field_attribute(".crm.deals.v1.MoveDealRequest.stage", "#[garde(length(min = 1))]")
        .field_attribute(".crm.deals.v1.MoveDealRequest.position", "#[garde(range(min = 0))]")
        .field_attribute(".crm.deals.v1.MoveDealRequest.lost_reason", "#[garde(length(chars, max = 1000))]")
        .compile_fds(descriptors)?;
    Ok(())
}
```

```rust
// src/grpc/pb.rs
tonic::include_proto!("crm.deals.v1");

pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("deals_descriptor");
```

4. Implement the generated trait on an `#[injectable]` struct, marked
   `#[grpc_service(server = <Name>Server)]`:

```rust
// src/grpc/mod.rs
pub mod pb;

use navree::grpc::{Request, Response, Status, grpc_service};
use navree::http::CurrentPrincipal;
use navree::prelude::*;

use pb::deals_server::{Deals, DealsServer};

#[injectable]
pub struct DealsGrpc {
    deals: Arc<DealService>,
}

#[grpc_service(server = DealsServer)]
#[authenticated] // every method inherits it; a method's #[can]/#[roles]/#[permissions] add to it
impl Deals for DealsGrpc {
    // Loads the record named by the message's `id` field in the call's unit of
    // work: NOT_FOUND / PERMISSION_DENIED before the body runs.
    #[can(Read, Deal, from = "id")]
    async fn get_deal(
        &self,
        _request: Request<pb::GetDealRequest>,
        uow: Uow,
        deal: Authorized<Deal, Read>,
    ) -> Result<Response<pb::Deal>> {
        let view = self.deals.view(&uow, deal.into_inner()).await?;
        Ok(Response::new(view.into()))
    }

    #[can(Read, Deal)]
    async fn list_deals(
        &self,
        request: Request<pb::ListDealsRequest>,
        uow: Uow,
        scope: Accessible<Deal, Read>, // SQL filter from the ability rules
    ) -> Result<Response<pb::ListDealsResponse>> {
        let page = self.deals.page(&uow, scope, &request.into_inner()).await?;
        Ok(Response::new(page.into()))
    }

    #[can(Move, Deal, from = "id")]
    async fn move_deal(
        &self,
        Valid(request): Valid<Request<pb::MoveDealRequest>>, // garde rules first
        uow: Uow,
        CurrentPrincipal(me): CurrentPrincipal,
        deal: Authorized<Deal, Move>,
    ) -> Result<Response<pb::Deal>> {
        let input = request.into_inner().try_into()?; // TryFrom -> AppError::bad_request
        let view = self.deals.move_to(&uow, me.user_id, deal.into_inner(), input).await?;
        Ok(Response::new(view.into()))
    }

    // A method may also return tonic's own error type, sent unchanged.
    #[can(Read, Deal)]
    async fn count_deals(
        &self,
        _request: Request<pb::CountDealsRequest>,
    ) -> Result<Response<pb::CountDealsResponse>, Status> {
        Err(Status::unimplemented("not yet"))
    }
}
```

   Rules the macro enforces: the first parameter is `Request<T>` (or
   `Valid<Request<T>>`, or `Request<Streaming<T>>` for client streams); any
   extractor may follow (`Uow`, `CurrentPrincipal`, `CurrentUser`,
   `Abilities`, `Accessible<E, A>`, `Authorized<E, A>`, `Scoped<T>`,
   `CurrentWorkspace`, or your own `FromRequestParts` whose rejection is an
   `AppError`); the result is `Result<Response<T>>` (AppError) or
   `Result<Response<T>, Status>`. It adds tonic's `#[async_trait]` itself.

5. Register it and its descriptors in the feature module, and add
   `GrpcModule` to the app root **after `DatabaseModule`**:

```rust
#[module(providers = [DealService], register = Self::wire)]
pub struct DealsModule;

impl DealsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_feature::<deal::Entity>())
            .grpc_service::<DealsGrpc>()                     // singleton of this module, served
            .grpc_descriptors(grpc::pb::FILE_DESCRIPTOR_SET); // reflection + Connect JSON
    }
}

// src/app.rs
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(HttpModule::from_config())
    .module(DatabaseModule::from_config())
    .module(AuthModule::from_config())                       // the authenticator
    .module(AccessModule::for_root(abilities::access_options())) // roles/permissions/can
    .module(GrpcModule::from_config())                       // section `grpc`
    .module(DealsModule)
```

`ModuleBuilderGrpcExt` (for `m.grpc_service` / `m.grpc_descriptors`) and
`GrpcModule` come with `navree::prelude::*` when the `grpc` feature is on.

6. Configure (all optional; `config/app.toml` or `APP_GRPC__*`):

```toml
[grpc]
# addr = "0.0.0.0:50051"   # own listener; default: share http.addr (h2c + HTTP/1.1)
web = true                  # gRPC-Web
connect = true              # Connect (JSON and protobuf)
# reflection = true         # default: true in debug builds, false in release
health = true               # grpc.health.v1.Health from the readiness checks
# timeout_secs = 30         # default: http.timeout_secs
max_message_bytes = 4194304
concurrency_per_connection = 256  # only the grpc.addr listener

[grpc.clients.deals]        # GrpcChannels::get("deals")
url = "http://deals.internal:8081"   # or https:// (system roots)
timeout_ms = 5000
forward_auth = true
# tls = { ca = "ca.pem", cert = "client.pem", key = "client.key" }
```

In code: `GrpcModule::for_root(GrpcOptions { addr: Some("0.0.0.0:50051".parse()?), ..GrpcOptions::default() })`.

## Templates

Interceptor (runs before authentication; any tonic `Interceptor`):

```rust
fn require_client_version(request: Request<()>) -> std::result::Result<Request<()>, Status> {
    match request.metadata().get("x-client-version") {
        Some(_) => Ok(request),
        None => Err(Status::failed_precondition("x-client-version is required")),
    }
}

#[grpc_service(server = DealsServer, interceptor = require_client_version)]
#[authenticated]
#[guard(Arc::new(ApiVersion))] // a navree_http::Guard; sees the metadata as headers
impl Deals for DealsGrpc { /* ... */ }
```

Server stream from the event bus (`events: Arc<EventBus>` field):

```rust
type DealEvents = Pin<Box<dyn Stream<Item = std::result::Result<pb::DealEvent, Status>> + Send>>;

// inside the #[grpc_service] impl:
type WatchDealsStream = DealEvents;

#[can(Read, Deal)]
async fn watch_deals(
    &self,
    _request: Request<pb::WatchDealsRequest>,
) -> Result<Response<Self::WatchDealsStream>> {
    let updates = BroadcastStream::new(self.events.subscribe_json("deal.updated"));
    let stream = updates.filter_map(|update| {
        let value = update.ok()?;
        Some(Ok(pb::DealEvent {
            deal_id: value["deal_id"].as_str()?.to_owned(),
            stage: value["stage"].as_str()?.to_owned(),
        }))
    });
    Ok(Response::new(Box::pin(stream) as DealEvents))
}
```

Calling another service (forwards request id, trace context, authorization,
workspace and the remaining deadline):

```rust
#[injectable]
pub struct DealLookup {
    channels: Arc<GrpcChannels>,
}

impl DealLookup {
    pub async fn get(&self, id: Uuid) -> Result<pb::Deal> {
        let mut deals = pb::deals_client::DealsClient::new(self.channels.get("deals")?);
        let reply = deals.get_deal(pb::GetDealRequest { id: id.to_string() }).await?; // Status -> AppError
        Ok(reply.into_inner())
    }
}
```

Test (in-process, whole pipeline, no port):

```rust
use navree::grpc::Code;
use navree::grpc::testing::TestChannel;
use navree::prelude::*;
use navree::testing::{TestApp, principal};

#[tokio::test]
async fn get_deal_checks_access() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(abilities::access_options()))
        .module(GrpcModule::from_config())
        .module(DealsModule)
        .build()
        .await;
    let (rep, deal_id) = seed(&app).await;
    let request = || pb::GetDealRequest { id: deal_id.to_string() };

    let mut guest = pb::deals_client::DealsClient::new(TestChannel::new(&app));
    assert_eq!(guest.get_deal(request()).await.unwrap_err().code(), Code::Unauthenticated);

    let mut owner = pb::deals_client::DealsClient::new(
        TestChannel::new(&app)
            .acting_as(principal(rep))                 // TestAuthenticator
            .metadata("x-client-version", "1.0"),      // any metadata
    );
    let deal = owner.get_deal(request()).await.unwrap().into_inner();
    assert_eq!(deal.owner_id, rep.to_string());
    app.shutdown().await.unwrap();
}
```

`.bearer(token)` sends real credentials to the app's authenticator; apps
that register their own authenticator need `TestAppBuilder::fake_auth()` for
`acting_as`. To test Connect or gRPC-Web, send HTTP with the normal
`TestClient`: `app.acting_as(p).post("/crm.deals.v1.Deals/GetDeal").json(&json!({"id": id})).send().await`
(Connect JSON, with the service's descriptors registered).

## Error mapping

`AppError` to status (`status_from_error`): 400 INVALID_ARGUMENT (field
errors as a `google.rpc.BadRequest` detail), 401 UNAUTHENTICATED, 403
PERMISSION_DENIED, 404 NOT_FOUND, 409 ALREADY_EXISTS, 412
FAILED_PRECONDITION, 413/429 RESOURCE_EXHAUSTED, 501 UNIMPLEMENTED, 503
UNAVAILABLE, 504 DEADLINE_EXCEEDED, anything else INTERNAL with the message
`Internal server error` (logged with the request id; `m.map_error` mappers
apply first). A `Status` you return is sent unchanged. `?` on a `Status` in
an HTTP handler or a gRPC method gives the AppError of the same meaning
(`error_from_status`).

## Checks

- `cargo build` and `cargo test`; a method without access fails to compile.
- Boot (`navree dev` or a `TestApp`) fails with every problem listed when a
  method is public and restricted at once, a non-public method has no
  `dyn Authenticator` (add `AuthModule`), `roles`/`permissions`/`can` has no
  `dyn RouteAuthorizer` (add `AccessModule`), or two modules register the
  same service.
- `grpcurl -plaintext localhost:8080 list` (debug builds: reflection on)
  lists the services; `grpcurl -plaintext localhost:8080
  grpc.health.v1.Health/Check` answers `SERVING` when the readiness checks
  pass.
- `curl -X POST http://localhost:8080/crm.deals.v1.Deals/GetDeal -H
  "content-type: application/json" -d '{"id":"..."}'` exercises Connect JSON.

## Gotchas

- Deny by default: put `#[public]`, `#[authenticated]`, `#[roles(..)]`,
  `#[permissions(..)]` or `#[can(..)]` on each method or on the impl block
  below `#[grpc_service]`. A `#[guard]` alone is not an access declaration.
- Methods inherit the impl block's attributes, so a `#[public]` method in an
  `#[authenticated]` impl is a compile error, and so is `#[can]` on both the
  impl block and a method: declare access per method in a service that
  mixes public and protected methods.
- HTTP-only attributes (`#[get]` and the verbs, `#[throttle]`, `#[timeout]`,
  `#[audit]`, `#[name]`, `#[api]`, `#[transactional]`) are compile errors.
- Do not write `#[tonic::async_trait]` on the impl: the macro adds it.
- `from = "id"` needs a unary request whose message has that field (string
  or integer, optional or not); an empty id is INVALID_ARGUMENT. The record
  is checked before the body runs.
- The unit of work commits when the method returns `Ok`. For server and
  bidirectional streams that is when the stream is returned: items sent
  later run outside it, so open `Db::transaction` per item when a stream
  writes.
- Connect JSON needs `m.grpc_descriptors(..)` for the service (otherwise
  UNIMPLEMENTED); protobuf does not. Compressed Connect messages and GET
  requests are not supported. On the shared port, `application/json` POSTs
  reach gRPC only when the path names a served service.
- gRPC needs HTTP/2 end to end: behind a proxy use h2c to the app (Caddy
  `reverse_proxy h2c://127.0.0.1:8080`).
- `grpc.addr` opens only with `serve`, after the HTTP port; other content
  types there get 415. `GrpcServer::wait_bound()` gives its address.
- `GrpcChannels::get(name)` is a 500 naming `grpc.clients.<name>` when it is
  not configured. Metadata you set on a request wins over forwarded values.
- Client deadlines (`grpc-timeout`) cut the method off and roll back; the
  default is `grpc.timeout_secs`, else `http.timeout_secs`. Streams end with
  DEADLINE_EXCEEDED at the client's deadline and UNAVAILABLE on shutdown.
- Reflection is off in release builds unless `grpc.reflection = true`.
- `CurrentWorkspace` reads `x-workspace` metadata (header resolution);
  `WorkspaceUow` under database isolation is not available in gRPC calls.
