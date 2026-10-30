//! The deal events: [`DealAssigned`] and [`DealWon`]. [`DealsModule`](crate::DealsModule) subscribes the queued
//! [`NotifyDealOwner`](crate::NotifyDealOwner) to both; other modules may
//! subscribe their own listeners with `m.on::<DealWon, L>()`.

use navree::events::Event;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A deal was given to an owner by someone else: created for them or
/// reassigned to them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DealAssigned {
    /// The deal.
    pub deal_id: Uuid,
    /// Its title when it was assigned.
    pub title: String,
    /// The new owner.
    pub owner_id: Uuid,
    /// Who assigned it.
    pub assigned_by: Uuid,
}

impl Event for DealAssigned {
    const NAME: &'static str = "deal.assigned";
}

/// A deal moved to `won` (or was created as won).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DealWon {
    /// The deal.
    pub deal_id: Uuid,
    /// Its title when it was won.
    pub title: String,
    /// Its owner.
    pub owner_id: Uuid,
    /// Its value in cents.
    pub amount_cents: i64,
    /// Its currency.
    pub currency: String,
    /// Who marked it won.
    pub won_by: Uuid,
}

impl Event for DealWon {
    const NAME: &'static str = "deal.won";
}
