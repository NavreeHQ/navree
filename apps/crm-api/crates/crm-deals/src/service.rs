//! [`DealService`]: the deal rules behind `/api/v1/deals`.

use std::sync::Arc;

use chrono::NaiveDate;
use crm_core::models::{Company, Contact, Deal, DealScopes, User};
use crm_core::subjects::DEAL;
use crm_core::{DEAL_UPDATED, DEALS_CHANNEL, Move, Stage, owner_for};
use navree::access::{Ability, Accessible, ActionMarker, Authorized, Read, Update};
use navree::contracts::{Broadcaster, Clock};
use navree::events::EventBus;
use navree::orm::access::AccessibleQuery;
use navree::orm::list::{ListQuery, ListQueryExt, ListResource};
use navree::orm::{OrmError, Record};
use navree::prelude::*;
use navree::validation::ValidationErrors;
use serde_json::json;
use uuid::Uuid;

use crate::dto::{BoardFilter, CreateDeal, DealList, MoveDeal, UpdateDeal};
use crate::events::{DealAssigned, DealWon};
use crate::positions;
use crate::resource::{DealBoard, DealResource};
use crate::stages::{self, Closing};

/// The currency of deals created without one.
pub const DEFAULT_CURRENCY: &str = "INR";

/// Lists, creates, changes and deletes deals, keeping every stage's
/// positions dense. Writes run in the caller's unit of work, so a deal,
/// its neighbours' positions, its audit rows and the queued listeners of
/// its events commit together. After every change commits, the deal is
/// broadcast as `deal.updated` `{deal}` on `private-crm.deals`.
///
/// Events: [`DealAssigned`] when a deal gets an owner other than the user
/// making the change, [`DealWon`] when it enters `won`.
#[injectable]
pub struct DealService {
    clock: Arc<dyn Clock>,
    events: Arc<EventBus>,
    broadcaster: Arc<dyn Broadcaster>,
}

impl DealService {
    /// One page of the deals `scope` allows that match `list` (read
    /// against [`DealList`]), newest first unless it sorts, with their
    /// embeds.
    pub async fn list(
        &self,
        uow: &Uow,
        scope: Accessible<Deal, Read>,
        list: &ListQuery,
    ) -> Result<Page<DealResource>> {
        let page = scope
            .apply(Deal::query())
            .list(list, &DealList::SPEC)?
            .with_all(DealResource::EMBEDS)
            .paginate(uow, list.page_request(&DealList::SPEC))
            .await?;
        Ok(page.map(|deal| deal.to_resource::<DealResource>()))
    }

    /// The board: every stage with the deals `scope` allows (only
    /// `filter.owner_id`'s, if given), in board order.
    pub async fn board(
        &self,
        uow: &Uow,
        scope: Accessible<Deal, Read>,
        filter: BoardFilter,
    ) -> Result<DealBoard> {
        let deals = scope
            .apply(Deal::query())
            .when_some(filter.owner_id, |query, owner| query.owned_by(owner))
            .in_board_order()
            .with_all(DealResource::EMBEDS)
            .get(uow)
            .await?;
        Ok(DealBoard::new(deals.to_resource_collection()))
    }

    /// The resource of `deal`, its embeds loaded.
    pub async fn show(&self, uow: &Uow, mut deal: Record<Deal>) -> Result<DealResource> {
        deal.load_all(uow, DealResource::EMBEDS).await?;
        Ok(deal.to_resource())
    }

    /// Creates a deal for `me` (or for `input.owner_id`, which needs
    /// `manage` on deals) at the end of its stage.
    pub async fn create(
        &self,
        uow: &Uow,
        ability: &Ability,
        me: Uuid,
        input: CreateDeal,
    ) -> Result<DealResource> {
        let (Some(title), Some(company_id), Some(amount_cents)) =
            (input.title, input.company_id, input.amount_cents)
        else {
            return Err(AppError::bad_request(
                "title, company_id and amount_cents are required",
            ));
        };
        let owner_id = owner_for(ability, DEAL, me, None, input.owner_id)?;
        let stage = input.stage.unwrap_or_default();
        let now = self.clock.now();

        let mut errors = ValidationErrors::new();
        References {
            company: Some(company_id),
            contact: input.contact_id,
            owner: (owner_id != me).then_some(owner_id),
        }
        .check(uow, &mut errors)
        .await?;
        let expected_close_date = parse_date(input.expected_close_date.as_deref(), &mut errors);
        let closing = stages::close(None, stage, input.lost_reason.as_deref(), now)
            .map_err(|error| errors.merge(error))
            .ok();
        let Some(closing) = closing.filter(|_| errors.is_empty()) else {
            return Err(errors.into_app_error());
        };

        let placement = positions::insert(&positions::column(uow, stage, None).await?, None);
        positions::apply(uow, &placement.shifts).await?;
        let deal = Deal::create(
            uow,
            Deal {
                title: title.trim().to_owned(),
                amount_cents,
                currency: currency(input.currency.as_deref()),
                stage,
                position: placement.position,
                expected_close_date,
                closed_at: closing.closed_at,
                lost_reason: closing.lost_reason,
                company_id,
                contact_id: input.contact_id,
                owner_id,
                ..Deal::default()
            },
        )
        .await?;
        self.changed(uow, me, None, deal).await
    }

