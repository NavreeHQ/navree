//! Integration tests for navree-mail: messages, transports, the `Mailer`,
//! queued sending and the `MailModule`.

mod api;
mod attachments;
mod commands;
mod coredns;
mod delivery;
mod dkim;
mod domains;
mod greenmail;
mod harness;
mod inbound;
mod ingest;
mod localstack;
mod log;
mod mailer;
mod mailpit;
mod message;
mod module;
mod named;
mod options;
mod queue;
mod real_servers;
mod render;
mod routes;
mod servers;
mod smtp;
mod support;
mod telemetry;
mod tenants;
mod transports;
