//! [`DealService`]: the deal rules behind `/api/v1/deals`.

use std::sync::Arc;

use chrono::NaiveDate;
use crm_core::entities::{activity, company, contact, deal};
use crm_core::subjects::DEAL;
use crm_core::{DEAL_UPDATED, DEALS_CHANNEL, Deal, Move, Stage, SubjectType, owner_for};
use keel::access::{Ability, Accessible, ActionMarker, Authorized, Read, Update};
use keel::auth::user;
use keel::contracts::{Broadcaster, Clock};
use keel::db::paginate;
use keel::events::EventBus;
use keel::prelude::*;
use keel::validation::ValidationErrors;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Expr, ExprTrait, Func, LikeExpr};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter,
    QueryOrder,
};
use serde_json::json;
use uuid::Uuid;

use crate::dto::{BoardFilter, CreateDeal, DealFilter, MoveDeal, UpdateDeal};
use crate::events::{DealAssigned, DealWon};
use crate::positions;
use crate::resource::{DealBoard, DealResource};
use crate::stages::{self, Closing};

/// The currency of deals created without one.
pub const DEFAULT_CURRENCY: &str = "USD";

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
    /// One page of the deals `scope` allows, newest first, matching
    /// `filter`.
    pub async fn list(
        &self,
        uow: &Uow,
        scope: Accessible<Deal, Read>,
        filter: DealFilter,
        page: PageQuery,
    ) -> Result<Page<DealResource>> {
        let mut select = scope.apply(deal::Entity::find());
        if let Some(stage) = filter.stage {
            select = select.filter(deal::Column::Stage.eq(stage));
        }
        if let Some(owner) = filter.owner_id {
            select = select.filter(deal::Column::OwnerId.eq(owner));
        }
        if let Some(company) = filter.company_id {
            select = select.filter(deal::Column::CompanyId.eq(company));
        }
        if let Some(search) = filter.search.as_deref().map(str::trim)
            && !search.is_empty()
        {
            let title = Expr::expr(Func::lower(Expr::col(deal::Column::Title)));
            select = select.filter(title.like(LikeExpr::new(contains(search)).escape('\\')));
        }
        let select = select
            .order_by_desc(deal::Column::CreatedAt)
            .order_by_desc(deal::Column::Id);
        let tx = uow.tx().await?;
        let page = paginate(select, &*tx, page).await?;
        Ok(Page {
            data: DealResource::load_many(&*tx, page.data).await?,
            meta: page.meta,
        })
    }

    /// The board: every stage with the deals `scope` allows (only
    /// `filter.owner_id`'s, if given), in board order.
    pub async fn board(
        &self,
        uow: &Uow,
        scope: Accessible<Deal, Read>,
        filter: BoardFilter,
    ) -> Result<DealBoard> {
        let mut select = scope.apply(deal::Entity::find());
        if let Some(owner) = filter.owner_id {
            select = select.filter(deal::Column::OwnerId.eq(owner));
        }
        let select = select
            .order_by_asc(deal::Column::Position)
            .order_by_asc(deal::Column::CreatedAt)
            .order_by_asc(deal::Column::Id);
        let tx = uow.tx().await?;
        let deals = select.all(&*tx).await?;
        Ok(DealBoard::new(DealResource::load_many(&*tx, deals).await?))
    }

    /// The resource of `deal`.
    pub async fn show(&self, uow: &Uow, deal: deal::Model) -> Result<DealResource> {
        let tx = uow.tx().await?;
        Ok(DealResource::load(&*tx, deal).await?)
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
        {
            let tx = uow.tx().await?;
            References {
                company: Some(company_id),
                contact: input.contact_id,
                owner: (owner_id != me).then_some(owner_id),
            }
            .check(&*tx, &mut errors)
            .await?;
        }
        let expected_close_date = parse_date(input.expected_close_date.as_deref(), &mut errors);
        let closing = stages::close(None, stage, input.lost_reason.as_deref(), now)
            .map_err(|error| errors.merge(error))
            .ok();
        let Some(closing) = closing.filter(|_| errors.is_empty()) else {
            return Err(errors.into_app_error());
        };

        let placement = positions::insert(&positions::column(uow, stage, None).await?, None);
        positions::apply(&*uow.tx().await?, &placement.shifts).await?;
        let deal = Repo::<deal::Entity>::insert(
            uow,
            deal::ActiveModel {
                id: Set(Uuid::now_v7()),
                title: Set(title.trim().to_owned()),
                amount_cents: Set(amount_cents),
                currency: Set(currency(input.currency.as_deref())),
                stage: Set(stage),
                position: Set(placement.position),
                expected_close_date: Set(expected_close_date),
                closed_at: Set(closing.closed_at),
                lost_reason: Set(closing.lost_reason),
                company_id: Set(company_id),
                contact_id: Set(input.contact_id),
                owner_id: Set(owner_id),
                created_at: Set(now),
                updated_at: Set(now),
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
        let current = deal.into_inner();
        let now = self.clock.now();

        let mut errors = ValidationErrors::new();
        {
            let tx = uow.tx().await?;
            // Only new references are checked: an edit form sending back a
            // company deleted since stays valid.
            References {
                company: input.company_id.filter(|id| *id != current.company_id),
                contact: input
                    .contact_id
                    .flatten()
                    .filter(|id| Some(*id) != current.contact_id),
                owner: (owner_id != current.owner_id).then_some(owner_id),
            }
            .check(&*tx, &mut errors)
            .await?;
        }
        let expected_close_date = input
            .expected_close_date
            .map(|date| parse_date(date.as_deref(), &mut errors));
        let stage = moving.unwrap_or(current.stage);
        let previous = Closing {
            closed_at: current.closed_at,
            lost_reason: current.lost_reason.clone(),
        };
        let closing = if moving.is_some() || (stage == Stage::Lost && input.lost_reason.is_some()) {
            stages::close(
                Some((current.stage, &previous)),
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

        let mut active = current.clone().into_active_model();
        if let Some(title) = input.title {
            active.title = Set(title.trim().to_owned());
        }
        if let Some(company_id) = input.company_id {
            active.company_id = Set(company_id);
        }
        if let Some(contact_id) = input.contact_id {
            active.contact_id = Set(contact_id);
        }
        if let Some(amount_cents) = input.amount_cents {
            active.amount_cents = Set(amount_cents);
        }
        if let Some(code) = input.currency.as_deref() {
            active.currency = Set(currency(Some(code)));
        }
        if let Some(date) = expected_close_date {
            active.expected_close_date = Set(date);
        }
        if let Some(stage) = moving {
            let placement = positions::insert(
                &positions::column(uow, stage, Some(current.id)).await?,
                None,
            );
            positions::apply(&*uow.tx().await?, &placement.shifts).await?;
            active.stage = Set(stage);
            active.position = Set(placement.position);
        }
        active.closed_at = Set(closing.closed_at);
        active.lost_reason = Set(closing.lost_reason);
        active.owner_id = Set(owner_id);
        active.updated_at = Set(now);
        let deal = Repo::<deal::Entity>::update(uow, active).await?;
        if moving.is_some() {
            positions::close_gap(uow, current.stage).await?;
        }
        self.changed(uow, me, Some(&current), deal).await
    }

    /// Moves `deal` to `input.stage` at the 0-based `input.position` within
    /// that stage (default and limit: the end); the deals after it shift
    /// down and the column it left closes up. Won and lost close the deal
    /// (lost needs a reason); an open stage reopens it.
    pub async fn move_to(
        &self,
        uow: &Uow,
        me: Uuid,
        deal: deal::Model,
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
        positions::apply(&*uow.tx().await?, &placement.shifts).await?;
        let before = deal.clone();
        let mut active = deal.into_active_model();
        active.stage = Set(stage);
        active.position = Set(placement.position);
        active.closed_at = Set(closing.closed_at);
        active.lost_reason = Set(closing.lost_reason);
        active.updated_at = Set(now);
        let deal = Repo::<deal::Entity>::update(uow, active).await?;
        if before.stage != stage {
            positions::close_gap(uow, before.stage).await?;
        }
        self.changed(uow, me, Some(&before), deal).await
    }

    /// Deletes `deal` with the activities attached to it (notes and tasks
    /// about a deal that is gone would point nowhere) and closes the gap it
    /// leaves in its column; every row goes through `Repo`, so the audit
    /// trail records each deletion. The broadcast carries its last state.
    pub async fn delete(&self, uow: &Uow, deal: deal::Model) -> Result<()> {
        let resource = self.show(uow, deal.clone()).await?;
        let stage = deal.stage;
        let activities = activity::Entity::find()
            .filter(activity::Column::SubjectType.eq(SubjectType::Deal))
            .filter(activity::Column::SubjectId.eq(deal.id))
            .all(&*uow.tx().await?)
            .await?;
        for activity in activities {
            Repo::<activity::Entity>::delete(uow, activity).await?;
        }
        Repo::<deal::Entity>::delete(uow, deal).await?;
        positions::close_gap(uow, stage).await?;
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
        before: Option<&deal::Model>,
        after: deal::Model,
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
        let resource = self.show(uow, after).await?;
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
    /// (soft-deleted companies and contacts count as missing).
    async fn check<C: ConnectionTrait>(
        &self,
        conn: &C,
        errors: &mut ValidationErrors,
    ) -> Result<(), DbErr> {
        if let Some(id) = self.company {
            let found = company::Entity::find_by_id(id)
                .filter(company::Column::DeletedAt.is_null())
                .count(conn)
                .await?;
            if found == 0 {
                errors.add("company_id", "company_id does not exist");
            }
        }
        if let Some(id) = self.contact {
            let found = contact::Entity::find_by_id(id)
                .filter(contact::Column::DeletedAt.is_null())
                .count(conn)
                .await?;
            if found == 0 {
                errors.add("contact_id", "contact_id does not exist");
            }
        }
        if let Some(id) = self.owner {
            let found = user::Entity::find_by_id(id).count(conn).await?;
            if found == 0 {
                errors.add("owner_id", "owner_id does not exist");
            }
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

/// A `LIKE` pattern matching values that contain `search`
/// (case-insensitive; `%`, `_` and `\` match themselves).
fn contains(search: &str) -> String {
    let mut pattern = String::from("%");
    for ch in search.to_lowercase().chars() {
        if matches!(ch, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    pattern.push('%');
    pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_patterns_escape_wildcards() {
        assert_eq!(contains("Acme"), "%acme%");
        assert_eq!(contains("50%_off\\"), "%50\\%\\_off\\\\%");
    }

    #[test]
    fn currencies_are_stored_upper_case_with_a_default() {
        assert_eq!(currency(None), "USD");
        assert_eq!(currency(Some("eur")), "EUR");
    }
}