    /// Changes the fields present in `input`. A new stage takes the deal to
    /// the end of that column (and needs `move` on it); a new owner needs
    /// `manage` on deals.
    pub async fn update(
        &self,
        uow: &Uow,
        me: Uuid,
        deal: Authorized<Deal, Update>,
        input: UpdateDeal,
    ) -> Result<DealResource> {
        let moving = input.stage.filter(|stage| *stage != deal.stage);
        if moving.is_some() && !deal.ability().can(Move::NAME, DEAL, Some(deal.record())) {
            return Err(AppError::forbidden().with_detail("You may not move this deal"));
        }
        let owner_id = owner_for(
            deal.ability(),
            DEAL,
            me,
            Some(deal.owner_id),
            input.owner_id,
        )?;
        let mut deal = deal.into_inner();
        let before = deal.clone();
        let now = self.clock.now();

        let mut errors = ValidationErrors::new();
        // Only new references are checked: an edit form sending back a
        // company deleted since stays valid.
        References {
            company: input.company_id.filter(|id| *id != deal.company_id),
            contact: input
                .contact_id
                .flatten()
                .filter(|id| Some(*id) != deal.contact_id),
            owner: (owner_id != deal.owner_id).then_some(owner_id),
        }
        .check(uow, &mut errors)
        .await?;
        let expected_close_date = input
            .expected_close_date
            .map(|date| parse_date(date.as_deref(), &mut errors));
        let stage = moving.unwrap_or(deal.stage);
        let previous = Closing {
            closed_at: deal.closed_at,
            lost_reason: deal.lost_reason.clone(),
        };
        let closing = if moving.is_some() || (stage == Stage::Lost && input.lost_reason.is_some()) {
            stages::close(
                Some((deal.stage, &previous)),
                stage,
                input.lost_reason.as_deref(),
                now,
            )
            .map_err(|error| errors.merge(error))
            .ok()
        } else {
            Some(previous)
        };
        let Some(closing) = closing.filter(|_| errors.is_empty()) else {
            return Err(errors.into_app_error());
        };

        if let Some(title) = input.title {
            deal.title = title.trim().to_owned();
        }
        if let Some(company_id) = input.company_id {
            deal.company_id = company_id;
        }
        if let Some(contact_id) = input.contact_id {
            deal.contact_id = contact_id;
        }
        if let Some(amount_cents) = input.amount_cents {
            deal.amount_cents = amount_cents;
        }
        if let Some(code) = input.currency.as_deref() {
            deal.currency = currency(Some(code));
        }
        if let Some(date) = expected_close_date {
            deal.expected_close_date = date;
        }
        if let Some(stage) = moving {
            let placement =
                positions::insert(&positions::column(uow, stage, Some(deal.id)).await?, None);
            positions::apply(uow, &placement.shifts).await?;
            deal.stage = stage;
            deal.position = placement.position;
        }
        deal.closed_at = closing.closed_at;
        deal.lost_reason = closing.lost_reason;
        deal.owner_id = owner_id;
        deal.save(uow).await?;
        if moving.is_some() {
            positions::close_gap(uow, before.stage).await?;
        }
        self.changed(uow, me, Some(&before), deal).await
    }

    /// Moves `deal` to `input.stage` at the 0-based `input.position` within
    /// that stage (default and limit: the end); the deals after it shift
    /// down and the column it left closes up. Won and lost close the deal
    /// (lost needs a reason); an open stage reopens it.
    pub async fn move_to(
        &self,
        uow: &Uow,
        me: Uuid,
        mut deal: Record<Deal>,
        input: MoveDeal,
    ) -> Result<DealResource> {
        let Some(stage) = input.stage else {
            return Err(AppError::bad_request("stage is required"));
        };
        let previous = Closing {
            closed_at: deal.closed_at,
            lost_reason: deal.lost_reason.clone(),
        };
        let now = self.clock.now();
        let closing = stages::close(
            Some((deal.stage, &previous)),
            stage,
            input.lost_reason.as_deref(),
            now,
        )
        .map_err(ValidationErrors::into_app_error)?;

        let index = input
            .position
            .map(|position| usize::try_from(position).unwrap_or(usize::MAX));
        let placement =
            positions::insert(&positions::column(uow, stage, Some(deal.id)).await?, index);
        positions::apply(uow, &placement.shifts).await?;
        let before = deal.clone();
        deal.stage = stage;
        deal.position = placement.position;
        deal.closed_at = closing.closed_at;
        deal.lost_reason = closing.lost_reason;
        deal.save(uow).await?;
        if before.stage != stage {
            positions::close_gap(uow, before.stage).await?;
        }
        self.changed(uow, me, Some(&before), deal).await
    }

