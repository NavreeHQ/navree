//! keel-auth integration tests: registration, login, JWT bearer
//! authentication, refresh-token rotation and logout through the full
//! keel-http router, driven by keel-testing's `TestClient`.

mod support;

mod access;
mod flow;
mod login;
mod logout;
mod module;
mod password_reset;
mod profile;
mod refresh;
mod register;
mod strategy;
mod subject;
mod two_factor;
mod verification;
