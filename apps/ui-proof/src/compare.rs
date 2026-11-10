//! The four screens that measure the speed of the table, the select, the date picker
//! and tabs, each as a page of its own with no shell around it: a table of 500
//! payments, a select and a combobox of 300 cities, a date picker, and tabs with a
//! menu. The rows and the cities come from the text files in `data/`.

use std::sync::{Arc, LazyLock};

use chrono::NaiveDate;
use http::Method;
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::dates::{DatePicker, PickerStyle};
use navree::ui::table::Direction;
use navree::ui::{
    BadgeVariant, Choice, Cmd, Combobox, DataTable, DropdownMenu, Heading, IntoNodes, MenuItem,
    Select, Stack, Tab, TableQuery, TableRow, Tabs, Text, View, el,
};

const CITY_FILE: &str = include_str!("../data/cities.txt");
const FIRST_NAME_FILE: &str = include_str!("../data/first-names.txt");
const SURNAME_FILE: &str = include_str!("../data/surnames.txt");
const STATUSES: [&str; 4] = ["Paid", "Pending", "Overdue", "Refunded"];

/// The lines of a data file that say something.
fn lines(text: &'static str) -> Vec<&'static str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

static CITIES: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let cities = lines(CITY_FILE);
    assert_eq!(cities.len(), 300, "data/cities.txt holds 300 cities");
    cities
});

/// Today on the date picker, so nothing depends on the clock.
fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or(NaiveDate::MIN)
}

/// `2129334` as `₹21,29,334`: the last three digits, then groups of two.
fn lakh(amount: i64) -> String {
    let digits = amount.to_string();
    let (head, tail) = digits.split_at(digits.len().saturating_sub(3));
    let mut out = String::from("₹");
    for (at, digit) in head.chars().enumerate() {
        if at > 0 && (head.len() - at).is_multiple_of(2) {
            out.push(',');
        }
        out.push(digit);
    }
    if !head.is_empty() {
        out.push(',');
    }
    out.push_str(tail);
    out
}

fn tone(status: &&'static str) -> BadgeVariant {
    match *status {
        "Paid" => BadgeVariant::Success,
        "Overdue" => BadgeVariant::Warning,
        _ => BadgeVariant::Neutral,
    }
}

#[derive(Clone, TableRow)]
#[table(per = 50)]
pub struct PaymentRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, label = "Name")]
    pub name: String,
    #[col(sortable, label = "City")]
    pub city: &'static str,
    #[col(sortable, numeric, label = "Amount")]
    pub amount: String,
    #[col(badge = tone, label = "Status")]
    pub status: &'static str,
    #[col(skip)]
    pub rupees: i64,
}

/// The 500 payments, the same on every run and in the other build of this screen.
fn payments() -> Vec<PaymentRow> {
    let first = lines(FIRST_NAME_FILE);
    let last = lines(SURNAME_FILE);
    assert_eq!(first.len(), 40, "data/first-names.txt holds 40 names");
    assert_eq!(last.len(), 25, "data/surnames.txt holds 25 names");
    (0..500_usize)
        .map(|at| {
            let rupees = ((at * 7919 + 211) % 503) as i64 * 4217 + 12_500;
            PaymentRow {
                id: at as i64 + 1,
                name: format!("{} {}", first[at % 40], last[(at * 7 + at / 40) % 25]),
                city: CITIES[(at * 17) % 300],
                amount: lakh(rupees),
                status: STATUSES[(at * 3 + at / 5) % 4],
                rupees,
            }
        })
        .collect()
}

static PAYMENTS: LazyLock<Vec<PaymentRow>> = LazyLock::new(payments);

/// What the query selects: in an app, this is SQL.
fn select(query: &TableQuery<PaymentRow>, all: &[PaymentRow]) -> (Vec<PaymentRow>, u64) {
    let needle = query.search().map(str::to_lowercase);
    let mut found: Vec<&PaymentRow> = all
        .iter()
        .filter(|row| {
            needle
                .as_deref()
                .is_none_or(|needle| row.name.to_lowercase().contains(needle))
        })
        .collect();
    if let Some((column, direction)) = query.sort() {
        match column {
            "name" => found.sort_by(|a, b| a.name.cmp(&b.name)),
            "city" => found.sort_by(|a, b| a.city.cmp(b.city).then(a.id.cmp(&b.id))),
            _ => found.sort_by_key(|row| row.rupees),
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

/// A heading and the screen under it.
fn page(title: &'static str, content: impl IntoNodes) -> View {
    View::new(title).body(
        el("main")
            .slot("page")
            .children((Heading::h1(title), content)),
    )
}

/// `GET /compare/table`.
fn table_page(query: &TableQuery<PaymentRow>) -> View {
    let (rows, total) = select(query, &PAYMENTS);
    page(
        "Customer payments",
        DataTable::new("cmp-table", query, &rows).total(total),
    )
}

fn city_choices() -> Vec<Choice> {
    CITIES
        .iter()
        .map(|city| Choice::new(*city, *city))
        .collect()
}

/// `GET /compare/select`.
fn select_page() -> View {
    page(
        "Choose a city",
        Stack::gap(4).children((
            el("div").children((
                el("label")
                    .attr("for", "cmp-select-trigger")
                    .child("Select"),
                Select::new("city")
                    .id("cmp-select")
                    .placeholder("Choose a city")
                    .options(city_choices()),
            )),
            el("div").children((
                el("label").attr("for", "cmp-combo-input").child("Combobox"),
                Combobox::new("city_search")
                    .id("cmp-combo")
                    .placeholder("Search cities")
                    .options(city_choices()),
            )),
        )),
    )
}

/// `GET /compare/dates`.
fn dates_page() -> View {
    page(
        "Choose a date",
        DatePicker::new("date")
            .id("cmp-date")
            .style(PickerStyle::Button)
            .placeholder("Pick a date")
            .today(today())
            .label("Date"),
    )
}

/// A menu item that writes what was chosen into the status line.
fn choose(label: &'static str) -> MenuItem {
    MenuItem::action(label).on_click(Cmd::text("cmp-chosen", format!("Chosen: {label}")))
}

/// `GET /compare/tabs`.
fn tabs_page() -> View {
    page(
        "Deal sections",
        Stack::gap(4).children((
            Tabs::new("cmp-tabs", "Deal sections")
                .tab(Tab::new("overview", "Overview").panel(Text::new("The overview panel.")))
                .tab(Tab::new("activity", "Activity").panel(Text::new("The activity panel.")))
                .tab(Tab::new("settings", "Settings").panel(Text::new("The settings panel."))),
            DropdownMenu::new("cmp-menu", "Actions")
                .item(choose("Edit"))
                .item(choose("Duplicate"))
                .item(choose("Archive")),
            el("p")
                .attr("id", "cmp-chosen")
                .attr("role", "status")
                .child("Nothing chosen yet."),
        )),
    )
}

pub struct ComparePages;

impl Injectable for ComparePages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for ComparePages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/compare/table",
                |query: TableQuery<PaymentRow>| async move { table_page(&query) },
            )
            .public(),
            RouteDef::on(Method::GET, "!/compare/select", || async { select_page() }).public(),
            RouteDef::on(Method::GET, "!/compare/dates", || async { dates_page() }).public(),
            RouteDef::on(Method::GET, "!/compare/tabs", || async { tabs_page() }).public(),
        ]
    }
}
