//! Workspace time zones and countries: the columns, the service, the
//! routes and the workspace `ZoneSource` behind navree-time's `CurrentZone`.

use std::sync::Arc;

use http::Method;
use navree_contracts::ZoneLevel;
use navree_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_time::{CurrentZone, TimeModule, Zones};
use navree_workspace::WorkspaceError;
use serde_json::{Value, json};

use crate::support::{World, builder};

#[tokio::test]
async fn new_workspaces_have_no_zone_or_country() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    assert_eq!(acme.timezone, None);
    assert_eq!(acme.country, None);
}

#[tokio::test]
async fn the_service_stores_normalized_zones_and_countries() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let workspaces = world.workspaces().await;

    let updated = {
        let workspaces = workspaces.clone();
        world
            .work(|uow| async move {
                workspaces
                    .set_time_zone(&uow, acme.id, Some("asia/calcutta"), Some("in"))
                    .await
            })
            .await
            .unwrap()
    };
    assert_eq!(updated.timezone.as_deref(), Some("Asia/Kolkata"));
    assert_eq!(updated.country.as_deref(), Some("IN"));
    let stored = workspaces.find(acme.id).await.unwrap().unwrap();
    assert_eq!(stored.timezone.as_deref(), Some("Asia/Kolkata"));

    let cleared = {
        let workspaces = workspaces.clone();
        world
            .work(|uow| async move { workspaces.set_time_zone(&uow, acme.id, None, None).await })
            .await
            .unwrap()
    };
    assert_eq!((cleared.timezone, cleared.country), (None, None));
}

#[tokio::test]
async fn unknown_zones_and_countries_are_refused() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let workspaces = world.workspaces().await;
    let error = {
        let workspaces = workspaces.clone();
        world
            .work(|uow| async move {
                workspaces
                    .set_time_zone(&uow, acme.id, Some("Mars/Base"), None)
                    .await
            })
            .await
            .unwrap_err()
    };
    assert!(
        matches!(&error, WorkspaceError::Invalid { field, .. } if *field == "timezone"),
        "{error:?}"
    );
    let error = world
        .work(|uow| async move {
            workspaces
                .set_time_zone(&uow, acme.id, None, Some("Atlantis"))
                .await
        })
        .await
        .unwrap_err();
    assert!(
        matches!(&error, WorkspaceError::Invalid { field, .. } if *field == "country"),
        "{error:?}"
    );
}

#[tokio::test]
async fn the_routes_create_show_and_update_the_zone() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let client = world.as_user(ada);
    client
        .post("/api/workspaces")
        .json(&json!({ "name": "Acme", "timezone": "Asia/Calcutta", "country": "in" }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("timezone", json!("Asia/Kolkata"))
        .assert_json_path("country", json!("IN"));
    client
        .post("/api/workspaces")
        .json(&json!({ "name": "Nowhere", "timezone": "Mars/Base" }))
        .send()
        .await
        .assert_validation_error("timezone");
    client
        .get("/api/workspaces/acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("timezone", json!("Asia/Kolkata"));
    client
        .put("/api/workspaces/acme/time-zone")
        .json(&json!({ "timezone": "Europe/Paris", "country": "FR" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("timezone", json!("Europe/Paris"))
        .assert_json_path("country", json!("FR"));
    client
        .put("/api/workspaces/acme/time-zone")
        .json(&json!({ "timezone": null, "country": "AE" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("timezone", Value::Null)
        .assert_json_path("country", json!("AE"));
    client
        .put("/api/workspaces/acme/time-zone")
        .json(&json!({ "country": "Atlantis" }))
        .send()
        .await
        .assert_validation_error("country");
}

#[tokio::test]
async fn members_without_update_permission_cannot_change_the_zone() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let bob = world.user("Bob", "bob@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    world.join(acme.id, bob, "member").await;
    world
        .as_user(bob)
        .put("/api/workspaces/acme/time-zone")
        .json(&json!({ "timezone": "Europe/Paris" }))
        .send()
        .await
        .assert_status(403);
}

/// `GET /api/zone-probe`: the request's zone.
struct ZoneProbe;

impl Injectable for ZoneProbe {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ZoneProbe) })
    }
}

impl Controller for ZoneProbe {
    fn prefix(&self) -> &'static str {
        "/zone-probe"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/", |zone: CurrentZone| async move {
                Json(json!({"zone": zone.name(), "level": zone.level(), "owner": zone.owner()}))
            })
            .public(),
        ]
    }
}

