//! navree-auth integration tests: registration, login, JWT bearer
//! authentication, refresh-token rotation and logout through the full
//! navree-http router, driven by navree-testing's `TestClient`.

mod support;

mod access;
mod flow;
mod login;
mod logout;
mod module;
mod refresh;
mod register;
mod strategy;
mod subject;
