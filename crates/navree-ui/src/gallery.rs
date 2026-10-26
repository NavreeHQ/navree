//! The component gallery: every component of the kit on one page, plus an
//! example dashboard and a sign-in page, with sample data. [`UiModule`]
//! serves it at `{mount}/gallery` in development; [`write`] renders it to
//! static HTML files (with the assets next to them) to look at in a browser.
//!
//! [`UiModule`]: crate::UiModule

use std::path::Path;

use navree_view::{ViewError, Views};
use serde_json::{Value, json};

use crate::{
    Action, Cell, Chart, ChartSize, Column, Command, CommandGroup, Crumb, Empty, FormErrors,
    MenuItem, NavGroup, NavItem, PageInfo, Row, SeriesTone, Shell, Sparkline, Table, Toast, Tone,
    UserMenu, assets,
};

/// A gallery page.
#[derive(Clone, Copy, Debug)]
pub struct GalleryPage {
    /// The path segment after `{mount}/gallery` (`""` for the components).
    pub slug: &'static str,
    /// The route name.
    pub route: &'static str,
    /// The template.
    pub template: &'static str,
    /// The file name when written to disk.
    pub file: &'static str,
}

/// The gallery's pages: the components, an example dashboard, sign-in.
pub const PAGES: [GalleryPage; 3] = [
    GalleryPage {
        slug: "",
        route: "ui.gallery",
        template: "ui/gallery.html",
        file: "index.html",
    },
    GalleryPage {
        slug: "dashboard",
        route: "ui.gallery.dashboard",
        template: "ui/gallery-dashboard.html",
        file: "dashboard.html",
    },
    GalleryPage {
        slug: "sign-in",
        route: "ui.gallery.sign_in",
        template: "ui/gallery-sign-in.html",
        file: "sign-in.html",
    },
];

/// The gallery's own templates.
pub const GALLERY_TEMPLATES: &[(&str, &str)] = &[
    ("ui/gallery.html", include_str!("../templates/ui/gallery.html")),
    (
        "ui/gallery-dashboard.html",
        include_str!("../templates/ui/gallery-dashboard.html"),
    ),
    (
        "ui/gallery-sign-in.html",
        include_str!("../templates/ui/gallery-sign-in.html"),
    ),
];

/// Where the gallery's pages link to each other.
#[derive(Clone, Debug)]
pub struct Links {
    components: String,
    dashboard: String,
    sign_in: String,
}

impl Links {
    /// Links between the routes under `mount` (`/_navree/ui/gallery`, ...).
    pub fn mounted(mount: &str) -> Self {
        let base = format!("{}/gallery", mount.trim_end_matches('/'));
        Self {
            components: base.clone(),
            dashboard: format!("{base}/dashboard"),
            sign_in: format!("{base}/sign-in"),
        }
    }

    /// Links between files in one directory (`index.html`, ...).
    pub fn files() -> Self {
        Self {
            components: "index.html".to_owned(),
            dashboard: "dashboard.html".to_owned(),
            sign_in: "sign-in.html".to_owned(),
        }
    }

    fn of(&self, slug: &str) -> &str {
        match slug {
            "dashboard" => &self.dashboard,
            "sign-in" => &self.sign_in,
            _ => &self.components,
        }
    }
}

/// [`Views`] with the kit's and the gallery's templates.
pub fn views() -> Result<Views, ViewError> {
    crate::views_with(GALLERY_TEMPLATES.iter().copied())
}

/// Renders gallery page `slug` (`""`, `"dashboard"` or `"sign-in"`) with
/// the assets at `assets_url`.
pub fn render(views: &Views, slug: &str, assets_url: &str, links: &Links) -> Result<String, ViewError> {
    let page = PAGES
        .iter()
        .find(|page| page.slug == slug)
        .unwrap_or(&PAGES[0]);
    views.render(page.template, context(slug, assets_url, links))
}

/// Writes every gallery page to `dir` (`index.html`, `dashboard.html`,
/// `sign-in.html`) and the assets to `dir/assets`, so the files open
/// directly in a browser.
pub fn write(dir: &Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    std::fs::create_dir_all(dir)?;
    assets::write_all(&dir.join("assets"))?;
    let views = views()?;
    let links = Links::files();
    for page in PAGES {
        let html = render(&views, page.slug, "assets", &links)?;
        std::fs::write(dir.join(page.file), html)?;
    }
    Ok(())
}

