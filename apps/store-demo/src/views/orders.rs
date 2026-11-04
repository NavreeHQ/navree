//! The orders page: a table whose search, filters, sort and pages run on the
//! server, whose rows open an order in a drawer, and which follows the shop live.

use chrono::NaiveDate;
use navree::ui::*;

use crate::data::{Fulfilment, Order, Payment, Status};
use crate::views::{country_name, money, when};

/// The id of the drawer a row opens.
pub const DRAWER: &str = "order";

/// The fulfilment filter's choices.
pub const FULFILMENTS: &[(&str, &str)] = &[
    ("unfulfilled", "Unfulfilled"),
    ("packed", "Packed"),
    ("shipped", "Shipped"),
    ("delivered", "Delivered"),
];

/// The payment filter's choices.
pub const PAYMENTS: &[(&str, &str)] = &[
    ("pending", "Pending"),
    ("paid", "Paid"),
    ("partially_refunded", "Partly refunded"),
    ("refunded", "Refunded"),
];

/// The country filter's choices: every country the shop ships to.
pub const COUNTRIES: &[(&str, &str)] = &[
    ("AR", "Argentina"),
    ("AT", "Austria"),
    ("AU", "Australia"),
    ("BE", "Belgium"),
    ("CA", "Canada"),
    ("CH", "Switzerland"),
    ("CZ", "Czechia"),
    ("DE", "Germany"),
    ("DK", "Denmark"),
    ("ES", "Spain"),
    ("FI", "Finland"),
    ("FR", "France"),
    ("GB", "United Kingdom"),
    ("IE", "Ireland"),
    ("IT", "Italy"),
    ("JP", "Japan"),
    ("MX", "Mexico"),
    ("NL", "Netherlands"),
    ("NO", "Norway"),
    ("NZ", "New Zealand"),
    ("PL", "Poland"),
    ("PT", "Portugal"),
    ("SE", "Sweden"),
    ("US", "United States"),
    ("ZA", "South Africa"),
];

/// An order, as the table shows it. Each column says what a reader can do with
/// it; the server runs the search, the filters, the sort and the pages.
#[derive(Clone, TableRow)]
#[table(
    cursor,
    capped = 10000,
    sort = "-placed_on",
    per = 25,
    per_pages(10, 25, 50, 100)
)]
pub struct OrderRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, sticky, label = "Order")]
    pub number: String,
    #[col(search, hidden)]
    pub customer: String,
    #[col(search, hidden)]
    pub email: String,
    // A relation's choices come from the page: each status with how many hold it.
    #[col(sortable, filter = relation)]
    pub status: String,
    #[col(filter = multi, options = FULFILMENTS)]
    pub fulfilment: String,
    #[col(filter = multi, options = PAYMENTS, hide_on_mobile)]
    pub payment: String,
    #[col(sortable, numeric)]
    pub total: i64,
    #[col(skip)]
    pub refunded: i64,
    #[col(filter = multi, options = COUNTRIES, hide_on_mobile, label = "Ships to")]
    pub country: String,
    #[col(skip)]
    pub city: String,
    #[col(sortable, filter = date, label = "Placed")]
    pub placed_on: NaiveDate,
    #[col(skip)]
    pub placed_at: i64,
}

impl From<Order> for OrderRow {
    fn from(order: Order) -> Self {
        Self {
            id: order.id,
            number: order.number,
            customer: order.customer,
            email: order.email,
            status: order.status,
            fulfilment: order.fulfilment,
            payment: order.payment,
            total: order.total,
            refunded: order.refunded,
            country: order.country,
            city: order.city,
            placed_on: order.placed_on,
            placed_at: order.placed_at,
        }
    }
}

/// An order's status as a badge with a dot, in its tone.
pub fn status_badge(key: &str) -> Badge {
    let status = Status::parse(key);
    Badge::new(status.label()).tone(status.tone()).dot()
}

/// How far the goods have got, as a badge.
pub fn fulfilment_badge(key: &str) -> Badge {
    let step = Fulfilment::parse(key);
    Badge::new(step.label()).tone(step.tone()).dot()
}

/// Where the money stands, as a badge.
pub fn payment_badge(key: &str) -> Badge {
    let payment = Payment::parse(key);
    Badge::new(payment.label()).tone(payment.tone()).dot()
}

/// The order's number, which opens its drawer, and the customer under it. With
/// JavaScript off the number goes to the order's own page.
fn order_cell(row: &OrderRow) -> Node {
    let open = el("form")
        .attr("method", "get")
        .attr("action", format!("/orders/{}/peek", row.id))
        .flag("data-ui-form", true)
        .uses(Uses::CORE)
        .child(
            Button::new(row.number.clone())
                .variant(ButtonVariant::Link)
                .submit()
                .attr("aria-haspopup", "dialog"),
        );
    Stack::gap(0)
        .children((open, Text::muted(row.customer.clone())))
        .into()
}

/// The total, and what went back, under it.
fn total_cell(row: &OrderRow) -> Node {
    Stack::gap(0)
        .children((
            Text::new(money(row.total)).inline(),
            (row.refunded > 0).then(|| Text::muted(format!("{} back", money(row.refunded)))),
        ))
        .into()
}

/// A flag and the country.
fn country_cell(row: &OrderRow) -> Node {
    Row::gap(2)
        .align(Align::Center)
        .children((
            Flag::country(&row.country).size(FlagSize::Xs).decorative(),
            Text::new(country_name(&row.country)).inline(),
        ))
        .into()
}

/// The orders table: one definition drives the query, the filters with their
/// counts, the drawing and the live refresh.
pub fn orders_table<'a>(
    query: &'a TableQuery<OrderRow>,
    page: &'a TablePage<OrderRow>,
    counts: &[(Status, u64)],
    today: NaiveDate,
) -> DataTable<'a, OrderRow> {
    let statuses = counts
        .iter()
        .map(|(status, n)| FilterOption::new(status.key(), status.label()).count(*n))
        .collect();
    DataTable::new("orders", query, page)
        .caption("Orders")
        .options("status", statuses)
        .cell("number", order_cell)
        .cell("status", |row| status_badge(&row.status))
        .cell("fulfilment", |row| fulfilment_badge(&row.fulfilment))
        .cell("payment", |row| payment_badge(&row.payment))
        .cell("total", total_cell)
        .cell("country", country_cell)
        .cell("placed_on", move |row| {
            Text::new(when(row.placed_at, today)).inline()
        })
        .filter_bar()
        .sticky_header()
        .live_from("/events")
        .live_event("orders")
        .empty(
            Empty::new("No order matches")
                .icon(icons::SEARCH_X)
                .description(
                    "Clear a filter, or search for an order number, a name or an address.",
                ),
        )
}

/// The drawer a row opens, before it is filled.
pub fn empty_drawer() -> Sheet {
    Sheet::new(DRAWER, "Order").width(SheetWidth::Xl)
}
