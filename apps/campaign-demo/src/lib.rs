//! `campaign-demo`: an example app on Navree UI. It follows one email campaign,
//! "Spring launch", sent by an invented company to 1,240 invented people.
//!
//! ```text
//! cargo run -p campaign-demo                       # http://127.0.0.1:8095
//! PORT=9000 cargo run -p campaign-demo             # another port
//! CAMPAIGN_SIMULATOR=off cargo run -p campaign-demo  # nothing happens by itself
//! ```
//!
//! - **The overview** (`/`): the campaign's figures, a chart and a funnel drawn
//!   with plain SVG elements, and a feed of what readers do, live over
//!   server-sent events. Opens a mail privacy proxy made sit behind a notice.
//! - **The recipients** (`/recipients`): a table whose search, status, country and
//!   date filters, sort and pages run on the server, with each recipient's status
//!   as a badge. It refreshes when the stream speaks.
//! - **A recipient**: a row opens them in a sheet (or a page, with JavaScript
//!   off): outcome chips, fact cards with a flag, and tabs with a timeline
//!   grouped by day.
//! - **A contact form**: a phone number read in the country chosen beside it, a
//!   country found by typing, a date typed or picked, and a toast on save.
//!
//! The data is SQLite, written afresh at every start (`CAMPAIGN_DATABASE_URL`,
//! `sqlite://target/campaign-demo.db?mode=rwc` by default). While the app runs, a
//! simulator makes someone open, click, convert or leave every few seconds
//! (`CAMPAIGN_SIMULATOR=<milliseconds>`, 3000 by default, or `off`).
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use navree::BoxFuture;
use navree::db::{DatabaseModule, Db};
use navree::kernel::Lifecycle;
use navree::prelude::*;
use navree::ui::{Shell, UiModule};

pub mod data;
pub mod live;
pub mod pages;
pub mod views;

use live::{Hub, now};

/// How the app runs.
#[derive(Clone, Debug)]
pub struct Settings {
    /// How often the simulator acts; `None` turns it off.
    pub simulate_every: Option<Duration>,
    /// Serves the routes the browser tests drive the app with (`/__test/...`).
    pub test_hooks: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            simulate_every: Some(Duration::from_secs(3)),
            test_hooks: false,
        }
    }
}

impl Settings {
    /// From `CAMPAIGN_SIMULATOR` and `CAMPAIGN_TEST_HOOKS`.
    pub fn from_env() -> Self {
        let simulate_every = match std::env::var("CAMPAIGN_SIMULATOR").as_deref() {
            Ok("off") | Ok("0") => None,
            Ok(ms) => ms
                .parse()
                .ok()
                .map(|ms: u64| Duration::from_millis(ms.max(200))),
            Err(_) => Some(Duration::from_secs(3)),
        };
        Self {
            simulate_every,
            test_hooks: std::env::var_os("CAMPAIGN_TEST_HOOKS").is_some(),
        }
    }
}

/// Writes the campaign when the app boots, then starts the simulator.
#[injectable]
pub struct Seeder {
    db: Arc<Db>,
    hub: Arc<Hub>,
    settings: Arc<Settings>,
}

impl Lifecycle for Seeder {
    fn on_init(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            self.hub.create(&self.db).await?;
            self.hub.seed(&self.db, now()).await
        })
    }

    fn on_bootstrap(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        if let Some(every) = self.settings.simulate_every {
            let hub = self.hub.clone();
            let db = self.db.clone();
            let stop = ctx.shutdown_token();
            ctx.tracker().spawn(async move {
                tokio::select! {
                    () = stop.cancelled() => {}
                    () = hub.simulate(db, every) => {}
                }
            });
        }
        Box::pin(async { Ok(()) })
    }
}

/// The campaign's pages, its data and its simulator.
pub struct CampaignModule {
    pub settings: Settings,
}

impl Module for CampaignModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(Factory::<Shell>::new(|_| async {
            Ok(Arc::new(views::shell()))
        }))
        .exports::<Shell>();
        m.value(Hub::new());
        m.value(self.settings.clone());
        m.provider::<Seeder>().lifecycle::<Seeder>();
        m.controller::<pages::Pages>();
    }
}

/// The app on `127.0.0.1:port`, on the database at `database_url`.
pub fn app(port: u16, database_url: String, settings: Settings) -> AppBuilder {
    let config = vec![
        ("APP_HTTP__ADDR".to_owned(), format!("127.0.0.1:{port}")),
        ("APP_DATABASE__URL".to_owned(), database_url),
        // The browser tests send every request from one address, and keep many
        // event streams open from it.
        (
            "APP_HTTP__PER_IP_RATE_LIMIT".to_owned(),
            "100000000".to_owned(),
        ),
        (
            "APP_HTTP__SERVER__MAX_CONNECTIONS_PER_IP".to_owned(),
            "10000".to_owned(),
        ),
    ];
    Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().env_vars("APP_", config),
        ))
        .module(HttpModule::from_config())
        .module(DatabaseModule::from_config())
        .module(UiModule::from_config())
        .module(CampaignModule { settings })
}
