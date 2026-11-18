//! Boots `navree-plugin-notices` in [`TestApp`] and checks its plugin report
//! and warning-free startup alongside the framework's modules.

#![allow(missing_docs)]

use std::sync::{Arc, Mutex};

use navree::crypto::generate_key;
use navree::helm::HelmModule;
use navree::i18n::I18nModule;
use navree::orm::OrmModule;
use navree::prelude::*;
use navree::schedule::ScheduleModule;
use navree::testing::TestApp;
use navree::ui::UiModule;
use navree::view::ViewModule;
use navree_plugin_notices::NoticesModule;
use serde_json::Value;

#[tokio::test]
async fn the_plugin_boots_and_is_listed() {
    let standalone = Navree::app()
        .module(NoticesModule::from_config())
        .plugin_report()
        .expect("valid plugin graph");
    assert_eq!(standalone.plugins.len(), 1);
    assert_eq!(standalone.plugins[0].info.name, "navree-plugin-notices");
    assert_eq!(standalone.plugins[0].info.namespace, "notices");
    assert_eq!(standalone.plugins[0].info.title, Some("Notice board"));

    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(NoticesModule::from_config())
        .build()
        .await;
    let report = app.ctx().plugins();
    let plugin = report
        .plugins
        .iter()
        .find(|plugin| plugin.info.name == "navree-plugin-notices")
        .expect("navree-plugin-notices is listed in plugins");
    assert_eq!(plugin.info.namespace, "notices");
    assert_eq!(plugin.info.title, Some("Notice board"));
    app.shutdown().await.expect("clean shutdown");
}

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

#[tokio::test(flavor = "current_thread")]
async fn the_plugin_is_a_community_plugin_and_boots_with_no_warning() {
    let warnings = Warnings::default();
    let _guard = warnings.install();

    let app = TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        .module(OrmModule)
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(I18nModule::from_config())
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(HelmModule::from_config())
        .module(ScheduleModule::for_root())
        .module(NoticesModule::from_config())
        .build()
        .await;

    let report = app.ctx().plugins();
    let plugin = report
        .plugins
        .iter()
        .find(|plugin| plugin.info.name == "navree-plugin-notices")
        .expect("navree-plugin-notices is listed in plugins");
    assert!(!plugin.first_party);
    let logged = warnings.all();
    assert!(
        logged.is_empty(),
        "unexpected warnings or errors: {logged:?}"
    );

    app.shutdown().await.expect("clean shutdown");
}
