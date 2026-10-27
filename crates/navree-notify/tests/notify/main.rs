//! navree-notify integration tests. Every test uses a fresh temporary SQLite
//! file; the HTTP drivers run against a local mock server.

mod support;

mod channels;
mod conformance;
mod drivers;
mod external;
mod held;
mod http;
mod module;
mod opt_outs;
mod orm;
mod people;
mod preferences;
mod push;
mod queue;
mod recipient;
mod rendering;
mod servers;
mod telemetry;
mod uow;
