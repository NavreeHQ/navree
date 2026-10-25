//! # crm-deals
//!
//! [`DealsModule`] (plan Task 34; spec §7): the sales pipeline.
//!
//! | Route | Access | Result |
//! |---|---|---|
//! | `GET /api/v1/deals?stage=&owner_id=&company_id=&search=&page=&per_page=` | `read Deal` | page of [`DealResource`], newest first |
//! | `GET /api/v1/deals/board?owner_id=` | `read Deal` | [`DealBoard`]: every stage in board order with `count`, `amount_cents` and its deals by `position` |
//! | `POST /api/v1/deals` [`CreateDeal`] | `create Deal` | 201 [`DealResource`], at the end of its stage |
//! | `GET /api/v1/deals/{id}` | `read` that deal | [`DealResource`] |
//! | `PATCH /api/v1/deals/{id}` [`UpdateDeal`] | `update` that deal (`move` too for a new stage) | [`DealResource`] |
//! | `DELETE /api/v1/deals/{id}` | `delete` that deal (never a won or lost one, except for admins) | 204; its activities go with it |
//! | `POST /api/v1/deals/{id}/move` [`MoveDeal`] `{stage, position?, lost_reason?}` | `move` that deal | [`DealResource`] |
//!
//! Owners: a deal belongs to its creator unless someone who can `manage`
//! deals names another owner (spec §9.3). Stages: `won` and `lost` set
//! `closed_at`, `lost` needs a `lost_reason` (a 400 on `lost_reason`), an
//! open stage reopens the deal (`closed_at` and `lost_reason` cleared).
//! Every stage's deals hold the positions `0..n`: `position` in a move is
//! the 0-based index within the target stage and the deals from there on
//! shift down (spec §9.4); other writes put the deal at the end of its
//! stage.
//!
//! Background work (spec §6): a deal given to someone other than the user
//! making the change emits [`DealAssigned`], a deal entering `won` emits
//! [`DealWon`]; the queued listener [`NotifyDealOwner`] sends the owner a
//! [`DealNotification`] (database, `private-users.{id}` broadcast and
//! mail) whose data is `{title, message, url, deal_id}` (spec §9.7).
//! After every change commits, the deal is broadcast as `deal.updated`
//! `{deal}` on `private-crm.deals` through `dyn Broadcaster`.
#![forbid(unsafe_code)]

mod controller;
mod dto;
mod events;
mod notification;
mod positions;
mod resource;
mod service;
mod stages;

use crm_core::CrmCoreModule;
use keel::prelude::*;

pub use controller::DealsController;
pub use dto::{BoardFilter, CreateDeal, DealFilter, MAX_AMOUNT_CENTS, MoveDeal, UpdateDeal};
pub use events::{DealAssigned, DealWon};
pub use notification::{
    Assigned, DealNotice, DealNotification, MAIL_TEMPLATE, NotifyDealOwner, Won,
};
pub use resource::{BoardStage, DealBoard, DealPage, DealResource, PageMetaSchema};
pub use service::{DEFAULT_CURRENCY, DealService};

/// Deals; see the [crate docs](crate).
#[module(
    imports = [CrmCoreModule],
    providers = [DealService],
    controllers = [DealsController],
    exports = [DealService],
    register = Self::wire,
)]
pub struct DealsModule;

impl DealsModule {
    /// The owner notifications (a queued listener of both events) and
    /// their mail template.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.listen::<DealAssigned, NotifyDealOwner>()
            .on::<DealWon, NotifyDealOwner>()
            .templates(notification::TEMPLATES);
    }
}
