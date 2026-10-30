---
name: navree-microservices
description: "Messages and events between services with navree-micro: `MicroModule` (`for_root(Transport::..)`, `from_config()`, the `micro` config section and `APP_MICRO__*`), `#[message_handlers]` with `#[message_pattern]` (request-response; options `dedupe`, `concurrency`) and `#[event_pattern]` (options `max_attempts`, `ordered`, `broadcast`, `concurrency`), `m.message_handlers::<T>()`, `m.forward_event::<E>()` / `forward_event_keyed` / `receive_event`, extractors `Payload<T>`, `Valid<Payload<T>>`, `MessageContext` plus every request extractor (`Uow`, `CurrentPrincipal`, `Authorized`, ...), access attributes and guards (deny by default), `ServiceClient::send` (timeout, retries, bearer, without_auth, header) and `emit` (key, header, immediately), `MicroError` and its HTTP mapping, the outbox, the inbox, retries, dead letters (`DeadLetters`, `micro:failed`, `micro:retry`, `micro:flush`), the TCP, memory, NATS, Kafka, RabbitMQ, Redis Streams and MQTT transports (features `micro`, `micro-nats`, `micro-kafka`, `micro-rabbitmq`, `micro-redis`, `micro-mqtt`), hybrid `serve` and `micro:listen`, named clients, trace context, a custom `MicroTransport` and the conformance suite, and `MicroFake` tests. Use when splitting a Navree module into a service, adding message or event handlers, calling another service, publishing events to other services, picking or configuring a broker, testing handlers or stubbing services, or debugging a 401/403/400/502/503/504 from a `send`, events that are not delivered, delivered twice or dead-lettered, or a boot error about a pattern."
---

# Microservices

`navree::micro` (crate `navree-micro`, facade feature `micro`, **opt-in**:
not in `full`) carries messages between services. A **message** is
request-response (`deals.get`: one handler replies); an **event** is a fact
(`deal.won`: every subscribing service gets a copy, one instance of each
handles it). Handlers are written like routes: a message reaches its
handler as request parts, through the app's authenticator, the guards, the
access check, the extractors and a unit of work that commits on `Ok`.
Everything below is in the code; when in doubt read
`crates/navree-micro/src` and its tests in `crates/navree-micro/tests/micro/`
(`support.rs` has complete handlers, `services.rs` two apps talking).

## Steps: a service that answers messages and handles events

1. Features. `micro` gives the TCP and in-memory transports; add the broker:

```toml
[dependencies]
navree = { git = "https://github.com/NavreeHQ/navree", features = ["micro", "micro-nats"] }
# micro-kafka (Kafka 4.0+), micro-rabbitmq, micro-redis (Redis 6.2+), micro-mqtt (MQTT 5)
```

2. Put payloads in a crate both sides share (plain serde types;
   `#[validate]` rules for `Valid<Payload<T>>`; events implement `navree::events::Event`
   with `NAME` = the pattern).

3. Write the handlers:

