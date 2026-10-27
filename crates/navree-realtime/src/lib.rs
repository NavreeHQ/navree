//! # navree-realtime
//!
//! `RealtimeModule`: a Pusher-protocol (version 7) WebSocket server with
//! public, private and presence channels, served by the app's own HTTP
//! server, so `pusher-js` and any Pusher client work unchanged.
//!
//! - **Connect** at `ws://host/app/{key}` (public route). The page's
//!   `Origin` must be allowed ([`RealtimeOptions::allowed_origins`], else the
//!   `http.cors` origins; requests without an `Origin` and same-origin pages
//!   are accepted); otherwise the client gets `pusher:error` 4009 and the
//!   socket closes. An unknown key gets 4001. The server answers with
//!   `pusher:connection_established {socket_id, activity_timeout}`.
//! - **Subscribe** with `pusher:subscribe` / `pusher:unsubscribe`; the
//!   server confirms with `pusher_internal:subscription_succeeded`.
//!   `private-…` and `presence-…` channels need
//!   `auth = "<key>:<hex HMAC-SHA256(secret, socket_id:channel[:channel_data])>"`,
//!   which `POST /broadcasting/auth` returns (authenticated; form or JSON
//!   `socket_id`, `channel_name`) when the channel authorizer registered with
//!   [`ModuleBuilderRealtimeExt::channel`] allows it; a bad signature gets
//!   `pusher:error` 4009. Presence channels report
//!   `{"presence": {ids, hash, count}}` and announce
//!   `pusher_internal:member_added` / `member_removed`.
//! - **Keep-alive**: `pusher:ping` → `pusher:pong`. After
//!   `activity_timeout` seconds of silence the server pings; a client that
//!   stays silent is closed with 4201 (reconnect).
//! - **Broadcast** in-process with [`Realtime::broadcast`] or through the
//!   `dyn Broadcaster` contract (navree-notify, navree-events listeners).
//!   [`Realtime::disconnect_user`] closes a user's connections (4009);
//!   [`Realtime::end_user_subscriptions`] (and
//!   `Broadcaster::end_user_subscriptions`, which navree-access and
//!   navree-auth call when a user's roles change or they sign out) closes
//!   them with 4200, so clients reconnect and authorize every private
//!   channel again.
//! - **Relay** across processes: the sockets live in the `serve` process,
//!   but queued jobs and scheduled tasks run in `queue:work` and
//!   `schedule:work`. With [`RealtimeOptions::relay_url`] (`auto`: the
//!   address in `http.addr`), their `dyn Broadcaster` posts each broadcast
//!   to the serving process's `POST /apps/{app_id}/events`, signed with a
//!   key derived from `realtime.secret` or the app key (shared by every
//!   process; messages older than five minutes are refused).
//! - **Shutdown**: connections are tracked by the app's task tracker and
//!   closed with 4200 (reconnect now) when the app shuts down.
//!
//! - **Backplane** across `serve` processes: with
//!   [`RealtimeOptions::backplane`] (`redis`: the [`Backplane`] provider
//!   named `redis`, from navree-redis's `RedisModule`), every process shares
//!   broadcasts, presence members and user disconnections, so several
//!   `serve` processes can sit behind a load balancer; workers broadcast
//!   through it without the relay. [`MemoryBackplane`] joins the apps of
//!   one process, for tests.
//!
//! Client events (`client-…`) are not relayed, and there is no Pusher HTTP
//! publishing API; broadcast from the app instead. Without a backplane,
//! several `serve` processes behind a load balancer each reach only their
//! own sockets.
//!
//! ```
//! use std::collections::HashMap;
//!
//! use navree_contracts::Principal;
//! use navree_kernel::{AppContext, BoxError, BoxFuture, Module, ModuleBuilder};
//! use navree_realtime::{ChannelDecision, ModuleBuilderRealtimeExt, Realtime};
//!
//! /// `private-crm.deals`: every signed-in user.
//! fn deals(
//!     _principal: &Principal,
//!     _params: &HashMap<String, String>,
//!     _ctx: &AppContext,
//! ) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
//!     Box::pin(async { Ok(ChannelDecision::Allow) })
//! }
//!
//! struct DealsModule;
//!
//! impl Module for DealsModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         m.channel("crm.deals", deals);
//!     }
//! }
//!
//! // Later, in a service holding `Arc<Realtime>`:
//! fn deal_updated(realtime: &Realtime, deal: &serde_json::Value) -> Result<(), BoxError> {
//!     realtime.broadcast(["private-crm.deals"], "deal.updated", serde_json::json!({ "deal": deal }))?;
//!     Ok(())
//! }
//! ```
//!
//! On the front end (`pusher-js`):
//!
//! ```js
//! new Pusher("navree-crm-key", {
//!   cluster: "",
//!   wsHost: "127.0.0.1",
//!   wsPort: 8080,
//!   forceTLS: false,
//!   enabledTransports: ["ws"],
//!   channelAuthorization: { endpoint: "/broadcasting/auth", transport: "ajax" },
//! });
//! ```
#![forbid(unsafe_code)]

mod backplane;
mod channel;
mod cluster;
mod controller;
mod module;
mod options;
mod protocol;
mod realtime;
mod relay;
mod socket;

pub use backplane::{Backplane, BackplaneHandler, MemoryBackplane};
pub use module::{ChannelDecision, ModuleBuilderRealtimeExt, RealtimeModule};
pub use options::{BackplaneKind, RealtimeOptions};
pub use realtime::{Realtime, RealtimeError};
