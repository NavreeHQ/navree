//! navree-auth integration tests: registration, login, JWT bearer
//! authentication, refresh-token rotation and logout through the full
//! navree-http router, driven by navree-testing's `TestClient`.

mod servers;
mod support;

mod access;
mod apps;
mod confirm;
mod exchange;
mod flow;
mod impersonation;
mod jwt;
mod login;
mod logout;
mod module;
mod offboarding;
mod orm;
mod passkeys;
mod password_reset;
mod passwordless;
mod platform;
mod profile;
mod refresh;
mod register;
mod session_pages;
mod session_strategy;
mod sessions;
mod sign_in_policy;
mod strategy;
mod subject;
mod telemetry;
mod tickets;
mod two_factor;
mod two_factor_policy;
mod verification;
