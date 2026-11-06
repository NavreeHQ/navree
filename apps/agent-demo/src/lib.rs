//! `agent-demo`: an example app on Navree UI with an assistant. It is the back office of
//! an invented kitchenware shop in Ludhiana, Kesar Kitchen Supplies, which sells across
//! India: its orders, how each was paid (UPI, card, net banking, cash on delivery) and
//! where each stands. Times are in India Standard Time and money in rupees.
//!
//! ```text
//! cargo run -p agent-demo                          # http://127.0.0.1:8098, a scripted model
//! AGENT_AI_KEY=... AGENT_AI_MODEL=... cargo run -p agent-demo   # a real model
//! ```
//!
//! Sign in as Simran Kaur, the manager (`simran.kaur@kesar.example`), or Arjun Mehta, of
//! the staff (`arjun.mehta@kesar.example`), both with the password `demo-password-2026`.
//!
//! - **"How did UPI do this week?"** The assistant shows a donut of the week's payments
//!   by method. It chooses the component and the period; the numbers are loaded when the
//!   page is drawn, under the reader's own access, and the model never sees them.
//! - **"Refund order KK-1042"** The run stops at `refund_order`, which needs approval:
//!   its card is drawn in the page's own place, with the order and the amount as text,
//!   and Approve runs it (checked again) and streams the rest of the answer. Only the
//!   manager is offered the tool at all.
//! - **"Show order KK-1042"** A card of the order with Mark delivered and Refund for the
//!   manager. Each button is signed for her, this conversation and this card, works
//!   once, and is checked again when pressed; Refund asks first in a dialog the page
//!   draws.
//! - **An order's page** (`/orders/{id}`) has a composer that tells the assistant which
//!   order it is about, signed for the reader and read under their access.
//! - **Live and stoppable**: the answer streams into the page through the live hub,
//!   in every tab, and Stop ends it.
//!
//! The data is SQLite (`AGENT_DATABASE_URL`, `sqlite://target/agent-demo.db?mode=rwc` by
//! default), written once. Without `AGENT_AI_KEY` the model is `AiFake`: with
//! `AGENT_TEST_HOOKS=1` the browser tests script what it answers (`POST /__test/script`).
#![forbid(unsafe_code)]

use std::sync::Arc;

use navree::access::{AccessModule, AccessOptions, AccessService};
use navree::ai::ui::ModuleBuilderAssistantExt;
use navree::ai::{AiModule, ModuleBuilderAiExt};
use navree::auth::{AuthModule, AuthPages, AuthService};
use navree::db::{DatabaseModule, Db, ModuleBuilderDbExt};
use navree::kernel::Lifecycle;
use navree::prelude::*;
use navree::session::SessionModule;
use navree::ui::{NavGroup, NavItem, Shell, UiModule, UserMenu, icons};
use navree::BoxFuture;

pub mod assistant;
pub mod pages;
pub mod shop;

/// How the app runs.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    /// Serves the routes the browser tests script the model with.
    pub test_hooks: bool,
    /// A real model: its provider, model and key. `None`: `AiFake`.
    pub model: Option<(String, String, String)>,
}

impl Settings {
    /// From `AGENT_TEST_HOOKS`, `AGENT_AI_PROVIDER` (`anthropic` by default),
    /// `AGENT_AI_MODEL` and `AGENT_AI_KEY`.
    pub fn from_env() -> Self {
        let model = match (std::env::var("AGENT_AI_MODEL"), std::env::var("AGENT_AI_KEY")) {
            (Ok(model), Ok(key)) if !key.is_empty() => Some((
                std::env::var("AGENT_AI_PROVIDER").unwrap_or_else(|_| "anthropic".to_owned()),
                model,
                key,
            )),
            _ => None,
        };
        Self {
            test_hooks: std::env::var_os("AGENT_TEST_HOOKS").is_some(),
            model,
        }
    }
}

