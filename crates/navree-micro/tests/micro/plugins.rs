//! A plugin's middleware in a service with no `HttpModule`: only the app adds a layer that wraps
//! every message handler, as only the app adds one for every request. And what the service's
//! message patterns require under a plugin's namespace, which the boot reads as it reads a
//! route's, with or without `HttpModule`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use navree_access::AccessModule;
use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::Permission;
use navree_db::DatabaseModule;
use navree_http::{AppError, CurrentPrincipal, LayerSlot, ModuleBuilderHttpExt, Result};
use navree_kernel::{AppBuilder, Module, ModuleBuilder, Navree};
use navree_macros::{injectable, message_handlers};
use navree_micro::{MicroFake, MicroModule, MicroOptions, ModuleBuilderMicroExt};
use navree_testing::{TestApp, TestAuthenticator, principal, temp_sqlite_url};
use uuid::Uuid;

use crate::support::{access, give_role};

use navree_plugin_khata::LedgerModule;
use navree_plugin_reports::ReportsPlugin;

/// Harpreet Sandhu, of Ludhiana.
const HARPREET: Uuid = Uuid::from_u128(0x0192_f0c1_0000_7000_8000_0000_0000_00a1);

/// The khata plugin, a community plugin whose crate is this module: the service's own modules
/// are in no crate it names.
mod navree_plugin_khata {
    use std::fmt;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use navree_contracts::AuthState;
    use navree_http::extract::Request;
    use navree_http::middleware::{self, Next};
    use navree_http::{AppError, GlobalLayer, LayerSlot};
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};
    use navree_testing::principal;

    /// The plugin's info.
    pub const KHATA: PluginInfo =
        PluginInfo::new("navree-plugin-khata", module_path!(), "khata", "1.0.0");

    /// A layer for every message, in `slot`, that takes every caller for Harpreet Sandhu: in
    /// the `AuthContext` slot it runs after authentication and sets who signed in.
    pub fn trust_me(slot: LayerSlot) -> GlobalLayer {
        GlobalLayer::new(slot, "trust-me", |_ctx, router| {
            Ok(router.layer(middleware::from_fn(
                |mut request: Request, next: Next| async move {
                    request
                        .extensions_mut()
                        .insert(AuthState(Some(principal(super::HARPREET))));
                    next.run(request).await
                },
            )))
        })
    }

    /// A layer that only records, in `applied`, that it was applied.
    pub fn watched(applied: Arc<AtomicBool>) -> GlobalLayer {
        GlobalLayer::new(LayerSlot::Uow, "watched", move |_ctx, router| {
            applied.store(true, Ordering::SeqCst);
            Ok(router)
        })
    }

    /// The plugin's own error: the khata is closed for the day.
    #[derive(Debug)]
    pub struct LedgerClosed;

    impl fmt::Display for LedgerClosed {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("the khata is closed for the day")
        }
    }

    impl std::error::Error for LedgerClosed {}

    /// The plugin's answer to its own error: 409.
    pub fn closed(_: &LedgerClosed) -> Option<AppError> {
        Some(AppError::conflict(
            "The khata of Ludhiana is closed for the day.",
        ))
    }

    /// The plugin's module: it names the plugin, then registers what `.0` adds.
    pub struct LedgerModule(Arc<dyn Fn(&mut ModuleBuilder) + Send + Sync>);

    impl LedgerModule {
        /// The plugin's module that registers what `register` adds.
        pub fn new(register: impl Fn(&mut ModuleBuilder) + Send + Sync + 'static) -> Self {
            Self(Arc::new(register))
        }
    }

    impl Module for LedgerModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(KHATA);
            (self.0)(m);
        }
    }

    /// A module of the plugin's crate that names no plugin itself.
    pub struct Helper(pub fn(&mut ModuleBuilder));

    impl Module for Helper {
        fn register(&self, m: &mut ModuleBuilder) {
            (self.0)(m);
        }
    }
}

/// The service's accounts.
#[injectable]
pub struct Accounts;

#[message_handlers]
impl Accounts {
    /// Who signed in.
    #[message_pattern("accounts.me")]
    #[authenticated]
    async fn whoami(&self, CurrentPrincipal(me): CurrentPrincipal) -> Result<Uuid> {
        Ok(me.user_id)
    }
}

/// The service's accounts feature: its handlers.
struct AccountsModule;

impl Module for AccountsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.message_handlers::<Accounts>();
    }
}

