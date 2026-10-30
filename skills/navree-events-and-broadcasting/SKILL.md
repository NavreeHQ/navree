---
name: navree-events-and-broadcasting
description: "Events and real time in Navree: typed events on the `EventBus`, sync and queued listeners, delivery and transactions, and the Pusher-protocol WebSocket server (`RealtimeModule`) with private and presence channels, channel authorizers, `dyn Broadcaster`, the relay for workers and Pusher JavaScript clients. Use when decoupling side effects with events or listeners, pushing live updates to browsers, authorizing private or presence channels, or broadcasting from jobs."
---

# Events and broadcasting

**Events** let a service announce that something happened without knowing
who reacts: `DealService` emits `DealWon`; one listener updates a counter,
another notifies the owner. The bus is in-process; queued listeners reach
workers through the queue. **Broadcasting** pushes events to browsers over
WebSockets: `RealtimeModule` is a Pusher-protocol (v7) server inside your
app, so `pusher-js` and any other Pusher client connect to it unchanged.

## Steps: an event and its listeners

1. `navree make:event deals deal_won` writes `events/deal_won.rs` (event
   name `deals.deal_won`; rename the `NAME` if you like, then keep it stable).
2. `navree make:listener deals notify_owner --event deal_won` writes the
   listener and subscribes it at `// navree:wire`.
3. Fill in `handle`; return `true` from `queued` for side effects that must
   wait for the commit or are slow (mail, notifications, external calls).
4. Emit from a service with an injected `Arc<EventBus>`.
5. The app needs `EventsModule::for_root()` (a new app has it) and
   `QueueModule` for queued listeners.

## Event and listeners

```rust
use navree::BoxFuture;
use navree::cache::Cache;
use navree::events::{Event, Listener};
use navree::notify::Notifier;
use navree::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DealWon {
    pub deal_id: Uuid,   // ids, not records: queued listeners get it as JSON
    pub title: String,
    pub owner_id: Uuid,
}

impl Event for DealWon {
    const NAME: &'static str = "deal.won";
}

/// Sync: runs inside `emit`, before the request commits.
#[injectable]
pub struct CountWins {
    cache: Arc<Cache>,
}

impl Listener<DealWon> for CountWins {
    fn handle<'a>(&'a self, event: &'a DealWon, _ctx: &'a AppContext) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            self.cache.increment(&format!("wins:{}", event.owner_id), 1).await?;
            Ok(())
        })
    }
}

/// Queued: its job joins the request's transaction and runs on a worker.
#[injectable]
pub struct NotifyOwner {
    notifier: Arc<Notifier>,
}

impl Listener<DealWon> for NotifyOwner {
    fn handle<'a>(&'a self, event: &'a DealWon, _ctx: &'a AppContext) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            // ... self.notifier.send(&owner, DealWonNotification { .. }).await?;
            let _ = (event, &self.notifier);
            Ok(())
        })
    }

    fn queued(&self) -> bool {
        true
    }
}
```

```rust
impl DealsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.listen::<DealWon, CountWins>()       // registers the provider and subscribes it
            .listen::<DealWon, NotifyOwner>();
        // m.on::<OtherEvent, NotifyOwner>()   // subscribe an already-registered provider to another event
        // navree:wire
    }
}
```

Emit (never from a constructor):

```rust
self.events.emit(DealWon { deal_id, title, owner_id }).await?; // events: Arc<EventBus>
```

