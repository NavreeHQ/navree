//! The 1,000-row table: sort, filter and page in the URL, morphed in place.

use std::sync::{Arc, LazyLock};

use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::form::Money;
use navree::ui::form::Rejected;
use navree::ui::table::Direction;
use navree::ui::{
    AppPage, AppShell, BadgeVariant, Button, DataTable, FilterBar, Options, Submit, TableQuery,
    TableRow, icons,
};
use serde::Deserialize;

use crate::deal_form::{self, NewDeal};

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StageFilter {
    Lead,
    Qualified,
    Proposal,
    Negotiation,
    Won,
}

const STAGES: [&str; 5] = ["Lead", "Qualified", "Proposal", "Negotiation", "Won"];
const OWNERS: [&str; 4] = [
    "Anu Lakhotia",
    "Arun Tiwari",
    "Gauri Hundal",
    "Esha Dhillon",
];
const COMPANIES: [&str; 6] = ["Agra", "Gulati", "Ilaichi", "Utsav", "Hariyali", "Sutlej"];

fn tone(stage: &&'static str) -> BadgeVariant {
    match *stage {
        "Won" => BadgeVariant::Success,
        "Negotiation" => BadgeVariant::Warning,
        _ => BadgeVariant::Neutral,
    }
}

#[derive(Clone, TableRow)]
#[table(per = 50, per_pages(25, 50, 100, 1000))]
pub struct DealRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, link = "/deals/{id}", label = "Deal")]
    pub title: String,
    #[col(sortable, hide_on_mobile)]
    pub company: &'static str,
    #[col(badge = tone)]
    pub stage: &'static str,
    #[col(sortable, numeric)]
    pub amount: Money,
    #[col(hide_on_mobile, label = "Owner")]
    pub owner: &'static str,
    #[col(sortable, label = "Updated")]
    pub updated_at: chrono::NaiveDate,
}

#[derive(Default, Deserialize, FilterBar)]
pub struct DealFilter {
    /// Search deals
    #[filter(search)]
    pub q: Option<String>,
    /// All stages
    pub stage: Option<StageFilter>,
}

/// A thousand rows, the same on every run.
pub fn rows(count: usize) -> Vec<DealRow> {
    let start = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap_or_default();
    (0..count)
        .map(|at| {
            let n = at as i64;
            DealRow {
                id: n + 1,
                title: format!("{} renewal {}", COMPANIES[at % 6], n + 1),
                company: COMPANIES[at % 6],
                stage: STAGES[(at * 7) % 5],
                amount: Money((n * 7919) % 5_000_000),
                owner: OWNERS[at % 4],
                updated_at: start + chrono::Days::new((at % 300) as u64),
            }
        })
        .collect()
}

static DEALS: LazyLock<Vec<DealRow>> = LazyLock::new(|| rows(1000));

/// What the query selects: in an app, this is SQL.
pub fn select(query: &TableQuery<DealRow, DealFilter>, deals: &[DealRow]) -> (Vec<DealRow>, u64) {
    let filter = query.filter();
    let needle = filter.q.as_deref().map(str::to_lowercase);
    let stage = filter
        .stage
        .map(|stage| navree::ui::form::Options::option_value(&stage));
    let mut found: Vec<&DealRow> = deals
        .iter()
        .filter(|deal| {
            needle
                .as_deref()
                .is_none_or(|needle| deal.title.to_lowercase().contains(needle))
        })
        .filter(|deal| stage.is_none_or(|stage| deal.stage.eq_ignore_ascii_case(stage)))
        .collect();
    if let Some((column, direction)) = query.sort() {
        match column {
            "title" => found.sort_by(|a, b| a.title.cmp(&b.title)),
            "company" => found.sort_by_key(|deal| deal.company),
            "amount" => found.sort_by_key(|deal| deal.amount),
            _ => found.sort_by_key(|deal| deal.updated_at),
        }
        if direction == Direction::Desc {
            found.reverse();
        }
    }
    let total = found.len() as u64;
    let skip = usize::try_from(query.offset()).unwrap_or(usize::MAX);
    let take = query.per_page() as usize;
    (
        found.into_iter().skip(skip).take(take).cloned().collect(),
        total,
    )
}

/// `GET /deals`.
fn index(shell: &AppShell, query: &TableQuery<DealRow, DealFilter>) -> AppPage {
    let (rows, total) = select(query, &DEALS);
    shell
        .page("Deals")
        .actions(
            Button::primary("New deal")
                .icon(icons::PLUS)
                .href("/deals/new"),
        )
        .body(
            DataTable::new("deals", query, &rows)
                .total(total)
                .caption("Deals in the pipeline"),
        )
}

pub struct DealPages;

impl Injectable for DealPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for DealPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        use http::Method;
        vec![
            RouteDef::on(
                Method::GET,
                "!/deals",
                |shell: AppShell, query: TableQuery<DealRow, DealFilter>| async move {
                    index(&shell, &query)
                },
            )
            .public(),
            RouteDef::on(Method::GET, "!/deals/new", |shell: AppShell| async move {
                deal_form::page(&shell)
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/deals",
                |shell: AppShell, deal: Result<Submit<NewDeal>, Rejected<NewDeal>>| async move {
                    deal_form::store(&shell, deal)
                },
            )
            .public(),
        ]
    }
}