struct ZoneProbeModule;

impl Module for ZoneProbeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<ZoneProbe>();
    }
}

async fn zoned_world() -> World {
    World::with(
        builder()
            .config("time.default_zone", "Europe/London")
            .module(TimeModule::from_config())
            .module(ZoneProbeModule),
    )
    .await
}

#[tokio::test]
async fn the_request_workspace_gives_the_zone() {
    let world = zoned_world().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let eve = world.user("Eve", "eve@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let gulf = world.workspace(ada, "Gulf").await;
    let workspaces = world.workspaces().await;
    {
        let workspaces = workspaces.clone();
        world
            .work(|uow| async move {
                workspaces
                    .set_time_zone(&uow, acme.id, Some("Asia/Kolkata"), None)
                    .await?;
                workspaces
                    .set_time_zone(&uow, gulf.id, None, Some("AE"))
                    .await
            })
            .await
            .unwrap();
    }

    let zone = |client: navree_testing::TestClient, workspace: Option<&'static str>| async move {
        let mut request = client.get("/api/zone-probe");
        if let Some(workspace) = workspace {
            request = request.header("X-Workspace", workspace);
        }
        request.send().await.assert_status(200).json_value()
    };
    let body = zone(world.as_user(ada), Some("acme")).await;
    assert_eq!(body["zone"], "Asia/Kolkata");
    assert_eq!(body["level"], "workspace");
    assert_eq!(body["owner"], acme.id.to_string());
    assert_eq!(
        zone(world.as_user(ada), Some("gulf")).await["zone"],
        "Asia/Dubai"
    );
    // Not a member, no workspace named, a guest: the default zone.
    assert_eq!(
        zone(world.as_user(eve), Some("acme")).await["zone"],
        "Europe/London"
    );
    assert_eq!(
        zone(world.as_user(ada), None).await["zone"],
        "Europe/London"
    );
    assert_eq!(
        zone(world.app.client(), Some("acme")).await["zone"],
        "Europe/London"
    );
}

#[tokio::test]
async fn jobs_find_workspace_zones_and_every_owner() {
    let world = zoned_world().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let plain = world.workspace(ada, "Plain").await;
    let workspaces = world.workspaces().await;
    world
        .work(|uow| async move {
            workspaces
                .set_time_zone(&uow, acme.id, Some("America/New_York"), None)
                .await
        })
        .await
        .unwrap();
    let zones = world.app.get::<Zones>().await;
    assert_eq!(zones.levels(), [ZoneLevel::Workspace]);
    assert_eq!(
        zones
            .zone_of(ZoneLevel::Workspace, acme.id)
            .await
            .unwrap()
            .name(),
        "America/New_York"
    );
    assert_eq!(
        zones
            .zone_of(ZoneLevel::Workspace, plain.id)
            .await
            .unwrap()
            .name(),
        "Europe/London"
    );
    let mut owners = zones.owners(ZoneLevel::Workspace).await.unwrap();
    owners.sort_by_key(|(id, _)| *id);
    let names: Vec<_> = owners.iter().map(|(id, zone)| (*id, zone.name())).collect();
    let mut expected = vec![(acme.id, "America/New_York"), (plain.id, "Europe/London")];
    expected.sort_by_key(|(id, _)| *id);
    assert_eq!(names, expected);
}
