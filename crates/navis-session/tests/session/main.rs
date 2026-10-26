//! navis-session integration tests: sessions, cookies and CSRF through the
//! full navis-http router, driven by navis-testing's `TestClient`.

mod support;

mod cookies;
mod csrf;
mod database;
mod lifecycle;
mod module;
mod spa;
