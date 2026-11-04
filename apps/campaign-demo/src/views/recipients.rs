//! The recipients page: a table whose search, filters, sort and pages run on the
//! server, whose rows open a recipient in a sheet, and which follows the
//! campaign live.

use chrono::NaiveDate;
use navree::ui::*;

use crate::data::{Recipient, Status};
use crate::views::{day_label, device_icon, place};

/// The country filter's choices: every country the list reads from.
pub const COUNTRIES: &[(&str, &str)] = &[
    ("AE", "United Arab Emirates"),
    ("AR", "Argentina"),
    ("AU", "Australia"),
    ("BR", "Brazil"),
    ("CA", "Canada"),
    ("DE", "Germany"),
    ("ES", "Spain"),
    ("FR", "France"),
    ("GB", "United Kingdom"),
    ("IE", "Ireland"),
    ("IN", "India"),
    ("IT", "Italy"),
    ("JP", "Japan"),
    ("KE", "Kenya"),
    ("KR", "South Korea"),
    ("MX", "Mexico"),
    ("NG", "Nigeria"),
    ("NL", "Netherlands"),
    ("NZ", "New Zealand"),
    ("PL", "Poland"),
    ("PT", "Portugal"),
    ("SE", "Sweden"),
    ("SG", "Singapore"),
    ("US", "United States"),
    ("ZA", "South Africa"),
];

/// The id of the sheet a row opens.
pub const SHEET: &str = "recipient";

/// A recipient, as the table shows it. Each column says what a reader can do
/// with it; the server runs the search, the filters, the sort and the pages.
#[derive(Clone, TableRow)]
#[table(sort = "-last_activity", per = 25, per_pages(10, 25, 50, 100))]
pub struct RecipientRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, sticky, label = "Recipient")]
    pub name: String,
    #[col(search, hidden)]
    pub email: String,
    // A relation's choices come from the page: each status with how many hold it.
    #[col(sortable, filter = relation)]
    pub status: String,
    #[col(filter = multi, options = COUNTRIES, hide_on_mobile, label = "Location")]
    pub country: String,
    #[col(skip)]
    pub city: String,
    #[col(hide_on_mobile)]
    pub device: String,
    #[col(sortable, numeric)]
    pub opens: i64,
    #[col(sortable, numeric)]
    pub clicks: i64,
    #[col(sortable, filter = date, label = "Last activity")]
    pub last_activity: Option<NaiveDate>,
}

impl From<Recipient> for RecipientRow {
    fn from(recipient: Recipient) -> Self {
        Self {
            id: recipient.id,
            name: recipient.name,
            email: recipient.email,
            status: recipient.status,
            country: recipient.country,
            city: recipient.city,
            device: recipient.device,
            opens: recipient.opens,
            clicks: recipient.clicks,
            last_activity: recipient.last_activity,
        }
    }
}

/// A status as a badge with a dot, in its tone.
pub fn status_badge(key: &str) -> Badge {
    let status = Status::parse(key).unwrap_or(Status::Delivered);
    Badge::new(status.label()).tone(status.tone()).dot()
}

/// The recipient's name, which opens their sheet, and their address under it.
/// With JavaScript off the name goes to the recipient's own page.
fn person_cell(row: &RecipientRow) -> Node {
    let open = el("form")
        .attr("method", "get")
        .attr("action", format!("/recipients/{}/peek", row.id))
        .flag("data-ui-form", true)
        .uses(Uses::CORE)
        .child(
            Button::new(row.name.clone())
                .variant(ButtonVariant::Link)
                .submit()
                .attr("aria-haspopup", "dialog"),
        );
    Stack::gap(0)
        .children((open, Text::muted(row.email.clone())))
        .into()
}

/// A flag, the city and the country.
fn place_cell(row: &RecipientRow) -> Node {
    Row::gap(2)
        .align(Align::Center)
        .children((
            Flag::country(&row.country).size(FlagSize::Xs).decorative(),
            Text::new(place(&row.city, &row.country)).inline(),
        ))
        .into()
}

fn device_cell(row: &RecipientRow) -> Node {
    Row::gap(2)
        .align(Align::Center)
        .children((
            icon(device_icon(&row.device)),
            Text::new(row.device.clone()).inline(),
        ))
        .into()
}

/// The recipients table: one definition drives the query, the filters with
/// their counts, the drawing and the live refresh.
pub fn recipients_table<'a>(
    query: &'a TableQuery<RecipientRow>,
    page: &'a TablePage<RecipientRow>,
    counts: &[(Status, u64)],
    today: NaiveDate,
) -> DataTable<'a, RecipientRow> {
    let statuses = counts
        .iter()
        .map(|(status, n)| FilterOption::new(status.key(), status.label()).count(*n))
        .collect();
    DataTable::new("recipients", query, page)
        .caption("Recipients of Spring launch")
        .options("status", statuses)
        .cell("name", person_cell)
        .cell("status", |row| status_badge(&row.status))
        .cell("country", place_cell)
        .cell("device", device_cell)
        .cell("last_activity", move |row| {
            Text::new(day_label(row.last_activity, today)).inline()
        })
        .filter_bar()
        .sticky_header()
        .live_from("/events")
        .live_event("recipients")
        .empty(
            Empty::new("No recipient matches")
                .icon(icons::USER_SEARCH)
                .description("Clear a filter, or search for part of a name or an address."),
        )
}

/// The sheet a row opens, before it is filled.
pub fn empty_sheet() -> Sheet {
    Sheet::new(SHEET, "Recipient").width(SheetWidth::Lg)
}
