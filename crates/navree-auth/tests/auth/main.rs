//! navree-auth integration tests: registration, login, JWT bearer
//! authentication, refresh-token rotation and logout through the full
//! navree-http router, driven by navree-testing's `TestClient`.

mod support;

mod access;
mod flow;
mod login;
mod logout;
mod module;
mod password_reset;
mod policy;
mod profile;
mod refresh;
mod register;
mod strategy;
mod subject;
mod two_factor;
mod verification;
