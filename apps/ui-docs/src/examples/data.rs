//! Table, DataTable, Stat and Bars.

use navree::ui::form::Money;
use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("data.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("table", "thumb", "A table", table_thumb).start(),
        single!("table", "default", "A table", table_default).start(),
        single!(
            "table",
            "states",
            "Sorted, selected, hidden on phones",
            table_states
        )
        .start(),
        single!("data-table", "thumb", "A data table", data_table_thumb)
            .start()
            .width(720)
            .height(360),
        single!("data-table", "default", "A data table", data_table_default)
            .start()
            .height(440),
        single!(
            "data-table",
            "bulk",
            "Selection and bulk actions",
            data_table_bulk
        )
        .start()
        .height(440),
        single!(
            "data-table",
            "filter-row",
            "Filters under the headers",
            data_table_filter_row
        )
        .start()
        .height(440),
        single!("data-table", "compact", "Compact rows", data_table_compact)
            .start()
            .height(360),
        gallery!(
            "data-table",
            "states",
            "Empty, loading and failed",
            data_table_states
        )
        .wide()
        .height(600),
        single!("stat", "thumb", "A figure", stat_thumb),
        single!("stat", "default", "Figures", stat_default)
            .start()
            .width(820),
        single!("stat", "bars", "Bars", stat_bars).start(),
    ]
}

fn table_thumb() -> Table {
    Table::new()
        .head((TableHead::new("Deal"), TableHead::new("Value").numeric()))
        .rows((
            TableRowEl::new((TableCell::new("Acme"), TableCell::new("$48,000").numeric())),
            TableRowEl::new((
                TableCell::new("Globex"),
                TableCell::new("$12,500").numeric(),
            )),
        ))
}

fn table_default() -> Table {
    Table::new()
        .caption("Deals closing this month")
        .head((
            TableHead::new("Deal"),
            TableHead::new("Stage"),
            TableHead::new("Owner"),
            TableHead::new("Value").numeric(),
        ))
        .rows((
            TableRowEl::new((
                TableCell::new("Acme renewal"),
                TableCell::new(Badge::new("Proposal")),
                TableCell::new("Ada Lovelace"),
                TableCell::new("$48,000").numeric(),
            )),
            TableRowEl::new((
                TableCell::new("Globex expansion"),
                TableCell::new(Badge::new("Won").variant(BadgeVariant::Success)),
                TableCell::new("Alan Turing"),
                TableCell::new("$12,500").numeric(),
            )),
            TableRowEl::new((
                TableCell::new("Initech pilot"),
                TableCell::new(Badge::new("Lead")),
                TableCell::new("Grace Hopper"),
                TableCell::new("$3,200").numeric(),
            )),
        ))
}

fn table_states() -> Table {
    Table::new()
        .head((
            TableHead::new("Deal").sort(true),
            TableHead::new("Owner").hide_on_mobile(),
            TableHead::new("Value").numeric(),
        ))
        .rows((
            TableRowEl::new((
                TableCell::new("Acme renewal"),
                TableCell::new("Ada Lovelace").hide_on_mobile(),
                TableCell::new("$48,000").numeric(),
            ))
            .key("42")
            .selected(true),
            TableRowEl::new((
                TableCell::new("Globex expansion"),
                TableCell::new("Alan Turing").hide_on_mobile(),
                TableCell::new("$12,500").numeric(),
            ))
            .key("43"),
        ))
}

/// The stage's look in the table.
fn tone(stage: &&'static str) -> BadgeVariant {
    match *stage {
        "Won" => BadgeVariant::Success,
        "Negotiation" => BadgeVariant::Warning,
        _ => BadgeVariant::Neutral,
    }
}

#[derive(Clone, TableRow)]
#[table(sort = "-amount", per = 5, per_pages(5, 25, 100))]
pub struct Deal {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, sticky, filter = text, link = "/deals/{id}", label = "Deal")]
    pub title: String,
    #[col(sortable, hide_on_mobile)]
    pub company: &'static str,
    #[col(badge = tone)]
    pub stage: &'static str,
    #[col(sortable, numeric, filter = range)]
    pub amount: Money,
    #[col(sortable, filter = date, label = "Closes")]
    pub closes_on: chrono::NaiveDate,
}