```rust
use navree::micro::MessageContext;
use navree::prelude::*;

#[injectable]
pub struct DealsHandlers {
    deals: Arc<DealService>,
    client: Arc<ServiceClient>,
}

#[message_handlers]
impl DealsHandlers {
    /// Request-response. `from = "id"` reads the payload's top-level `id`.
    #[message_pattern("deals.get")]
    #[can(Read, Deal, from = "id")]
    async fn get(&self, uow: Uow, deal: Authorized<Deal, Read>) -> Result<DealView> {
        self.deals.view(&uow, deal.into_inner()).await
    }

    /// Validated input; `dedupe` stores the reply under the message id.
    #[message_pattern("deals.move", dedupe)]
    #[can(Move, Deal, from = "id")]
    async fn move_deal(&self, uow: Uow, deal: Authorized<Deal, Move>, Valid(Payload(input)): Valid<Payload<MoveDeal>>) -> Result<DealView> {
        let moved = self.deals.move_to(&uow, deal.into_inner(), input).await?;
        // Inside a unit of work: written to the outbox, published after commit.
        self.client.emit("deal.moved", &DealMoved::from(&moved)).key(moved.id).await?;
        Ok(moved)
    }

    /// An event: no access attribute (a compile error); returns Result<()>.
    #[event_pattern("company.renamed", max_attempts = 10)]
    async fn company_renamed(&self, uow: Uow, msg: MessageContext, Payload(event): Payload<CompanyRenamed>) -> Result<()> {
        tracing::info!(attempt = msg.attempt(), actor = ?msg.actor(), "renaming");
        self.deals.rename_company(&uow, event.company_id, &event.name).await
    }
}
```

   Rules the macro enforces (compile errors): every `#[message_pattern]`
   has `#[public]`, `#[authenticated]`, `#[roles]`, `#[permissions]` or
   `#[can]` (on the method or the impl; a `#[guard]` alone is not one);
   `#[event_pattern]` takes none; methods are `async fn(&self, ..)`;
   message patterns have no wildcards; patterns are lower-case dotted
   segments (`[a-z0-9_-]`); `max_attempts`, `ordered` and `broadcast` are
   event-only, `dedupe` message-only; `broadcast` and `ordered` exclude
   each other; one handler per pattern; route attributes (`#[throttle]`,
   `#[get]`) are refused. `#[guard(expr)]` and `#[timeout("2s")]` work as
   on routes (`expr` may use `self`).

4. Register in the feature module, and add `MicroModule` to the root
   (global: modules inject `Arc<ServiceClient>` without importing it):

```rust
impl DealsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.message_handlers::<DealsHandlers>()
            .forward_event_keyed::<DealWon>(|e| e.deal_id) // in-process DealWon also leaves the process
            .receive_event::<CacheCleared>();              // broadcast: re-emitted on this instance's EventBus
    }
}

Navree::app()
    .module(ConfigModule::for_root(ConfigSources::defaults()))
    .module(HttpModule::from_config())       // optional: hybrid serve
    .module(DatabaseModule::from_config())   // optional: outbox, inbox, dead letters in the DB
    .module(AuthModule::from_config())       // the authenticator for forwarded tokens
    .module(AccessModule::for_root(rules))   // roles / permissions / #[can]
    .module(MicroModule::from_config())      // section `micro`
    .module(DealsModule)
```

5. Configure (`config/app.toml`, or `APP_MICRO__SERVICE`, `APP_MICRO__TRANSPORT`, `APP_MICRO__URL`):

```toml
[micro]
service = "deals"               # consumer group + navree-service header; default app.name
transport = "nats"              # tcp | nats | kafka | rabbitmq | redis | mqtt | memory
url = "nats://127.0.0.1:4222"
request_timeout_ms = 5000       # default deadline of send
concurrency = 64
# codec = "msgpack"; listen_with_serve = true; forward_auth = true; outbox = true; inbox = true
# inbox_retention_hours = 72; outbox_poll_ms = 1000; max_message_bytes = 1048576; auto_setup (debug builds)

[micro.events]
max_attempts = 5
backoff = { type = "exponential", base_secs = 1, max_secs = 300 }
timeout_secs = 120
# dead_letter = "database" | "transport" | "log"   (default: database with DatabaseModule, else transport)

[micro.nats]     # stream = "NAVREE", jetstream = true, replicas, max_age_hours, ack_wait_secs, credentials, token, tls
[micro.kafka]    # partitions = 6, replication_factor, retention_hours, tls, sasl = { mechanism, username, password }, start_from
[micro.rabbitmq] # exchange = "navree", queue_type = "quorum", prefetch = 32, tls.ca
[micro.redis]    # prefix = "navree:", max_len = 100000, claim_after_ms = 60000, block_ms = 5000
[micro.mqtt]     # prefix = "navree", client_id, keep_alive_secs, session_expiry_secs, username, password, tls
[micro.tcp]      # addr = "127.0.0.1:4000", pool_size = 4, tls { cert, key, ca }
[micro.tcp.routes]
"companies.*" = "companies.internal:4000"   # longest prefix wins; micro.url for the rest
```

