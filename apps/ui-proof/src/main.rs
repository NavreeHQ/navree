//! `ui-proof`: navree-ui's proof pages, as one binary.
//!
//! ```text
//! cargo run --release -p ui-proof -- serve          # http://127.0.0.1:8090
//! PORT=9000 ui-proof serve                          # another port
//! ui-proof bench                                    # render times, no server
//! ```
//!
//! | Page | Shows |
//! |---|---|
//! | `/` | a dashboard: stats, a lazy region, a polling region |
//! | `/deals?per=1000` | the 1,000-row table: sort, filter, page in the URL, morphed |
//! | `/deals/new` | the deal form |
//! | `/plain` | a page with nothing interactive: no script at all |
//! | `/gallery` | every component in its states |
#![forbid(unsafe_code)]

use std::sync::Arc;

use navree::http::extract::Request;
use navree::http::middleware::{self, Next};
use navree::http::response::Response;
use navree::http::{GlobalLayer, LayerSlot};
use navree::prelude::*;
use navree::ui::{NavGroup, NavItem, Shell, UiModule, UserMenu, icons};

mod bench;
mod dashboard;
mod deal_form;
mod deals;
mod gallery;

/// The app's one shell.
fn shell() -> Shell {
    Shell::new("Navree CRM")
        .context("Proof pages")
        .group(
            NavGroup::new("Sales")
                .item(NavItem::new("Overview", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(NavItem::new("Deals", "/deals?per=1000").icon(icons::HANDSHAKE).count("1000"))
                .item(NavItem::new("New deal", "/deals/new").icon(icons::CIRCLE_PLUS)),
        )
        .group(
            NavGroup::new("System")
                .item(NavItem::new("Plain page", "/plain").icon(icons::FILE))
                .item(NavItem::new("Gallery", "/gallery").icon(icons::LAYOUT_GRID)),
        )
        .user(UserMenu::new("Ada Lovelace", "ada@navree.test").link("Settings", "/plain").link("Sign out", "/plain"))
}

struct ProofModule;

impl Module for ProofModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(Factory::<Shell>::new(|_| async { Ok(Arc::new(shell())) })).exports::<Shell>();
        m.http_layer(GlobalLayer::new(LayerSlot::Context, "proof.timing", |_, router| Ok(router.layer(middleware::from_fn(timing)))));
        m.controller::<deals::DealPages>().controller::<dashboard::DashboardPages>().controller::<gallery::GalleryPages>();
    }
}

/// `Server-Timing: app`: the time from the router to the finished response,
/// so a benchmark can tell the app's time from the network's.
async fn timing(request: Request, next: Next) -> Response {
    let start = std::time::Instant::now();
    let mut response = next.run(request).await;
    if let Ok(value) = http::HeaderValue::from_str(&format!("app;dur={:.3}", start.elapsed().as_secs_f64() * 1000.0)) {
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
            ("APP_HTTP__PER_IP_RATE_LIMIT".to_owned(), "100000000".to_owned()),
        ],
    );
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpModule::from_config())
        .module(UiModule::from_config())
        .module(ProofModule)
        .run()
}
