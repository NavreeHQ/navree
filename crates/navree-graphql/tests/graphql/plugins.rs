//! What the app's GraphQL fields require under a plugin's namespace: the boot reads a field's
//! declaration as it reads a route's.

use async_graphql::{Request, Response};
use navree_contracts::Permission;
use navree_graphql::{GraphqlSchema, ModuleBuilderGraphqlExt};
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::{injectable, resolver};
use navree_testing::{TestApp, TestAppBuilder, principal};
use serde_json::json;
use uuid::Uuid;

use crate::support::{RecordingAudit, builder, give_role};

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

/// What the reports plugin describes, when it describes anything.
const REPORTS_VIEW: &[Permission] = &[Permission::new(
    "reports.view",
    "See the sales reports of the Ludhiana office",
)];

/// The app's sales figures.
#[injectable]
pub struct SalesResolver;

#[resolver]
impl SalesResolver {
    /// The month's sales, for the holders of `reports.view`.
    #[query]
    #[permissions("reports.view")]
    async fn sales_month(&self) -> u32 {
        1200
    }
}

/// The app's sales feature: its resolver.
struct SalesModule;

impl Module for SalesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.resolver::<SalesResolver>();
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

/// `{ salesMonth }` as `viewer`.
async fn sales_month(app: &TestApp, viewer: Uuid) -> Response {
    let request = Request::new("{ salesMonth }").data(principal(viewer));
    app.get::<GraphqlSchema>().await.execute(request).await
}

/// The `code` extension of the first error of `response`.
fn code(response: &Response) -> String {
    response.errors[0]
        .extensions
        .as_ref()
        .and_then(|extensions| extensions.get("code"))
        .and_then(|code| code.clone().into_json().ok())
        .and_then(|code| code.as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[tokio::test]
async fn a_graphql_field_under_a_plugins_namespace_is_read_as_a_route_is() {
    let audit = RecordingAudit::default();

    // `salesMonth` asks for `reports.view`, which no subject maps and the reports plugin does
    // not describe: the plugin's policies would decide who may resolve the field.
    let app = builder(&audit)
        .module(SalesModule)
        .module(ReportsPlugin(&[]));
    let error = boot_error(app).await;
    let expected = "plugin `navree-plugin-reports` uses the namespace `reports`, the first word of the permission `reports.view` that the GraphQL query `sales_month` of `SalesModule` requires, and the plugin describes no permission of that name; a plugin's policies decide the names under its namespace, so they would decide who may use it: require a permission the plugin describes, or name the app's permission under another word";
    assert!(error.contains(expected), "{error}");

    // The plugin describes `reports.view`: the field requires it on purpose, the app boots,
    // and who resolves the field is the app's grant.
    let app = builder(&audit)
        .module(SalesModule)
        .module(ReportsPlugin(REPORTS_VIEW))
        .build()
        .await;
    let (harpreet, meera) = (Uuid::now_v7(), Uuid::now_v7());
    give_role(&app, harpreet, "munshi", &["reports.view"]).await;
    let response = sales_month(&app, harpreet).await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert_eq!(
        response.data.into_json().unwrap(),
        json!({ "salesMonth": 1200 })
    );
    let response = sales_month(&app, meera).await;
    assert_eq!(response.errors.len(), 1, "{:?}", response.errors);
    assert_eq!(code(&response), "FORBIDDEN");
    app.shutdown().await.unwrap();

    // Without the plugin the app boots as before.
    let app = builder(&audit).module(SalesModule).build().await;
    assert!(
        app.ctx()
            .plugins()
            .plugins
            .iter()
            .all(|plugin| plugin.first_party)
    );
    app.shutdown().await.unwrap();
}

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

#[tokio::test]
async fn a_plugins_mapping_of_another_crates_error_is_refused_in_an_app_with_no_http_module() {
    use std::any::type_name;

    use navree_graphql::{GraphqlModule, GraphqlOptions};
    use navree_kernel::Navree;

    #[injectable]
    struct PublicHealthResolver;

    #[resolver]
    impl PublicHealthResolver {
        #[query]
        #[public]
        async fn city(&self) -> &'static str {
            "Ludhiana"
        }
    }

    struct PublicGraphql;

    impl Module for PublicGraphql {
        fn register(&self, m: &mut ModuleBuilder) {
            m.resolver::<PublicHealthResolver>();
        }
    }

    // Without the plugin mapper, a GraphQL app with no `HttpModule` boots cleanly.
    let app = Navree::app()
        .module(GraphqlModule::for_root(GraphqlOptions::default()))
        .module(PublicGraphql)
        .boot()
        .await
        .expect("GraphQL boots without HttpModule");
    app.shutdown().await.unwrap();

    // With the plugin mapping `std::io::Error`, the boot fails with no `HttpModule` in the app.
    let error = Navree::app()
        .module(GraphqlModule::for_root(GraphqlOptions::default()))
        .module(PublicGraphql)
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
}