/// Five deals, the same on every run.
fn deals() -> Vec<Deal> {
    let day = |d| chrono::NaiveDate::from_ymd_opt(2026, 11, d).unwrap_or_default();
    [
        (1, "Acme renewal", "Acme", "Negotiation", 4_800_000, day(30)),
        (2, "Globex expansion", "Globex", "Won", 1_250_000, day(4)),
        (3, "Initech pilot", "Initech", "Lead", 320_000, day(21)),
        (4, "Hooli platform", "Hooli", "Proposal", 2_100_000, day(18)),
        (5, "Stark support", "Stark", "Qualified", 640_000, day(12)),
    ]
    .map(|(id, title, company, stage, amount, closes_on)| Deal {
        id,
        title: title.to_owned(),
        company,
        stage,
        amount: Money(amount),
        closes_on,
    })
    .to_vec()
}

fn data_table_thumb() -> Node {
    let query = TableQuery::<Deal>::parse("/deals", "");
    let rows: Vec<Deal> = deals().into_iter().take(2).collect();
    DataTable::new("deals", &query, &rows).into()
}

fn data_table_default() -> Node {
    let query = TableQuery::<Deal>::parse("/deals", "");
    let page = TablePage::new(deals(), 128);
    DataTable::new("deals", &query, &page)
        .caption("Deals")
        .export("/deals/export")
        .into()
}

fn data_table_bulk() -> Node {
    let query = TableQuery::<Deal>::parse("/deals", "");
    let page = TablePage::new(deals(), 128);
    DataTable::new("deals", &query, &page)
        .caption("Deals")
        .bulk(
            "/deals/bulk",
            [
                BulkAction::new("archive", "Archive").icon(icons::ARCHIVE),
                BulkAction::new("delete", "Delete")
                    .destructive()
                    .confirm("Delete {n} deals?"),
            ],
        )
        .row_actions(|deal| {
            vec![
                RowAction::link("Open", format!("/deals/{}", deal.id)),
                RowAction::post("Archive", format!("/deals/{}/archive", deal.id)),
            ]
        })
        .into()
}

fn data_table_filter_row() -> Node {
    let query = TableQuery::<Deal>::parse("/deals", "f.amount.min=500000");
    let page = TablePage::new(deals(), 4);
    DataTable::new("deals", &query, &page)
        .caption("Deals")
        .filter_row()
        .into()
}

fn data_table_compact() -> Node {
    let query = TableQuery::<Deal>::parse("/deals", "density=compact");
    let page = TablePage::new(deals(), 5);
    DataTable::new("deals", &query, &page)
        .caption("Deals")
        .into()
}

fn data_table_states() -> Vec<(&'static str, Node)> {
    let query = TableQuery::<Deal>::parse("/deals", "");
    let none: Vec<Deal> = Vec::new();
    vec![
        (
            "No rows",
            DataTable::new("empty", &query, &none)
                .caption("Deals")
                .empty(Empty::new("No deals yet").icon(icons::INBOX))
                .into(),
        ),
        (
            "Loading",
            DataTable::new("loading", &query, &none)
                .caption("Deals")
                .loading()
                .into(),
        ),
        (
            "Failed",
            DataTable::new("failed", &query, &none)
                .caption("Deals")
                .failed("The deals couldn't be loaded. Try again in a minute.")
                .into(),
        ),
    ]
}

fn stat_thumb() -> Stat {
    Stat::new("Open deals", "312").trend("+12 this week", true)
}

fn stat_default() -> Grid {
    Grid::cols(3).gap(4).children((
        Stat::new("Open deals", "312").trend("+12 this week", true),
        Stat::new("Pipeline value", "$4.2M").trend("+3.1%", true),
        Stat::new("Won this month", "$640K").trend("-2.4%", false),
    ))
}

fn stat_bars() -> Card {
    Card::titled("Revenue by month").body(Bars::new(
        "Revenue by month, rising from 12 to 61",
        &[12, 18, 15, 24, 31, 28, 36, 41, 39, 48, 55, 61],
    ))
}
