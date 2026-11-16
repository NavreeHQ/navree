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
