//! `store-demo`: an example app on Navree UI. It runs the back office of an
//! invented shop, Fernhill Supply, which sells goods for the home and the
//! outdoors from three warehouses.
//!
//! ```text
//! cargo run -p store-demo                        # http://127.0.0.1:8095
//! PORT=9000 cargo run -p store-demo              # another port
//! STORE_SIMULATOR=off cargo run -p store-demo    # nothing happens by itself
//! ```
//!
//! - **The dashboard** (`/`): today's figures, the order activity as it happens,
//!   live over server-sent events, the shelves running low and today's best
//!   sellers.
//! - **The orders** (`/orders`): a table whose search, status, fulfilment,
//!   payment, country and date filters, sort and pages run on the server. It
//!   refreshes when the stream speaks.
//! - **An order**: a row opens it in a drawer over the table (or a page, with
//!   JavaScript off): its lines, its steps from placed to delivered, the customer,
//!   the payment, the route from the warehouse, the notes, and refund, resend and
//!   cancel, each confirmed in a dialog and checked again on the server.
//! - **The inventory** (`/inventory`): every product with its stock, a badge when
//!   it runs low, and a restock form in its row.
//!
//! The data is SQLite, written afresh at every start (`STORE_DATABASE_URL`,
//! `sqlite://target/store-demo.db?mode=rwc` by default). While the app runs, a
//! simulator places, packs, ships and delivers orders and restocks shelves every
//! few seconds (`STORE_SIMULATOR=<milliseconds>`, 3000 by default, or `off`).
//! `STORE_LOG_QUERIES=1` logs every statement as JSON lines, for
//! `e2e/ui/measure-store.mjs`, which measures the app.
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
pub mod shop;
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
    /// From `STORE_SIMULATOR` and `STORE_TEST_HOOKS`.
    pub fn from_env() -> Self {
        let simulate_every = match std::env::var("STORE_SIMULATOR").as_deref() {
            Ok("off") | Ok("0") => None,
            Ok(ms) => ms
                .parse()
                .ok()
                .map(|ms: u64| Duration::from_millis(ms.max(200))),
            Err(_) => Some(Duration::from_secs(3)),
        };
        Self {
            simulate_every,
            test_hooks: std::env::var_os("STORE_TEST_HOOKS").is_some(),
        }
    }
}

/// Writes the shop when the app boots, then starts the simulator.
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

/// The shop's pages, its data and its simulator.
pub struct StoreModule {
    pub settings: Settings,
}

impl Module for StoreModule {
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

/// The app on `127.0.0.1:port`, on the database at `database_url`. With
/// `STORE_LOG_QUERIES` set it logs every statement it runs, which is how the
/// query plans on the website are found.
pub fn app(port: u16, database_url: String, settings: Settings) -> AppBuilder {
    let mut config = vec![
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
    if std::env::var_os("STORE_LOG_QUERIES").is_some() {
        config.push(("APP_DATABASE__LOG_QUERIES".to_owned(), "true".to_owned()));
        config.push(("APP_LOG__LEVEL".to_owned(), "debug".to_owned()));
        config.push(("APP_LOG__FORMAT".to_owned(), "json".to_owned()));
    }
    Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().env_vars("APP_", config),
        ))
        .module(HttpModule::from_config())
        .module(DatabaseModule::from_config())
        .module(UiModule::from_config())
        .module(StoreModule { settings })
}
