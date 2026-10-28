//! navree-domains: adding domains, DNS verification, workspace
//! resolution, the routes, certificates (ACME and probes), managed zones,
//! the commands, real database servers and a real ACME server (Pebble).

mod adding;
mod certificates;
mod claims;
mod cloudflare;
mod commands;
mod https;
mod managed;
mod pebble;
mod probes;
mod resolution;
mod routes;
mod servers;
mod transfer;
mod support;
mod verification;
