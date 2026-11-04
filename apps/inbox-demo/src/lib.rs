//! `inbox-demo`: an example app on Navree UI. It is the support desk of an invented
//! software company, Fernway, whose customers are studios, clinics and schools that
//! take bookings with its product.
//!
//! ```text
//! cargo run -p inbox-demo                        # http://127.0.0.1:8096
//! PORT=9000 cargo run -p inbox-demo              # another port
//! INBOX_SIMULATOR=off cargo run -p inbox-demo    # nothing happens by itself
//! ```
//!
//! - **Three panes** that a person resizes by dragging or with the keys, saved for
//!   each agent on the server: the conversations, the open conversation, and its
//!   customer. On a phone they show one at a time, with links back.
//! - **The conversations** (`/inbox`): a view (all open, mine, unassigned, snoozed,
//!   closed), search, channel and priority filters, each row with its unread count,
//!   priority, channel and the time left to answer, which the server counts down.
//! - **A conversation** (`/inbox/{id}`): the customer's messages and the team's
//!   answers, internal notes set apart, files as chips, and a composer with saved
//!   replies. Assign it to someone found by typing, change its status and priority,
//!   snooze it to a day, or close it.
//! - **The customer**: their plan, what they have paid, their open conversations, a
//!   timeline of their history, and tags.
//! - **Live**: new conversations and messages arrive over server-sent events, in the
//!   list and in the open conversation, while what an agent types stays as it is.
//! - **Keys**: `j` and `k` move through the list, `r` replies, `e` closes, `/`
//!   searches, `?` lists the keys, and Ctrl+K opens the command palette.
//! - **No JavaScript**: every action is a link or a form.
//!
//! The data is SQLite, written afresh at every start (`INBOX_DATABASE_URL`,
//! `sqlite://target/inbox-demo.db?mode=rwc` by default): the hand-written conversations
//! and 100,000 generated ones of 10 to 100 messages each, about 5.5 million messages
//! (`INBOX_SCALE=<conversations>` for another size, `0` for none). Everything long loads
//! as it is read: fifty conversations at a time, thirty messages at a time, by cursor,
//! and the customer pane after the page. A simulator makes customers
//! and teammates act every few seconds (`INBOX_SIMULATOR=<milliseconds>`, 6000 by
//! default, or `off`), and the clock redraws the timers every 30 seconds
//! (`INBOX_TICK=<milliseconds>` or `off`). There is no sign-in: a visitor works as
//! one of the team and can switch to another from the account menu.
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use navree::BoxFuture;
use navree::db::{DatabaseModule, Db};
use navree::kernel::Lifecycle;
use navree::prelude::*;
use navree::ui::UiModule;

pub mod data;
pub mod live;
pub mod pages;
pub mod queries;
pub mod scale;
pub mod views;

use live::{Hub, now};

/// How the app runs.
#[derive(Clone, Debug)]
pub struct Settings {
    /// How often the simulator acts; `None` turns it off.
    pub simulate_every: Option<Duration>,
    /// How often the timers are drawn again; `None` turns the clock off.
    pub tick_every: Option<Duration>,
    /// Serves the routes the browser tests drive the app with (`/__test/...`).
    pub test_hooks: bool,
    /// How many generated conversations the desk holds beside the hand-written ones.
    pub scale: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            simulate_every: Some(Duration::from_secs(6)),
            tick_every: Some(Duration::from_secs(30)),
            test_hooks: false,
            scale: scale::FULL,
        }
    }
}

/// A duration in milliseconds from the environment: `off` or `0` for none.
fn every(name: &str, default: Duration, least: u64) -> Option<Duration> {
    match std::env::var(name).as_deref() {
        Ok("off") | Ok("0") => None,
        Ok(ms) => ms
            .parse()
            .ok()
            .map(|ms: u64| Duration::from_millis(ms.max(least))),
        Err(_) => Some(default),
    }
}

impl Settings {
    /// From `INBOX_SIMULATOR`, `INBOX_TICK`, `INBOX_TEST_HOOKS` and `INBOX_SCALE`.
    pub fn from_env() -> Self {
        Self {
            simulate_every: every("INBOX_SIMULATOR", Duration::from_secs(6), 500),
            tick_every: every("INBOX_TICK", Duration::from_secs(30), 1000),
            test_hooks: std::env::var_os("INBOX_TEST_HOOKS").is_some(),
            scale: std::env::var("INBOX_SCALE")
                .ok()
                .and_then(|n| n.replace(['_', ','], "").parse().ok())
                .map_or(scale::FULL, |n: i64| n.clamp(0, 1_000_000)),
        }
    }
}

/// Writes the desk when the app boots, then starts the simulator and the clock.
#[injectable]
pub struct Seeder {
    db: Arc<Db>,
    hub: Arc<Hub>,
    settings: Arc<Settings>,
}

impl Lifecycle for Seeder {
    fn on_init(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            let started = std::time::Instant::now();
            let made = self.hub.seed(&self.db, now(), self.settings.scale).await?;
            eprintln!(
                "inbox-demo: wrote {} generated conversations, {} messages and {} customers in {:.1} s",
                made.conversations,
                made.messages,
                made.customers,
                started.elapsed().as_secs_f64()
            );
            Ok(())
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
        if let Some(every) = self.settings.tick_every {
            let hub = self.hub.clone();
            let stop = ctx.shutdown_token();
            ctx.tracker().spawn(async move {
                tokio::select! {
                    () = stop.cancelled() => {}
                    () = hub.tick(every) => {}
                }
            });
        }
        Box::pin(async { Ok(()) })
    }
}

/// The inbox's pages, its data, its simulator and its clock.
pub struct InboxModule {
    pub settings: Settings,
}

impl Module for InboxModule {
    fn register(&self, m: &mut ModuleBuilder) {
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
        // The browser tests send every request from one address, and keep many event
        // streams open from it.
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
        .module(InboxModule { settings })
}
