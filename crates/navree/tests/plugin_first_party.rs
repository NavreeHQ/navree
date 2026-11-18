//! First-party plugins keep the rules of the framework's own modules: duplicates with an app's
//! modules warn instead of failing the boot, a view hook may replace a built-in filter, a
//! community plugin may not take over a first-party plugin's template, a later module of a
//! first-party plugin does not settle an open takeover, and a first-party plugin's namespace is
//! not compared with the app's handlers.

use std::sync::{Arc, Mutex};

use http::Method;
use navree::access::{AccessModule, AccessOptions, AccessService, Subject};
use navree::config::ConfigSection;
use navree::contracts::{Job, JobContext};
use navree::crypto::generate_key;
use navree::db::{DatabaseModule, Db};
use navree::flags::{FlagStoreKind, FlagsModule, FlagsOptions};
use navree::http::{Controller, RouteDef};
use navree::i18n::{I18nModule, Locale};
use navree::mail::{MailModule, MailOptions, TransportConfig};
use navree::notify::NotifyModule;
use navree::prelude::*;
use navree::queue::{QueueDriver, QueueModule, QueueOptions};
use navree::testing::{TestApp, TestAppBuilder, principal};
use navree::ui::UiModule;
use navree::view::{ViewModule, ViewOptions, Views};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// Warnings logged on this thread while the guard lives, with each event's target.
#[derive(Clone, Default)]
struct Warnings(Arc<Mutex<Vec<(String, String)>>>);

impl Warnings {
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(self.clone())
    }

    fn at(&self, target: &str) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(at, _)| at == target)
            .map(|(_, message)| message.clone())
            .collect()
    }

    fn all(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|(_, message)| message.clone())
            .collect()
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
            let target = event.metadata().target().to_owned();
            self.0.lock().unwrap().push((target, message.0));
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// An app job with the same job name as `NotifyModule`'s `SendNotificationJob`.
#[derive(Serialize, Deserialize)]
struct CrmSendJob {
    office: String,
}

impl Job for CrmSendJob {
    const NAME: &'static str = "notify.send";

    async fn handle(self, _ctx: JobContext) -> std::result::Result<(), BoxError> {
        Ok(())
    }
}

/// An app section reading the same `notify` key as `NotifyModule`.
#[derive(Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct CrmNotifySection {
    batch_size: u32,
}

impl ConfigSection for CrmNotifySection {
    const KEY: &'static str = "notify";
}

/// An app module whose template, job and section share names with `NotifyModule`.
struct CrmModule;

impl Module for CrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("notify/digest.html", "Digest from the Ludhiana CRM")])
            .job::<CrmSendJob>()
            .config_section::<CrmNotifySection>();
    }
}

/// A test app with the dependencies `NotifyModule::from_config()` resolves.
fn notify_builder() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(ViewModule::for_root(ViewOptions {
            dirs: Vec::new(),
            reload: false,
        }))
        .module(UiModule)
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Memory,
            ..QueueOptions::default()
        }))
        .module(MailModule::for_root(MailOptions {
            from: "Ludhiana Desk <noreply@ludhiana.example>".to_owned(),
            transport: TransportConfig::Array,
            ..MailOptions::default()
        }))
}

// None of the 31 converted packages contributes an `AppSheet` of its own, so the static-file
// half is proven on a stand-in module inside `navree-helm` (`a_first_party_plugins_static_file_beside_an_apps_only_warns`).
#[tokio::test(flavor = "current_thread")]
async fn a_first_party_plugins_template_static_file_job_and_section_beside_an_apps_only_warn() {
    let warnings = Warnings::default();
    let _guard = warnings.install();

    let app = notify_builder()
        .module(NotifyModule::from_config())
        .module(CrmModule)
        .build()
        .await;

    let notify = app
        .ctx()
        .plugins()
        .plugins
        .into_iter()
        .find(|plugin| plugin.info.name == "navree-notify")
        .expect("navree-notify listed");
    assert!(notify.first_party, "{notify:?}");

    let view_warnings = warnings.at("navree::view");
    assert!(
        view_warnings.iter().any(|warning| warning.contains(
            "embedded template `notify/digest.html` is registered by both `NotifyModule` and `CrmModule`; `CrmModule`'s is used, and this becomes an error in 2.0"
        )),
        "{view_warnings:?}"
    );

    let queue_warnings = warnings.at("navree::queue");
    assert!(
        queue_warnings.iter().any(|warning| warning.contains(
            "job `notify.send` is registered by both `NotifyModule` and `CrmModule`; `NotifyModule`'s runs, and this becomes an error in 2.0"
        )),
        "{queue_warnings:?}"
    );

    let config_warnings = warnings.at("navree::config");
    assert!(
        config_warnings.iter().any(|warning| warning
            .contains("configuration section `notify` is read by two types:")
            && warning.contains("(NotifyModule)")
            && warning.contains("(CrmModule); this becomes an error in 2.0")),
        "{config_warnings:?}"
    );

    let rendered = app
        .get::<Views>()
        .await
        .render("notify/digest", json!({}))
        .expect("render digest");
    assert_eq!(rendered, "Digest from the Ludhiana CRM");

    app.shutdown().await.unwrap();
}

