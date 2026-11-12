//! `ui-proof`: navree-ui's proof pages, as one binary.
//!
//! ```text
//! cargo run --release -p ui-proof -- serve          # http://127.0.0.1:8090
//! PORT=9000 ui-proof serve                          # another port
//! ui-proof bench                                    # render times, no server
//! UI_PROOF_LOGBOOK=1 ui-proof serve                 # with Logbook at /_logbook (build with --features logbook)
//! ```
//!
//! | Page | Shows |
//! |---|---|
//! | `/` | a dashboard: stats, a lazy region, a polling region |
//! | `/deals?per_page=1000` | the 1,000-row table: sort, filter, page in the URL, morphed |
//! | `/deals/new` | the deal form |
//! | `/plain` | a page with nothing interactive: no script at all |
//! | `/gallery` | every component in its states |
//! | `/menus` | tabs, toggle groups, dropdown menus and toasts in their states |
//! | `/menubar` | context menus, menu bars and the navigation menu in their states |
//! | `/phone` | a contact form with phone numbers, and the phone input in its states |
//! | `/display` | the timeline, activity feed, fact cards and badges in every state |
//! | `/display/live` | a live feed and a live table on one server-sent event stream |
//! | `/themes` | every theme in light and dark, the theme menu, `?theme=` and `?mode=` |
//! | `/themes/layouts/{layout}` | the same page in each layout, folding three ways |
//! | `/themes/builder` | the theme builder, saving to memory and importing |
//! | `/themes/scopes` | the app's, a workspace's (locked or not) and a user's choice |
//! | `/pages` | error pages, sign-in and the states inside a page, in every case |
//! | `/flow` | the stepper, slider, tree view and toolbar in every state, and a wizard at `/flow/wizard/start` |
//! | `/markdown` | Markdown and code blocks: a reply, tables, links, pictures, raw HTML, code in every language, and `/markdown/render?text=` for any text |
//! | `/upload` | file uploads: a photo, documents with chunks, a form sent by hand, a field in a form, and the states |
//! | `/otp` | one-time code fields of every length and state, a form that posts them and one that sends itself |
//! | `/carousel` | carousels with buttons, pickers, going round, rotation, two in view, vertical and right to left |
//! | `/charts` | every chart type in its states; `/charts/report` a report over a window |
//! | `/charts/grids` | the calendar and matrix heat maps and the scatter plot in every state |
//! | `/islands` | a part of the page drawn in the browser by a script of the app's own, through a morph and away |
//! | `/parts` | the small parts of the base components, the colour picker, an avatar group, a trace, a questionnaire, a select with flags, flags by pointer and a sparkline in each row |
//! | `/site` | a public website of marketing blocks: home, pricing, privacy, contact, sitemap and robots |
//! | `/compare/{screen}` | the four screens that measure speed: `table`, `select`, `dates` and `tabs` |
#![forbid(unsafe_code)]

use std::sync::Arc;

use navree::http::extract::Request;
use navree::http::middleware::{self, Next};
use navree::http::response::Response;
use navree::http::{GlobalLayer, LayerSlot};
#[cfg(feature = "logbook")]
use navree::logbook::{LogbookOptions, StoreKind};
use navree::prelude::*;
use navree::ui::{NavGroup, NavItem, PagesOptions, Shell, UiModule, UserMenu, icons};

mod agra_theme;
mod bench;
mod charts;
mod compare;
mod dashboard;
mod dates;
mod deal_form;
mod deals;
mod display;
mod flow;
mod gallery;
mod grids;
mod icons_flags;
mod islands;
mod markdown;
mod menubar;
mod menus;
mod otp_carousel;
mod pages;
mod parts;
mod phone;
mod select;
mod site;
mod themes;
mod upload;

