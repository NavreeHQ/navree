//! [`DealsModule`]: `GET /api/deals` and `POST /api/deals` (SPEC §3.4, §3.5).
//!
//! | Route | Access | Result |
//! |---|---|---|
//! | `GET /api/deals?page=&per_page=` | authenticated (JWT, user loaded) | page of deals with their owners, newest first |
//! | `POST /api/deals` `{title, amount_cents, stage}` | authenticated (JWT, user loaded) | 201 the deal, owned by the caller |

mod controller;
mod dto;
pub mod entity;
mod resource;
mod service;

use keel::prelude::*;

pub use controller::DealsController;
pub use service::DealService;

/// Deals.
#[module(
    providers = [DealService],
    controllers = [DealsController],
    exports = [DealService],
)]
pub struct DealsModule;