6. Run: `serve` (with `HttpModule`) starts HTTP and the listener together
   (unless `listen_with_serve = false`); `micro:listen` runs the listener
   alone. `micro:setup` declares streams/topics/queues (run it in deploys;
   `auto_setup` does it at listener start in debug builds).

## Templates

Calling another service from a route or a handler:

```rust
let deal: DealView = self.client.send("deals.get", &GetDeal { id }).await?;   // MicroError -> AppError via ?
let quote: Value = self.client
    .send("pricing.quote", &input)
    .timeout(Duration::from_millis(800)) // else micro.request_timeout_ms; capped by an inherited deadline
    .retries(2)                          // timeouts, transport failures, NoResponders only; same message id
    .bearer(token)                       // or .without_auth(); default forwards the caller's Authorization
    .header("x-service-key", key)
    .await?;
self.client.emit("security.login_failed", &alert).immediately().await?; // skip the outbox
```

Matching remote failures:

```rust
match self.client.send::<_, DealView>("deals.get", &GetDeal { id }).await {
    Ok(deal) => Some(deal),
    Err(MicroError::Remote(e)) if e.status() == StatusCode::NOT_FOUND => None, // e.message(), e.errors(), e.body()
    Err(e) => return Err(e.into()),
}
```

A named client for another transport (sends only):

```toml
[micro.clients.billing]
transport = "tcp"
url = "billing.internal:4000"
request_timeout_ms = 2000
```

```rust
Navree::app().module(MicroModule::from_config().client("billing")) // for_root registers options.clients itself
#[injectable]
pub struct InvoiceService {
    #[inject(name = "billing")]
    billing: Arc<ServiceClient>,
}
```

Tests with `MicroFake` (in-memory broker, instant retries):

```rust
use navree::micro::{MicroFake, Reply};
use navree::testing::{TestApp, principal};

let micro = MicroFake::new();
let app = TestApp::builder()
    .temp_database()
    .module(DatabaseModule::from_config())
    .module(MicroModule::for_root(micro.transport())) // listener starts at boot
    .module(DealsModule)
    .build()
    .await;

// Into the app's handlers, through the whole pipeline:
micro.client().request("deals.move", &bad).await.assert_error(401, "Unauthorized");
micro.client().acting_as(principal(rep)).request("deals.move", &bad).await.assert_validation_error("position");
let deal: DealView = micro.client().acting_as(principal(rep)).send("deals.get", &GetDeal { id }).await.unwrap();
micro.client().emit("deal.won", &won).await.unwrap();               // returns once handled or dead-lettered
micro.client().emit("deal.won", &won).redelivered().await.unwrap(); // same message id: proves the inbox

// Stubs for what the app sends elsewhere (unstubbed patterns: NoResponders -> 503):
micro.on("companies.get", Reply::json(&company));   // also Reply::error(AppError::not_found()),
micro.on("pricing.quote", Reply::timeout());        // Reply::from_fn(|p: Quote| Ok(..)), Reply::sequence([..])
micro.assert_sent("companies.get", |p: &GetCompany| p.id == company_id);
micro.assert_sent_times("companies.get", 1);
micro.assert_emitted("deal.won", |e: &DealWon| e.deal_id == id); // recorded at commit, never on rollback
micro.assert_nothing_emitted();
assert!(micro.dead_letters().is_empty());
app.shutdown().await.unwrap();
```

Over a network transport in a test, start the listener yourself:
`MicroModule::start_listener(app.ctx()).await?`.

## Error mapping (`?` on a `MicroError`)

| `MicroError` | HTTP |
|---|---|
| `Remote` 4xx | the same status, message and field errors |
| `Remote` 5xx, `Decode` | 502 |
| `Timeout { pattern, after }` | 504 |
| `NoResponders`, `Transport`, `ShutDown` | 503 |
| `Encode`, `PayloadTooLarge`, `InvalidPattern` | 500 |