/// The app's one shell.
pub(crate) fn shell() -> Shell {
    Shell::new("Navree CRM")
        .context("Proof pages")
        .group(
            NavGroup::new("Sales")
                .item(NavItem::new("Overview", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(
                    NavItem::new("Deals", "/deals?per_page=1000")
                        .icon(icons::HANDSHAKE)
                        .count("1000"),
                )
                .item(NavItem::new("New deal", "/deals/new").icon(icons::CIRCLE_PLUS))
                .item(NavItem::new("Dates", "/dates").icon(icons::CALENDAR))
                .item(NavItem::new("Phone numbers", "/phone").icon(icons::PHONE)),
        )
        .group(
            NavGroup::new("System")
                .item(NavItem::new("Plain page", "/plain").icon(icons::FILE))
                .item(
                    NavItem::new("Gallery", "/gallery")
                        .icon(icons::LAYOUT_GRID)
                        .prefetch(),
                )
                .item(NavItem::new("Select", "/select").icon(icons::LIST))
                .item(NavItem::new("Icons and flags", "/icons-flags").icon(icons::FLAG))
                .item(NavItem::new("Menus and toasts", "/menus").icon(icons::LIST))
                .item(NavItem::new("Menu bars", "/menubar").icon(icons::LIST))
                .item(NavItem::new("States", "/pages").icon(icons::SHIELD_ALERT))
                .item(NavItem::new("Display", "/display").icon(icons::LAYERS))
                .item(NavItem::new("Live activity", "/display/live").icon(icons::ACTIVITY))
                .item(NavItem::new("Flow", "/flow").icon(icons::LIST_TREE))
                .item(NavItem::new("One-time codes", "/otp").icon(icons::SHIELD_ALERT))
                .item(NavItem::new("Carousels", "/carousel").icon(icons::LAYOUT_GRID))
                .item(NavItem::new("Upload", "/upload").icon(icons::UPLOAD))
                .item(NavItem::new("Markdown", "/markdown").icon(icons::FILE))
                .item(NavItem::new("Charts", "/charts").icon(icons::CHART_LINE))
                .item(NavItem::new("Report", "/charts/report").icon(icons::CHART_COLUMN)),
        )
        .user(
            UserMenu::new("Anu Lakhotia", "anu@navree.test")
                .link("Settings", "/plain")
                .action("Sign out", "/sign-out"),
        )
}

struct ProofModule;

impl Module for ProofModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(Factory::<Shell>::new(|_| async { Ok(Arc::new(shell())) }))
            .exports::<Shell>();
        // The upload a claim form's receipt field stores its file with, named after the field.
        m.factory(
            Factory::<navree::ui::Uploads>::new(|_| async { Ok(Arc::new(upload::receipts())) })
                .named("receipt"),
        );
        m.factory(Factory::<dyn navree::http::Authenticator>::new(|_| async {
            Ok(Arc::new(pages::CookieAuth) as Arc<dyn navree::http::Authenticator>)
        }))
        .exports::<dyn navree::http::Authenticator>();
        m.http_layer(GlobalLayer::new(
            LayerSlot::Context,
            "proof.timing",
            |_, router| Ok(router.layer(middleware::from_fn(timing))),
        ));
        m.controller::<deals::DealPages>()
            .controller::<dashboard::DashboardPages>()
            .controller::<gallery::GalleryPages>()
            .controller::<select::SelectPages>()
            .controller::<icons_flags::IconPages>()
            .controller::<dates::DatePages>()
            .controller::<menus::MenuPages>()
            .controller::<menubar::BarPages>()
            .controller::<phone::PhonePages>()
            .controller::<display::DisplayPages>()
            .controller::<themes::ThemePages>()
            .controller::<pages::Pages>()
            .controller::<flow::FlowPages>()
            .controller::<otp_carousel::CodePages>()
            .controller::<upload::UploadPages>()
            .controller::<upload::ClaimPages>()
            .controller::<markdown::MarkdownPages>()
            .controller::<charts::ChartPages>()
            .controller::<grids::GridPages>()
            .controller::<islands::IslandPages>()
            .controller::<compare::ComparePages>()
            .controller::<parts::PartsPages>()
            .controller::<site::SitePages>();
        // The script the islands page draws its island with, served by the toolkit.
        m.contribute(islands::TALLY);
    }
}

/// `Server-Timing: app`: the time from the router to the finished response,
/// so a benchmark can tell the app's time from the network's.
async fn timing(request: Request, next: Next) -> Response {
    let start = std::time::Instant::now();
    let mut response = next.run(request).await;
    if let Ok(value) = http::HeaderValue::from_str(&format!(
        "app;dur={:.3}",
        start.elapsed().as_secs_f64() * 1000.0
    )) {
        response.headers_mut().insert("server-timing", value);
    }
    response
}

fn main() -> Result<(), BoxError> {
    if std::env::args().nth(1).as_deref() == Some("bench") {
        bench::run();
        return Ok(());
    }
    let port = std::env::var("PORT").unwrap_or_else(|_| "8090".to_owned());
    let settings = vec![
        ("APP_HTTP__ADDR".to_owned(), format!("127.0.0.1:{port}")),
        // The benchmark sends every request from one address.
        (
            "APP_HTTP__PER_IP_RATE_LIMIT".to_owned(),
            "100000000".to_owned(),
        ),
        // A browser suite keeps hundreds of idle connections open from this one address,
        // and Firefox keeps them between pages; the default cap of 256 closes every one
        // over it. A test of many open event streams can ask for another number.
        (
            "APP_HTTP__SERVER__MAX_CONNECTIONS_PER_IP".to_owned(),
            std::env::var("UI_PROOF_CONNECTIONS_PER_IP").unwrap_or_else(|_| "100000".to_owned()),
        ),
    ];
    let sources = ConfigSources::new().env_vars("APP_", settings);
    let app = Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpModule::from_config())
        .module(UiModule::with_pages(
            // Error pages show their details only when asked: a development build of the
            // proof app must look like the release one.
            PagesOptions::default().debug(std::env::var_os("UI_PROOF_DEBUG").is_some()),
        ))
        .module(ProofModule);
    // A development build only (`--features logbook`), and then only when asked: it
    // records every request, which a speed measurement shouldn't pay for.
    #[cfg(feature = "logbook")]
    if std::env::var_os("UI_PROOF_LOGBOOK").is_some() {
        let options = LogbookOptions {
            enabled: Some(true),
            store: StoreKind::Memory,
            ..LogbookOptions::default()
        };
        return app.module(LogbookModule::for_root(options)).run();
    }
    app.run()
}
