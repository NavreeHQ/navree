//! Integration tests for navree-events: the event bus, sync and queued
//! listeners, and the JSON side (`EventSink`, `subscribe_json`).

mod support;

mod json;
mod lifecycle;
mod listeners;
mod queued;
mod scopes;
mod telemetry;
mod wiring;
