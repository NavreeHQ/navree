//! keel-session integration tests: sessions, cookies and CSRF through the
//! full keel-http router, driven by keel-testing's `TestClient`.

mod support;

mod cookies;
mod csrf;
mod database;
mod lifecycle;
mod module;
mod spa;