    /// Deletes `deal` with the activities attached to it (notes and tasks
    /// about a deal that is gone would point nowhere) and closes the gap it
    /// leaves in its column. Every record is deleted through the model, so
    /// the audit trail records each deletion. The broadcast carries its
    /// last state.
    pub async fn delete(&self, uow: &Uow, mut deal: Record<Deal>) -> Result<()> {
        let resource = self.show(uow, deal.clone()).await?;
        for mut activity in deal.activities().get(uow).await? {
            activity.delete(uow).await?;
        }
        deal.delete(uow).await?;
        positions::close_gap(uow, deal.stage).await?;
        self.broadcast(uow, &resource);
        Ok(())
    }

    /// After `me` changed a deal from `before` (`None`: created) to
    /// `after`: emits its events, broadcasts it once the unit of work
    /// commits, and returns its resource.
    async fn changed(
        &self,
        uow: &Uow,
        me: Uuid,
        before: Option<&Record<Deal>>,
        mut after: Record<Deal>,
    ) -> Result<DealResource> {
        let new_owner = before.is_none_or(|before| before.owner_id != after.owner_id);
        if new_owner && after.owner_id != me {
            self.events
                .emit(DealAssigned {
                    deal_id: after.id,
                    title: after.title.clone(),
                    owner_id: after.owner_id,
                    assigned_by: me,
                })
                .await?;
        }
        let won =
            after.stage == Stage::Won && before.is_none_or(|before| before.stage != Stage::Won);
        if won {
            self.events
                .emit(DealWon {
                    deal_id: after.id,
                    title: after.title.clone(),
                    owner_id: after.owner_id,
                    amount_cents: after.amount_cents,
                    currency: after.currency.clone(),
                    won_by: me,
                })
                .await?;
        }
        after.load_all(uow, DealResource::EMBEDS).await?;
        let resource = after.to_resource::<DealResource>();
        self.broadcast(uow, &resource);
        Ok(resource)
    }

    /// Broadcasts `deal` as `deal.updated` on `private-crm.deals` once the
    /// unit of work commits (never for a change that rolls back). A failed
    /// broadcast is logged: the change itself stands.
    fn broadcast(&self, uow: &Uow, deal: &DealResource) {
        let broadcaster = self.broadcaster.clone();
        let data = json!({ "deal": deal });
        uow.after_commit(move || {
            Box::pin(async move {
                let channels = vec![DEALS_CHANNEL.to_owned()];
                if let Err(error) = broadcaster
                    .broadcast(channels, DEAL_UPDATED.to_owned(), data)
                    .await
                {
                    tracing::warn!(
                        target: "crm::deals",
                        %error,
                        "the deal.updated broadcast failed"
                    );
                }
            })
        });
    }
}

/// The records a write points at, each checked when present.
struct References {
    company: Option<Uuid>,
    contact: Option<Uuid>,
    owner: Option<Uuid>,
}

impl References {
    /// Adds a `<field> does not exist` error for each missing record
    /// (deleted companies and contacts count as missing: the models leave
    /// them out).
    async fn check(&self, uow: &Uow, errors: &mut ValidationErrors) -> Result<(), OrmError> {
        if let Some(id) = self.company
            && Company::query().where_key(id).doesnt_exist(uow).await?
        {
            errors.add("company_id", "company_id does not exist");
        }
        if let Some(id) = self.contact
            && Contact::query().where_key(id).doesnt_exist(uow).await?
        {
            errors.add("contact_id", "contact_id does not exist");
        }
        if let Some(id) = self.owner
            && User::query().where_key(id).doesnt_exist(uow).await?
        {
            errors.add("owner_id", "owner_id does not exist");
        }
        Ok(())
    }
}

/// A `YYYY-MM-DD` date (already validated; an error is recorded otherwise).
fn parse_date(date: Option<&str>, errors: &mut ValidationErrors) -> Option<NaiveDate> {
    let date = date?;
    match NaiveDate::parse_from_str(date, "%Y-%m-%d") {
        Ok(date) => Some(date),
        Err(_) => {
            errors.add(
                "expected_close_date",
                "expected_close_date must be a valid date in the format YYYY-MM-DD",
            );
            None
        }
    }
}

/// The stored currency code: upper-case, [`DEFAULT_CURRENCY`] when absent.
fn currency(code: Option<&str>) -> String {
    code.map_or_else(|| DEFAULT_CURRENCY.to_owned(), str::to_ascii_uppercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn currencies_are_stored_upper_case_with_a_default() {
        assert_eq!(currency(None), "INR");
        assert_eq!(currency(Some("eur")), "EUR");
    }
}
