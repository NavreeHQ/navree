//! `/api/admin/flags`: the routes admin screens use (permission
//! `flags.manage`).

use navree_flags::{FLAGS_MANAGE_PERMISSION, FlagScope};
use serde_json::json;
use uuid::Uuid;

use crate::support::{NewPipeline, World};

async fn admin(world: &World) -> Uuid {
    let admin = world.user("Admin").await;
    world.grant(admin, FLAGS_MANAGE_PERMISSION).await;
    admin
}

#[tokio::test]
async fn the_admin_routes_need_flags_manage() {
    let world = World::new().await;
    let rep = world.user("Rep").await;
    world
        .as_user(rep)
        .get("/api/admin/flags")
        .send()
        .await
        .assert_error(403, None);
    world
        .app
        .client()
        .get("/api/admin/flags")
        .send()
        .await
        .assert_error(401, None);
}

#[tokio::test]
async fn admins_list_flags_with_their_state() {
    let world = World::new().await;
    let admin = admin(&world).await;
    let flags = world.flags().await;
    flags
        .activate::<NewPipeline>(FlagScope::user(Uuid::now_v7()))
        .await
        .unwrap();

    let body = world
        .as_user(admin)
        .get("/api/admin/flags")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let data = body["data"].as_array().unwrap();
    let names: Vec<&str> = data.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["ai-summaries", "deal-export", "new-pipeline"]);
    assert_eq!(
        data[2],
        json!({
            "name": "new-pipeline",
            "description": "The redesigned deal pipeline board with drag-and-drop between stages.",
            "default": false,
            "expose": false,
            "everyone": null,
            "rollout": null,
            "killed": false,
            "overrides": 1,
            "updated_at": null,
            "updated_by": null,
        })
    );
    assert_eq!(data[0]["expose"], true);
    assert_eq!(data[1]["default"], true);
}

#[tokio::test]
async fn admins_set_the_global_state_rollout_and_kill_switch() {
    let world = World::new().await;
    let admin = admin(&world).await;

    let body = world
        .as_user(admin)
        .put("/api/admin/flags/new-pipeline")
        .json(&json!({"everyone": true, "rollout": {"percent": 25, "of": "workspaces"}}))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["flag"]["everyone"], true);
    assert_eq!(
        body["flag"]["rollout"],
        json!({"percent": 25, "of": "workspaces"})
    );
    assert_eq!(
        body["flag"]["updated_by"],
        json!(admin),
        "the request's user"
    );
    assert_eq!(body["overrides"], json!([]));

    // Only the fields present change; null unsets.
    let body = world
        .as_user(admin)
        .put("/api/admin/flags/new-pipeline")
        .json(&json!({"everyone": null, "killed": true}))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["flag"]["everyone"], json!(null));
    assert_eq!(body["flag"]["killed"], true);
    assert_eq!(
        body["flag"]["rollout"]["percent"], 25,
        "the rollout is kept"
    );
    let state = world.flags().await.state::<NewPipeline>().await.unwrap();
    assert!(state.killed && state.everyone.is_none());

    world
        .as_user(admin)
        .put("/api/admin/flags/new-pipeline")
        .json(&json!({"rollout": null, "killed": false}))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("flag.rollout", json!(null))
        .assert_json_path("flag.killed", json!(false));
}

#[tokio::test]
async fn admins_manage_overrides() {
    let world = World::new().await;
    let admin = admin(&world).await;
    let (ada, acme) = (Uuid::now_v7(), Uuid::now_v7());

    world
        .as_user(admin)
        .put("/api/admin/flags/new-pipeline/overrides")
        .json(&json!({"user_id": ada, "active": true}))
        .send()
        .await
        .assert_status(200);
    let body = world
        .as_user(admin)
        .put("/api/admin/flags/new-pipeline/overrides")
        .json(&json!({"workspace_id": acme, "active": false}))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["flag"]["overrides"], 2);
    assert_eq!(
        body["overrides"],
        json!([
            {"flag": "new-pipeline", "scope_type": "user", "scope_id": ada, "active": true},
            {"flag": "new-pipeline", "scope_type": "workspace", "scope_id": acme, "active": false},
        ])
    );
    let flags = world.flags().await;
    assert!(
        flags
            .active::<NewPipeline>(FlagScope::user(ada))
            .await
            .unwrap()
    );

    world
        .as_user(admin)
        .delete(&format!(
            "/api/admin/flags/new-pipeline/overrides?user_id={ada}"
        ))
        .send()
        .await
        .assert_status(204);
    assert!(
        !flags
            .active::<NewPipeline>(FlagScope::user(ada))
            .await
            .unwrap()
    );
    world
        .as_user(admin)
        .get("/api/admin/flags/new-pipeline")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("flag.overrides", json!(1));
}

#[tokio::test]
async fn admin_mistakes_are_client_errors() {
    let world = World::new().await;
    let admin = admin(&world).await;
    world
        .as_user(admin)
        .get("/api/admin/flags/nope")
        .send()
        .await
        .assert_error(404, "unknown flag `nope`");
    world
        .as_user(admin)
        .put("/api/admin/flags/nope")
        .json(&json!({"killed": true}))
        .send()
        .await
        .assert_error(404, None);
    world
        .as_user(admin)
        .put("/api/admin/flags/new-pipeline")
        .json(&json!({"rollout": {"percent": 150}}))
        .send()
        .await
        .assert_error(
            400,
            "invalid rollout: percent must not be greater than 100 (got 150)",
        );
    world
        .as_user(admin)
        .put("/api/admin/flags/new-pipeline")
        .json(&json!({"everyone": "yes"}))
        .send()
        .await
        .assert_error(400, None);
    world
        .as_user(admin)
        .put("/api/admin/flags/new-pipeline/overrides")
        .json(&json!({"user_id": Uuid::now_v7(), "workspace_id": Uuid::now_v7(), "active": true}))
        .send()
        .await
        .assert_validation_error("user_id");
    world
        .as_user(admin)
        .delete("/api/admin/flags/new-pipeline/overrides")
        .send()
        .await
        .assert_validation_error("user_id");
}
