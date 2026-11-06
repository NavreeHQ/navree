//! navree-realtime integration tests: a tokio-tungstenite client against the
//! app served by `navree_http::serve` on 127.0.0.1:0, and live streams read
//! through the app's router.

mod auth;
mod backplane;
mod channels;
mod connection;
mod lifecycle;
mod live_support;
mod presence;
mod relay;
mod streams;
mod support;
mod telemetry;
