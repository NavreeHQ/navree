//! `GET /api/v1/dashboard` (spec §7 and §9.6): [`Dashboard`], built by
//! [`DashboardService`] and served by [`DashboardController`].
//!
//! - `pipeline`: every stage in board order with its deal count and amount;
//! - `won_this_month`: deals won since the first of this month (UTC);
//! - `open_deals`: deals not won or lost, with their amount weighted by the
//!   stage's probability (rounded down to the cent);
//! - `tasks`: the signed-in user's open tasks (assigned to them, or owned
//!   by them when unassigned) overdue, due today and upcoming (UTC days);
//! - `recent_activities`: the ten newest activities.
//!
//! Deal numbers are team-wide (every deal the user may read) and sum amounts
//! across currencies.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Datelike, Months, NaiveTime, Utc};
use crm_core::entities::{activity, deal};
use crm_core::{Activity, Deal, Stage};
use keel::access::{Accessible, CurrentAccess, Read};
use keel::contracts::Clock;
use keel::prelude::*;
use sea_orm::sea_query::{Expr, Func};
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DbErr, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Select,
};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::due::{Due, Today, open_tasks};
use crate::resource::{ActivityResource, present};

/// How many activities `recent_activities` lists.
pub const RECENT_ACTIVITIES: u64 = 10;

/// The dashboard; see the [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Dashboard {
    /// Every stage, in board order.
    pub pipeline: Vec<PipelineStage>,
    /// Deals won this month (UTC).
    pub won_this_month: DealTotals,
    /// Deals not won or lost.
    pub open_deals: OpenDeals,
    /// The signed-in user's open tasks.
    pub tasks: TaskCounts,
    /// The ten newest activities, newest first.
    pub recent_activities: Vec<ActivityResource>,
}

/// One stage of the pipeline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = DashboardPipelineStage)]
pub struct PipelineStage {
    /// The stage.
    pub stage: Stage,
    /// How many deals are in it.
    pub count: i64,
    /// Their amounts, summed across currencies.
    pub amount_cents: i64,
}

/// A number of deals and their amounts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = DashboardDealTotals)]
pub struct DealTotals {
    /// How many deals.
    pub count: i64,
    /// Their amounts, summed across currencies.
    pub amount_cents: i64,
}

/// The open deals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = DashboardOpenDeals)]
pub struct OpenDeals {
    /// How many deals.
    pub count: i64,
    /// Their amounts, summed across currencies.
    pub amount_cents: i64,
    /// Their amounts weighted by their stages' probabilities, rounded down.
    pub weighted_amount_cents: i64,
}

/// Open tasks by due date (UTC days).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = DashboardTaskCounts)]
pub struct TaskCounts {
    /// Due before today.
    pub overdue: u64,
    /// Due today.
    pub due_today: u64,
    /// Due after today.
    pub upcoming: u64,
}

/// Builds the [`Dashboard`].
#[injectable]
pub struct DashboardService {
    clock: Arc<dyn Clock>,
}

impl DashboardService {
    /// The dashboard for user `me`: deal numbers over the deals `deals`
    /// allows, tasks and recent activities among those `activities` allows.
    pub async fn build(
        &self,
        uow: &Uow,
        me: Uuid,
        deals: Accessible<Deal, Read>,
        activities: Accessible<Activity, Read>,
    ) -> Result<Dashboard> {
        let now = self.clock.now();
        let tx = uow.tx().await?;
        let conn = &*tx;
        let by_stage = stage_totals(conn, deals.clone()).await?;
        let pipeline: Vec<PipelineStage> = Stage::ALL
            .iter()
            .map(|&stage| {
                let totals = by_stage.get(&stage).copied().unwrap_or_default();
                PipelineStage {
                    stage,
                    count: totals.count,
                    amount_cents: totals.amount_cents,
                }
            })
            .collect();
        let open = pipeline.iter().filter(|row| !row.stage.is_closed());
        let weighted: i128 = open
            .clone()
            .map(|row| i128::from(row.amount_cents) * i128::from(row.stage.probability()))
            .sum::<i128>()
            / 100;
        let open_deals = OpenDeals {
            count: open.clone().map(|row| row.count).sum(),
            amount_cents: open.map(|row| row.amount_cents).sum(),
            weighted_amount_cents: i64::try_from(weighted).unwrap_or(i64::MAX),
        };
        let won_this_month = won_between(conn, deals, month_of(now)).await?;
        let tasks = task_counts(conn, &activities, me, Today::of(now)).await?;
        let recent = activities
            .apply(activity::Entity::find())
            .order_by_desc(activity::Column::CreatedAt)
            .order_by_desc(activity::Column::Id)
            .limit(RECENT_ACTIVITIES)
            .all(conn)
            .await?;
        Ok(Dashboard {
            pipeline,
            won_this_month,
            open_deals,
            tasks,
            recent_activities: present(conn, recent).await?,
        })
    }
}