/// The context of gallery page `slug`.
pub fn context(slug: &str, assets_url: &str, links: &Links) -> Value {
    let (title, crumbs) = match slug {
        "dashboard" => ("Dashboard", vec![Crumb::page("Dashboard")]),
        "sign-in" => ("Sign in", vec![Crumb::page("Sign in")]),
        _ => (
            "Components",
            vec![Crumb::link("Develop", links.of("")), Crumb::page("Components")],
        ),
    };
    let shell = shell(links)
        .assets(assets_url)
        .title(title)
        .current(links.of(slug))
        .crumbs(crumbs);
    json!({
        "shell": shell,
        "links": {
            "components": links.of(""),
            "dashboard": links.of("dashboard"),
            "sign_in": links.of("sign-in"),
        },
        "users": users_table(),
        "deals": deals_table(),
        "empty_table": empty_table(),
        "loading_table": loading_table(),
        "requests": requests_chart(),
        "signups": signups_chart(),
        "latency": latency_chart(),
        "sparks": {
            "revenue": Sparkline::new([12.0, 14.0, 13.0, 17.0, 16.0, 19.0, 22.0, 21.0, 25.0]),
            "users": Sparkline::new([30.0, 32.0, 31.0, 35.0, 38.0, 37.0, 41.0, 44.0, 46.0]),
            "errors": Sparkline::new([9.0, 7.0, 8.0, 6.0, 7.0, 5.0, 4.0, 5.0, 3.0]).trend("up"),
            "latency": Sparkline::new([42.0, 44.0, 41.0, 48.0, 52.0, 49.0, 55.0, 58.0, 61.0]).trend("down"),
        },
        "form_errors": form_errors(),
        "roles": [["admin", "Admin"], ["manager", "Manager"], ["rep", "Sales rep"]],
        "timezones": ["UTC", "Europe/London", "America/New_York", "Asia/Kolkata"],
        "menu": dropdown_menu(),
        "activity": activity(),
        "details": [
            ["Workspace", "Acme Inc."],
            ["Plan", "Business, billed yearly"],
            ["Region", "eu-west-1"],
            ["Created", "12 March 2026"],
        ],
    })
}

trait ShellExt {
    fn crumbs(self, crumbs: Vec<Crumb>) -> Self;
}

impl ShellExt for Shell {
    fn crumbs(mut self, crumbs: Vec<Crumb>) -> Self {
        for crumb in crumbs {
            self = self.crumb(crumb);
        }
        self
    }
}

/// The gallery's shell: a Helm-like admin panel of Acme Inc.
fn shell(links: &Links) -> Shell {
    Shell::new("Helm")
        .context("Acme Inc.")
        .home(links.of("dashboard"))
        .switch(MenuItem::label("Tools"))
        .switch(
            MenuItem::link("Helm", links.of("dashboard"))
                .icon("ship-wheel")
                .description("Admin panel")
                .shortcut("Ctrl 1")
                .checked(true),
        )
        .switch(
            MenuItem::link("Lighthouse", "#lighthouse")
                .icon("activity")
                .description("Errors and performance")
                .shortcut("Ctrl 2")
                .checked(false),
        )
        .switch(
            MenuItem::link("Fleet", "#fleet")
                .icon("boxes")
                .description("Queues and workers")
                .shortcut("Ctrl 3")
                .checked(false),
        )
        .switch(
            MenuItem::link("Logbook", "#logbook")
                .icon("scroll-text")
                .description("Requests, queries and jobs")
                .shortcut("Ctrl 4")
                .checked(false),
        )
        .switch(MenuItem::separator())
        .switch(MenuItem::link("Add a workspace", "#workspaces/new").icon("plus"))
        .group(
            NavGroup::new("Platform")
                .item(
                    NavItem::new("Dashboard", links.of("dashboard")).icon("layout-dashboard"),
                )
                .item(
                    NavItem::new("Users", "#users")
                        .icon("users")
                        .badge("1,204")
                        .child("All users", "#users")
                        .child_with_badge("Invitations", "#users/invitations", "3")
                        .child("Roles and permissions", "#users/roles"),
                )
                .item(NavItem::new("Workspaces", "#workspaces").icon("building-2"))
                .item(NavItem::new("Billing", "#billing").icon("credit-card")),
        )
        .group(
            NavGroup::new("Operate")
                .item(NavItem::new("Lighthouse", "#lighthouse").icon("activity").badge("12"))
                .item(NavItem::new("Fleet", "#fleet").icon("boxes"))
                .item(NavItem::new("Logbook", "#logbook").icon("scroll-text"))
                .item(NavItem::new("Schedule", "#schedule").icon("clock")),
        )
        .group(
            NavGroup::new("Develop")
                .item(NavItem::new("Components", links.of("")).icon("layout-grid"))
                .item(NavItem::new("Sign-in page", links.of("sign-in")).icon("log-in"))
                .item(NavItem::new("Settings", "#settings").icon("settings")),
        )
        .user(
            UserMenu::new("Ada Lovelace", "ada@acme.test")
                .item(MenuItem::link("Account", "#account").icon("user-round"))
                .item(MenuItem::link("Notifications", "#notifications").icon("bell"))
                .item(MenuItem::link("Keyboard shortcuts", "#shortcuts").icon("keyboard"))
                .item(MenuItem::separator())
                .item(MenuItem::link("Sign out", links.of("sign-in")).icon("log-out")),
        )
        .commands(
            CommandGroup::new("Actions")
                .item(Command::dialog("Invite a user", "invite-dialog").icon("user-plus").hint("Users"))
                .item(Command::link("Create a workspace", "#workspaces/new").icon("plus"))
                .item(Command::link("Run a scheduled task", "#schedule").icon("play").keywords("cron job"))
                .item(Command::link("Open the documentation", "https://navree.com/docs").icon("book-open").hint("navree.com")),
        )
        .toast(Toast::success("Signed in").description("Welcome back, Ada."))
}

