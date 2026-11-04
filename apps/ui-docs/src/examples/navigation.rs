//! The app shell, Breadcrumb, Pagination.

use navree::ui::*;

use crate::catalog::{Example, gallery, page, single};

const SOURCE: &str = include_str!("navigation.rs");

pub fn all() -> Vec<Example> {
    vec![
        page!("app-shell", "default", "The app shell", app_shell_default)
            .fill()
            .width(1100)
            .height(640),
        page!(
            "app-shell",
            "full",
            "A whole page: figures, a table and a drawer",
            app_shell_full
        )
        .fill()
        .width(1440)
        .height(840)
        .open(),
        single!("breadcrumb", "default", "A trail", breadcrumb_default),
        single!("pagination", "default", "Page 5 of 12", pagination_default),
        gallery!(
            "pagination",
            "edges",
            "First, middle and last",
            pagination_edges
        )
        .wide(),
    ]
}

fn app_shell_default() -> AppPage {
    let shell = Shell::new("Cloudworks CRM")
        .context("Production")
        .group(
            NavGroup::new("Sales")
                .item(NavItem::new("Overview", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(
                    NavItem::new("Deals", "/deals")
                        .icon(icons::HANDSHAKE)
                        .count("312"),
                )
                .item(NavItem::new("Contacts", "/contacts").icon(icons::USERS))
                .item(
                    NavItem::new("Reports", "/reports")
                        .icon(icons::CHART_COLUMN)
                        .sub(NavItem::new("Pipeline", "/reports/pipeline"))
                        .sub(NavItem::new("Forecast", "/reports/forecast")),
                ),
        )
        .group(
            NavGroup::new("Workspace")
                .item(NavItem::new("Settings", "/settings").icon(icons::SETTINGS)),
        )
        .user(
            UserMenu::new("Gurpreet Singh", "gurpreet@example.com")
                .link("Profile", "/profile")
                .action("Sign out", "/sign-out"),
        );
    shell
        .page("Deals", "/deals")
        .actions(
            Button::primary("New deal")
                .icon(icons::PLUS)
                .href("/deals/new"),
        )
        .body(Stack::gap(5).children((
            Grid::cols(3).gap(4).children((
                Stat::new("Open deals", "312").trend("+12 this week", true),
                Stat::new("Pipeline value", "₹4.2 crore").trend("+3.1%", true),
                Stat::new("Won this month", "₹64 lakh").trend("-2.4%", false),
            )),
            deals_table(),
        )))
}

/// A deal still open, as the pages' tables show it.
#[derive(Clone, TableRow)]
#[table(sort = "-amount", per = 8, per_pages(8, 25, 100))]
pub struct OpenDeal {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, sticky, link = "/deals/{id}", label = "Deal")]
    pub title: String,
    #[col(filter = multi, options = OWNERS, hide_on_mobile)]
    pub owner: &'static str,
    #[col(sortable, filter = multi, options = STAGES, badge = stage_tone)]
    pub stage: &'static str,
    #[col(sortable, numeric, filter = range, label = "Amount")]
    pub amount: i64,
    #[col(hide_on_mobile)]
    pub city: &'static str,
    #[col(sortable, filter = date, label = "Closes")]
    pub closes_on: chrono::NaiveDate,
}

const OWNERS: &[(&str, &str)] = &[
    ("Harleen Kaur", "Harleen Kaur"),
    ("Gurpreet Singh", "Gurpreet Singh"),
    ("Priya Verma", "Priya Verma"),
    ("Vikram Chopra", "Vikram Chopra"),
];

const STAGES: &[(&str, &str)] = &[
    ("Lead", "Lead"),
    ("Qualified", "Qualified"),
    ("Proposal", "Proposal"),
    ("Negotiation", "Negotiation"),
    ("Won", "Won"),
];

/// A stage's look in the table.
fn stage_tone(stage: &&'static str) -> BadgeVariant {
    match *stage {
        "Won" => BadgeVariant::Success,
        "Negotiation" => BadgeVariant::Warning,
        _ => BadgeVariant::Neutral,
    }
}

/// `₹48,00,000`: rupees grouped the Indian way.
fn rupees(amount: i64) -> String {
    format!("₹{}", store_demo::views::thousands(amount))
}

