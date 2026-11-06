//! The organization routes (`/api/organizations...`) and the partition
//! routes (`/api/workspaces/{workspace}/partitions...`).

use navree_workspace::{PartitionSelector, WorkspaceSelector};
use serde_json::json;

use crate::support::{World, builder, org_builder};

#[tokio::test]
async fn a_user_creates_lists_shows_updates_and_deletes_organizations() {
    let world = World::with(org_builder()).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let client = world.as_user(ada);

    let created = client
        .post("/api/organizations")
        .json(&json!({ "name": "Agra Group", "country": "gb", "timezone": "Europe/London" }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("__type", json!("Organization"))
        .assert_json_path("slug", json!("agra-group"))
        .assert_json_path("country", json!("GB"))
        .assert_json_path("role", json!("owner"))
        .assert_json_path("workspaces", json!("all"));
    assert_eq!(
        created.header("location"),
        Some("/api/organizations/agra-group")
    );
    client
        .post("/api/organizations")
        .json(&json!({ "name": "X", "timezone": "nowhere at all" }))
        .send()
        .await
        .assert_validation_error("timezone");

    client
        .get("/api/organizations")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.slug", json!("agra-group"))
        .assert_json_path("data.0.role", json!("owner"));
    client
        .get("/api/organizations/agra-group")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("timezone", json!("Europe/London"));
    // `null` clears a field; an absent field stays.
    client
        .patch("/api/organizations/agra-group")
        .json(&json!({ "name": "Agra", "timezone": null }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!("Agra"))
        .assert_json_path("timezone", json!(null))
        .assert_json_path("country", json!("GB"));
    client
        .delete("/api/organizations/agra-group")
        .send()
        .await
        .assert_status(204);
    client
        .get("/api/organizations/agra-group")
        .send()
        .await
        .assert_error(404, "Organization not found.");
    world
        .app
        .client()
        .get("/api/organizations")
        .send()
        .await
        .assert_error(401, None);
}

#[tokio::test]
async fn organization_routes_follow_the_organization_role() {
    let world = World::with(org_builder()).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let linus = world.user("Laksh", "laksh@example.com").await;
    let ken = world.user("Ken", "ken@example.com").await;
    let agra = world.organization(ada, "Agra").await;
    let uk = world.workspace(ada, "Agra Pune").await;
    let de = world.workspace(ada, "Agra DE").await;
    world.attach(agra.id, uk.id).await;
    world.attach(agra.id, de.id).await;
    world
        .grant(agra.id, grace, "admin", WorkspaceSelector::All)
        .await;
    world
        .grant(agra.id, linus, "member", WorkspaceSelector::only([uk.id]))
        .await;

    // Non-members get a 403; members read.
    world
        .as_user(ken)
        .get("/api/organizations/agra")
        .send()
        .await
        .assert_error(403, "You are not a member of this organization.");
    let member = world.as_user(linus);
    member
        .get("/api/organizations/agra")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("role", json!("member"))
        .assert_json_path("workspaces", json!([uk.id]));
    member
        .get("/api/organizations/agra/members")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.__type", json!("OrganizationMember"))
        .assert_json_path("data.0.user.name", json!("Anu"));
    member
        .get("/api/organizations/agra/workspaces")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.slug", json!("agra-uk"))
        .assert_json_path("data.0.role", json!("member"))
        .assert_json_path("data.0.organization_id", json!(agra.id));
    member
        .patch("/api/organizations/agra")
        .json(&json!({ "name": "Mine" }))
        .send()
        .await
        .assert_error(403, None);
    member
        .post("/api/organizations/agra/members")
        .json(&json!({ "user_id": ken }))
        .send()
        .await
        .assert_error(403, None);

    // An admin manages members, but not ownership or deletion.
    let admin = world.as_user(grace);
    admin
        .post("/api/organizations/agra/members")
        .json(&json!({ "user_id": ken, "role": "partner", "workspaces": [de.id] }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("role", json!("partner"))
        .assert_json_path("workspaces", json!([de.id]));
    admin
        .post("/api/organizations/agra/members")
        .json(&json!({ "user_id": ken }))
        .send()
        .await
        .assert_error(409, "The user is already a member of this organization.");
    admin
        .patch(&format!("/api/organizations/agra/members/{ken}"))
        .json(&json!({ "workspaces": "all" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("workspaces", json!("all"))
        .assert_json_path("role", json!("partner"));
    admin
        .patch(&format!("/api/organizations/agra/members/{ada}"))
        .json(&json!({ "role": "admin" }))
        .send()
        .await
        .assert_status(409);
    admin
        .patch(&format!("/api/organizations/agra/members/{ken}"))
        .json(&json!({ "role": "owner" }))
        .send()
        .await
        .assert_validation_error("role");
    admin
        .delete("/api/organizations/agra")
        .send()
        .await
        .assert_error(403, None);
    admin
        .post("/api/organizations/agra/transfer")
        .json(&json!({ "user_id": grace }))
        .send()
        .await
        .assert_error(403, None);
    admin
        .delete(&format!("/api/organizations/agra/members/{ken}"))
        .send()
        .await
        .assert_status(204);

    // The owner transfers; members leave; the owner can't.
    let owner = world.as_user(ada);
    owner
        .post("/api/organizations/agra/leave")
        .send()
        .await
        .assert_status(409);
    member
        .post("/api/organizations/agra/leave")
        .send()
        .await
        .assert_status(204);
    owner
        .post("/api/organizations/agra/transfer")
        .json(&json!({ "user_id": grace }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("owner_id", json!(grace))
        .assert_json_path("role", json!("admin"));
}

#[tokio::test]
async fn limited_admins_grant_only_the_workspaces_they_reach() {
    let world = World::with(org_builder()).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let ken = world.user("Ken", "ken@example.com").await;
    let agra = world.organization(ada, "Agra").await;
    let uk = world.workspace(ada, "Agra Pune").await;
    let de = world.workspace(ada, "Agra DE").await;
    world.attach(agra.id, uk.id).await;
    world.attach(agra.id, de.id).await;
    world
        .grant(agra.id, grace, "admin", WorkspaceSelector::only([uk.id]))
        .await;
    let admin = world.as_user(grace);

    // Not themselves to everything, nor others to more than they reach.
    admin
        .patch(&format!("/api/organizations/agra/members/{grace}"))
        .json(&json!({ "workspaces": "all" }))
        .send()
        .await
        .assert_error(403, "You can only grant workspaces you reach.")
        .assert_error_code(403, "workspace_grant_forbidden");
    admin
        .post("/api/organizations/agra/members")
        .json(&json!({ "user_id": ken, "workspaces": [de.id] }))
        .send()
        .await
        .assert_error(403, "You can only grant workspaces you reach.")
        .assert_error_code(403, "workspace_grant_forbidden");
    admin
        .post("/api/organizations/agra/members")
        .json(&json!({ "user_id": ken }))
        .send()
        .await
        .assert_error(403, "You can only grant workspaces you reach.")
        .assert_error_code(403, "workspace_grant_forbidden");
    admin
        .post("/api/organizations/agra/members")
        .json(&json!({ "user_id": ken, "workspaces": [uk.id] }))
        .send()
        .await
        .assert_status(201);
    // Nor change members who reach more than they do.
    admin
        .delete(&format!("/api/organizations/agra/members/{ada}"))
        .send()
        .await
        .assert_error(403, "You can only grant workspaces you reach.")
        .assert_error_code(403, "workspace_grant_forbidden");
    // And they see only their workspaces.
    admin
        .get("/api/organizations/agra/workspaces")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.__type", json!("Workspace"))
        .assert_json_path("data.0.id", json!(uk.id))
        .assert_json_path("data.0.organization_id", json!(agra.id))
        .assert_json_path("data.0.role", json!("admin"))
        .json_value();
    let listed = admin
        .get("/api/organizations/agra/workspaces")
        .send()
        .await
        .json_value();
    assert_eq!(listed["data"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn workspaces_are_attached_and_detached_over_http() {
    let world = World::with(org_builder()).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.organization(ada, "Agra").await;
    let mine = world.workspace(ada, "Agra Pune").await;
    let theirs = world.workspace(grace, "Gauri Co").await;
    world
        .grant(agra.id, grace, "admin", WorkspaceSelector::All)
        .await;
    let owner = world.as_user(ada);

    owner
        .post("/api/organizations/agra/workspaces")
        .json(&json!({ "workspace": "agra-uk" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("organization_id", json!(agra.id));
    // Attaching takes the workspace's owner.
    owner
        .post("/api/organizations/agra/workspaces")
        .json(&json!({ "workspace": theirs.id }))
        .send()
        .await
        .assert_error(
            403,
            "Only the workspace's owner can attach it to an organization.",
        )
        .assert_error_code(403, "not_the_workspace_owner");
    world
        .as_user(grace)
        .post("/api/organizations/agra/workspaces")
        .json(&json!({ "workspace": "gauri-co" }))
        .send()
        .await
        .assert_status(200);
    owner
        .post("/api/organizations/agra/workspaces")
        .json(&json!({ "workspace": "nope" }))
        .send()
        .await
        .assert_error(404, "Workspace not found.");
    owner
        .get("/api/workspaces/gauri-co")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("role", json!("owner"));
    owner
        .delete(&format!("/api/organizations/agra/workspaces/{}", mine.id))
        .send()
        .await
        .assert_status(204);
    owner
        .delete("/api/organizations/agra/workspaces/agra-uk")
        .send()
        .await
        .assert_error(404, "The workspace is not part of this organization.");
}

#[tokio::test]
async fn access_requests_over_http() {
    let world = World::with(org_builder()).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let linus = world.user("Laksh", "laksh@example.com").await;
    let agra = world.organization(ada, "Agra").await;
    let uk = world.workspace(ada, "Agra Pune").await;
    world.attach(agra.id, uk.id).await;

    let asking = world.as_user(grace);
    let request = asking
        .post("/api/organizations/agra/access-requests")
        .json(&json!({ "workspace_id": uk.id, "message": "UK lead" }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("__type", json!("AccessRequest"))
        .assert_json_path("status", json!("pending"))
        .json_value();
    let id = request["id"].as_str().unwrap().to_owned();
    asking
        .post("/api/organizations/agra/access-requests")
        .json(&json!({}))
        .send()
        .await
        .assert_error(409, "An access request is already pending.");
    // Requesters aren't members: they can't read the queue.
    asking
        .get("/api/organizations/agra/access-requests")
        .send()
        .await
        .assert_error(403, "You are not a member of this organization.");

    let owner = world.as_user(ada);
    owner
        .get("/api/organizations/agra/access-requests")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.user.email", json!("gauri@example.com"))
        .assert_json_path("data.0.message", json!("UK lead"));
    owner
        .post(&format!(
            "/api/organizations/agra/access-requests/{id}/approve"
        ))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("request.status", json!("approved"))
        .assert_json_path("member.role", json!("member"))
        .assert_json_path("member.workspaces", json!([uk.id]));
    owner
        .post(&format!(
            "/api/organizations/agra/access-requests/{id}/decline"
        ))
        .send()
        .await
        .assert_error(409, "The access request has already been decided.");
    asking
        .get("/api/workspaces/agra-uk")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("role", json!("member"));

    let other = world
        .as_user(linus)
        .post("/api/organizations/agra/access-requests")
        .json(&json!({}))
        .send()
        .await
        .assert_status(201)
        .json_value();
    owner
        .post(&format!(
            "/api/organizations/agra/access-requests/{}/decline",
            other["id"].as_str().unwrap()
        ))
        .json(&json!({ "reason": "Not now" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("status", json!("declined"))
        .assert_json_path("reason", json!("Not now"));
}

#[tokio::test]
async fn partitions_over_http() {
    let world = World::with(org_builder()).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let linus = world.user("Laksh", "laksh@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    world.join(agra.id, grace, "admin").await;
    world.join(agra.id, linus, "member").await;
    let owner = world.as_user(ada);

    owner
        .post("/api/workspaces/agra/partitions")
        .json(&json!({ "name": "agra.co.uk" }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("__type", json!("WorkspacePartition"))
        .assert_json_path("slug", json!("agra-co-uk"));
    let de = world.partition(agra.id, "agra.de").await;
    owner
        .patch("/api/workspaces/agra/partitions/agra-de")
        .json(&json!({ "name": "agra.de (EU)" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!("agra.de (EU)"));
    world
        .as_user(linus)
        .post("/api/workspaces/agra/partitions")
        .json(&json!({ "name": "x" }))
        .send()
        .await
        .assert_error(403, None);

    // Limit Laksh to one partition; he lists only that one.
    let uk = world
        .partitions()
        .await
        .resolve(agra.id, "agra-co-uk")
        .await
        .unwrap();
    owner
        .put(&format!("/api/workspaces/agra/members/{linus}/partitions"))
        .json(&json!({ "partitions": [uk.id] }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("partitions", json!([uk.id]));
    owner
        .get(&format!("/api/workspaces/agra/members/{linus}/partitions"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("partitions", json!([uk.id]));
    world
        .as_user(linus)
        .get("/api/workspaces/agra/partitions")
        .send()
        .await
        .assert_status(200)
        .assert_json_path(
            "data",
            json!([{
                "__type": "WorkspacePartition",
                "id": uk.id,
                "workspace_id": agra.id,
                "name": "agra.co.uk",
                "slug": "agra-co-uk",
                "created_at": uk.created_at,
                "updated_at": uk.updated_at,
            }]),
        );

    // A partition-limited admin can't widen anyone, themselves included,
    // or manage partitions.
    world
        .limit_partitions(agra.id, grace, PartitionSelector::only([uk.id]))
        .await;
    let admin = world.as_user(grace);
    admin
        .put(&format!("/api/workspaces/agra/members/{grace}/partitions"))
        .json(&json!({ "partitions": "all" }))
        .send()
        .await
        .assert_error(403, "You can only grant partitions you see.")
        .assert_error_code(403, "partition_grant_forbidden");
    admin
        .put(&format!("/api/workspaces/agra/members/{linus}/partitions"))
        .json(&json!({ "partitions": [de.id] }))
        .send()
        .await
        .assert_error(403, "You can only grant partitions you see.")
        .assert_error_code(403, "partition_grant_forbidden");
    admin
        .delete(&format!("/api/workspaces/agra/partitions/{}", uk.id))
        .send()
        .await
        .assert_error(
            403,
            "Only members who see every partition can manage partitions.",
        )
        .assert_error_code(403, "partition_manager_required");

    owner
        .delete(&format!("/api/workspaces/agra/partitions/{}", de.id))
        .send()
        .await
        .assert_status(204);
    owner
        .delete(&format!("/api/workspaces/agra/partitions/{}", de.id))
        .send()
        .await
        .assert_error(404, "Partition not found.");
}

#[tokio::test]
async fn the_routes_are_off_with_the_features() {
    let world = World::with(builder()).await;
    let ada = world.user("Anu", "anu@example.com").await;
    world.workspace(ada, "Agra").await;
    let client = world.as_user(ada);
    client
        .get("/api/organizations")
        .send()
        .await
        .assert_status(404);
    client
        .get("/api/workspaces/agra/partitions")
        .send()
        .await
        .assert_status(404);
    // Workspace resources still carry `organization_id`.
    client
        .get("/api/workspaces/agra")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("organization_id", json!(null));
}