/// A module of the service that adds the khata plugin's layer: the service's own choice.
struct KhataLayer;

impl Module for KhataLayer {
    fn register(&self, m: &mut ModuleBuilder) {
        m.http_layer(navree_plugin_khata::trust_me(LayerSlot::AuthContext));
    }
}

/// A service with no `HttpModule`: its configuration, its messages on `micro` and its
/// accounts.
fn service(micro: &MicroFake) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(MicroModule::for_root(MicroOptions {
            service: "accounts".into(),
            ..MicroOptions::from(micro.transport())
        }))
        .module(AccountsModule)
}

/// The boot error of the service with `plugin`, which must not boot.
async fn boot_error(plugin: LedgerModule) -> String {
    let micro = MicroFake::new();
    let builder = TestAuthenticator::install(service(&micro).module(plugin));
    match builder.boot().await {
        Ok(_) => panic!("the service booted with what the plugin's module registers"),
        Err(error) => error.to_string(),
    }
}

/// The refusal of the layer `name` that the khata plugin's `LedgerModule` adds, as an app with
/// `HttpModule` gives it.
fn refusal(name: &str) -> String {
    format!(
        "plugin `navree-plugin-khata` adds the app-wide layer `{name}` in `LedgerModule`, which runs before every route's access check; only a module of the app adds app-wide layers: remove the plugin or update it to a version that leaves the layer to the app, and to use the plugin's layer, add it from a module of the app (m.http_layer)"
    )
}

#[tokio::test]
async fn a_plugin_layer_fails_the_boot_of_a_service_without_http() {
    // A layer that takes every caller for Harpreet Sandhu, around every handler.
    let error = boot_error(LedgerModule::new(|m| {
        m.http_layer(navree_plugin_khata::trust_me(LayerSlot::AuthContext));
    }))
    .await;
    assert!(error.contains(&refusal("trust-me")), "{error}");
    assert_eq!(
        error.matches("adds the app-wide layer").count(),
        1,
        "one refusal per layer: {error}"
    );

    // Every slot, the edge included, and the session slot, which messages never pass.
    for slot in [
        LayerSlot::Edge,
        LayerSlot::Session,
        LayerSlot::AuthContext,
        LayerSlot::Uow,
        LayerSlot::Context,
    ] {
        let error = boot_error(LedgerModule::new(move |m| {
            m.http_layer(navree_plugin_khata::trust_me(slot));
        }))
        .await;
        assert!(error.contains(&refusal("trust-me")), "{slot:?}: {error}");
    }

    // A layer contributed directly, as `http_layer` contributes it.
    let error = boot_error(LedgerModule::new(|m| {
        m.contribute(navree_plugin_khata::trust_me(LayerSlot::Context));
    }))
    .await;
    assert!(error.contains(&refusal("trust-me")), "{error}");

    // The refusal comes before any layer is applied: the plugin's code in it never runs.
    let applied = Arc::new(AtomicBool::new(false));
    let seen = applied.clone();
    let error = boot_error(LedgerModule::new(move |m| {
        m.http_layer(navree_plugin_khata::watched(seen.clone()));
    }))
    .await;
    assert!(error.contains(&refusal("watched")), "{error}");
    assert!(
        !applied.load(Ordering::SeqCst),
        "the plugin's layer was applied"
    );
}