/// The eight largest open deals, the same on every run.
fn open_deals() -> Vec<OpenDeal> {
    let day = |m, d| chrono::NaiveDate::from_ymd_opt(2026, m, d).unwrap_or_default();
    [
        (
            1,
            "Sutlej Supply renewal",
            "Harleen Kaur",
            "Negotiation",
            4_800_000,
            "Ludhiana",
            day(11, 30),
        ),
        (
            2,
            "Rajpur Road Learning seats",
            "Priya Verma",
            "Proposal",
            2_730_000,
            "Dehradun",
            day(11, 26),
        ),
        (
            3,
            "Phulkari Textiles platform",
            "Gurpreet Singh",
            "Proposal",
            2_100_000,
            "Patiala",
            day(11, 18),
        ),
        (
            4,
            "Awadh Kitchens rollout",
            "Vikram Chopra",
            "Qualified",
            1_560_000,
            "Lucknow",
            day(12, 9),
        ),
        (
            5,
            "Haveli Home expansion",
            "Harleen Kaur",
            "Won",
            1_250_000,
            "Jaipur",
            day(11, 4),
        ),
        (
            6,
            "Kashi Yatra bookings",
            "Priya Verma",
            "Negotiation",
            975_000,
            "Varanasi",
            day(11, 20),
        ),
        (
            7,
            "Pind Pantry support",
            "Gurpreet Singh",
            "Qualified",
            640_000,
            "Amritsar",
            day(11, 12),
        ),
        (
            8,
            "Doaba Traders pilot",
            "Vikram Chopra",
            "Lead",
            320_000,
            "Jalandhar",
            day(11, 21),
        ),
    ]
    .map(
        |(id, title, owner, stage, amount, city, closes_on)| OpenDeal {
            id,
            title: title.to_owned(),
            owner,
            stage,
            amount,
            city,
            closes_on,
        },
    )
    .to_vec()
}

/// The open deals as a DataTable: rupees, a flag beside each city, days as `30 Nov`,
/// the filters and the pages.
fn deals_table() -> Node {
    let query = TableQuery::<OpenDeal>::parse("/deals", "per=8");
    let page = TablePage::new(open_deals(), 312);
    DataTable::new("deals", &query, &page)
        .caption("Open deals")
        .cell("amount", |deal| Text::new(rupees(deal.amount)).inline())
        .cell("closes_on", |deal| {
            Text::new(deal.closes_on.format("%-d %b").to_string()).inline()
        })
        .cell("city", |deal| {
            Row::gap(2).align(Align::Center).children((
                Flag::country("IN").size(FlagSize::Xs).decorative(),
                Text::new(deal.city).inline(),
            ))
        })
        .filter_bar()
        .into()
}

