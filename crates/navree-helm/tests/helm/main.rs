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
mod signin;
mod ui;