#[tokio::test]
async fn the_service_may_add_a_plugins_layer() {
    let micro = MicroFake::new();
    let builder = service(&micro)
        .module(KhataLayer)
        .module(LedgerModule::new(|_| {}));
    let app = TestApp::from_builder(builder).await;

    // The service's own choice, made in a module of its own: a guest is taken for Harpreet
    // Sandhu.
    let me: Uuid = micro.client().send("accounts.me", &()).await.unwrap();
    assert_eq!(me, HARPREET);

    // The module that adds the layer is the service's: the plugin lists its own module only.
    let plugins: Vec<_> = app
        .ctx()
        .plugins()
        .plugins
        .into_iter()
        .filter(|plugin| !plugin.first_party)
        .collect();
    assert_eq!(plugins.len(), 1);
    let modules: Vec<&str> = plugins[0]
        .modules
        .iter()
        .map(|module| module.id.name())
        .collect();
    assert_eq!(modules, ["LedgerModule"]);
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_service_with_no_plugin_answers_as_before() {
    let micro = MicroFake::new();
    let app = TestApp::from_builder(service(&micro)).await;
    assert!(
        app.ctx()
            .plugins()
            .plugins
            .iter()
            .all(|plugin| plugin.first_party)
    );

    // A guest is refused, and a caller who signed in is who signed in.
    micro
        .client()
        .request("accounts.me", &())
        .await
        .assert_error(401, "Unauthorized");
    let me: Uuid = micro
        .client()
        .acting_as(principal(HARPREET))
        .send("accounts.me", &())
        .await
        .unwrap();
    assert_eq!(me, HARPREET);
    app.shutdown().await.unwrap();
}

/// An answer to every input or output error: 503, the khata is busy.
fn busy(_: &std::io::Error) -> Option<AppError> {
    Some(AppError::service_unavailable(
        "The khata of Amritsar is busy.",
    ))
}

/// A module of the service that maps every input or output error: the service's own choice.
struct IoErrors;

impl Module for IoErrors {
    fn register(&self, m: &mut ModuleBuilder) {
        m.map_error::<std::io::Error>(busy);
    }
}

/// The refusal of the khata plugin's `LedgerModule` mapping the errors of type `error`, as an
/// app with `HttpModule` gives it.
fn maps(error: &str) -> String {
    format!(
        "plugin `navree-plugin-khata` answers the app's errors of type `{error}` (m.map_error in `LedgerModule`), a type of another crate; only a module of the app maps another crate's errors: remove the plugin or update it to a version that maps only its own errors, and to use the plugin's mapping, register it from a module of the app (m.map_error)"
    )
}

#[tokio::test]
async fn a_plugin_mapping_of_another_crates_errors_fails_the_boot_of_a_service_without_http() {
    // Every input or output error a handler of the service raises would answer 503, with the
    // plugin's text.
    let error = boot_error(LedgerModule::new(|m| {
        m.map_error::<std::io::Error>(busy);
    }))
    .await;
    let expected = maps(std::any::type_name::<std::io::Error>());
    assert!(error.contains(&expected), "{error}");
    assert_eq!(
        error.matches("answers the app's errors of type").count(),
        1,
        "one refusal per mapping: {error}"
    );

    // Its own error type is the plugin's to map.
    let micro = MicroFake::new();
    let plugin = LedgerModule::new(|m| {
        m.map_error::<navree_plugin_khata::LedgerClosed>(navree_plugin_khata::closed);
    });
    let app = TestApp::from_builder(service(&micro).module(plugin)).await;
    app.shutdown().await.unwrap();

    // The service maps another crate's errors from a module of its own, beside the plugin.
    let micro = MicroFake::new();
    let builder = service(&micro)
        .module(IoErrors)
        .module(LedgerModule::new(|_| {}));
    let app = TestApp::from_builder(builder).await;
    app.shutdown().await.unwrap();
}

/// The reports plugin, a community plugin whose crate is this module: its namespace is
/// `reports`.
mod navree_plugin_reports {
    use navree_contracts::{ModuleBuilderPermissionsExt, Permission};
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};

    /// The plugin's info.
    const REPORTS: PluginInfo =
        PluginInfo::new("navree-plugin-reports", module_path!(), "reports", "1.0.0");

    /// The plugin's module: it names the plugin and describes the permissions `.0`.
    pub struct ReportsPlugin(pub &'static [Permission]);

    impl Module for ReportsPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(REPORTS).permissions(self.0);
        }
    }
}

/// What the reports plugin describes, when it describes anything.
const REPORTS_VIEW: &[Permission] = &[Permission::new(
    "reports.view",
    "See the sales reports of the Ludhiana office",
)];

/// The service's sales figures.
#[injectable]
pub struct Sales;

#[message_handlers]
impl Sales {
    /// The month's sales, for the holders of `reports.view`.
    #[message_pattern("sales.month")]
    #[permissions("reports.view")]
    async fn month(&self) -> Result<u32> {
        Ok(1200)
    }
}

/// The service's sales feature: its handlers.
struct SalesModule;

impl Module for SalesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.message_handlers::<Sales>();
    }
}

/// A service with the access module and no `HttpModule`: its configuration, the database at
/// `database`, its messages on `micro` and its sales.
fn sales_service(micro: &MicroFake, database: &str) -> AppBuilder {
    let sources = ConfigSources::new()
        .set("database.url", database)
        .set("database.migrate_on_boot", true);
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(DatabaseModule)
        .module(AccessModule::for_root(access()))
        .module(MicroModule::for_root(MicroOptions {
            service: "sales".into(),
            ..MicroOptions::from(micro.transport())
        }))
        .module(SalesModule)
}

