//! The module's routes: `/api/workspaces...` and `/api/invitations/accept`.

use serde_json::json;

use crate::support::{World, builder};

#[tokio::test]
async fn a_user_creates_lists_shows_renames_and_deletes_workspaces() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let client = world.as_user(ada);

    let created = client
        .post("/api/workspaces")
        .json(&json!({ "name": "Acme Inc." }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("__type", json!("Workspace"))
        .assert_json_path("slug", json!("acme-inc"))
        .assert_json_path("role", json!("owner"))
        .assert_json_path("owner_id", json!(ada));
    assert_eq!(created.header("location"), Some("/api/workspaces/acme-inc"));
    client
        .post("/api/workspaces")
        .json(&json!({ "name": "Taken", "slug": "acme-inc" }))
        .send()
        .await
        .assert_validation_error("slug");
    client
        .post("/api/workspaces")
        .json(&json!({ "name": "" }))
        .send()
        .await
        .assert_validation_error("name");

    client
        .get("/api/workspaces")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.slug", json!("acme-inc"))
        .assert_json_path("data.0.role", json!("owner"));
    client
        .get("/api/workspaces/acme-inc")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!("Acme Inc."));
    client
        .patch("/api/workspaces/acme-inc")
        .json(&json!({ "name": "Acme Corp" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!("Acme Corp"));
    client
        .delete("/api/workspaces/acme-inc")
        .send()
        .await
        .assert_status(204);
    client
        .get("/api/workspaces/acme-inc")
        .send()
        .await
        .assert_error(404, "Workspace not found.");
    world
        .app
        .client()
        .get("/api/workspaces")
        .send()
        .await
        .assert_error(401, None);
}

#[tokio::test]
async fn permissions_follow_the_workspace_role() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let linus = world.user("Linus", "linus@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    world.join(acme.id, grace, "admin").await;
    world.join(acme.id, linus, "member").await;

    // A member reads but does not manage.
    let member = world.as_user(linus);
    member
        .get("/api/workspaces/acme/members")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.__type", json!("WorkspaceMember"))
        .assert_json_path("data.0.user.name", json!("Ada"));
    member
        .patch("/api/workspaces/acme")
        .json(&json!({ "name": "Mine" }))
        .send()
        .await
        .assert_error(403, None);
    member
        .get("/api/workspaces/acme/invitations")
        .send()
        .await
        .assert_error(403, None);

    // An admin manages members and invitations but cannot delete or
    // transfer the workspace, or grant `owner`.
    let admin = world.as_user(grace);
    admin
        .patch(&format!("/api/workspaces/acme/members/{linus}"))
        .json(&json!({ "role": "viewer" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("role", json!("viewer"));
    admin
        .patch(&format!("/api/workspaces/acme/members/{linus}"))
        .json(&json!({ "role": "owner" }))
        .send()
        .await
        .assert_error(403, "Only an owner can grant or change the owner role.");
    admin
        .delete(&format!("/api/workspaces/acme/members/{ada}"))
        .send()
        .await
        .assert_error(403, "Only an owner can grant or change the owner role.");
    admin
        .patch(&format!("/api/workspaces/acme/members/{linus}"))
        .json(&json!({ "role": "auditor" }))
        .send()
        .await
        .assert_validation_error("role");
    admin
        .delete("/api/workspaces/acme")
        .send()
        .await
        .assert_error(403, None);
    admin
        .post("/api/workspaces/acme/transfer")
        .json(&json!({ "user_id": grace }))
        .send()
        .await
        .assert_error(403, None);
    admin
        .delete(&format!("/api/workspaces/acme/members/{linus}"))
        .send()
        .await
        .assert_status(204);

    // The owner can't be removed or demoted, even by themselves.
    let owner = world.as_user(ada);
    owner
        .patch(&format!("/api/workspaces/acme/members/{ada}"))
        .json(&json!({ "role": "admin" }))
        .send()
        .await
        .assert_error(
            409,
            "The workspace owner can't be removed or demoted; transfer ownership first.",
        );
    owner
        .post("/api/workspaces/acme/leave")
        .send()
        .await
        .assert_status(409);

    // Transfer, then the old owner can leave.
    owner
        .post("/api/workspaces/acme/transfer")
        .json(&json!({ "user_id": grace }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("owner_id", json!(grace))
        .assert_json_path("role", json!("admin"));
    owner
        .post("/api/workspaces/acme/leave")
        .send()
        .await
        .assert_status(204);
    owner
        .get("/api/workspaces/acme")
        .send()
        .await
        .assert_error(403, "You are not a member of this workspace.");
}

#[tokio::test]
async fn members_are_added_by_id() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    world.workspace(ada, "Acme").await;
    let owner = world.as_user(ada);
    let added = owner
        .post("/api/workspaces/acme/members")
        .json(&json!({ "user_id": grace }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("role", json!("member"))
        .assert_json_path("user_id", json!(grace));
    assert_eq!(
        added.header("location"),
        Some(format!("/api/workspaces/acme/members/{grace}").as_str())
    );
    owner
        .post("/api/workspaces/acme/members")
        .json(&json!({ "user_id": grace }))
        .send()
        .await
        .assert_error(409, "The user is already a member of this workspace.");
}

#[tokio::test]
async fn invitations_are_sent_listed_revoked_and_accepted() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let owner = world.as_user(ada);

    let sent = owner
        .post("/api/workspaces/acme/invitations")
        .json(&json!({ "email": "grace@example.com", "role": "admin" }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("__type", json!("WorkspaceInvitation"))
        .assert_json_path("email", json!("grace@example.com"))
        .assert_json_path("role", json!("admin"));
    let body = sent.json_value();
    assert!(body.get("token").is_none() && body.get("token_hash").is_none());
    owner
        .get("/api/workspaces/acme/invitations")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.email", json!("grace@example.com"));
    owner
        .post("/api/workspaces/acme/invitations")
        .json(&json!({ "email": "ada@example.com" }))
        .send()
        .await
        .assert_status(409);

    // The token travels in the mail only.
    let mails = world.mails().await;
    let text = mails.last().unwrap().text.clone().unwrap();
    let token = text
        .split("token=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned();

    let invitee = world.as_user(grace);
    invitee
        .post("/api/invitations/accept")
        .json(&json!({ "token": "wrong" }))
        .send()
        .await
        .assert_error(404, "Invitation not found.");
    invitee
        .post("/api/invitations/accept")
        .json(&json!({ "token": token }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("workspace.id", json!(acme.id))
        .assert_json_path("workspace.role", json!("admin"))
        .assert_json_path("member.role", json!("admin"));
    invitee
        .get("/api/workspaces")
        .send()
        .await
        .assert_json_path("data.0.slug", json!("acme"));

    // Revoking: the token stops working.
    let revoked = owner
        .post("/api/workspaces/acme/invitations")
        .json(&json!({ "email": "linus@example.com" }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    owner
        .delete(&format!(
            "/api/workspaces/acme/invitations/{}",
            revoked["id"].as_str().unwrap()
        ))
        .send()
        .await
        .assert_status(204);
    owner
        .get("/api/workspaces/acme/invitations")
        .send()
        .await
        .assert_json_path("data", json!([]));
}

#[tokio::test]
async fn expired_invitations_answer_410() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    world.workspace(ada, "Acme").await;
    world
        .as_user(ada)
        .post("/api/workspaces/acme/invitations")
        .json(&json!({ "email": "grace@example.com" }))
        .send()
        .await
        .assert_status(201);
    let text = world.mails().await.last().unwrap().text.clone().unwrap();
    let token = text
        .split("token=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned();
    world
        .clock
        .advance(std::time::Duration::from_secs(8 * 24 * 3600));
    world
        .as_user(grace)
        .post("/api/invitations/accept")
        .json(&json!({ "token": token }))
        .send()
        .await
        .assert_error(410, "The invitation has expired.");
}

#[tokio::test]
async fn the_routes_can_be_turned_off() {
    let world = World::with(builder().config("workspace.routes", false)).await;
    let ada = world.user("Ada", "ada@example.com").await;
    world
        .as_user(ada)
        .get("/api/workspaces")
        .send()
        .await
        .assert_status(404);
}
