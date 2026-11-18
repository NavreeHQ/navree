//! `FlagsModule` as a first-party plugin.

use std::sync::{Arc, Mutex};

use navree_flags::FlagsModule;
use navree_kernel::__private::{acts_for, is_app_module};
use navree_kernel::{Module, ModuleBuilder, ModuleId};

use crate::support::{NewPipeline, World, platform};

/// Every warning or error logged on this thread while the guard lives (the
/// test runs on a current-thread runtime, so the boot logs on this thread).
#[derive(Clone, Default)]
struct Warnings(Arc<Mutex<Vec<String>>>);

impl Warnings {
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(self.clone())
    }

    fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

struct Message(String);

impl tracing::field::Visit for Message {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl tracing::Subscriber for Warnings {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() <= tracing::Level::WARN
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() <= tracing::Level::WARN {
            let mut message = Message(String::new());
            event.record(&mut message);
            self.0.lock().unwrap().push(message.0);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

#[tokio::test]
async fn flags_names_itself_a_first_party_plugin_with_the_namespace_flags() {
    let world = World::new().await;
    let report = world.app.ctx().plugins();
    assert!(
        report.plugins.iter().all(|plugin| plugin.first_party),
        "{report:?}"
    );
    let matching: Vec<_> = report
        .plugins
        .iter()
        .filter(|plugin| plugin.info.name == "navree-flags")
        .collect();
    let [plugin] = matching.as_slice() else {
        panic!("expected one navree-flags plugin: {report:?}")
    };
    assert_eq!(plugin.info.namespace, "flags");
    assert_eq!(plugin.info.version, env!("CARGO_PKG_VERSION"));
    assert!(plugin.first_party);

    let flags_module = plugin
        .modules
        .iter()
        .find(|module| module.id.type_name() == "navree_flags::module::FlagsModule")
        .expect("FlagsModule is listed under navree-flags");
    assert_eq!(
        flags_module.commands,
        [
            "flags:list",
            "flags:on",
            "flags:off",
            "flags:forget",
            "flags:rollout",
            "flags:kill",
            "flags:revive",
            "flags:purge",
        ]
    );
    assert!(flags_module.publishable.is_empty());
}

/// An app module that imports a flag registration.
struct Deals;

impl Module for Deals {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(FlagsModule::for_feature::<NewPipeline>());
    }
}

#[tokio::test]
async fn a_module_that_imports_for_feature_stays_the_apps_own() {
    let world = World::with(platform().module(Deals).module(FlagsModule::from_config())).await;
    let ctx = world.app.ctx();
    let deals = ModuleId::of::<Deals>();
    let feature = FlagsModule::for_feature::<NewPipeline>().id();

    assert!(is_app_module(ctx, deals));
    assert_eq!(
        acts_for(ctx, feature).map(|info| info.name),
        Some("navree-flags")
    );
    assert!(acts_for(ctx, deals).is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn the_flags_module_names_itself_and_boots_with_no_warning() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let _world = World::new().await;
    let logged = warnings.all();
    assert!(logged.is_empty(), "{logged:?}");
}