/// `COUNT(id)` and `CAST(COALESCE(SUM(amount_cents), 0) AS BIGINT)` of the
/// deals `select` matches (the cast keeps PostgreSQL's `NUMERIC` sums
/// readable as `i64`).
fn with_totals(select: Select<deal::Entity>) -> Select<deal::Entity> {
    let amount = Func::cast_as(
        Func::coalesce([
            Expr::from(Func::sum(Expr::col(deal::Column::AmountCents))),
            Expr::val(0_i64),
        ]),
        "BIGINT",
    );
    select
        .column_as(
            Expr::from(Func::count(Expr::col(deal::Column::Id))),
            "count",
        )
        .column_as(Expr::from(amount), "amount_cents")
}

/// The count and amount of the deals in each stage that has any.
async fn stage_totals<C: ConnectionTrait>(
    conn: &C,
    deals: Accessible<Deal, Read>,
) -> Result<HashMap<Stage, DealTotals>, DbErr> {
    let select = deals
        .apply(deal::Entity::find())
        .select_only()
        .column(deal::Column::Stage);
    let rows: Vec<(String, i64, i64)> = with_totals(select)
        .group_by(deal::Column::Stage)
        .into_tuple()
        .all(conn)
        .await?;
    rows.into_iter()
        .map(|(stage, count, amount_cents)| {
            let stage = Stage::from_str(&stage).map_err(|error| DbErr::Type(error.to_string()))?;
            Ok((
                stage,
                DealTotals {
                    count,
                    amount_cents,
                },
            ))
        })
        .collect()
}

/// The deals won within `[start, end)`.
async fn won_between<C: ConnectionTrait>(
    conn: &C,
    deals: Accessible<Deal, Read>,
    (start, end): (DateTime<Utc>, DateTime<Utc>),
) -> Result<DealTotals, DbErr> {
    let select = deals
        .apply(deal::Entity::find())
        .select_only()
        .filter(deal::Column::Stage.eq(Stage::Won))
        .filter(deal::Column::ClosedAt.gte(start))
        .filter(deal::Column::ClosedAt.lt(end));
    let totals: Option<(i64, i64)> = with_totals(select).into_tuple().one(conn).await?;
    let (count, amount_cents) = totals.unwrap_or_default();
    Ok(DealTotals {
        count,
        amount_cents,
    })
}

/// The first instants of `now`'s month and of the next one (UTC).
fn month_of(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let first = now
        .date_naive()
        .with_day(1)
        .unwrap_or_else(|| now.date_naive());
    let start = first.and_time(NaiveTime::MIN).and_utc();
    let end = start
        .checked_add_months(Months::new(1))
        .unwrap_or(DateTime::<Utc>::MAX_UTC);
    (start, end)
}

/// `me`'s open tasks, overdue, due today and upcoming: assigned to `me`,
/// or owned by `me` when unassigned.
async fn task_counts<C: ConnectionTrait>(
    conn: &C,
    activities: &Accessible<Activity, Read>,
    me: Uuid,
    today: Today,
) -> Result<TaskCounts, DbErr> {
    let mine = Condition::any()
        .add(activity::Column::AssigneeId.eq(me))
        .add(
            Condition::all()
                .add(activity::Column::AssigneeId.is_null())
                .add(activity::Column::OwnerId.eq(me)),
        );
    let count = |due: Due| {
        activities
            .clone()
            .apply(activity::Entity::find())
            .filter(open_tasks())
            .filter(mine.clone())
            .filter(due.condition(today))
            .count(conn)
    };
    Ok(TaskCounts {
        overdue: count(Due::Overdue).await?,
        due_today: count(Due::Today).await?,
        upcoming: count(Due::Upcoming).await?,
    })
}

/// `GET /api/v1/dashboard`.
#[injectable]
pub struct DashboardController {
    dashboard: Arc<DashboardService>,
}

#[controller("/dashboard", version = "1")]
impl DashboardController {
    /// The signed-in user's dashboard.
    #[get("/")]
    #[can(Read, Deal)]
    #[api(
        summary = "The dashboard",
        description = "Pipeline, won this month and open deal totals are team-wide (amounts summed across currencies); task counts are the signed-in user's (assigned to them, or owned by them when unassigned), by UTC day.",
        tag = "Dashboard",
        response = Dashboard,
        responses(401, 403)
    )]
    async fn show(
        &self,
        uow: Uow,
        CurrentAccess(access): CurrentAccess,
        deals: Accessible<Deal, Read>,
        activities: Accessible<Activity, Read>,
    ) -> Result<Json<Dashboard>> {
        let me = access.user.ok_or_else(AppError::unauthenticated)?;
        Ok(Json(
            self.dashboard.build(&uow, me, deals, activities).await?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn months_run_from_the_first_to_the_first_utc() {
        let at = |y, m, d, h| Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap();
        assert_eq!(
            month_of(at(2026, 10, 28, 10)),
            (at(2026, 10, 1, 0), at(2026, 11, 1, 0))
        );
        assert_eq!(
            month_of(at(2026, 12, 31, 23)),
            (at(2026, 12, 1, 0), at(2027, 1, 1, 0))
        );
        assert_eq!(
            month_of(at(2026, 10, 1, 0)),
            (at(2026, 10, 1, 0), at(2026, 11, 1, 0))
        );
    }
}