struct User {
    id: &'static str,
    name: &'static str,
    email: &'static str,
    role: &'static str,
    status: (&'static str, Tone),
    deals: &'static str,
    seen: &'static str,
}

const USERS: [User; 8] = [
    User { id: "u_01", name: "Ada Lovelace", email: "ada@acme.test", role: "Admin", status: ("Active", Tone::Success), deals: "24", seen: "2 min ago" },
    User { id: "u_02", name: "Grace Hopper", email: "grace@acme.test", role: "Manager", status: ("Active", Tone::Success), deals: "18", seen: "1 hour ago" },
    User { id: "u_03", name: "Alan Turing", email: "alan@acme.test", role: "Sales rep", status: ("Invited", Tone::Warning), deals: "0", seen: "Never" },
    User { id: "u_04", name: "Katherine Johnson", email: "katherine@acme.test", role: "Sales rep", status: ("Active", Tone::Success), deals: "31", seen: "Yesterday" },
    User { id: "u_05", name: "Edsger Dijkstra", email: "edsger@acme.test", role: "Sales rep", status: ("Suspended", Tone::Danger), deals: "7", seen: "3 weeks ago" },
    User { id: "u_06", name: "Barbara Liskov", email: "barbara@acme.test", role: "Manager", status: ("Active", Tone::Success), deals: "15", seen: "5 hours ago" },
    User { id: "u_07", name: "Donald Knuth", email: "donald@acme.test", role: "Sales rep", status: ("Active", Tone::Success), deals: "11", seen: "2 days ago" },
    User { id: "u_08", name: "Margaret Hamilton", email: "margaret@acme.test", role: "Admin", status: ("Active", Tone::Success), deals: "9", seen: "Today" },
];

fn users_table() -> Table {
    let rows = USERS.iter().enumerate().map(|(i, user)| {
        let row = Row::new(user.id)
            .cell(Cell::lines(user.name, user.email).href(format!("#users/{}", user.id)))
            .cell(Cell::badge(user.role, Tone::Neutral))
            .cell(Cell::status(user.status.0, user.status.1))
            .cell(Cell::number(user.deals))
            .cell(Cell::muted(user.seen))
            .action(MenuItem::link("View profile", format!("#users/{}", user.id)).icon("user-round"))
            .action(MenuItem::link("Edit", format!("#users/{}/edit", user.id)).icon("pencil"))
            .action(MenuItem::link("Copy ID", "#copy").icon("copy").shortcut(user.id))
            .action(MenuItem::separator())
            .action(MenuItem::dialog("Remove", "remove-dialog").icon("trash-2").danger());
        if i == 1 { row.selected() } else { row }
    });
    Table::new("users")
        .caption("Users")
        .column(Column::new("name", "Name").sortable())
        .column(Column::new("role", "Role").sortable())
        .column(Column::new("status", "Status"))
        .column(Column::new("deals", "Open deals").numeric().sortable())
        .column(Column::new("seen", "Last seen").hide_on_mobile())
        .rows(rows)
        .bulk(Action::link("Export", "#export").icon("download").ghost())
        .bulk(Action::dialog("Remove", "remove-dialog").icon("trash-2").danger())
        .sort("name", "?sort={sort}")
        .page(PageInfo::new(1, 8, 1204), "?page={page}")
        .per_page("?per_page={per_page}", &[8, 15, 25, 50])
}

fn deals_table() -> Table {
    let deals = [
        ("d_311", "Northwind renewal", "Northwind Traders", ("Won", Tone::Success), "$48,000", true),
        ("d_310", "Globex expansion", "Globex Corporation", ("Negotiation", Tone::Neutral), "$126,500", false),
        ("d_309", "Initech pilot", "Initech", ("Proposal", Tone::Neutral), "$12,800", false),
        ("d_308", "Umbrella security audit", "Umbrella Corp.", ("Lost", Tone::Outline), "$64,000", false),
        ("d_307", "Hooli workspace seats", "Hooli", ("Won", Tone::Success), "$9,600", true),
    ];
    Table::new("deals")
        .caption("Recent deals")
        .column(Column::new("title", "Deal"))
        .column(Column::new("company", "Company").hide_on_mobile())
        .column(Column::new("stage", "Stage"))
        .column(Column::new("amount", "Amount").numeric())
        .rows(deals.into_iter().map(|(id, title, company, stage, amount, won)| {
            Row::new(id)
                .cell(Cell::strong(title).href(format!("#deals/{id}")))
                .cell(Cell::muted(company))
                .cell(Cell::badge(stage.0, stage.1))
                .cell(if won { Cell::positive(amount) } else { Cell::number(amount) })
        }))
}

fn empty_table() -> Table {
    Table::new("webhooks")
        .caption("Webhook endpoints")
        .column(Column::new("url", "Endpoint"))
        .column(Column::new("events", "Events"))
        .column(Column::new("status", "Status"))
        .empty(
            Empty::new("No webhook endpoints")
                .description("Endpoints receive signed events when records change. Add one to start receiving deliveries.")
                .icon("webhook")
                .action(Action::dialog("Add endpoint", "invite-dialog").icon("plus")),
        )
        .page(PageInfo::new(1, 15, 0), "?page={page}")
}

fn loading_table() -> Table {
    Table::new("jobs")
        .caption("Jobs")
        .column(Column::new("job", "Job"))
        .column(Column::new("queue", "Queue"))
        .column(Column::new("status", "Status"))
        .column(Column::new("runtime", "Runtime").numeric())
        .selectable()
        .loading(4)
}

fn requests_chart() -> Chart {
    let days: Vec<String> = (1..=30).map(|d| format!("Oct {d}")).collect();
    let values = [
        412.0, 438.0, 401.0, 455.0, 498.0, 362.0, 341.0, 470.0, 512.0, 534.0, 501.0, 548.0, 402.0,
        388.0, 561.0, 589.0, 602.0, 577.0, 631.0, 455.0, 431.0, 644.0, 672.0, 698.0, 655.0, 712.0,
        540.0, 518.0, 731.0, 768.0,
    ];
    let last_month = values.iter().enumerate().map(|(i, v)| v * 0.82 + (i % 5) as f64 * 9.0);
    Chart::lines(days)
        .series("Requests per minute", SeriesTone::Accent, values)
        .dashed_series("Previous 30 days", SeriesTone::Muted, last_month)
}

fn signups_chart() -> Chart {
    Chart::bars(["May", "Jun", "Jul", "Aug", "Sep", "Oct"])
        .series("Sign-ups", SeriesTone::Muted, [182.0, 214.0, 198.0, 263.0, 241.0, 318.0])
        .highlight(5)
        .size(ChartSize::Medium)
}

fn latency_chart() -> Chart {
    let hours: Vec<String> = (0..24).map(|h| format!("{h:02}:00")).collect();
    let p50 = (0..24).map(|h| 38.0 + ((h as f64) / 3.0).sin().abs() * 14.0);
    let p95 = (0..24).map(|h| 96.0 + ((h as f64) / 2.5).sin().abs() * 60.0);
    Chart::bars(hours)
        .series("p50", SeriesTone::Muted, p50)
        .series("p95", SeriesTone::Amber, p95)
        .suffix(" ms")
        .size(ChartSize::Small)
        .label_every(4)
}

fn form_errors() -> FormErrors {
    FormErrors::from_body(&json!({
        "statusCode": 400,
        "message": ["email must be a valid email address", "name is required"],
        "error": "Bad Request",
        "errors": {
            "email": ["email must be a valid email address"],
            "name": ["name is required"]
        }
    }))
}

fn dropdown_menu() -> Vec<MenuItem> {
    vec![
        MenuItem::label("Columns"),
        MenuItem::link("Name", "#").checked(true),
        MenuItem::link("Role", "#").checked(true),
        MenuItem::link("Last seen", "#").checked(false),
        MenuItem::separator(),
        MenuItem::link("Export as CSV", "#export").icon("download").shortcut("Ctrl E"),
        MenuItem::link("Import users", "#import").icon("upload"),
        MenuItem::separator(),
        MenuItem::dialog("Delete all", "remove-dialog").icon("trash-2").danger(),
    ]
}

fn activity() -> Value {
    json!([
        { "icon": "user-plus", "text": "Grace Hopper invited Alan Turing", "time": "2 min ago" },
        { "icon": "circle-check", "text": "Deal Northwind renewal marked won", "time": "18 min ago" },
        { "icon": "webhook", "text": "Webhook endpoint billing.acme.test disabled after 5 failures", "time": "1 hour ago" },
        { "icon": "key-round", "text": "Ada Lovelace created an API token", "time": "3 hours ago" },
        { "icon": "refresh-cw", "text": "Nightly export finished in 42 s", "time": "Yesterday" },
    ])
}