- Listeners run in module dependency order, then subscription order. A
  failing listener doesn't stop the others; `emit` then returns
  `EventsError::ListenersFailed`, and `?` makes the request a 500 (rolling
  back, queued listeners' jobs included).
- Sync listeners' database work joins the request's transaction; anything
  else they do (HTTP calls, mail) happens even if the request later fails.
- Queued listener jobs are named `events.listener.<event>.<listener type
  path>` and retried with the default job options. Moving or renaming a
  queued listener renames its jobs.
- `bus.subscribe_json("deal.won")` gives a broadcast receiver of the JSON
  payloads; `dyn EventSink` (`navree::contracts`) publishes by name.

## Broadcasting to browsers

Add `RealtimeModule::from_config()` (with `AuthModule` for private channels).

```toml
[realtime]
key = "my-app-key"                       # clients connect to ws://host/app/{key}
allowed_origins = ["http://localhost:3000"]  # empty = http.cors.allowed_origins
relay_url = "auto"                       # queue:work and schedule:work hand broadcasts to `serve`
```

Broadcast through `Arc<dyn Broadcaster>`, after the commit:

```rust
use navree::contracts::Broadcaster;
use serde_json::json;

pub fn broadcast_after_commit(uow: &Uow, broadcaster: Arc<dyn Broadcaster>, deal_id: Uuid) {
    let data = json!({ "deal_id": deal_id });
    uow.after_commit(move || {
        Box::pin(async move {
            let channels = vec!["private-deals".to_owned()];
            if let Err(error) = broadcaster.broadcast(channels, "deal.updated".to_owned(), data).await {
                tracing::warn!(%error, "the deal.updated broadcast failed");
            }
        })
    });
}
```

`dyn Broadcaster` works from every process (it relays to `serve` when
`realtime.relay_url` is set and all processes share `APP_KEY`).
`Realtime::broadcast` only reaches sockets of its own process: don't use it
in jobs, listeners or scheduled tasks.

## Authorize private and presence channels

```rust
use std::collections::HashMap;

use navree::BoxFuture;
use navree::contracts::Principal;
use navree::prelude::*;
use navree::realtime::ChannelDecision;

/// `private-users.{id}`: only that user.
fn user_channel(
    principal: &Principal,
    params: &HashMap<String, String>,
    _ctx: &AppContext,
) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
    let own = params.get("id") == Some(&principal.user_id.to_string());
    Box::pin(async move { Ok(if own { ChannelDecision::Allow } else { ChannelDecision::Deny }) })
}

// in any module's wire (no import needed):
// m.channel("users.{id}", user_channel);
```

Patterns omit the `private-`/`presence-` prefix; `{name}` matches one
segment without `.`; the first matching pattern decides; unmatched channels
are refused. Return `ChannelDecision::Presence(json!({ "name": .. }))` for
presence member info. Resolve services (the ability factory, `AuthService`)
from `ctx`. Clients authorize at `POST /broadcasting/auth` (signed in).

Front end (`pusher-js`):

```ts
import Pusher from "pusher-js";

const pusher = new Pusher("my-app-key", {
  cluster: "mt1", // required by pusher-js, unused with wsHost
  wsHost: "127.0.0.1", wsPort: 8080, wssPort: 8080, forceTLS: false,
  enabledTransports: ["ws", "wss"], disableStats: true,
  channelAuthorization: { endpoint: "/broadcasting/auth", transport: "ajax" },
});
pusher.subscribe("private-deals").bind("deal.updated", (payload) => { /* ... */ });
```

## Test

```rust
let fake = QueueFake::new();
let app = TestApp::builder()
    .temp_database()                       // a SQLite file: needs the `sqlite` feature in dev-dependencies
    .module(DatabaseModule::from_config())
    .module(CacheModule::from_config())
    .module(QueueModule::from_config())
    .module(EventsModule::for_root())
    .module(DealsModule)
    .override_value(Arc::new(fake.queue()))
    .build()
    .await;
let owner = Uuid::now_v7();
app.get::<EventBus>().await.emit(DealWon { deal_id: Uuid::now_v7(), title: "Acme".into(), owner_id: owner }).await.unwrap();
let job = navree::events::listener_job_name::<DealWon, NotifyOwner>();
assert!(fake.envelopes().iter().any(|e| e.job_type == job));   // the queued listener was dispatched
let wins: Option<i64> = app.get::<Cache>().await.get(&format!("wins:{owner}")).await.unwrap();
assert_eq!(wins, Some(1));                                        // the sync listener ran
```

There is no broadcaster fake: implement `Broadcaster` with a `Mutex<Vec<..>>`
recorder and install it with `.override_value::<dyn Broadcaster>(recorder.clone())`.
Test channel authorizers over HTTP:
`app.acting_as(principal(me)).post("/broadcasting/auth").json(&json!({ "socket_id": "1234.5678", "channel_name": format!("private-users.{me}") }))`.

## Common mistakes

- Broadcasting before the commit (users see changes that roll back): use
  `uow.after_commit`.
- Workers' broadcasts reaching nobody: set `realtime.relay_url` and give
  every process the same `APP_KEY`.
- Several `serve` processes behind a load balancer: each reaches only its
  own sockets unless they share a backplane: `realtime.backplane = "redis"`
  with `RedisModule` and the same `realtime.secret` everywhere (skill
  `navree-redis`).
- Subscribing without the channel prefix: a private channel is
  `private-deals`, a presence channel `presence-deals.42`.
- Registering a listener twice with `listen` in one module (duplicate
  provider): use `on` for the second event.

## Docs

- https://navree.com/docs/digging-deeper/events/
- https://navree.com/docs/digging-deeper/broadcasting/