Handler errors reply with the error's status and JSON body; 5xx bodies say
"Internal server error" (details are logged on the handler's side).

## Checks

- `cargo run -- micro:patterns` lists every pattern with kind, handler,
  access and module (`--json` for tools); it is the message twin of `routes`.
- `cargo run -- micro:send deals.get '{"id":"..."}'` and `micro:emit`
  exercise a running service.
- `micro:failed`, `micro:retry <id|all>`, `micro:flush` (or the
  `DeadLetters` service) manage dead letters stored in the database.
- `/health/ready` (with `ObserveModule`) includes the `micro` check: down
  while the transport is disconnected.
- Tests: `cargo test -p navree-micro --all-features` runs the conformance
  suite on memory, TCP and every broker against test servers in
  `tests/micro/*_server.rs`; `NAVREE_TEST_{NATS,KAFKA,RABBITMQ,REDIS,MQTT}_URL`
  runs it against real brokers.

## Gotchas

- **Boot errors name the key or the pattern.** A transport whose feature is
  off: ``micro.transport = "kafka" needs the `micro-kafka` feature of
  navree``. Two
  handlers for one pattern: `... is handled twice: by .. and by ..`. A
  wildcard event pattern on Kafka or Redis Streams: `uses a wildcard, which
  the kafka transport does not support`. A non-public message pattern
  without `AuthModule` (or another `dyn Authenticator`), or `#[roles]` /
  `#[can]` without `AccessModule`, fails the boot too.
- **Messages from jobs, commands and event handlers are anonymous**:
  `#[authenticated]` handlers answer 401. Use `.bearer(token)` or a
  `#[public]` handler behind a `#[guard]`. Events never carry the token;
  the actor travels as `navree-actor` and becomes the handler's unit of
  work actor (`MessageContext::actor()`).
- **Every service must verify the forwarded JWT**: the same
  `APP_AUTH__JWT__SECRET` in each.
- **Deadlines shrink down a chain**: a call made while handling a message
  gets what is left of that message's deadline. A request past its deadline
  is dropped without running the handler; a handler cut off at the deadline
  rolls back and sends no reply.
- **`send` is not part of your transaction**: the other service commits on
  its own. Use events (outbox + inbox) for changes across services.
- **Retries only for idempotent calls**: `.retries(n)` on writes needs
  `dedupe` on the receiving handler (needs `DatabaseModule`).
- **Permanent failures skip retries**: an event whose payload does not
  decode or validate, or that a guard refuses (400, 401, 403, 413, 415,
  422), goes to the dead letters at once.
- **The inbox needs `DatabaseModule`**; then every non-broadcast event
  handler is deduplicated by message id in its unit of work. Without it,
  make handlers idempotent (upserts). Side effects outside the database
  belong in queued mail, jobs or after-commit hooks.
- **Ordering**: same key = same order within an instance. `ordered` holds
  across instances on NATS, RabbitMQ and memory; on Redis Streams, MQTT and
  TCP only within an instance; Kafka orders per partition (per key). A
  retried event loses its place unless the handler is `ordered`.
- **Broadcast handlers** are at most once: no retries, no dead letters, no
  inbox.
- **Renaming `micro.service` starts a new, empty subscription**; on Kafka a
  new group starts at the end of its topics (`start_from = "earliest"` to
  read history).
- **TCP and NATS without JetStream are at most once** for events; TCP has
  no dead-letter destination (use `dead_letter = "database"` or `"log"`).
- **Kafka** needs Kafka 4.0+ (KIP-848 consumer groups); reads gzip/snappy
  batches, not lz4/zstd. Requests to a pattern with no topic fail at once
  with `NoResponders`.
- **Trace context**: a handler runs in a navree-telemetry
  `TelemetryContext` that continues the sender's `traceparent`; HTTP calls,
  queries and messages made inside stay in that trace. With
  `observe.tracing.enabled = false` messages carry no `traceparent`.
- **Custom transports** implement `MicroTransport` (and `DeliveryActions`
  for each received message) and should pass
  `navree::micro::conformance::run(&harness)`. Boot finishes when the
  transport calls `Subscribed::signal` from `listen_subscribed` (default:
  at once); override it when the broker drops messages sent before it has
  registered the subscriptions, as core NATS does.
