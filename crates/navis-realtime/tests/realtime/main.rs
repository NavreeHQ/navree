//! navis-realtime integration tests: a tokio-tungstenite client against the
//! app served by `axum::serve` on 127.0.0.1:0.

mod auth;
mod channels;
mod connection;
mod lifecycle;
mod presence;
mod relay;
mod support;
