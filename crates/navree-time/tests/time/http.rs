//! `CurrentZone`, `WithZone` and `GET /api/time/zones` over HTTP.

use std::sync::Arc;

use axum::extract::Query;
use http::Method;
use navree_contracts::{ZoneLevel, ZoneOwner};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::{TestApp, principal};
use navree_time::{CurrentZone, DayRange, TimeModule, WithZone, ZonesFake};
use serde_json::{Value, json};
use uuid::Uuid;

/// Routes that read the request's zone.
struct ProbeController;

impl Injectable for ProbeController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ProbeController) })
    }
}

impl Controller for ProbeController {
    fn prefix(&self) -> &'static str {
        "/probe"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/zone", |zone: CurrentZone| async move {
                Json(json!({
                    "zone": zone.name(),
                    "level": zone.level(),
                    "owner": zone.owner(),
                    "from_country": zone.from_country(),
                }))
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "/report",
                |zone: CurrentZone, Query(range): Query<DayRange>| async move {
                    let (from, to) = range.utc_bounds(&zone);
                    WithZone::new(&zone, json!({"from": from, "to": to, "days": range.day_count()}))
                },
            )
            .public(),
            RouteDef::on(Method::GET, "/list", |zone: CurrentZone| async move {
                WithZone::new(&zone, vec![1, 2, 3])
            })
            .public(),
            RouteDef::on(Method::GET, "/today", |zone: CurrentZone| async move {
                Ok::<_, AppError>(Json(json!({"today": zone.today()})))
            })
            .public(),
        ]
    }
}

struct ProbeModule;

impl Module for ProbeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<ProbeController>();
    }
}

struct Fixture {
    app: TestApp,
    user: Uuid,
    other_user: Uuid,
    workspace: Uuid,
    gulf_workspace: Uuid,
    broken_workspace: Uuid,
}

async fn fixture() -> Fixture {
    let (user, other_user) = (Uuid::now_v7(), Uuid::now_v7());
    let (workspace, gulf_workspace, broken_workspace) =
        (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let users = ZonesFake::new(ZoneLevel::User).owner(ZoneOwner::new(user).zone("America/New_York"));
    let workspaces = ZonesFake::new(ZoneLevel::Workspace)
        .header("X-Workspace")
        .owner(ZoneOwner::new(workspace).zone("Asia/Calcutta"))
        .owner(ZoneOwner::new(gulf_workspace).country("ae"))
        .owner(ZoneOwner::new(broken_workspace).zone("Not/AZone"));
    let app = TestApp::builder()
        .config("time.default_zone", "Europe/London")
        .module(TimeModule::from_config())
        .module(users.module())
        .module(workspaces.module())
        .module(ProbeModule)
        .build()
        .await;
    Fixture {
        app,
        user,
        other_user,
        workspace,
        gulf_workspace,
        broken_workspace,
    }
}

async fn zone_of(request: navree_testing::TestRequest) -> Value {
    request.send().await.assert_status(200).json_value()
}

#[tokio::test]
async fn guests_get_the_default_zone() {
    let f = fixture().await;
    let body = zone_of(f.app.client().get("/api/probe/zone")).await;
    assert_eq!(
        body,
        json!({"zone": "Europe/London", "level": null, "owner": null, "from_country": false})
    );
}

#[tokio::test]
async fn a_user_zone_wins_over_the_workspace_zone() {
    let f = fixture().await;
    let body = zone_of(
        f.app
            .acting_as(principal(f.user))
            .get("/api/probe/zone")
            .header("X-Workspace", &f.workspace.to_string()),
    )
    .await;
    assert_eq!(body["zone"], "America/New_York");
    assert_eq!(body["level"], "user");
    assert_eq!(body["owner"], f.user.to_string());
}

#[tokio::test]
async fn the_workspace_zone_applies_when_the_user_has_none() {
    let f = fixture().await;
    let body = zone_of(
        f.app
            .acting_as(principal(f.other_user))
            .get("/api/probe/zone")
            .header("X-Workspace", &f.workspace.to_string()),
    )
    .await;
    assert_eq!(body["zone"], "Asia/Kolkata", "the alias is normalized");
    assert_eq!(body["level"], "workspace");
    assert_eq!(body["owner"], f.workspace.to_string());
}

#[tokio::test]
async fn a_country_without_a_zone_gives_its_primary_zone() {
    let f = fixture().await;
    let body = zone_of(
        f.app
            .client()
            .get("/api/probe/zone")
            .header("X-Workspace", &f.gulf_workspace.to_string()),
    )
    .await;
    assert_eq!(body["zone"], "Asia/Dubai");
    assert_eq!(body["from_country"], true);
}

#[tokio::test]
async fn a_bad_stored_zone_falls_through_to_the_next_level() {
    let f = fixture().await;
    let body = zone_of(
        f.app
            .client()
            .get("/api/probe/zone")
            .header("X-Workspace", &f.broken_workspace.to_string()),
    )
    .await;
    assert_eq!(body["zone"], "Europe/London");
    assert_eq!(body["level"], Value::Null);
}

#[tokio::test]
async fn reports_count_days_in_the_tenant_zone_and_echo_it() {
    let f = fixture().await;
    let body = f
        .app
        .client()
        .get("/api/probe/report")
        .query(&[("from", "2026-10-01"), ("to", "2026-10-31")])
        .header("X-Workspace", &f.workspace.to_string())
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body,
        json!({
            "from": "2026-09-30T18:30:00Z",
            "to": "2026-10-31T18:30:00Z",
            "days": 31,
            "timeZone": "Asia/Kolkata",
        })
    );
}

