//! The Settings screens (feature `settings`): the types the user manages,
//! their values at a level with the source of each, saving changed fields
//! and making one inherit again.

use navree_helm::ModuleBuilderHelmExt;
use navree_kernel::{Module, ModuleBuilder};
use navree_settings::{ModuleBuilderSettingsExt, Scope, Settings, SettingsModule, Source};
use navree_testing::{TestApp, TestClient, TestResponse};
use serde_json::json;
use uuid::Uuid;

use crate::support::*;

/// What the member portal shows.
#[derive(
    navree_settings::Settings,
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    navree_validation::Validate,
)]
#[settings(key = "portal", scopes(global, workspace))]
pub struct PortalSettings {
    #[validate(skip)]
    pub show_leaderboard: bool,
    #[validate(length(max = 20))]
    pub welcome_text: String,
    #[validate(range(min = 1, max = 100))]
    pub page_size: u32,
    #[validate(skip)]
    pub tags: Vec<String>,
}

impl Default for PortalSettings {
    fn default() -> Self {
        PortalSettings {
            show_leaderboard: false,
            welcome_text: "Welcome".to_owned(),
            page_size: 25,
            tags: Vec::new(),
        }
    }
}

/// Billing mail, managed with its own permission.
#[derive(
    navree_settings::Settings,
    Clone,
    Debug,
    Default,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    navree_validation::Validate,
)]
#[settings(key = "billing-mail", scopes(global), permission = "billing.manage")]
pub struct BillingMail {
    #[validate(skip)]
    pub sender: String,
}

struct SettingsFixtures;

impl Module for SettingsFixtures {
    fn register(&self, m: &mut ModuleBuilder) {
        m.settings::<PortalSettings>()
            .settings::<BillingMail>()
            // These tests are about the screens, whoever may reach a level:
            // `workspaces::access::settings_screens` is about who may.
            .helm_staff_workspaces(|_| async { Ok(true) });
    }
}

async fn settings_app() -> TestApp {
    builder()
        .module(SettingsFixtures)
        .module(SettingsModule::from_config())
        .build()
        .await
}

/// A user with `helm.view` plus `permissions`.
async fn admin(app: &TestApp, role: &str, permissions: &[&str]) -> (Uuid, TestClient) {
    let id = Uuid::now_v7();
    let mut all = vec!["helm.view"];
    all.extend_from_slice(permissions);
    give_role(app, id, role, &all).await;
    (id, app.acting_as(navree_testing::principal(id)))
}

async fn page(client: &TestClient, path: &str) -> TestResponse {
    client.get(path).header("accept", "text/html").send().await
}

async fn submit(client: &TestClient, path: &str, form: &[(&str, &str)]) -> TestResponse {
    client
        .post(path)
        .header("accept", "text/html")
        .form(form)
        .send()
        .await
}

/// The page's text with the escaped slashes restored.
fn text(response: &TestResponse) -> String {
    response.text().replace("&#x2f;", "/")
}

#[tokio::test]
async fn the_menu_and_index_show_the_types_the_user_manages() {
    let app = settings_app().await;
    let (_, rep) = user(&app, "rep").await;
    let (_, settings_admin) = admin(&app, "settings-admin", &["settings.manage"]).await;
    let (_, billing) = admin(&app, "billing-admin", &["billing.manage"]).await;

    // Without a permission: no menu entry, and the screens are refused.
    let dashboard = text(&page(&rep, "/admin").await.assert_status(200));
    assert!(!dashboard.contains("/admin/settings"), "{dashboard}");
    page(&rep, "/admin/settings").await.assert_status(403);
    page(&rep, "/admin/settings/portal")
        .await
        .assert_status(403);

    let dashboard = text(&page(&settings_admin, "/admin").await.assert_status(200));
    assert!(
        dashboard.contains("href=\"/admin/settings\""),
        "{dashboard}"
    );
    let index = text(
        &page(&settings_admin, "/admin/settings")
            .await
            .assert_status(200),
    );
    assert!(index.contains("What the member portal shows."), "{index}");
    assert!(index.contains("/admin/settings/portal"), "{index}");
    // Billing mail needs its own permission.
    assert!(!index.contains("/admin/settings/billing-mail"), "{index}");
    page(&settings_admin, "/admin/settings/billing-mail")
        .await
        .assert_status(403);

    let index = text(&page(&billing, "/admin/settings").await.assert_status(200));
    assert!(index.contains("/admin/settings/billing-mail"), "{index}");
    assert!(!index.contains("/admin/settings/portal\""), "{index}");

    page(&settings_admin, "/admin/settings/nope")
        .await
        .assert_status(404);
}

