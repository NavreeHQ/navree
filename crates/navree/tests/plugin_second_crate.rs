//! A plugin split across two crates (`navree-plugin-khata` and its `-core` crate): a plain
//! module of the second crate that only the plugin's modules import acts for that plugin and is
//! held to every plugin rule, while a plain framework module that only a plugin imports acts for
//! no plugin and boots as before.

use std::sync::Arc;

use navree::contracts::AuditSink;
use navree::kernel::ModuleId;
use navree::prelude::*;
use navree::testing::{TestApp, TestAppBuilder};
use serde_json::Value;

/// A second crate that names no plugin, standing for `navree-plugin-khata-core`.
mod navree_plugin_khata_core {
    use std::sync::Arc;

    use navree::BoxFuture;
    use navree::access::Policy;
    use navree::contracts::{AuditEntry, AuditSink};
    use navree::http::{GlobalLayer, LayerSlot};
    use navree::prelude::*;

    /// A policy of the second crate that grants on every subject.
    pub struct EverySubject;

    impl Policy for EverySubject {
        fn define(_ctx: &AbilityCtx, rules: &mut Rules) {
            rules.can(MANAGE, ALL);
        }
    }

    /// An app-wide HTTP layer built in the second crate.
    pub fn trace_layer() -> GlobalLayer {
        GlobalLayer::new(LayerSlot::Context, "khata-trace", |_ctx, router| Ok(router))
    }

    /// An audit sink built in the second crate.
    pub struct KhataSink;

    impl AuditSink for KhataSink {
        fn record(&self, _entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
            Box::pin(async { Ok(()) })
        }
    }

    /// The second crate's plain module: it names no plugin and registers what its function adds.
    pub struct KhataCoreModule(pub fn(&mut ModuleBuilder));

    impl Module for KhataCoreModule {
        fn register(&self, m: &mut ModuleBuilder) {
            (self.0)(m);
        }
    }

    /// Registers `EverySubject` from the second crate's module.
    pub fn writes_rules_on_all(m: &mut ModuleBuilder) {
        m.import(AccessModule::for_policy::<EverySubject>());
    }

    /// Registers `trace_layer` from the second crate's module.
    pub fn adds_app_wide_layer(m: &mut ModuleBuilder) {
        m.http_layer(trace_layer());
    }

    /// Registers `KhataSink` as the app's audit sink from the second crate's module.
    pub fn takes_audit_sink(m: &mut ModuleBuilder) {
        m.factory(Factory::<dyn AuditSink>::new(|_| async {
            Ok(Arc::new(KhataSink) as Arc<dyn AuditSink>)
        }))
        .exports::<dyn AuditSink>();
    }
}

/// The plugin crate `navree-plugin-khata`: its module names the plugin and imports its second
/// crate's module.
mod navree_plugin_khata {
    use navree::plugin::PluginInfo;
    use navree::prelude::*;

    use crate::navree_plugin_khata_core::KhataCoreModule;

    /// The plugin's info.
    const KHATA: PluginInfo =
        PluginInfo::new("navree-plugin-khata", module_path!(), "khata", "1.0.0");

    /// The plugin's module, importing `KhataCoreModule`.
    pub struct KhataPlugin(pub fn(&mut ModuleBuilder));

    impl Module for KhataPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(KHATA).import(KhataCoreModule(self.0));
        }
    }

    /// A plugin module that imports a plain framework module (`UiModule`) directly.
    pub struct KhataUiPlugin;

    impl Module for KhataUiPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(KHATA).import(UiModule);
        }
    }
}

/// A minimal app on a temporary database with HTTP and access enabled.
fn khata_app() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(DatabaseModule::from_config())
        .module(AccessModule)
}

/// The boot error of an app that must not boot.
async fn boot_error(builder: TestAppBuilder) -> String {
    match builder.try_build().await {
        Ok(_) => panic!("the app booted"),
        Err(error) => error.to_string(),
    }
}

