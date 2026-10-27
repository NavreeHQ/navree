//! Integration tests of navree-billing on the fake provider: checkout, the
//! webhook pipeline, subscription changes, trials, usage, invoices, seats,
//! the routes and the commands.

mod support;

mod checkout;
mod commands;
mod orm;
mod routes;
mod seats;
mod subscriptions;
mod trials;
mod usage;
mod webhooks;
