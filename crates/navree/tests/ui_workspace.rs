//! Workspaces in Navree UI pages, written as an app writes them
//! (the facade and its prelude): two workspaces, a browser switching between
//! them through `POST /workspace/switch`, a page drawing the
//! `WorkspaceSwitcher` with the current one marked, and a DataTable over a
//! `#[model(workspace)]` model that lists only the current workspace's rows.

use std::sync::Arc;

use http::Method;
use navree::crypto::generate_key;
use navree::db::navree_schema::{Migration, Schema, SchemaMigration, SchemaResult};
use navree::db::{Db, Executor, Statement, Value as DbValue};
use navree::http::{Controller, RouteDef};
use navree::prelude::*;
use navree::session::SessionOptions;
use navree::testing::{TestApp, TestClient, principal};
use navree::ui::{DataTable, TableQuery, TableRow, UiModule, View};
use navree::workspace::{NewWorkspace, Workspace, WorkspaceSwitcher, Workspaces};
use serde_json::{Value, json};
use uuid::Uuid;

/// `ws_deals(id, workspace_id, title)`: a table whose rows belong to one
/// workspace each.
struct CreateDeals;

impl Migration for CreateDeals {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("ws_deals", |t| {
                t.uuid("id").primary();
                t.uuid("workspace_id");
                t.string("title");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("ws_deals").await
    }
}

/// A deal: `#[model(workspace)]` scopes every query to the request's
/// workspace.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "ws_deals", workspace)]
pub struct Deal {
    /// The key.
    pub id: Uuid,
    /// The workspace it belongs to.
    pub workspace_id: Uuid,
    /// The title.
    pub title: String,
}

/// A deal as the table shows it.
#[derive(Clone, TableRow)]
#[table(sort = "title", per = 25)]
pub struct DealRow {
    /// The key.
    #[col(skip)]
    pub id: Uuid,
    /// The title.
    #[col(sortable, search)]
    pub title: String,
}

/// `GET /deals`: the switcher and the deals of the current workspace.
struct DealsPages;

impl Injectable for DealsPages {
    fn construct(
        _r: Resolver,
    ) -> navree::BoxFuture<'static, Result<Self, navree::kernel::KernelError>> {
        Box::pin(async { Ok(DealsPages) })
    }
}

impl Controller for DealsPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/deals",
                |ws: CurrentWorkspace,
                 switcher: WorkspaceSwitcher,
                 uow: Uow,
                 query: TableQuery<DealRow>| async move {
                    let page = query
                        .load_as(Deal::query(), &uow, |record| {
                            let deal = record.into_inner();
                            DealRow {
                                id: deal.id,
                                title: deal.title,
                            }
                        })
                        .await
                        .map_err(AppError::internal)?;
                    Ok::<_, AppError>(View::new(format!("Deals of {}", ws.name)).body((
                        switcher,
                        DataTable::new("deals", &query, &page).caption("Deals"),
                    )))
                },
            )
            .authenticated()
            .page(),
        ]
    }
}

struct DealsModule;

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("ws_deals")
                .after(["workspace"])
                .migration(SchemaMigration::new(
                    "m20261105_000001_create_ws_deals",
                    CreateDeals,
                )),
        )
        .controller::<DealsPages>();
    }
}

async fn app() -> TestApp {
    TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.users_table", "")
        .config("workspace.resolvers", json!(["session", "header"]))
        .module(CryptoModule)
        .module(SessionModule::for_root(SessionOptions::default()))
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(UiModule)
        .module(DealsModule)
        .build()
        .await
}

async fn create(app: &TestApp, owner: Uuid, name: &str) -> Workspace {
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let workspace = app
        .get::<Workspaces>()
        .await
        .create(&uow, owner, NewWorkspace::named(name))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    workspace
}

async fn deal(app: &TestApp, workspace: Uuid, title: &str) {
    app.get::<Db>()
        .await
        .execute(&Statement::portable(
            "INSERT INTO ws_deals (id, workspace_id, title) VALUES (?, ?, ?)",
            [
                DbValue::from(Uuid::now_v7()),
                DbValue::from(workspace),
                DbValue::from(title),
            ],
        ))
        .await
        .unwrap();
}

async fn switch(client: &TestClient, workspace: &str) {
    client
        .post("/workspace/switch")
        .xsrf()
        .header("accept", "text/html")
        .form(&[("workspace", workspace), ("next", "/deals")])
        .send()
        .await
        .assert_status(303);
}

async fn page(client: &TestClient) -> String {
    client
        .get("/deals")
        .header("accept", "text/html")
        .send()
        .await
        .assert_status(200)
        .text()
}

// Switching in the browser moves the page, its switcher and its
// table to the chosen workspace, and only to one the user belongs to.
#[tokio::test]
async fn a_navree_ui_page_follows_the_workspace_the_browser_switched_to() {
    let app = app().await;
    let ada = Uuid::now_v7();
    let grace = Uuid::now_v7();
    let agra = create(&app, ada, "Agra").await;
    let gulati = create(&app, ada, "Gulati").await;
    let ilaichi = create(&app, grace, "Ilaichi").await;
    deal(&app, agra.id, "Agra renewal").await;
    deal(&app, agra.id, "Agra upsell").await;
    deal(&app, gulati.id, "Gulati pilot").await;
    deal(&app, ilaichi.id, "Ilaichi secret").await;

    let client = app.acting_as(principal(ada));
    client.csrf_cookie().await;

    switch(&client, "agra").await;
    let html = page(&client).await;
    assert!(html.contains("Deals of Agra"), "{html}");
    assert!(
        html.contains("Agra renewal") && html.contains("Agra upsell"),
        "{html}"
    );
    assert!(!html.contains("Gulati pilot"), "{html}");
    assert!(
        !html.contains("Ilaichi"),
        "another user's workspace is never listed: {html}"
    );
    assert_eq!(html.matches(r#"name="workspace""#).count(), 2, "{html}");
    let current = html
        .find(r#"aria-current="true""#)
        .expect("a current workspace");
    assert!(html[current..].contains("Agra"), "{html}");

    switch(&client, "gulati").await;
    let html = page(&client).await;
    assert!(html.contains("Deals of Gulati"), "{html}");
    assert!(html.contains("Gulati pilot"), "{html}");
    assert!(!html.contains("Agra renewal"), "{html}");
    let current = html
        .find(r#"aria-current="true""#)
        .expect("a current workspace");
    assert!(html[current..].contains("Gulati"), "{html}");

    // Ilaichi isn't Anu's: refused, and the page stays in Gulati.
    client
        .post("/workspace/switch")
        .xsrf()
        .form(&[("workspace", ilaichi.id.to_string().as_str())])
        .send()
        .await
        .assert_error_code(403, "not_a_member");
    let html = page(&client).await;
    assert!(html.contains("Deals of Gulati"), "{html}");
    assert!(!html.contains("Ilaichi secret"), "{html}");
}