#[tokio::test]
async fn non_object_data_is_wrapped() {
    let f = fixture().await;
    let body = zone_of(f.app.client().get("/api/probe/list")).await;
    assert_eq!(body, json!({"data": [1, 2, 3], "timeZone": "Europe/London"}));
}

#[tokio::test]
async fn today_is_the_local_day() {
    let f = fixture().await;
    let body = zone_of(f.app.client().get("/api/probe/today")).await;
    assert!(body["today"].as_str().unwrap().len() == 10, "{body}");
}

#[tokio::test]
async fn the_zone_list_is_public_reference_data() {
    let f = fixture().await;
    let body = f
        .app
        .client()
        .get("/api/time/zones")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let data = body["data"].as_array().unwrap();
    assert!(data.len() > 300);
    let kolkata = data
        .iter()
        .find(|z| z["name"] == "Asia/Kolkata")
        .expect("Asia/Kolkata is listed");
    assert_eq!(kolkata["offset"], "+05:30");
    assert_eq!(kolkata["offset_minutes"], 330);
    assert_eq!(kolkata["countries"], json!(["IN"]));
    assert!(data.iter().all(|z| z["name"] != "Asia/Calcutta"));

    let india = f
        .app
        .client()
        .get("/api/time/zones")
        .query(&[("country", "in")])
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(india["data"].as_array().unwrap().len(), 1);

    f.app
        .client()
        .get("/api/time/zones")
        .query(&[("country", "XX")])
        .send()
        .await
        .assert_validation_error("country");
}

#[tokio::test]
async fn the_zone_list_can_be_turned_off() {
    let app = TestApp::builder()
        .config("time.routes", false)
        .module(TimeModule::from_config())
        .build()
        .await;
    app.client()
        .get("/api/time/zones")
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn current_zone_needs_the_module() {
    let app = TestApp::builder().module(ProbeModule).build().await;
    app.client()
        .get("/api/probe/zone")
        .send()
        .await
        .assert_status(500);
}

#[tokio::test]
async fn a_bad_range_is_a_bad_request() {
    let f = fixture().await;
    f.app
        .client()
        .get("/api/probe/report")
        .query(&[("from", "2026-10-31"), ("to", "not-a-day")])
        .send()
        .await
        .assert_status(400);
}
