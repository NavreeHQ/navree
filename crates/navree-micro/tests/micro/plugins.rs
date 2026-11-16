//! A plugin's middleware in a service with no `HttpModule`: only the app adds a layer that wraps
//! every message handler, as only the app adds one for every request.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use navree_config::{ConfigModule, ConfigSources};
use navree_http::{CurrentPrincipal, LayerSlot, ModuleBuilderHttpExt, Result};
use navree_kernel::{AppBuilder, Module, ModuleBuilder, Navree};
use navree_macros::{injectable, message_handlers};
use navree_micro::{MicroFake, MicroModule, MicroOptions, ModuleBuilderMicroExt};
use navree_testing::{TestApp, TestAuthenticator, principal};
use uuid::Uuid;

use navree_plugin_khata::LedgerModule;

/// Harpreet Sandhu, of Ludhiana.
const HARPREET: Uuid = Uuid::from_u128(0x0192_f0c1_0000_7000_8000_0000_0000_00a1);

/// The khata plugin, a community plugin whose crate is this module: the service's own modules
/// are in no crate it names.
mod navree_plugin_khata {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use navree_contracts::AuthState;
    use navree_http::extract::Request;
    use navree_http::middleware::{self, Next};
    use navree_http::{GlobalLayer, LayerSlot};
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};
    use navree_testing::principal;

    /// The plugin's info.
    const KHATA: PluginInfo =
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
        Ok(_) => panic!("the service booted with the plugin's layer"),
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
    let plugins = app.ctx().plugins().plugins;
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
    assert!(app.ctx().plugins().plugins.is_empty());

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
