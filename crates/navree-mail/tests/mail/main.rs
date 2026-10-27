//! Integration tests for navree-mail: messages, transports, the `Mailer`,
//! queued sending and the `MailModule`.

mod api;
mod delivery;
mod dkim;
mod domains;
mod harness;
mod log;
mod mailer;
mod message;
mod module;
mod named;
mod options;
mod queue;
mod render;
mod routes;
mod smtp;
mod support;
mod telemetry;
mod tenants;
mod transports;
