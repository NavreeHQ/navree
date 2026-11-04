//! `board-demo`: an example app on Navree UI. One product team plans a release on
//! a board: five columns of cards that move by pointer, touch or keys.
//!
//! ```text
//! cargo run -p board-demo                      # http://127.0.0.1:8097
//! PORT=9000 cargo run -p board-demo            # another port
//! BOARD_SIMULATOR=off cargo run -p board-demo  # no teammate moves cards by itself
//! ```
//!
//! - **The board** (`/`): Backlog, Ready, In progress, Review and Done. Ready, In
//!   progress and Review hold at most 5, 3 and 2 cards, and the server holds every
//!   move to that, to who may close a card, and to where the card really is now.
//!   The browser moves a card at once and puts it back, with a toast, when the
//!   server says no. With JavaScript off each card has a form to move it.
//! - **Live**: every move, by anyone, goes out on a server-sent event stream, and
//!   every open board fetches itself again, the cards sliding to their new places.
//! - **A card** opens in a drawer over the board: its description, a checklist
//!   whose lines move the same way, comments, and what happened to it.
//! - **Filters** by person, label and due date, and a palette (`Ctrl` or `Cmd` and
//!   `K`) that jumps to any card.
//!
//! The data is SQLite, written afresh at every start (`BOARD_DATABASE_URL`,
//! `sqlite://target/board-demo.db?mode=rwc` by default). While the app runs, a
//! teammate moves a card on every 25 seconds or so (`BOARD_SIMULATOR=<milliseconds>`,
//! or `off`).
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
pub mod store;
pub mod views;

use live::{Hub, now};

/// How the app runs.
#[derive(Clone, Debug)]
pub struct Settings {
    /// How often the teammate moves a card; `None` turns it off.
    pub simulate_every: Option<Duration>,
    /// Serves the routes the browser tests drive the app with (`/__test/...`).
    pub test_hooks: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            simulate_every: Some(Duration::from_secs(25)),
            test_hooks: false,
        }
    }
}

impl Settings {
    /// From `BOARD_SIMULATOR` and `BOARD_TEST_HOOKS`.
    pub fn from_env() -> Self {
        let simulate_every = match std::env::var("BOARD_SIMULATOR").as_deref() {
            Ok("off") | Ok("0") => None,
            Ok(ms) => ms
                .parse()
                .ok()
                .map(|ms: u64| Duration::from_millis(ms.max(1_000))),
            Err(_) => Some(Duration::from_secs(25)),
        };
        Self {
            simulate_every,
            test_hooks: std::env::var_os("BOARD_TEST_HOOKS").is_some(),
        }
    }
}

/// Writes the board when the app boots, then starts the teammate.
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

/// The board's pages, its data and its teammate.
pub struct BoardModule {
    pub settings: Settings,
}

impl Module for BoardModule {
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
        .module(BoardModule { settings })
}
