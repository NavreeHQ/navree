//! `GET /api/v1/dashboard`: [`Dashboard`], built by
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

use std::sync::Arc;

use chrono::{DateTime, Datelike, Months, NaiveTime, Utc};
use crm_core::Stage;
use crm_core::models::{Activity, ActivityScopes, Deal, DealScopes};
use navree::access::{Accessible, CurrentAccess, Read};
use navree::contracts::Clock;
use navree::openapi::ToSchema;
use navree::orm::access::AccessibleQuery;
use navree::orm::{Builder, Expr, OrmError, Row, TableBuilder};
use navree::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use crate::due::{Due, Today};
use crate::resource::ActivityResource;

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
        let by_stage = stage_totals(uow, &deals).await?;
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
            amount_cents: open
                .map(|row| row.amount_cents)
                .fold(0, i64::saturating_add),
            weighted_amount_cents: i64::try_from(weighted).unwrap_or(i64::MAX),
        };
        let (start, end) = month_of(now);
        let won_this_month =
            totals(uow, deals.apply(Deal::query()).won_between(start, end)).await?;
        let tasks = task_counts(uow, &activities, me, Today::of(now)).await?;
        let recent = activities
            .apply(Activity::query())
            .newest_first()
            .limit(RECENT_ACTIVITIES)
            .with_all(ActivityResource::EMBEDS)
            .get(uow)
            .await?;
        Ok(Dashboard {
            pipeline,
            won_this_month,
            open_deals,
            tasks,
            recent_activities: recent.to_resource_collection(),
        })
    }
}

/// Amounts are summed in two parts, `amount_cents / 2^32` and
/// `amount_cents % 2^32`: neither sum can overflow 64 bits for any number
/// of deals a CRM holds (SQLite fails the whole query when an integer
/// `SUM` overflows), and [`combine`] puts them back together.
const AMOUNT_SPLIT: i64 = 1 << 32;

/// The deals of `deals` counted, with the two parts of their amount sum
/// ([`AMOUNT_SPLIT`]) as `count`, `amount_high` and `amount_low`, read from
/// a derived table of the query (its conditions, the access filter
/// included, stay inside it). The high part is `(amount - amount % 2^32) /
/// 2^32`, an exact division on every database (MySQL's `/` keeps
/// decimals), and empty sums are zero.
fn with_totals(deals: Builder<Deal>) -> TableBuilder {
    let amount = || Expr::col("amount_cents");
    let low = || amount().binary("%", Expr::val(AMOUNT_SPLIT));
    let sum = |part: Expr| Expr::coalesce([Expr::sum(part), Expr::val(0_i64)]);
    TableBuilder::from_sub(deals, "deals")
        .select_expr(Expr::count(Expr::col("id")), "count")
        .select_expr(
            sum(amount().sub(low()).div(Expr::val(AMOUNT_SPLIT))),
            "amount_high",
        )
        .select_expr(sum(low()), "amount_low")
}

/// The [`DealTotals`] of a row of [`with_totals`].
fn totals_of(row: &Row) -> Result<DealTotals, OrmError> {
    Ok(DealTotals {
        count: row.get("count")?,
        amount_cents: combine(row.get("amount_high")?, row.get("amount_low")?),
    })
}

/// `high * 2^32 + low` ([`with_totals`]), saturated to `i64`.
fn combine(high: i64, low: i64) -> i64 {
    let total = i128::from(high) * i128::from(AMOUNT_SPLIT) + i128::from(low);
    i64::try_from(total).unwrap_or(if total < 0 { i64::MIN } else { i64::MAX })
}

/// The count and amount of the deals in each stage that has any: one
/// grouped query.
async fn stage_totals(
    uow: &Uow,
    deals: &Accessible<Deal, Read>,
) -> Result<HashMap<Stage, DealTotals>> {
    let rows = with_totals(deals.apply(Deal::query()))
        .select_expr(Expr::col("stage"), "stage")
        .group_by(["stage"])
        .get(uow)
        .await?;
    rows.iter()
        .map(|row| {
            // Read through the model's column, so its cast parses the stage.
            let stage = Deal::STAGE.decode(row).map_err(AppError::internal)?;
            Ok((stage, totals_of(row)?))
        })
        .collect()
}

/// The count and amount of the deals `deals` matches.
async fn totals(uow: &Uow, deals: Builder<Deal>) -> Result<DealTotals, OrmError> {
    match with_totals(deals).first(uow).await? {
        Some(row) => totals_of(&row),
        None => Ok(DealTotals::default()),
    }
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
async fn task_counts(
    uow: &Uow,
    activities: &Accessible<Activity, Read>,
    me: Uuid,
    today: Today,
) -> Result<TaskCounts, OrmError> {
    let count = |due: Due| {
        let tasks = activities
            .apply(Activity::query())
            .open_tasks()
            .for_user(me);
        due.apply(tasks, today).count(uow)
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
    fn split_sums_combine_and_saturate() {
        let split = |amount: i64| (amount / AMOUNT_SPLIT, amount % AMOUNT_SPLIT);
        for amount in [0, 1, 333, AMOUNT_SPLIT - 1, AMOUNT_SPLIT, i64::MAX] {
            let (high, low) = split(amount);
            assert_eq!(combine(high, low), amount);
        }
        let (high, low) = split(i64::MAX);
        assert_eq!(combine(high * 3, low * 3), i64::MAX);
        assert_eq!(combine(-high * 3, -low * 3), i64::MIN);
    }

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
