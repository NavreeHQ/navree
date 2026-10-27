//! The workspace's settings and members' notification preferences
//! (`/api/v1/settings`).

mod support;

use navree::workspace::WorkspaceRole;
use serde_json::json;
use support::{add_member, boot, in_workspace, sign_in, user, workspace};

#[tokio::test]
async fn owners_rename_the_workspace_and_change_its_slug() {
    let (app, _guard) = boot().await;
    let owner = user(&app, "Olivia Owner", "olivia@example.com").await;
    let team = workspace(&app, &owner, "Acme").await;
    let other = workspace(&app, &owner, "Globex").await;
    let token = sign_in(&app, "olivia@example.com").await;

    in_workspace(
        app.client().get("/api/v1/settings/workspace"),
        &token,
        &team.slug,
    )
    .send()
    .await
    .assert_status(200)
    .assert_json_path("name", json!("Acme"));
    in_workspace(
        app.client().patch("/api/v1/settings/workspace"),
        &token,
        &team.slug,
    )
    .json(&json!({ "name": "Acme Inc.", "slug": "acme-inc" }))
    .send()
    .await
    .assert_status(200)
    .assert_json_path("name", json!("Acme Inc."))
    .assert_json_path("slug", json!("acme-inc"));
    // The new slug names the workspace from now on.
    in_workspace(
        app.client().get("/api/v1/settings/workspace"),
        &token,
        "acme-inc",
    )
    .send()
    .await
    .assert_status(200);
    // A slug is unique and lower-case.
    in_workspace(
        app.client().patch("/api/v1/settings/workspace"),
        &token,
        "acme-inc",
    )
    .json(&json!({ "slug": other.slug }))
    .send()
    .await
    .assert_validation_error("slug");
    in_workspace(
        app.client().patch("/api/v1/settings/workspace"),
        &token,
        "acme-inc",
    )
    .json(&json!({ "slug": "Not A Slug" }))
    .send()
    .await
    .assert_validation_error("slug");
}

#[tokio::test]
async fn members_cannot_change_the_workspace() {
    let (app, _guard) = boot().await;
    let owner = user(&app, "Olivia Owner", "olivia@example.com").await;
    let team = workspace(&app, &owner, "Acme").await;
    let member = user(&app, "Max Member", "max@example.com").await;
    add_member(&app, &team, &member, WorkspaceRole::Member).await;
    let token = sign_in(&app, "max@example.com").await;

    in_workspace(
        app.client().get("/api/v1/settings/workspace"),
        &token,
        &team.slug,
    )
    .send()
    .await
    .assert_status(200);
    in_workspace(
        app.client().patch("/api/v1/settings/workspace"),
        &token,
        &team.slug,
    )
    .json(&json!({ "name": "Mine" }))
    .send()
    .await
    .assert_status(403);
}

#[tokio::test]
async fn the_front_end_reads_the_rules_of_the_role_in_the_workspace() {
    let (app, _guard) = boot().await;
    let owner = user(&app, "Olivia Owner", "olivia@example.com").await;
    let team = workspace(&app, &owner, "Acme").await;
    let viewer = user(&app, "Vera Viewer", "vera@example.com").await;
    add_member(&app, &team, &viewer, WorkspaceRole::Viewer).await;
    let token = sign_in(&app, "vera@example.com").await;

    let rules = in_workspace(
        app.client().get("/api/v1/settings/abilities"),
        &token,
        &team.slug,
    )
    .send()
    .await
    .assert_status(200)
    .json_value();
    let rules = rules.as_array().expect("CASL rules");
    let grants = |action: &str, subject: &str| {
        rules.iter().any(|rule| {
            rule["action"] == action && rule["subject"] == subject && rule.get("inverted").is_none()
        })
    };
    assert!(grants("read", "Project"), "{rules:?}");
    assert!(!grants("create", "Project"), "{rules:?}");
}

#[tokio::test]
async fn each_member_keeps_their_own_notification_preferences() {
    let (app, _guard) = boot().await;
    let owner = user(&app, "Olivia Owner", "olivia@example.com").await;
    let team = workspace(&app, &owner, "Acme").await;
    let member = user(&app, "Max Member", "max@example.com").await;
    add_member(&app, &team, &member, WorkspaceRole::Member).await;
    let owner_token = sign_in(&app, "olivia@example.com").await;
    let member_token = sign_in(&app, "max@example.com").await;

    in_workspace(
        app.client().get("/api/v1/settings/notifications"),
        &member_token,
        &team.slug,
    )
    .send()
    .await
    .assert_status(200)
    .assert_json_path("weekly_summary", json!(true))
    .assert_json_path("product_updates", json!(false));
    in_workspace(
        app.client().put("/api/v1/settings/notifications"),
        &member_token,
        &team.slug,
    )
    .json(&json!({ "weekly_summary": false }))
    .send()
    .await
    .assert_status(200)
    .assert_json_path("weekly_summary", json!(false))
    .assert_json_path("project_activity", json!(true));
    in_workspace(
        app.client().get("/api/v1/settings/notifications"),
        &member_token,
        &team.slug,
    )
    .send()
    .await
    .assert_json_path("weekly_summary", json!(false));
    // The owner's are untouched.
    in_workspace(
        app.client().get("/api/v1/settings/notifications"),
        &owner_token,
        &team.slug,
    )
    .send()
    .await
    .assert_json_path("weekly_summary", json!(true));
}
