//! The provider clients: Stripe and Paddle against a local mock server
//! (wiremock) answering with recorded-shape payloads, the provider
//! conformance suite on the fake, Stripe and Paddle, and an app wired to
//! each real driver end to end.

mod fixtures;
mod servers;
mod support;

mod app;
mod conformance;
mod idempotency;
mod paddle;
mod payments;
mod registry;
mod stripe;
