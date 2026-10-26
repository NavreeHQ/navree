//! Boot: the module's configuration, servers merged from several modules,
//! and the mistakes that stop the boot.

use navree_db::DatabaseModule;
use navree_http::AppError;
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::{injectable, mcp_server};
use navree_mcp::testing::McpTestClient;
use navree_mcp::{McpModule, McpOptions, McpRegistry, McpSettings, ModuleBuilderMcpExt};
use navree_testing::TestApp;
use serde_json::{Value, json};

use crate::support::{builder, user};

/// A second server, registered by another module.
#[injectable]
pub struct ReportsMcp;

#[mcp_server]
#[authenticated]
impl ReportsMcp {
    /// Today's revenue.
    #[mcp_tool(read_only)]
    async fn revenue_today(&self) -> Result<u64, AppError> {
        Ok(1200)
    }
}

pub struct ReportsModule;

impl Module for ReportsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.mcp_server::<ReportsMcp>();
    }
}

/// A server whose tool clashes with `CrmMcp`'s `list_deals`.
#[injectable]
pub struct ClashingMcp;

#[mcp_server]
impl ClashingMcp {
    /// Another list.
    #[mcp_tool(name = "list_deals")]
    #[public]
    async fn other_list(&self) -> Result<Value, AppError> {
        Ok(json!([]))
    }
}

pub struct ClashingModule;

impl Module for ClashingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.mcp_server::<ClashingMcp>();
    }
}

#[tokio::test]
async fn servers_of_several_modules_share_one_endpoint() {
    let app = builder().module(ReportsModule).build().await;
    let registry = app.get::<McpRegistry>().await;
    let names: Vec<&str> = registry.tools().map(|tool| tool.name.as_str()).collect();
    assert!(
        names.contains(&"list_deals") && names.contains(&"revenue_today"),
        "{names:?}"
    );

    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    mcp.call_tool("revenue_today", json!({}))
        .await
        .assert_ok()
        .assert_json_path("", json!(1200));
}

#[tokio::test]
async fn duplicate_names_fail_the_boot_naming_both_modules() {
    let error = builder()
        .module(ClashingModule)
        .try_build()
        .await
        .map(drop)
        .expect_err("the boot fails")
        .to_string();
    assert!(
        error.contains("two MCP tools named `list_deals`"),
        "{error}"
    );
    assert!(
        error.contains("DealsModule") && error.contains("ClashingModule"),
        "{error}"
    );
}

#[tokio::test]
async fn access_checks_need_the_access_module() {
    let error = TestApp::builder()
        .temp_database()
        .module(DatabaseModule)
        // Imports `AccessModule::for_feature` only: no `AccessModule`.
        .module(crate::support::DealsModule)
        .module(McpModule::from_config())
        .try_build()
        .await
        .map(drop)
        .expect_err("the boot fails")
        .to_string();
    assert!(error.contains("add AccessModule"), "{error}");
}

#[tokio::test]
async fn invalid_options_fail_the_boot_naming_the_key() {
    let error = builder()
        .config("mcp.session_ttl_secs", 0)
        .try_build()
        .await
        .map(drop)
        .expect_err("the boot fails")
        .to_string();
    assert!(error.contains("mcp.session_ttl_secs"), "{error}");

    let error = TestApp::builder()
        .module(McpModule::for_root(McpOptions {
            path: "mcp".to_owned(),
            ..McpOptions::default()
        }))
        .try_build()
        .await
        .map(drop)
        .expect_err("the boot fails")
        .to_string();
    assert!(error.contains("mcp.path"), "{error}");
}

#[tokio::test]
async fn for_root_takes_the_options_in_code() {
    let app = TestApp::builder()
        .module(McpModule::for_root(McpOptions {
            name: Some("navree-crm".into()),
            title: Some("Navree CRM".into()),
            version: Some("1.4.0".into()),
            instructions: Some("Amounts are in cents.".into()),
            ..McpOptions::default()
        }))
        .build()
        .await;
    let settings = app.get::<McpSettings>().await;
    assert_eq!(
        (
            settings.name.as_str(),
            settings.title.as_str(),
            settings.version.as_str()
        ),
        ("navree-crm", "Navree CRM", "1.4.0")
    );
    let mcp = McpTestClient::new(app.acting_as(user())).initialize().await;
    assert_eq!(mcp.server_info()["serverInfo"]["title"], "Navree CRM");
    assert_eq!(mcp.server_info()["instructions"], "Amounts are in cents.");
    assert!(mcp.list_tools().await.is_empty());
}
