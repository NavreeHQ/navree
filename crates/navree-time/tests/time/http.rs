//! `CurrentZone`, `WithZone` and `GET /api/time/zones` over HTTP.

use std::sync::Arc;

use axum::extract::Query;
use http::{Method, StatusCode};
use navree_contracts::{ZoneLevel, ZoneOwner};
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{App, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_time::{CurrentZone, DayRange, TimeModule, WithZone, ZonesFake};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{app, get};

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
                Ok::<_, AppError>(Json(json!({"today": zone.today(), "now": zone.now().to_rfc3339()})))
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
    app: App,
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
    let users =
        ZonesFake::new(ZoneLevel::User).owner(ZoneOwner::new(user).zone("America/New_York"));
    let workspaces = ZonesFake::new(ZoneLevel::Workspace)
        .header("X-Workspace")
        .owner(ZoneOwner::new(workspace).zone("Asia/Calcutta"))
        .owner(ZoneOwner::new(gulf_workspace).country("ae"))
        .owner(ZoneOwner::new(broken_workspace).zone("Not/AZone"));
    let app = app(&[("time.default_zone", json!("Europe/London"))])
        .module(TimeModule::from_config())
        .module(users.module())
        .module(workspaces.module())
        .module(ProbeModule)
        .boot()
        .await
        .unwrap();
    Fixture {
        app,
        user,
        other_user,
        workspace,
        gulf_workspace,
        broken_workspace,
    }
}

fn in_workspace(id: Uuid) -> Vec<(&'static str, String)> {
    vec![("X-Workspace", id.to_string())]
}

#[tokio::test]
async fn guests_get_the_default_zone() {
    let f = fixture().await;
    let reply = get(&f.app, "/api/probe/zone", None, &[]).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.body,
        json!({"zone": "Europe/London", "level": null, "owner": null, "from_country": false})
    );
}

#[tokio::test]
async fn a_user_zone_wins_over_the_workspace_zone() {
    let f = fixture().await;
    let body = get(
        &f.app,
        "/api/probe/zone",
        Some(f.user),
        &in_workspace(f.workspace),
    )
    .await
    .body;
    assert_eq!(body["zone"], "America/New_York");
    assert_eq!(body["level"], "user");
    assert_eq!(body["owner"], f.user.to_string());
}

#[tokio::test]
async fn the_workspace_zone_applies_when_the_user_has_none() {
    let f = fixture().await;
    let body = get(
        &f.app,
        "/api/probe/zone",
        Some(f.other_user),
        &in_workspace(f.workspace),
    )
    .await
    .body;
    assert_eq!(body["zone"], "Asia/Kolkata", "the alias is normalized");
    assert_eq!(body["level"], "workspace");
    assert_eq!(body["owner"], f.workspace.to_string());
}

#[tokio::test]
async fn a_country_without_a_zone_gives_its_primary_zone() {
    let f = fixture().await;
    let body = get(
        &f.app,
        "/api/probe/zone",
        None,
        &in_workspace(f.gulf_workspace),
    )
    .await
    .body;
    assert_eq!(body["zone"], "Asia/Dubai");
    assert_eq!(body["from_country"], true);
}

#[tokio::test]
async fn a_bad_stored_zone_falls_through_to_the_next_level() {
    let f = fixture().await;
    let body = get(
        &f.app,
        "/api/probe/zone",
        None,
        &in_workspace(f.broken_workspace),
    )
    .await
    .body;
    assert_eq!(body["zone"], "Europe/London");
    assert_eq!(body["level"], Value::Null);
}

#[tokio::test]
async fn reports_count_days_in_the_tenant_zone_and_echo_it() {
    let f = fixture().await;
    let reply = get(
        &f.app,
        "/api/probe/report?from=2026-10-01&to=2026-10-31",
        None,
        &in_workspace(f.workspace),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.body,
        json!({
            "from": "2026-09-30T18:30:00Z",
            "to": "2026-10-31T18:30:00Z",
            "days": 31,
            "timeZone": "Asia/Kolkata",
        })
    );
}

#[tokio::test]
async fn a_bad_range_is_a_bad_request() {
    let f = fixture().await;
    let reply = get(
        &f.app,
        "/api/probe/report?from=2026-10-31&to=not-a-day",
        None,
        &[],
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn non_object_data_is_wrapped() {
    let f = fixture().await;
    let body = get(&f.app, "/api/probe/list", None, &[]).await.body;
    assert_eq!(body, json!({"data": [1, 2, 3], "timeZone": "Europe/London"}));
}

#[tokio::test]
async fn today_and_now_are_on_the_local_clock() {
    let f = fixture().await;
    let body = get(&f.app, "/api/probe/today", None, &[]).await.body;
    assert_eq!(body["today"].as_str().unwrap().len(), 10, "{body}");
    let now = body["now"].as_str().unwrap();
    assert!(now.ends_with("+00:00") || now.ends_with("+01:00"), "{now}");
}

#[tokio::test]
async fn the_zone_list_is_public_reference_data() {
    let f = fixture().await;
    let reply = get(&f.app, "/api/time/zones", None, &[]).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.cache_control.as_deref(), Some("public, max-age=3600"));
    let data = reply.body["data"].as_array().unwrap();
    assert!(data.len() > 300);
    let kolkata = data
        .iter()
        .find(|z| z["name"] == "Asia/Kolkata")
        .expect("Asia/Kolkata is listed");
    assert_eq!(kolkata["offset"], "+05:30");
    assert_eq!(kolkata["offset_minutes"], 330);
    assert_eq!(kolkata["countries"], json!(["IN"]));
    assert!(data.iter().all(|z| z["name"] != "Asia/Calcutta"));

    let india = get(&f.app, "/api/time/zones?country=in", None, &[]).await;
    assert_eq!(india.body["data"].as_array().unwrap().len(), 1);
    let search = get(&f.app, "/api/time/zones?search=york", None, &[]).await;
    assert_eq!(search.body["data"][0]["name"], "America/New_York");

    let unknown = get(&f.app, "/api/time/zones?country=XX", None, &[]).await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
    assert!(
        unknown.body["errors"]["country"][0]
            .as_str()
            .unwrap()
            .contains("ISO 3166-1"),
        "{}",
        unknown.body
    );
}

#[tokio::test]
async fn the_zone_list_can_be_turned_off() {
    let app = app(&[("time.routes", json!(false))])
        .module(TimeModule::from_config())
        .boot()
        .await
        .unwrap();
    let reply = get(&app, "/api/time/zones", None, &[]).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn current_zone_needs_the_module() {
    let app = app(&[]).module(ProbeModule).boot().await.unwrap();
    let reply = get(&app, "/api/probe/zone", None, &[]).await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
}
