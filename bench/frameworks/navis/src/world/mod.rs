//! [`WorldModule`]: `GET /json`, `GET /db` and `GET /queries` (SPEC §3.1-3.3).

mod controller;
pub mod entity;

use navis::prelude::*;

pub use controller::WorldController;

/// The framework-overhead and raw-query endpoints.
#[module(controllers = [WorldController])]
pub struct WorldModule;