/// The CRM's shell, with how many deals, contacts and companies it holds.
fn crm_shell() -> Shell {
    Shell::new("Cloudworks CRM")
        .context("Sales, Chandigarh")
        .group(
            NavGroup::new("Sales")
                .item(NavItem::new("Overview", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(
                    NavItem::new("Deals", "/deals")
                        .icon(icons::HANDSHAKE)
                        .count("312"),
                )
                .item(
                    NavItem::new("Contacts", "/contacts")
                        .icon(icons::USERS)
                        .count("1,284"),
                )
                .item(
                    NavItem::new("Companies", "/companies")
                        .icon(icons::BUILDING_COMPLEX)
                        .count("486"),
                )
                .item(NavItem::new("Reports", "/reports").icon(icons::CHART_COLUMN)),
        )
        .group(
            NavGroup::new("Workspace")
                .item(NavItem::new("Settings", "/settings").icon(icons::SETTINGS)),
        )
        .user(
            UserMenu::new("Harleen Kaur", "harleen@example.com")
                .link("Profile", "/profile")
                .action("Sign out", "/sign-out"),
        )
}

/// The same CRM, filled: the figures, the open deals, and one deal open in a
/// drawer over them with its history.
fn app_shell_full() -> AppPage {
    let drawer = Sheet::new("deal", "Sutlej Supply renewal")
        .width(SheetWidth::Lg)
        .description("Sutlej Supply Co., Ludhiana, Punjab")
        .body(
            Stack::gap(4).children((
                Row::gap(2).children((
                    Badge::new("Negotiation")
                        .variant(BadgeVariant::Warning)
                        .dot(),
                    Badge::new("Closes 30 Nov").icon(icons::CALENDAR),
                )),
                FactGrid::new().cols(2).cards([
                    FactCard::new("Amount", "₹48,00,000")
                        .icon(icons::INDIAN_RUPEE)
                        .detail("120 seats, for a year"),
                    FactCard::new("Contact", "Rajveer Brar")
                        .icon(icons::USER_ROUND)
                        .detail("+91 98765 43210"),
                ]),
                Timeline::new("deal-history", "Activity")
                    .heading(3)
                    .time_zone("Asia/Kolkata", 19_800)
                    .events([
                        TimelineEvent::new("Harleen Kaur created the deal")
                            .at(1_793_162_700)
                            .detail("From the Ludhiana trade fair")
                            .icon(icons::CIRCLE_PLUS),
                        TimelineEvent::new("Harleen Kaur called Rajveer Brar")
                            .at(1_793_268_600)
                            .detail("25 minutes: seats for three plants")
                            .icon(icons::PHONE)
                            .tone(Tone::Info),
                        TimelineEvent::new("Gurpreet Singh sent the proposal")
                            .at(1_793_424_900)
                            .detail("₹52,00,000 for 120 seats")
                            .icon(icons::SEND)
                            .tone(Tone::Info),
                        TimelineEvent::new("Rajveer Brar asked for 8% off")
                            .at(1_793_620_800)
                            .icon(icons::MESSAGE_SQUARE)
                            .tone(Tone::Warning),
                        TimelineEvent::new("Vikram Chopra approved the discount")
                            .at(1_793_681_400)
                            .detail("₹48,00,000, to close by 30 November")
                            .icon(icons::BADGE_CHECK)
                            .tone(Tone::Success),
                        TimelineEvent::new("Harleen Kaur moved it to Negotiation")
                            .at(1_793_772_900)
                            .icon(icons::MOVE_RIGHT)
                            .tone(Tone::Warning),
                        TimelineEvent::new("Contract sent for signature")
                            .time_text("Next")
                            .icon(icons::SIGNATURE)
                            .current(),
                    ]),
            )),
        );
    crm_shell()
        .page("Deals", "/deals")
        .crumb("Overview", "/")
        .actions((
            Button::outline("Export").icon(icons::DOWNLOAD),
            Button::primary("New deal")
                .icon(icons::PLUS)
                .href("/deals/new"),
        ))
        .body(Stack::gap(5).children((
            Grid::cols(4).gap(3).children((
                Stat::new("Pipeline", "₹4.2 crore").trend("+3.1% this month", true),
                Stat::new("Won this month", "₹68.5 lakh").trend("+12.4% on October", true),
                Stat::new("Open deals", "312").trend("+12 this week", true),
                Stat::new("Win rate", "38%").trend("-2.1 points", false),
            )),
            deals_table(),
            // Drawn open over the page, as it looks after a click on a deal.
            el("div").flag("data-preview-open", true).child(drawer),
        )))
}

fn breadcrumb_default() -> Breadcrumb {
    Breadcrumb::new()
        .link("Deals", "/deals")
        .link("Sutlej Supply", "/companies/sutlej-supply")
        .current("Sutlej Supply renewal")
}

fn pagination_default() -> Pagination {
    Pagination::new(5, 12, |page| format!("/deals?page={page}"))
}

fn pagination_edges() -> impl IntoIterator<Item = (&'static str, Pagination)> {
    [
        (
            "Page 1 of 12",
            Pagination::new(1, 12, |page| format!("?page={page}")),
        ),
        (
            "Page 6 of 12",
            Pagination::new(6, 12, |page| format!("?page={page}")),
        ),
        (
            "Page 12 of 12",
            Pagination::new(12, 12, |page| format!("?page={page}")),
        ),
        (
            "One page",
            Pagination::new(1, 1, |page| format!("?page={page}")),
        ),
    ]
}
