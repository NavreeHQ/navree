//! Integration tests of navree-helm: the panel's JSON API (routing,
//! authorization, CRUD, search, sorting, filters, actions and metrics),
//! its pages and sign-in, and the checks it makes at boot.

mod support;

mod actions;
mod api;
mod boot;
mod files;
mod metrics;
mod pages;
#[cfg(feature = "settings")]
mod settings;
mod signin;
mod ui;