#[tokio::test]
async fn the_form_shows_values_and_where_they_come_from() {
    let app = settings_app().await;
    let (_, admin) = admin(&app, "settings-admin", &["settings.manage"]).await;
    let settings = app.get::<Settings>().await;
    let db = app.get::<navree_db::Db>().await;
    let uow = db.uow(None, None).await;
    settings
        .update::<PortalSettings>(&uow, Scope::global(), json!({"welcome_text": "Hello"}))
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let form = text(
        &page(&admin, "/admin/settings/portal")
            .await
            .assert_status(200),
    );
    assert!(form.contains("value=\"Hello\""), "{form}");
    assert!(form.contains("Set at this level"), "{form}");
    assert!(form.contains("Default value"), "{form}");
    assert!(form.contains("Inherit welcome text"), "{form}");
    assert!(form.contains("type=\"number\""), "{form}");
    assert!(form.contains("role=\"switch\""), "{form}");

    // A workspace, by id: its values inherit the global ones.
    let acme = Uuid::now_v7();
    let form = text(
        &page(
            &admin,
            &format!("/admin/settings/portal?level=workspace&target={acme}"),
        )
        .await
        .assert_status(200),
    );
    assert!(form.contains("Inherited from the global level"), "{form}");
    assert!(!form.contains("Inherit welcome text"), "{form}");
    // A level without its target asks for it.
    let form = text(
        &page(&admin, "/admin/settings/portal?level=workspace")
            .await
            .assert_status(200),
    );
    assert!(form.contains("Name the workspace"), "{form}");
    // Levels the type does not allow, and bad targets, are refused.
    page(&admin, "/admin/settings/portal?level=user&target=x")
        .await
        .assert_status(400);
    page(&admin, "/admin/settings/portal?level=workspace&target=acme")
        .await
        .assert_status(400);
}

#[tokio::test]
async fn saving_stores_only_the_changed_fields() {
    let app = settings_app().await;
    let (admin_id, admin) = admin(&app, "settings-admin", &["settings.manage"]).await;
    let settings = app.get::<Settings>().await;
    let acme = Uuid::now_v7();
    let url = format!("/admin/settings/portal?level=workspace&target={acme}");

    let response = submit(
        &admin,
        &url,
        &[
            ("_csrf", ""),
            ("show_leaderboard", "false"),
            ("show_leaderboard", "true"),
            ("welcome_text", "Welcome"),
            ("page_size", "40"),
            ("tags", "[\"a\", \"b\"]"),
        ],
    )
    .await
    .assert_status(303);
    assert_eq!(response.header("location"), Some(url.as_str()));

    let scope = Scope::workspace(acme).in_organization(None);
    let portal = settings.resolve::<PortalSettings>(scope).await.unwrap();
    assert!(portal.show_leaderboard);
    assert_eq!(portal.page_size, 40);
    assert_eq!(portal.tags, ["a", "b"]);
    // The unchanged text still follows the level above.
    assert_eq!(portal.source("welcome_text"), Some(Source::Default));
    assert_eq!(portal.overridden, ["page_size", "show_leaderboard", "tags"]);
    let rows = settings.rows(Some("portal")).await.unwrap();
    assert_eq!(rows[0].updated_by, Some(admin_id));

    // Nothing changed: nothing written.
    submit(&admin, &url, &[("page_size", "40")])
        .await
        .assert_status(303);

    // Invalid values come back with the field named.
    let response = submit(&admin, &url, &[("page_size", "0"), ("welcome_text", "Hi")])
        .await
        .assert_status(400);
    let body = text(&response);
    assert!(body.contains("page_size must not be less than 1"), "{body}");
    assert!(body.contains("value=\"Hi\""), "{body}");
    let response = submit(&admin, &url, &[("page_size", "many"), ("tags", "[")])
        .await
        .assert_status(400);
    let body = text(&response);
    assert!(body.contains("Page size must be a whole number"), "{body}");
    assert!(body.contains("Tags must be valid JSON"), "{body}");
    assert_eq!(
        settings
            .get::<PortalSettings>(scope)
            .await
            .unwrap()
            .page_size,
        40
    );

    // Inherit makes one field follow the level above again.
    submit(
        &admin,
        &format!("/admin/settings/portal/inherit?level=workspace&target={acme}"),
        &[("_inherit", "page_size")],
    )
    .await
    .assert_status(303);
    let portal = settings.resolve::<PortalSettings>(scope).await.unwrap();
    assert_eq!(portal.page_size, 25);
    assert_eq!(portal.source("page_size"), Some(Source::Default));
    submit(
        &admin,
        &format!("/admin/settings/portal/inherit?level=workspace&target={acme}"),
        &[("_inherit", "colour")],
    )
    .await
    .assert_status(400);

    // Users without the permission cannot write.
    let (_, rep) = user(&app, "rep").await;
    submit(&rep, &url, &[("page_size", "10")])
        .await
        .assert_status(403);
}

#[tokio::test]
async fn without_the_settings_module_there_are_no_screens() {
    let app = app().await;
    let (_, admin) = admin(&app, "settings-admin", &["settings.manage"]).await;
    let dashboard = text(&page(&admin, "/admin").await.assert_status(200));
    assert!(!dashboard.contains("/admin/settings"), "{dashboard}");
    page(&admin, "/admin/settings").await.assert_status(404);
}

/// Settings at a workspace's level, with no `WorkspaceModule` and nobody named as staff.
struct UncheckedFixtures;

impl Module for UncheckedFixtures {
    fn register(&self, m: &mut ModuleBuilder) {
        m.settings::<PortalSettings>();
    }
}

#[tokio::test]
async fn with_no_way_to_check_the_level_of_a_workspace_is_refused() {
    let app = builder()
        .module(UncheckedFixtures)
        .module(SettingsModule::from_config())
        .build()
        .await;
    let (_, admin) = admin(&app, "settings-admin", &["settings.manage"]).await;
    let acme = Uuid::now_v7();
    let url = format!("/admin/settings/portal?level=workspace&target={acme}");
    let response = page(&admin, &url).await.assert_status(403);
    assert!(response.text().contains("WorkspaceModule"), "{}", response.text());
    submit(&admin, &url, &[("page_size", "40")])
        .await
        .assert_status(403);
    // The global level is the permission's.
    page(&admin, "/admin/settings/portal").await.assert_status(200);
}
