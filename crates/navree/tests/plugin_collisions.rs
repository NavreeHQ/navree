//! The framework's own plugins beside an app's modules: a template, a static file, a job and a
//! configuration section given twice are warnings between them, never boot errors, while a
//! plugin that is not the framework's own fails the boot instead.
//!
//! A first-party plugin needs a module type of a first-party crate, so `Probe` takes the id of
//! `navree::time::TimeModule` and names navree-time's plugin, the way the framework's own crate
//! would.

use navree::ModuleId;
use navree::config::ConfigSection;
use navree::contracts::{Job, JobContext};
use navree::kernel::PluginInfo;
use navree::prelude::*;
use navree::queue::{QueueDriver, QueueOptions};
use navree::testing::TestApp;
use navree::ui::AppSheet;
use navree::view::{ViewOptions, Views};
use serde::{Deserialize, Serialize};

/// Two sheets of one name and different text.
static PROBE_BRAND: AppSheet = AppSheet::new("brand", b".hero{color:var(--accent)}", 1024);
static CRM_BRAND: AppSheet = AppSheet::new("brand", b".hero{color:var(--ink-1)}", 1024);

/// A job that sends the notices of the day.
#[derive(Serialize, Deserialize)]
struct Digest {
    ward: String,
}

impl Job for Digest {
    const NAME: &'static str = "notices.digest";

    async fn handle(self, _ctx: JobContext) -> std::result::Result<(), BoxError> {
        Ok(())
    }
}

/// The notices section as the framework's plugin reads it.
#[derive(Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct TimeNotices {
    title: String,
}

impl ConfigSection for TimeNotices {
    const KEY: &'static str = "notices";
}

/// The same section as the CRM reads it.
#[derive(Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct CrmNotices {
    per_page: u32,
}

impl ConfigSection for CrmNotices {
    const KEY: &'static str = "notices";
}

/// A module that takes the id of navree-time's module and names its plugin: the kernel finds
/// the plugin first-party.
struct Probe;
impl Module for Probe {
    fn id(&self) -> ModuleId {
        ModuleId::of::<navree::time::TimeModule>()
    }

    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(PluginInfo::new(
            "navree-time",
            "navree_time",
            "time",
            env!("CARGO_PKG_VERSION"),
        ))
        .templates(&[("mail/welcome.html", "Welcome from the probe")])
        .job::<Digest>()
        .contribute(PROBE_BRAND)
        .config_section::<TimeNotices>();
    }
}

/// An app module with the same four things.
struct CrmModule;
impl Module for CrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("mail/welcome.html", "Welcome from the CRM")])
            .job::<Digest>()
            .contribute(CRM_BRAND)
            .config_section::<CrmNotices>();
    }
}

fn notices() -> PluginInfo {
    PluginInfo::new(
        "navree-plugin-notices",
        env!("CARGO_CRATE_NAME"),
        "notices",
        "1.0.0",
    )
}

/// A plugin that is not the framework's own, with a welcome mail outside its namespace.
struct NoticesPlugin;
impl Module for NoticesPlugin {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(notices())
            .templates(&[("mail/welcome.html", "Welcome from Meera Sharma")]);
    }
}

/// A second module of the same plugin, which imports the framework's plugin and replaces its
/// welcome mail.
struct NoticesShell;
impl Module for NoticesShell {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(notices())
            .import(Probe)
            .templates(&[("mail/welcome.html", "Welcome from the notices plugin")]);
    }
}

/// A test app with the modules that check the four names.
fn modules() -> navree::testing::TestAppBuilder {
    TestApp::builder()
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .module(ViewModule::for_root(ViewOptions {
            dirs: Vec::new(),
            reload: false,
        }))
        .module(UiModule)
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Memory,
            ..QueueOptions::default()
        }))
}

#[tokio::test]
async fn a_first_party_plugin_collides_like_a_module_of_today() {
    // A template, a static file, a job and a section beside today's module: warnings, never an
    // error.
    let app = modules().module(Probe).module(CrmModule).build().await;
    assert!(!navree::kernel::__private::is_community_plugin(
        app.ctx(),
        ModuleId::of::<navree::time::TimeModule>()
    ));
    assert_eq!(
        app.get::<Views>()
            .await
            .render("mail/welcome", serde_json::json!({}))
            .unwrap(),
        "Welcome from the CRM"
    );
    let sheet = app
        .client()
        .get(&PROBE_BRAND.url())
        .send()
        .await
        .assert_status(200);
    assert_eq!(sheet.text().as_bytes(), PROBE_BRAND.bytes());
    app.shutdown().await.unwrap();

    // A plugin that is not the framework's own fails the boot instead.
    let Err(error) = modules()
        .module(NoticesPlugin)
        .module(CrmModule)
        .try_build()
        .await
    else {
        panic!("fails")
    };
    assert!(
        error.to_string().contains(
            "embedded template `mail/welcome.html` is registered by both `NoticesPlugin` and `CrmModule`; a plugin's templates are under its namespace"
        ),
        "{error}"
    );

    // A plugin that is not the framework's own may not take over a template of a first-party
    // plugin it imports.
    let Err(error) = modules().module(NoticesShell).try_build().await else {
        panic!("fails")
    };
    let error = error.to_string();
    assert!(
        error.contains("embedded template `mail/welcome.html` of `"),
        "{error}"
    );
    assert!(
        error.contains(
            "cannot be replaced by `NoticesShell`, a module of the plugin `navree-plugin-notices`; to use the plugin's version, copy it to resources/views/mail/welcome.html or register it from a module of the app"
        ),
        "{error}"
    );
}