#[tokio::test]
async fn a_plugin_in_two_crates_cannot_write_rules_on_all_add_an_app_wide_layer_or_take_the_audit_sink_from_its_second_crate()
 {
    use navree_plugin_khata::KhataPlugin;
    use navree_plugin_khata_core::{
        EverySubject, KhataCoreModule, adds_app_wide_layer, takes_audit_sink, trace_layer,
        writes_rules_on_all,
    };

    // 1. Writing rules on `all` from the second crate's module fails the boot.
    let policy_name = std::any::type_name::<EverySubject>();
    let policy_module = AccessModule::for_policy::<EverySubject>().name();
    let expected_policy = format!(
        "plugin `navree-plugin-khata` registers the policy `{policy_name}` in `{policy_module}`, which writes a rule on `all`; a plugin's policy writes rules on its own subjects only: remove the plugin or update it to a version whose policy keeps to them"
    );
    let error = boot_error(khata_app().module(KhataPlugin(writes_rules_on_all))).await;
    assert!(
        error.contains(&expected_policy),
        "expected `{expected_policy}` in:\n{error}"
    );

    // 2. Adding an app-wide HTTP layer from the second crate's module fails the boot.
    let expected_layer = "plugin `navree-plugin-khata` adds the app-wide layer `khata-trace` in `KhataCoreModule`, which runs before every route's access check; only a module of the app adds app-wide layers: remove the plugin or update it to a version that leaves the layer to the app, and to use the plugin's layer, add it from a module of the app (m.http_layer)";
    let error = boot_error(khata_app().module(KhataPlugin(adds_app_wide_layer))).await;
    assert!(
        error.contains(expected_layer),
        "expected `{expected_layer}` in:\n{error}"
    );

    // 3. Registering the app's audit sink from the second crate's module fails validation and
    // boot.
    let expected_sink = "plugin `navree-plugin-khata` provides `dyn navree_contracts::audit::AuditSink` in `KhataCoreModule` (only the plugin's modules import it), which receives the app's audit records; only a module of the app registers it: remove the plugin or update it to a version that leaves it to the app, and an app has one audit sink: to use the plugin's, register it from a module of an app that has no other (it cannot be used beside navree-audit's AuditModule)";
    let error = boot_error(khata_app().module(KhataPlugin(takes_audit_sink))).await;
    assert!(
        error.contains(expected_sink),
        "expected `{expected_sink}` in:\n{error}"
    );
    let validated = Navree::app()
        .module(KhataPlugin(takes_audit_sink))
        .validate()
        .expect_err("the second crate registers the audit sink")
        .to_string();
    assert_eq!(validated, expected_sink);

    // When a module of the app imports the second crate's module, or opts in from a module of
    // its own, the app boots.
    struct AppOptsIn;
    impl Module for AppOptsIn {
        fn register(&self, m: &mut ModuleBuilder) {
            m.import(AccessModule::for_policy::<EverySubject>())
                .http_layer(trace_layer())
                .factory(Factory::<dyn AuditSink>::new(|_| async {
                    Ok(Arc::new(navree_plugin_khata_core::KhataSink) as Arc<dyn AuditSink>)
                }))
                .exports::<dyn AuditSink>();
        }
    }
    let app = khata_app()
        .module(KhataPlugin(|_| {}))
        .module(KhataCoreModule(|_| {}))
        .module(AppOptsIn)
        .build()
        .await;
    assert!(app.ctx().try_get::<dyn AuditSink>().await.is_some());
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_plain_framework_module_a_plugin_imports_boots_as_before() {
    use navree_plugin_khata::KhataUiPlugin;

    // `UiModule` is a plain module of a framework crate (`navree_ui`), imported here only by the
    // community plugin: it acts for no plugin, its app-wide layers and command are not refused,
    // and the app boots cleanly.
    let app = khata_app().module(KhataUiPlugin).build().await;
    assert_eq!(
        navree::kernel::__private::acts_for(app.ctx(), UiModule.id()),
        None
    );
    let report = app.ctx().plugins();
    let [plugin] = report.plugins.as_slice() else {
        panic!("one plugin")
    };
    assert_eq!(plugin.info.name, "navree-plugin-khata");
    let ids: Vec<ModuleId> = plugin.modules.iter().map(|module| module.id).collect();
    assert_eq!(ids, [ModuleId::of::<KhataUiPlugin>()]);
    app.shutdown().await.unwrap();
}
