//! navree-micro integration tests: transports, the message pipeline,
//! events, the module and services talking to each other.

mod events;
mod request_response;
mod services;
mod support;
mod transports;

#[cfg(feature = "nats")]
mod nats;
#[cfg(feature = "nats")]
mod nats_server;
#[cfg(feature = "mqtt")]
mod mqtt;
#[cfg(feature = "mqtt")]
mod mqtt_server;
#[cfg(feature = "redis")]
mod redis;
#[cfg(feature = "redis")]
mod redis_server;
