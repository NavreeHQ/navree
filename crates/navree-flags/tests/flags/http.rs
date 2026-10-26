//! Flags in requests: `CurrentFlags`, `#[requires_flag]`, `GET /api/flags`
//! and the OpenAPI document.

use navree_api::{ApiDocs, ApiModule};
use navree_flags::FlagScope;
use serde_json::json;
use uuid::Uuid;

use crate::support::{AiSummaries, DealExport, NewPipeline, World, builder};

#[tokio::test]
async fn current_flags_resolve_for_the_user_and_the_workspace() {
    let world = World::new().await;
    let flags = world.flags().await;
    let ada = world.user("Ada").await;
    let grace = world.user("Grace").await;
    let acme = world.workspace(ada, "Acme").await;

    let body = world
        .as_user(ada)
        .get("/api/v1/deals/board")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body, json!({"board": "v1", "user": ada, "workspace": null}));

    flags.activate::<NewPipeline>(&acme).await.unwrap();
    // Without naming the workspace, the workspace's override does not apply.
    world
        .as_user(ada)
        .get("/api/v1/deals/board")
        .send()
        .await
        .assert_json_path("board", json!("v1"));
    // Named with X-Workspace, it does.
    world
        .as_user(ada)
        .get("/api/v1/deals/board")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("board", json!("v2"))
        .assert_json_path("workspace", json!(acme.id));
    // A user who is not a member does not get the workspace's flags.
    world
        .as_user(grace)
        .get("/api/v1/deals/board")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("board", json!("v1"))
        .assert_json_path("workspace", json!(null));
}

#[tokio::test]
async fn requires_flag_hides_the_route_while_the_flag_is_off() {
    let world = World::new().await;
    let flags = world.flags().await;
    let rep = world.user("Rep").await;

    world
        .as_user(rep)
        .get("/api/v1/deals/forecast")
        .send()
        .await
        .assert_error(404, None);

    flags
        .activate::<NewPipeline>(FlagScope::user(rep))
        .await
        .unwrap();
    world
        .as_user(rep)
        .get("/api/v1/deals/forecast")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("currency", json!("USD"));

    // Access is still declared and checked: guests get 401.
    world
        .app
        .client()
        .get("/api/v1/deals/forecast")
        .send()
        .await
        .assert_error(401, None);

    // The kill switch hides it again.
    flags.kill::<NewPipeline>().await.unwrap();
    world
        .as_user(rep)
        .get("/api/v1/deals/forecast")
        .send()
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn the_flag_is_checked_before_extractors_and_authorization() {
    let world = World::new().await;
    let rep = world.user("Rep").await;

    // The body would be a 400, and the path id a 400 too: the 404 comes first.
    world
        .as_user(rep)
        .post("/api/v1/deals/forecast/not-a-uuid")
        .body("not json")
        .send()
        .await
        .assert_error(404, None);
    // The route needs `deals.manage`, which rep lacks: still a 404.
    world
        .as_user(rep)
        .get("/api/v1/deals/managed-forecast")
        .send()
        .await
        .assert_error(404, None);

    world
        .flags()
        .await
        .activate::<NewPipeline>(FlagScope::everyone())
        .await
        .unwrap();
    world
        .as_user(rep)
        .post("/api/v1/deals/forecast/not-a-uuid")
        .json(&json!({}))
        .send()
        .await
        .assert_error(400, None);
    world
        .as_user(rep)
        .get("/api/v1/deals/managed-forecast")
        .send()
        .await
        .assert_error(403, None);
    let id = Uuid::now_v7();
    world
        .as_user(rep)
        .post(&format!("/api/v1/deals/forecast/{id}"))
        .json(&json!({"week": 3}))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("body.week", json!(3));
}

#[tokio::test]
async fn requires_flag_can_answer_403() {
    let world = World::new().await;
    let rep = world.user("Rep").await;
    world
        .as_user(rep)
        .get("/api/v1/deals/summary")
        .send()
        .await
        .assert_error(403, None);
    world
        .flags()
        .await
        .activate::<AiSummaries>(FlagScope::user(rep))
        .await
        .unwrap();
    world
        .as_user(rep)
        .get("/api/v1/deals/summary")
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn default_on_flags_keep_their_routes_until_turned_off() {
    let world = World::new().await;
    let rep = world.user("Rep").await;
    world
        .as_user(rep)
        .get("/api/v1/deals/export")
        .send()
        .await
        .assert_status(200);
    world
        .flags()
        .await
        .deactivate::<DealExport>(FlagScope::everyone())
        .await
        .unwrap();
    world
        .as_user(rep)
        .get("/api/v1/deals/export")
        .send()
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn get_flags_serves_exposed_flags_to_the_front_end() {
    let world = World::new().await;
    let ada = world.user("Ada").await;
    let acme = world.workspace(ada, "Acme").await;
    let flags = world.flags().await;
    flags.activate::<AiSummaries>(&acme).await.unwrap();
    flags
        .activate::<NewPipeline>(FlagScope::everyone())
        .await
        .unwrap();

    let response = world
        .as_user(ada)
        .get("/api/flags")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        response.json_value(),
        json!({"ai-summaries": true}),
        "only flags marked expose leave the server"
    );
    assert_eq!(
        response.header("cache-control"),
        Some("private, max-age=60")
    );

    world
        .as_user(ada)
        .get("/api/flags")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("ai-summaries", json!(false));
    world
        .app
        .client()
        .get("/api/flags")
        .send()
        .await
        .assert_error(401, None);
}

#[tokio::test]
async fn the_routes_can_be_turned_off() {
    let world = World::with(
        builder()
            .config("flags.routes", false)
            .config("flags.admin_routes", false),
    )
    .await;
    let ada = world.user("Ada").await;
    world
        .as_user(ada)
        .get("/api/flags")
        .send()
        .await
        .assert_error(404, None);
    world
        .as_user(ada)
        .get("/api/admin/flags")
        .send()
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn openapi_names_the_flag_of_a_route() {
    let world = World::with(builder().module(ApiModule::from_config())).await;
    let docs = world.app.get::<ApiDocs>().await;
    let document = docs.document().await.expect("the document builds");
    let paths = &document["paths"];
    assert_eq!(
        paths["/api/v1/deals/forecast"]["get"]["x-navree-flag"],
        "new-pipeline"
    );
    assert_eq!(
        paths["/api/v1/deals/summary"]["get"]["x-navree-flag"],
        "ai-summaries"
    );
    assert!(
        paths["/api/v1/deals/board"]["get"]
            .get("x-navree-flag")
            .is_none()
    );
    assert_eq!(paths["/api/flags"]["get"]["operationId"], "flags.index");
    assert_eq!(
        paths["/api/admin/flags/{name}"]["put"]["operationId"],
        "flags.admin.update"
    );
}
