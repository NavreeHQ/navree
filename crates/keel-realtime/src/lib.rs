//! # keel-realtime
//!
//! `RealtimeModule`: a Pusher-protocol (version 7) WebSocket server with
//! public, private and presence channels, served by the app's own HTTP
//! server, so `laravel-echo` and `pusher-js` work unchanged.
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
//!   `dyn Broadcaster` contract (keel-notify, keel-events listeners).
//!   [`Realtime::disconnect_user`] closes a user's connections (4009).
//! - **Shutdown**: connections are tracked by the app's task tracker and
//!   closed with 4200 (reconnect now) when the app shuts down.
//!
//! Client events (`client-…`) are not relayed, and there is no Pusher HTTP
//! publishing API; broadcast from the app instead.
//!
//! ```
//! use std::collections::HashMap;
//!
//! use keel_contracts::Principal;
//! use keel_kernel::{AppContext, BoxError, BoxFuture, Module, ModuleBuilder};
//! use keel_realtime::{ChannelDecision, ModuleBuilderRealtimeExt, Realtime};
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
//! On the front end (`laravel-echo` with `pusher-js`):
//!
//! ```js
//! new Echo({
//!   broadcaster: "pusher",
//!   key: "keel-crm-key",
//!   cluster: "",
//!   wsHost: "127.0.0.1",
//!   wsPort: 8080,
//!   forceTLS: false,
//!   enabledTransports: ["ws"],
//!   authEndpoint: "/broadcasting/auth",
//! });
//! ```
#![forbid(unsafe_code)]

mod channel;
mod controller;
mod module;
mod options;
mod protocol;
mod realtime;
mod socket;

pub use module::{ChannelDecision, ModuleBuilderRealtimeExt, RealtimeModule};
pub use options::RealtimeOptions;
pub use realtime::{Realtime, RealtimeError};