/// Writes the shop and its people when the app boots.
#[injectable]
pub struct Seeder {
    db: Arc<Db>,
    auth: Arc<AuthService>,
    access: Arc<AccessService>,
}

impl Lifecycle for Seeder {
    fn on_init(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            shop::seed(&self.db, &self.auth, &self.access, chrono::Utc::now()).await
        })
    }
}

/// The app's one shell.
fn shell() -> Shell {
    Shell::new(shop::SHOP)
        .context("Back office")
        .group(
            NavGroup::plain()
                .item(NavItem::new("Assistant", "/assistant").icon(icons::BOT))
                .item(NavItem::new("Orders", "/orders").icon(icons::RECEIPT)),
        )
        .user(UserMenu::new("Account", "Signed in").action("Sign out", "/sign-out"))
}

/// The shop: its orders, its people, its assistant and its pages.
pub struct ShopModule {
    pub settings: Settings,
}

impl Module for ShopModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global();
        m.migrations(shop::migrations())
            .import(AccessModule::for_feature::<shop::Order>());
        m.factory(Factory::<Shell>::new(|_| async { Ok(Arc::new(shell())) }))
            .exports::<Shell>();
        m.value(self.settings.clone());
        m.provider::<Seeder>().lifecycle::<Seeder>();
        m.agent::<assistant::ShopAssistant>()
            .assistant::<assistant::ShopAssistant>(assistant::config());
        m.controller::<pages::Pages>();
    }
}

/// The app on `127.0.0.1:port`, on the database at `database_url`.
pub fn app(port: u16, database_url: String, settings: Settings) -> AppBuilder {
    let mut config = vec![
        ("APP_HTTP__ADDR".to_owned(), format!("127.0.0.1:{port}")),
        ("APP_DATABASE__URL".to_owned(), database_url),
        ("APP_DATABASE__MIGRATE_ON_BOOT".to_owned(), "true".to_owned()),
        (
            "APP_APP__KEY".to_owned(),
            std::env::var("AGENT_APP_KEY").unwrap_or_else(|_| navree::crypto::generate_key()),
        ),
        // The demo serves plain HTTP on this machine only.
        ("APP_SESSION__SECURE".to_owned(), "false".to_owned()),
        ("APP_SESSION__HOST_PREFIX".to_owned(), "false".to_owned()),
    ];
    match &settings.model {
        Some((provider, model, key)) => {
            let name = provider.to_ascii_uppercase();
            config.extend([
                ("APP_AI__DEFAULT".to_owned(), "assistant".to_owned()),
                (format!("APP_AI__PROVIDERS__{name}__DRIVER"), provider.clone()),
                (format!("APP_AI__PROVIDERS__{name}__API_KEY"), key.clone()),
                ("APP_AI__MODELS__ASSISTANT__PROVIDER".to_owned(), provider.clone()),
                ("APP_AI__MODELS__ASSISTANT__MODEL".to_owned(), model.clone()),
            ]);
        }
        None => config.push(("APP_AI__FAKE".to_owned(), "true".to_owned())),
    }
    if settings.test_hooks {
        // The browser tests send every request from one address and keep event
        // streams open from it.
        config.push((
            "APP_HTTP__PER_IP_RATE_LIMIT".to_owned(),
            "100000000".to_owned(),
        ));
        config.push((
            "APP_HTTP__SERVER__MAX_CONNECTIONS_PER_IP".to_owned(),
            "10000".to_owned(),
        ));
    }
    Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().env_vars("APP_", config),
        ))
        .module(HttpModule::from_config())
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(SessionModule::from_config())
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities: shop::abilities,
            ..AccessOptions::default()
        }))
        .module(UiModule::from_config())
        .module(AuthPages::new().app_name(shop::SHOP).home("/assistant"))
        // The live hub: pages listen on `GET /broadcasting/stream`.
        .module(RealtimeModule::from_config())
        .module(AiModule::from_config())
        .module(ShopModule { settings })
}
