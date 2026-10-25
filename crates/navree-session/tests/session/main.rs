//! navree-session integration tests: sessions, cookies and CSRF through the
//! full navree-http router, driven by navree-testing's `TestClient`.

mod support;

mod cookies;
mod csrf;
mod database;
mod lifecycle;
mod module;
mod spa;
