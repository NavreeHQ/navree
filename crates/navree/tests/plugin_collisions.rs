//! A plugin that is not one of the framework's own packages beside an app's modules: a template
//! it registers outside its namespace with the name of one of the app's, or one that replaces a
//! template of an app's module it imports, fails the boot, where two of the app's own modules
//! would only be warned about.

use navree::config::ConfigSection;
use navree::contracts::{Job, JobContext};
use navree::prelude::*;
use navree::queue::{QueueDriver, QueueOptions};
use navree::testing::TestApp;
use navree::ui::AppSheet;
use navree::view::ViewOptions;
use serde::{Deserialize, Serialize};

/// The CRM's sheet.
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

/// The notices section as the CRM reads it.
#[derive(Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct CrmNotices {
    per_page: u32,
}

impl ConfigSection for CrmNotices {
    const KEY: &'static str = "notices";
}

/// An app module with a welcome mail, a job, a sheet and a section.
struct CrmModule;
impl Module for CrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("mail/welcome.html", "Welcome from the CRM")])
            .job::<Digest>()
            .contribute(CRM_BRAND)
            .config_section::<CrmNotices>();
    }
}

/// The notices plugin, which is not one of the framework's own: its crate is this module, so
/// the app's modules at the top of the file are in no crate it names.
mod navree_plugin_notices {
    use navree::kernel::PluginInfo;
    use navree::prelude::*;

    use crate::CrmModule;

    /// The plugin's info.
    fn notices() -> PluginInfo {
        PluginInfo::new("navree-plugin-notices", module_path!(), "notices", "1.0.0")
    }

    /// The plugin, with a welcome mail outside its namespace.
    pub struct NoticesPlugin;
    impl Module for NoticesPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(notices())
                .templates(&[("mail/welcome.html", "Welcome from Meera Sharma")]);
        }
    }

    /// A second module of the plugin, which imports the app's CRM and replaces its welcome
    /// mail.
    pub struct NoticesShell;
    impl Module for NoticesShell {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(notices())
                .import(CrmModule)
                .templates(&[("mail/welcome.html", "Welcome from the notices plugin")]);
        }
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
async fn a_plugin_that_is_not_the_frameworks_own_does_not_collide_like_a_module_of_today() {
    use navree_plugin_notices::{NoticesPlugin, NoticesShell};

    // Its welcome mail beside the CRM's fails the boot.
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

    // It may not take over a template of an app's module it imports.
    let Err(error) = modules()
        .module(CrmModule)
        .module(NoticesShell)
        .try_build()
        .await
    else {
        panic!("fails")
    };
    let error = error.to_string();
    assert!(
        error.contains(
            "embedded template `mail/welcome.html` of `CrmModule` cannot be replaced by `NoticesShell`, a module of the plugin `navree-plugin-notices`; to settle it, put the template you want, the plugin's or your own, in a template folder of the app, or register it from a module of the app that comes after `NoticesShell`"
        ),
        "{error}"
    );
}
