//! Settings groups whose read or write permission names a community plugin's
//! namespace.

use navree_contracts::{ModuleBuilderPermissionsExt, Permission};
use navree_kernel::{Module, ModuleBuilder, PluginInfo};
use navree_settings::{ModuleBuilderSettingsExt, SettingsModule};
use navree_validation::Validate;
use serde::{Deserialize, Serialize};

use crate::support::platform;

mod navree_plugin_khata {
    use super::*;

    const KHATA: PluginInfo =
        PluginInfo::new("navree-plugin-khata", module_path!(), "khata", "1.0.0");

    pub struct KhataPlugin(pub bool);

    impl Module for KhataPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(KHATA);
            if self.0 {
                m.permissions(&[
                    Permission::new("khata.manage", "Manage Ludhiana khata settings"),
                    Permission::new("khata.read", "Read Ludhiana khata settings"),
                ]);
            }
        }
    }
}

#[derive(
    navree_settings::Settings, Clone, Debug, Default, PartialEq, Serialize, Deserialize, Validate,
)]
#[settings(
    key = "ledger-write",
    scopes(global, workspace),
    permission = "khata.manage"
)]
struct LedgerWriteSettings {
    #[validate(skip)]
    pub auto_close: bool,
}

#[derive(
    navree_settings::Settings, Clone, Debug, Default, PartialEq, Serialize, Deserialize, Validate,
)]
#[settings(
    key = "ledger-read",
    scopes(global, workspace),
    read_permission = "khata.read"
)]
struct LedgerReadSettings {
    #[validate(skip)]
    pub currency: String,
}

struct AppWriteSettings;

impl Module for AppWriteSettings {
    fn register(&self, m: &mut ModuleBuilder) {
        m.settings::<LedgerWriteSettings>();
    }
}

struct AppReadSettings;

impl Module for AppReadSettings {
    fn register(&self, m: &mut ModuleBuilder) {
        m.settings::<LedgerReadSettings>();
    }
}

#[tokio::test]
async fn a_settings_group_that_requires_such_a_permission_fails_the_boot() {
    use navree_plugin_khata::KhataPlugin;

    // Write permission under `khata` not described by the plugin: boot fails.
    let write_error = match platform()
        .module(SettingsModule::from_config())
        .module(KhataPlugin(false))
        .module(AppWriteSettings)
        .try_build()
        .await
    {
        Ok(_) => panic!("boot must fail when `khata.manage` is not described by the plugin"),
        Err(error) => error.to_string(),
    };
    let expected_write = "plugin `navree-plugin-khata` uses the namespace `khata`, the first word of the permission `khata.manage` that the settings group `ledger-write` of `AppWriteSettings` requires, and the plugin describes no permission of that name; a plugin's policies decide the names under its namespace, so they would decide who may use it: require a permission the plugin describes, or name the app's permission under another word";
    assert!(
        write_error.contains(expected_write),
        "expected `{expected_write}` in:\n{write_error}"
    );

    // Read permission under `khata` not described by the plugin: boot fails.
    let read_error = match platform()
        .module(SettingsModule::from_config())
        .module(KhataPlugin(false))
        .module(AppReadSettings)
        .try_build()
        .await
    {
        Ok(_) => panic!("boot must fail when `khata.read` is not described by the plugin"),
        Err(error) => error.to_string(),
    };
    let expected_read = "plugin `navree-plugin-khata` uses the namespace `khata`, the first word of the permission `khata.read` that the settings group `ledger-read` of `AppReadSettings` requires, and the plugin describes no permission of that name; a plugin's policies decide the names under its namespace, so they would decide who may use it: require a permission the plugin describes, or name the app's permission under another word";
    assert!(
        read_error.contains(expected_read),
        "expected `{expected_read}` in:\n{read_error}"
    );

    // When the plugin describes both permissions, both settings groups boot.
    let app = platform()
        .module(SettingsModule::from_config())
        .module(KhataPlugin(true))
        .module(AppWriteSettings)
        .module(AppReadSettings)
        .build()
        .await;
    app.shutdown().await.unwrap();
}