/// An app module with a receipt template formatted through the `money` filter.
struct ReceiptModule;

impl Module for ReceiptModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("ledger/receipt.txt", "{{ amount | money(\"INR\") }}")]);
    }
}

/// A community plugin for the view-hook and template-takeover checks.
mod navree_plugin_notices {
    use navree::notify::NotifyModule;
    use navree::plugin::PluginInfo;
    use navree::prelude::*;

    use super::AppDigestSeed;

    fn notices() -> PluginInfo {
        PluginInfo::new("navree-plugin-notices", module_path!(), "notices", "1.0.0")
    }

    /// Tries to replace the built-in `money` filter from a community plugin.
    pub struct NoticesFilterPlugin;

    impl Module for NoticesFilterPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(notices()).view_environment(|env| {
                env.add_filter("money", |_: i64, _: &str| "Rs 0".to_owned());
            });
        }
    }

    /// Imports `NotifyModule` and tries to replace its `notify/digest.html` template.
    pub struct NoticesTakeover;

    impl Module for NoticesTakeover {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(notices())
                .import(NotifyModule)
                .templates(&[("notify/digest.html", "Digest from Meera Sharma")]);
        }
    }

    /// Imports the app's seed module and replaces `notify/digest.html`.
    pub struct NoticesReplacesApp;

    impl Module for NoticesReplacesApp {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(notices())
                .import(AppDigestSeed)
                .templates(&[("notify/digest.html", "Digest from the notices plugin")]);
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_first_party_plugins_view_hook_may_replace_a_filter() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let locales_dir = tempfile::tempdir().expect("locales dir");
    let shown = locales_dir.path().to_string_lossy().replace('\\', "/");

    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("i18n.dir", shown.as_str())
        .config("i18n.supported", json!(["en-IN"]))
        .config("i18n.default", "en-IN")
        .module(ViewModule::for_root(ViewOptions {
            dirs: Vec::new(),
            reload: false,
        }))
        .module(I18nModule::from_config())
        .module(ReceiptModule)
        .build()
        .await;

    let logged = warnings.all();
    assert!(logged.is_empty(), "{logged:?}");

    let i18n = app
        .ctx()
        .plugins()
        .plugins
        .into_iter()
        .find(|plugin| plugin.info.name == "navree-i18n")
        .expect("navree-i18n listed");
    assert!(i18n.first_party, "{i18n:?}");

    let rendered = app
        .get::<Views>()
        .await
        .render("ledger/receipt", json!({ "amount": 10_000_000 }))
        .expect("render receipt");
    let expected = Locale::current().format_money(10_000_000, "INR");
    assert_eq!(rendered, expected);
    assert!(rendered.contains('₹'), "{rendered}");
    assert!(rendered.contains("1,00,000.00"), "{rendered}");

    app.shutdown().await.unwrap();

    // A community plugin replacing `money` fails the boot.
    let Err(error) = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(ViewModule::for_root(ViewOptions {
            dirs: Vec::new(),
            reload: false,
        }))
        .module(navree_plugin_notices::NoticesFilterPlugin)
        .try_build()
        .await
    else {
        panic!("community plugin replacing a built-in filter must fail the boot")
    };
    let text = error.to_string();
    assert!(
        text.contains(
            "plugin `navree-plugin-notices`: m.view_environment in `NoticesFilterPlugin`"
        ) && text.contains("money"),
        "{text}"
    );
}

#[tokio::test]
async fn a_community_plugin_may_not_take_over_a_template_of_a_first_party_plugin_it_imports() {
    let Err(error) = notify_builder()
        .module(NotifyModule::from_config())
        .module(navree_plugin_notices::NoticesTakeover)
        .try_build()
        .await
    else {
        panic!("community plugin replacing a first-party template must fail the boot")
    };
    let text = error.to_string();
    assert!(
        text.contains(
            "embedded template `notify/digest.html` of `NotifyModule` cannot be replaced by `NoticesTakeover`, a module of the plugin `navree-plugin-notices`; to settle it, put the template you want, the plugin's or your own, in a template folder of the app, or register it from a module of the app that comes after `NoticesTakeover`"
        ),
        "{text}"
    );
}

