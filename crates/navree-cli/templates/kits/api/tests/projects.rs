//! The example feature: projects stay inside their workspace, and each
//! workspace role gets what `config/app.toml` and `src/abilities.rs` say.

mod support;

use navree::workspace::WorkspaceRole;
use serde_json::json;
use support::{add_member, boot, in_workspace, sign_in, user, workspace};

#[tokio::test]
async fn members_create_list_filter_and_update_projects() {
    let (app, _guard) = boot().await;
    let owner = user(&app, "Olivia Owner", "olivia@example.com").await;
    let team = workspace(&app, &owner, "Acme").await;
    let token = sign_in(&app, "olivia@example.com").await;

    for name in ["Website", "Mobile app", "Web shop"] {
        in_workspace(app.client().post("/api/v1/projects"), &token, &team.slug)
            .json(&json!({ "name": name, "description": "Launch it." }))
            .send()
            .await
            .assert_status(201)
            .assert_json_path("name", json!(name))
            .assert_json_path("status", json!("active"))
            .assert_json_path("created_by", json!(owner.id));
    }
    in_workspace(app.client().get("/api/v1/projects"), &token, &team.slug)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", json!(3))
        .assert_json_path("data.0.name", json!("Web shop"));
    in_workspace(app.client().get("/api/v1/projects"), &token, &team.slug)
        .query(&[("search", "Web")])
        .send()
        .await
        .assert_json_path("meta.total", json!(2));

    let page = in_workspace(app.client().get("/api/v1/projects"), &token, &team.slug)
        .send()
        .await
        .json_value();
    let id = page["data"][0]["id"].as_str().expect("an id").to_owned();
    in_workspace(
        app.client().patch(&format!("/api/v1/projects/{id}")),
        &token,
        &team.slug,
    )
    .json(&json!({ "status": "archived" }))
    .send()
    .await
    .assert_status(200)
    .assert_json_path("status", json!("archived"));
    in_workspace(app.client().get("/api/v1/projects"), &token, &team.slug)
        .query(&[("status", "archived")])
        .send()
        .await
        .assert_json_path("meta.total", json!(1));

    // Invalid input is a 400 naming the field.
    in_workspace(app.client().post("/api/v1/projects"), &token, &team.slug)
        .json(&json!({ "name": "" }))
        .send()
        .await
        .assert_validation_error("name");
}

#[tokio::test]
async fn projects_never_leave_their_workspace() {
    let (app, _guard) = boot().await;
    let olivia = user(&app, "Olivia Owner", "olivia@example.com").await;
    let acme = workspace(&app, &olivia, "Acme").await;
    let grace = user(&app, "Grace Hopper", "grace@example.com").await;
    let globex = workspace(&app, &grace, "Globex").await;
    let olivia_token = sign_in(&app, "olivia@example.com").await;
    let grace_token = sign_in(&app, "grace@example.com").await;

    let project = in_workspace(app.client().post("/api/v1/projects"), &olivia_token, &acme.slug)
        .json(&json!({ "name": "Secret plan" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let id = project["id"].as_str().expect("an id");

    // Another workspace's project is not found, even by id.
    in_workspace(app.client().get("/api/v1/projects"), &grace_token, &globex.slug)
        .send()
        .await
        .assert_json_path("meta.total", json!(0));
    in_workspace(
        app.client().get(&format!("/api/v1/projects/{id}")),
        &grace_token,
        &globex.slug,
    )
    .send()
    .await
    .assert_status(404);
    // And a workspace someone is not a member of is closed to them.
    in_workspace(app.client().get("/api/v1/projects"), &grace_token, &acme.slug)
        .send()
        .await
        .assert_status(403);
    // A request that names no workspace is a 400.
    app.client()
        .get("/api/v1/projects")
        .bearer(&grace_token)
        .send()
        .await
        .assert_status(400);
}

#[tokio::test]
async fn roles_decide_who_changes_what() {
    let (app, _guard) = boot().await;
    let owner = user(&app, "Olivia Owner", "olivia@example.com").await;
    let team = workspace(&app, &owner, "Acme").await;
    let member = user(&app, "Max Member", "max@example.com").await;
    add_member(&app, &team, &member, WorkspaceRole::Member).await;
    let viewer = user(&app, "Vera Viewer", "vera@example.com").await;
    add_member(&app, &team, &viewer, WorkspaceRole::Viewer).await;
    let owner_token = sign_in(&app, "olivia@example.com").await;
    let member_token = sign_in(&app, "max@example.com").await;
    let viewer_token = sign_in(&app, "vera@example.com").await;

    let owners = in_workspace(app.client().post("/api/v1/projects"), &owner_token, &team.slug)
        .json(&json!({ "name": "The owner's" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let members = in_workspace(app.client().post("/api/v1/projects"), &member_token, &team.slug)
        .json(&json!({ "name": "The member's" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let path = |project: &serde_json::Value| format!("/api/v1/projects/{}", project["id"].as_str().unwrap());

    // Members change their own projects only.
    in_workspace(app.client().patch(&path(&members)), &member_token, &team.slug)
        .json(&json!({ "name": "Renamed" }))
        .send()
        .await
        .assert_status(200);
    in_workspace(app.client().patch(&path(&owners)), &member_token, &team.slug)
        .json(&json!({ "name": "Mine now" }))
        .send()
        .await
        .assert_status(403);
    // Viewers read, and nothing else.
    in_workspace(app.client().get(&path(&owners)), &viewer_token, &team.slug)
        .send()
        .await
        .assert_status(200);
    in_workspace(app.client().post("/api/v1/projects"), &viewer_token, &team.slug)
        .json(&json!({ "name": "Nope" }))
        .send()
        .await
        .assert_status(403);
    // Owners change and delete any project.
    in_workspace(app.client().delete(&path(&members)), &owner_token, &team.slug)
        .send()
        .await
        .assert_status(204);
}
