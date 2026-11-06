//! # crm-deals
//!
//! [`DealsModule`]: the sales pipeline.
//!
//! | Route | Access | Result |
//! |---|---|---|
//! | `GET /api/v1/deals?filter[stage]=&filter[owner_id]=&filter[company_id]=&search=&sort=&page=&per_page=` ([`DealList`]) | `read Deal` | page of [`DealResource`], newest first |
//! | `GET /api/v1/deals/board?owner_id=` | `read Deal` | [`DealBoard`]: every stage in board order with `count`, `amount_cents` and its deals by `position` |
//! | `POST /api/v1/deals` [`CreateDeal`] | `create Deal` | 201 [`DealResource`], at the end of its stage |
//! | `GET /api/v1/deals/{id}` | `read` that deal | [`DealResource`] |
//! | `PATCH /api/v1/deals/{id}` [`UpdateDeal`] | `update` that deal (`move` too for a new stage) | [`DealResource`] |
//! | `DELETE /api/v1/deals/{id}` | `delete` that deal (never a won or lost one, except for admins) | 204; its activities go with it |
//! | `POST /api/v1/deals/{id}/move` [`MoveDeal`] `{stage, position?, lost_reason?}` | `move` that deal | [`DealResource`] |
//!
//! Owners: a deal belongs to its creator unless someone who can `manage`
//! deals names another owner. Stages: `won` and `lost` set
//! `closed_at`, `lost` needs a `lost_reason` (a 400 on `lost_reason`), an
//! open stage reopens the deal (`closed_at` and `lost_reason` cleared).
//! Every stage's deals hold the positions `0..n`: `position` in a move is
//! the 0-based index within the target stage and the deals from there on
//! shift down; other writes put the deal at the end of its
//! stage.
//!
//! Background work: a deal given to someone other than the user
//! making the change emits [`DealAssigned`], a deal entering `won` emits
//! [`DealWon`]; the queued listener [`NotifyDealOwner`] sends the owner a
//! [`DealNotification`] (database, `private-users.{id}` broadcast and
//! mail) whose data is `{title, message, url, deal_id}`.
//! After every change commits, the deal is broadcast as `deal.updated`
//! `{deal}` on `private-crm.deals` through `dyn Broadcaster`.
//!
//! MCP ([`DealsMcp`], served at `/mcp` when the app has `McpModule`): the
//! tools `list_deals` (`read Deal`; [`ListDeals`]) and `move_deal`
//! (`move` that deal; [`MoveDealArgs`]), the resources `crm://deals/{id}`
//! (`read` that deal) and `crm://pipeline` (the board), and the prompt
//! `follow-up-email` ([`FollowUpArgs`]).
#![forbid(unsafe_code)]

mod controller;
mod dto;
mod events;
mod mcp;
mod notification;
mod positions;
mod resource;
mod service;
mod stages;

use crm_core::CrmCoreModule;
use navree::api::{ModuleBuilderApiExt, RouteDoc};
use navree::openapi::{Parameter, Schema};
use navree::prelude::*;

pub use controller::DealsController;
pub use dto::{BoardFilter, CreateDeal, DealList, MAX_AMOUNT_CENTS, MoveDeal, UpdateDeal};
pub use events::{DealAssigned, DealWon};
pub use mcp::{DealsMcp, FollowUpArgs, ListDeals, MoveDealArgs};
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
    /// The owner notifications (a queued listener of both events), their
    /// mail template and the deals part of the MCP server.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.api_route(RouteDoc::get("/api/v1/deals").list::<DealList>());
        // The board is not a list: `owner_id` is a plain parameter.
        m.api_route(
            RouteDoc::get("/api/v1/deals/board").query(
                Parameter::query("owner_id", Schema::string().format("uuid"))
                    .described("Only this owner's deals."),
            ),
        );
        m.listen::<DealAssigned, NotifyDealOwner>()
            .on::<DealWon, NotifyDealOwner>()
            .templates(notification::TEMPLATES);
        m.mcp_server::<DealsMcp>();
    }
}