/// An app module that embeds `notify/digest.html` before a community plugin replaces it.
struct AppDigestSeed;

impl Module for AppDigestSeed {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("notify/digest.html", "Seed digest of the Ludhiana app")]);
    }
}

/// An app module registered after the community plugin to settle the open takeover.
struct AppDigestChoice;

impl Module for AppDigestChoice {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("notify/digest.html", "Chosen by Harpreet Sandhu")]);
    }
}

#[tokio::test]
async fn a_later_module_of_a_first_party_plugin_does_not_settle_a_community_plugins_takeover() {
    // `NotifyModule` comes after `NoticesReplacesApp` and embeds `notify/digest.html`, yet it
    // acts for `navree-notify` rather than the app, so the open takeover remains unsettled.
    let Err(error) = notify_builder()
        .module(AppDigestSeed)
        .module(navree_plugin_notices::NoticesReplacesApp)
        .module(NotifyModule::from_config())
        .try_build()
        .await
    else {
        panic!("a first-party plugin module must not settle a community plugin's takeover")
    };
    let text = error.to_string();
    assert!(
        text.contains(
            "embedded template `notify/digest.html` of `AppDigestSeed` cannot be replaced by `NoticesReplacesApp`, a module of the plugin `navree-plugin-notices`"
        ),
        "{text}"
    );

    // A module of the app registered after `NoticesReplacesApp` settles it.
    let app = notify_builder()
        .module(AppDigestSeed)
        .module(navree_plugin_notices::NoticesReplacesApp)
        .module(AppDigestChoice)
        .build()
        .await;
    let rendered = app
        .get::<Views>()
        .await
        .render("notify/digest", json!({}))
        .expect("render settled digest");
    assert_eq!(rendered, "Chosen by Harpreet Sandhu");
    app.shutdown().await.unwrap();
}

/// An app subject under the `flags` word, beside `FlagsModule`'s `flags` namespace.
struct FlagsBoard;

impl Subject for FlagsBoard {
    const SUBJECT: &'static str = "flags.Board";
    const PERMISSION_PREFIX: &'static str = "flags";
}

#[injectable]
struct FlagsAppController;

impl Controller for FlagsAppController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/flags-export", || async { "exported" })
                .permissions(&["flags.export"]),
            RouteDef::on(Method::GET, "/flags-board", || async { "board" }).can(
                "read",
                "flags.Board",
                None,
            ),
        ]
    }
}

/// An app module with routes and a subject under `flags`, beside `FlagsModule`.
struct FlagsExportModule;

impl Module for FlagsExportModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.imports(AccessModule::for_subject::<FlagsBoard>())
            .controller::<FlagsAppController>();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_first_party_plugins_namespace_is_not_compared_with_the_apps_handlers() {
    let warnings = Warnings::default();
    let _guard = warnings.install();

    let app = TestApp::builder()
        .temp_database()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(FlagsModule::for_root(FlagsOptions {
            store: FlagStoreKind::Memory,
            routes: false,
            admin_routes: false,
            ..FlagsOptions::default()
        }))
        .module(FlagsExportModule)
        .build()
        .await;

    let logged = warnings.all();
    assert!(logged.is_empty(), "{logged:?}");

    let flags = app
        .ctx()
        .plugins()
        .plugins
        .into_iter()
        .find(|plugin| plugin.info.name == "navree-flags")
        .expect("navree-flags listed");
    assert!(flags.first_party);
    assert_eq!(flags.info.namespace, "flags");

    let db = app.get::<Db>().await;
    let access = app.get::<AccessService>().await;
    let (harpreet, meera) = (Uuid::now_v7(), Uuid::now_v7());
    let uow = db.uow(None, None).await;
    access.create_role(&uow, "munshi", None).await.unwrap();
    access.grant(&uow, "munshi", "flags.export").await.unwrap();
    access.grant(&uow, "munshi", "flags.read").await.unwrap();
    access.assign_role(&uow, harpreet, "munshi").await.unwrap();
    uow.commit().await.unwrap();

    app.acting_as(principal(harpreet))
        .get("/api/flags-export")
        .send()
        .await
        .assert_status(200);
    app.acting_as(principal(harpreet))
        .get("/api/flags-board")
        .send()
        .await
        .assert_status(200);
    app.acting_as(principal(meera))
        .get("/api/flags-export")
        .send()
        .await
        .assert_status(403);
    app.acting_as(principal(meera))
        .get("/api/flags-board")
        .send()
        .await
        .assert_status(403);

    app.shutdown().await.unwrap();
}
