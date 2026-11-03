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
//! | `/deals?per=1000` | the 1,000-row table: sort, filter, page in the URL, morphed |
//! | `/deals/new` | the deal form |
//! | `/plain` | a page with nothing interactive: no script at all |
//! | `/gallery` | every component in its states |
//! | `/menus` | tabs, toggle groups, dropdown menus and toasts in their states |
//! | `/phone` | a contact form with phone numbers, and the phone input in its states |
#![forbid(unsafe_code)]

use std::sync::Arc;

use navree::http::extract::Request;
use navree::http::middleware::{self, Next};
use navree::http::response::Response;
use navree::http::{GlobalLayer, LayerSlot};
#[cfg(feature = "logbook")]
use navree::logbook::{LogbookOptions, StoreKind};
use navree::prelude::*;
use navree::ui::{NavGroup, NavItem, Shell, UiModule, UserMenu, icons};

mod bench;
mod dashboard;
mod dates;
mod deal_form;
mod deals;
mod gallery;
mod icons_flags;
mod menus;
mod phone;
mod select;

/// The app's one shell.
fn shell() -> Shell {
    Shell::new("Navree CRM")
        .context("Proof pages")
        .group(
            NavGroup::new("Sales")
                .item(NavItem::new("Overview", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(
                    NavItem::new("Deals", "/deals?per=1000")
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
                .item(NavItem::new("Menus and toasts", "/menus").icon(icons::LIST)),
        )
        .user(
            UserMenu::new("Ada Lovelace", "ada@navree.test")
                .link("Settings", "/plain")
                .action("Sign out", "/sign-out"),
        )
}

struct ProofModule;

impl Module for ProofModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(Factory::<Shell>::new(|_| async { Ok(Arc::new(shell())) }))
            .exports::<Shell>();
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
            .controller::<phone::PhonePages>();
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
    let sources = ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_HTTP__ADDR".to_owned(), format!("127.0.0.1:{port}")),
            // The benchmark sends every request from one address.
            (
                "APP_HTTP__PER_IP_RATE_LIMIT".to_owned(),
                "100000000".to_owned(),
            ),
            // A browser suite keeps hundreds of idle connections open from this one address,
            // and Firefox keeps them between pages; the default cap of 256 closes every one over it.
            (
                "APP_HTTP__SERVER__MAX_CONNECTIONS_PER_IP".to_owned(),
                "100000".to_owned(),
            ),
        ],
    );
    let app = Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpModule::from_config())
        .module(UiModule::from_config())
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
