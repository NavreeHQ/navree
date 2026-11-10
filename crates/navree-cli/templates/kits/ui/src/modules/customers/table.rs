//! The customers table. One definition says what a reader can do with each
//! column; the same names are in the address, so a link to a table and a call to
//! the API are written alike:
//!
//! ```text
//! /customers?search=ludhiana&filter[city][in]=Ludhiana,Amritsar&sort=-monthly_paise&per_page=20
//! ```
//!
//! Search, sort, filters and pages run on the server, in the database.

use chrono::NaiveDate;
use navree::ui::*;
use uuid::Uuid;

use super::entity::{CITIES, Customer, PLANS, STATUSES, Status};
use crate::ui::format::{day, phone_text, rupees};
use crate::ui::live::{TOPIC, stream};

/// A customer as the table shows it. Each field is a column of the `customers`
/// table, under the same name.
#[derive(Clone, TableRow)]
#[table(sort = "name", per = 10, per_pages(10, 20, 40))]
pub struct CustomerRow {
    #[col(skip)]
    pub id: Uuid,
    #[col(sortable, search, sticky, filter = text, link = "/customers/{id}", label = "Customer")]
    pub name: String,
    #[col(search, hidden)]
    pub email: String,
    #[col(hidden)]
    pub phone: String,
    #[col(sortable, search, filter = multi, options = CITIES, hide_on_mobile)]
    pub city: String,
    #[col(sortable, filter = select, options = PLANS)]
    pub plan: String,
    #[col(sortable, filter = multi, options = STATUSES)]
    pub status: String,
    #[col(sortable, numeric, filter = range, kind = money, label = "Monthly (₹)")]
    pub monthly_paise: i64,
    #[col(sortable, filter = date, hide_on_mobile, label = "Customer since")]
    pub since: NaiveDate,
    #[col(filter = boolean, hidden, label = "Auto-pay")]
    pub autopay: bool,
}

impl From<Customer> for CustomerRow {
    fn from(customer: Customer) -> Self {
        Self {
            id: customer.id,
            name: customer.name,
            email: customer.email,
            phone: customer.phone,
            city: customer.city,
            plan: customer.plan,
            status: customer.status,
            monthly_paise: customer.monthly_paise,
            since: customer.since,
            autopay: customer.autopay,
        }
    }
}

/// A status as a badge with a dot, in its colour.
fn status_badge(key: &str) -> Badge {
    let status = Status::parse(key);
    Badge::new(status.label()).tone(status.tone()).dot()
}

/// The table: the rows of one page, drawn with their own cells and the
/// toolkit's words in the reader's language, refetched when the live stream
/// speaks.
pub fn customers_table<'a>(
    query: &'a TableQuery<CustomerRow>,
    page: &'a TablePage<CustomerRow>,
    labels: &'a TableLabels,
) -> DataTable<'a, CustomerRow> {
    DataTable::new("customers", query, page)
        .labels(labels)
        .caption("Customers")
        .cell("status", |row| status_badge(&row.status))
        .cell("monthly_paise", |row| {
            Text::new(rupees(row.monthly_paise)).inline()
        })
        .cell("phone", |row| Text::new(phone_text(&row.phone)).inline())
        .cell("since", |row| Text::new(day(row.since)).inline())
        .row_href(|row| format!("/customers/{}", row.id))
        .live_on(stream().on(TOPIC))
        .empty(
            Empty::new("No customer matches")
                .icon(icons::SEARCH_X)
                .description("Clear a filter, or search for a name, an email or a city.")
                .actions(Button::outline("Add a customer").href("/customers/new")),
        )
}
