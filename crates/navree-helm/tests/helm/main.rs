//! Integration tests of navree-helm: the panel's JSON API (routing,
//! authorization, CRUD, search, sorting, filters, actions and metrics),
//! its pages and sign-in, and the checks it makes at boot. The resources
//! are navree-orm models; the suite runs on the database
//! `NAVREE_TEST_DATABASE` names, see `servers`.

mod servers;
mod support;

mod actions;
mod api;
mod boot;
mod files;
mod fixes;
mod metrics;
mod pages;
#[cfg(feature = "settings")]
mod settings;
mod signin;
mod toolbar;
mod ui;
mod workspaces;
