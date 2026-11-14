//! A community plugin that reads a configuration section outside its
//! namespace is warned about at boot. A test binary of its own, because it
//! reads the warnings the boot logs.

use std::sync::{Arc, Mutex};

use navree_config::{ConfigModule, ConfigSection, ConfigSources, ModuleBuilderConfigExt};
use navree_kernel::{Module, ModuleBuilder, Navree, PluginInfo};
use serde::Deserialize;

/// The warnings logged on this thread while the guard lives (the tests run on a
/// current-thread runtime, so the boot logs on this thread).
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
        if *event.metadata().level() == tracing::Level::WARN {
            let mut message = Message(String::new());
            event.record(&mut message);
            self.0.lock().unwrap().push(message.0);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

#[derive(Clone, Debug, Default, Deserialize, ConfigSection)]
#[config(key = "notices")]
#[serde(default)]
struct NoticesOptions {
    /// How long a notice stays up, in days.
    notice_days: u32,
}

#[derive(Clone, Debug, Default, Deserialize, ConfigSection)]
#[config(key = "notices.weekly")]
#[serde(default)]
struct WeeklyOptions {
    /// The day the weekly notice goes out, such as `monday`.
    day: String,
}

#[derive(Clone, Debug, Default, Deserialize, ConfigSection)]
#[config(key = "digest")]
#[serde(default)]
struct DigestOptions {
    /// How many notices a digest holds.
    size: u32,
}

/// A module of the community plugin `navree-plugin-notices` (namespace
/// `notices`) that reads `notices`, `notices.weekly` and `digest`.
struct Notices;

impl Module for Notices {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(PluginInfo::new(
            "navree-plugin-notices",
            "plugin_sections",
            "notices",
            "1.0.0",
        ))
        .config_section::<NoticesOptions>()
        .config_section::<WeeklyOptions>()
        .config_section::<DigestOptions>();
    }
}

/// A module of the app, naming no plugin, that reads the same three sections.
struct Desk;

impl Module for Desk {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config_section::<NoticesOptions>()
            .config_section::<WeeklyOptions>()
            .config_section::<DigestOptions>();
    }
}

/// The warnings about sections read outside a namespace while an app of
/// `ConfigModule` and `module` boots.
async fn outside_namespace_warnings(module: impl Module) -> Vec<String> {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let _app = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::default()))
        .module(module)
        .boot()
        .await
        .expect("boots");
    warnings
        .all()
        .into_iter()
        .filter(|warning| warning.contains("reads the configuration section"))
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn a_community_plugin_reading_a_section_outside_its_namespace_warns() {
    // `notices` and `notices.weekly` are inside the namespace; `digest` is not.
    let digest = "plugin `navree-plugin-notices` reads the configuration section `digest`, outside its namespace `notices`";
    assert_eq!(outside_namespace_warnings(Notices).await, [digest]);
    // A module that names no plugin reads what it likes.
    assert!(outside_namespace_warnings(Desk).await.is_empty());
}