#[tokio::test]
async fn a_message_pattern_under_a_plugins_namespace_is_read_without_http() {
    // `sales.month` asks for `reports.view`, which no subject maps and the reports plugin does
    // not describe: the plugin's policies would decide who may send the message. The service
    // has no `HttpModule`, and the boot still reads the pattern.
    let (database, _keep) = temp_sqlite_url();
    let micro = MicroFake::new();
    let builder = sales_service(&micro, &database).module(ReportsPlugin(&[]));
    let error = match TestAuthenticator::install(builder).boot().await {
        Ok(_) => panic!("the service booted beside the plugin"),
        Err(error) => error.to_string(),
    };
    let expected = "plugin `navree-plugin-reports` uses the namespace `reports`, the first word of the permission `reports.view` that the message pattern `sales.month` of `SalesModule` requires, and the plugin describes no permission of that name; a plugin's policies decide the names under its namespace, so they would decide who may use it: require a permission the plugin describes, or name the app's permission under another word";
    assert!(error.contains(expected), "{error}");

    // The plugin describes `reports.view`: the pattern requires it on purpose, the service
    // boots, and who may send the message is the service's grant.
    let (database, _keep) = temp_sqlite_url();
    let micro = MicroFake::new();
    let builder = sales_service(&micro, &database).module(ReportsPlugin(REPORTS_VIEW));
    let app = TestApp::from_builder(builder).await;
    let (harpreet, meera) = (Uuid::now_v7(), Uuid::now_v7());
    give_role(&app, harpreet, "munshi", &["reports.view"]).await;
    let month: u32 = micro
        .client()
        .acting_as(principal(harpreet))
        .send("sales.month", &())
        .await
        .unwrap();
    assert_eq!(month, 1200);
    micro
        .client()
        .acting_as(principal(meera))
        .request("sales.month", &())
        .await
        .assert_error(403, "Forbidden");
    app.shutdown().await.unwrap();

    // Without the plugin the service boots as before.
    let (database, _keep) = temp_sqlite_url();
    let micro = MicroFake::new();
    let app = TestApp::from_builder(sales_service(&micro, &database)).await;
    assert!(
        app.ctx()
            .plugins()
            .plugins
            .iter()
            .all(|plugin| plugin.first_party)
    );
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_refusal_says_why_the_module_acts_for_the_plugin() {
    mod khata_core {
        use navree_kernel::{Module, ModuleBuilder};

        pub struct SecondCrate(pub fn(&mut ModuleBuilder));

        impl Module for SecondCrate {
            fn register(&self, m: &mut ModuleBuilder) {
                (self.0)(m);
            }
        }
    }

    let krate = navree_plugin_khata::KHATA.crate_name;
    let io_error = std::any::type_name::<std::io::Error>();

    // 1. Module of the plugin's crate (`Helper`) in a service with no `HttpModule`.
    let micro = MicroFake::new();
    let in_crate =
        TestAuthenticator::install(service(&micro).module(LedgerModule::new(|_| {})).module(
            navree_plugin_khata::Helper(|m| {
                m.http_layer(navree_plugin_khata::trust_me(LayerSlot::AuthContext))
                    .map_error::<std::io::Error>(busy);
            }),
        ))
        .boot()
        .await
        .expect_err("refused")
        .to_string();
    assert!(
        in_crate.contains(&format!(
            "adds the app-wide layer `trust-me` in `Helper` (a module of the plugin's crate `{krate}`),"
        )),
        "{in_crate}"
    );
    assert!(
        in_crate.contains(&format!(
            "answers the app's errors of type `{io_error}` (m.map_error in `Helper` (a module of the plugin's crate `{krate}`)),"
        )),
        "{in_crate}"
    );

    // 2. Module of a second crate imported only by the plugin (`SecondCrate`).
    let by_import = boot_error(LedgerModule::new(|m| {
        m.import(khata_core::SecondCrate(|m| {
            m.http_layer(navree_plugin_khata::trust_me(LayerSlot::AuthContext))
                .map_error::<std::io::Error>(busy);
        }));
    }))
    .await;
    assert!(
        by_import.contains(
            "adds the app-wide layer `trust-me` in `SecondCrate` (only the plugin's modules import it),"
        ),
        "{by_import}"
    );
    assert!(
        by_import.contains(&format!(
            "answers the app's errors of type `{io_error}` (m.map_error in `SecondCrate` (only the plugin's modules import it)),"
        )),
        "{by_import}"
    );
}
