//! What the app's tools, resources and prompts require under a plugin's namespace: the boot
//! reads an item's declaration as it reads a route's.

use std::sync::Arc;

use navree_contracts::Permission;
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::injectable;
use navree_mcp::{McpItem, McpOutput, McpServer, ModuleBuilderMcpExt, ToolDef};
use navree_testing::TestAppBuilder;
use serde_json::json;

use crate::support::builder;

use navree_plugin_khata::KhataPlugin;
use navree_plugin_reports::ReportsPlugin;

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

/// The khata plugin, a community plugin whose crate is this module: its namespace is `khata`,
/// and it registers nothing.
mod navree_plugin_khata {
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};

    /// The plugin's info.
    const KHATA: PluginInfo =
        PluginInfo::new("navree-plugin-khata", module_path!(), "khata", "1.0.0");

    /// The plugin's module: it names the plugin.
    pub struct KhataPlugin;

    impl Module for KhataPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(KHATA);
        }
    }
}

/// What the reports plugin describes, when it describes anything.
const REPORTS_VIEW: &[Permission] = &[Permission::new(
    "reports.view",
    "See the sales reports of the Ludhiana office",
)];

/// The app's khata tool: a summary for those who can read `khata`, a subject no module
/// registers.
fn summary() -> McpItem {
    McpItem::tool(
        ToolDef::new("khata_summary").can("read", "khata", None),
        |_| async { Ok(McpOutput::Json(json!("The khata of Ludhiana is settled."))) },
    )
}

/// The app's khata server, written without the macro.
#[injectable]
struct KhataMcp;

impl McpServer for KhataMcp {
    fn items(self: Arc<Self>) -> Vec<McpItem> {
        vec![summary()]
    }
}

/// A module of the app that offers [`KhataMcp`].
struct KhataModule;

impl Module for KhataModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.mcp_server::<KhataMcp>();
    }
}

/// The boot error of an app that must not boot.
async fn boot_error(builder: TestAppBuilder) -> String {
    builder
        .try_build()
        .await
        .map(drop)
        .expect_err("the boot must fail")
        .to_string()
}

#[tokio::test]
async fn an_mcp_item_under_a_plugins_namespace_is_read_as_a_route_is() {
    // The test app's tool `report_sales` asks for `reports.view`, which no subject maps and the
    // reports plugin does not describe: the plugin's policies would decide who may call it.
    let error = boot_error(builder().module(ReportsPlugin(&[]))).await;
    let expected = "plugin `navree-plugin-reports` uses the namespace `reports`, the first word of the permission `reports.view` that the MCP tool `report_sales` of `DealsModule` requires, and the plugin describes no permission of that name; a plugin's policies decide the names under its namespace, so they would decide who may use it: require a permission the plugin describes, or name the app's permission under another word";
    assert!(error.contains(expected), "{error}");

    // The plugin describes `reports.view`: the tool requires it on purpose, and the app boots.
    builder()
        .module(ReportsPlugin(REPORTS_VIEW))
        .try_build()
        .await
        .map(drop)
        .expect("a tool of the app may require a permission the plugin describes");

    // The subject a tool checks, which nothing registers as the plugin's.
    let error = boot_error(builder().module(KhataModule).module(KhataPlugin)).await;
    let expected = "plugin `navree-plugin-khata` uses the namespace `khata`, the first word of the subject `khata` that the MCP tool `khata_summary` of `KhataModule` checks, and the plugin registers no subject of that name; a plugin's policies decide the names under its namespace, so they would decide who may use it: check a subject the plugin registers, or name the app's subject under another word";
    assert!(error.contains(expected), "{error}");

    // Without the plugin the tool's subject is the app's to name.
    builder()
        .module(KhataModule)
        .try_build()
        .await
        .map(drop)
        .expect("the app boots without the plugin");
}

#[tokio::test]
async fn a_plugins_mapping_of_another_crates_error_is_refused_in_an_app_with_no_http_module() {
    use std::any::type_name;

    use navree_http::{AppError, ModuleBuilderHttpExt};
    use navree_kernel::Navree;
    use navree_mcp::{McpModule, McpOptions};

    mod navree_plugin_mapper {
        use navree_http::{AppError, ModuleBuilderHttpExt};
        use navree_kernel::{Module, ModuleBuilder, PluginInfo};

        const MAPPER: PluginInfo =
            PluginInfo::new("navree-plugin-mapper", module_path!(), "mapper", "1.0.0");

        pub struct MapperPlugin;

        impl Module for MapperPlugin {
            fn register(&self, m: &mut ModuleBuilder) {
                m.plugin(MAPPER)
                    .map_error::<std::io::Error>(|_| Some(AppError::forbidden()));
            }
        }
    }

    // Without the plugin mapper, an MCP app with no `HttpModule` boots cleanly.
    let app = Navree::app()
        .module(McpModule::for_root(McpOptions::default()))
        .boot()
        .await
        .expect("MCP boots without HttpModule");
    app.shutdown().await.unwrap();

    // With the plugin mapping `std::io::Error`, the boot fails with no `HttpModule` in the app.
    let error = Navree::app()
        .module(McpModule::for_root(McpOptions::default()))
        .module(navree_plugin_mapper::MapperPlugin)
        .boot()
        .await
        .expect_err("foreign error mapper from a plugin must fail the boot")
        .to_string();
    let io_error = type_name::<std::io::Error>();
    let expected = format!(
        "plugin `navree-plugin-mapper` answers the app's errors of type `{io_error}` (m.map_error in `MapperPlugin`), a type of another crate; only a module of the app maps another crate's errors: remove the plugin or update it to a version that maps only its own errors, and to use the plugin's mapping, register it from a module of the app (m.map_error)"
    );
    assert!(
        error.contains(&expected),
        "expected `{expected}` in:\n{error}"
    );

    // A module of the app may still map `std::io::Error` in an app with no `HttpModule`.
    struct AppMapsIo;
    impl Module for AppMapsIo {
        fn register(&self, m: &mut ModuleBuilder) {
            m.map_error::<std::io::Error>(|_| Some(AppError::bad_request("io")));
        }
    }
    let app = Navree::app()
        .module(McpModule::for_root(McpOptions::default()))
        .module(AppMapsIo)
        .boot()
        .await
        .expect("the app's own mapper boots");
    app.shutdown().await.unwrap();
}
