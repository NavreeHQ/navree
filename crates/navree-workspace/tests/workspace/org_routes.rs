//! The organization routes (`/api/organizations...`) and the partition
//! routes (`/api/workspaces/{workspace}/partitions...`).

use navree_workspace::{PartitionSelector, WorkspaceSelector};
use serde_json::json;

use crate::support::{World, builder, org_builder};

#[tokio::test]
async fn a_user_creates_lists_shows_updates_and_deletes_organizations() {
    let world = World::with(org_builder()).await;
    let ada = world.user("Ada", "ada@example.com").await;
    let client = world.as_user(ada);

    let created = client
        .post("/api/organizations")
        .json(&json!({ "name": "Acme Group", "country": "gb", "timezone": "Europe/London" }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("__type", json!("Organization"))
        .assert_json_path("slug", json!("acme-group"))
        .assert_json_path("country", json!("GB"))
        .assert_json_path("role", json!("owner"))
        .assert_json_path("workspaces", json!("all"));
    assert_eq!(
        created.header("location"),
        Some("/api/organizations/acme-group")
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
        .assert_json_path("data.0.slug", json!("acme-group"))
        .assert_json_path("data.0.role", json!("owner"));
    client
        .get("/api/organizations/acme-group")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("timezone", json!("Europe/London"));
    // `null` clears a field; an absent field stays.
    client
        .patch("/api/organizations/acme-group")
        .json(&json!({ "name": "Acme", "timezone": null }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!("Acme"))
        .assert_json_path("timezone", json!(null))
        .assert_json_path("country", json!("GB"));
    client
        .delete("/api/organizations/acme-group")
        .send()
        .await
        .assert_status(204);
    client
        .get("/api/organizations/acme-group")
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
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let linus = world.user("Linus", "linus@example.com").await;
    let ken = world.user("Ken", "ken@example.com").await;
    let acme = world.organization(ada, "Acme").await;
    let uk = world.workspace(ada, "Acme UK").await;
    let de = world.workspace(ada, "Acme DE").await;
    world.attach(acme.id, uk.id).await;
    world.attach(acme.id, de.id).await;
    world
        .grant(acme.id, grace, "admin", WorkspaceSelector::All)
        .await;
    world
        .grant(acme.id, linus, "member", WorkspaceSelector::only([uk.id]))
        .await;

    // Non-members get a 403; members read.
    world
        .as_user(ken)
        .get("/api/organizations/acme")
        .send()
        .await
        .assert_error(403, "You are not a member of this organization.");
    let member = world.as_user(linus);
    member
        .get("/api/organizations/acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("role", json!("member"))
        .assert_json_path("workspaces", json!([uk.id]));
    member
        .get("/api/organizations/acme/members")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.__type", json!("OrganizationMember"))
        .assert_json_path("data.0.user.name", json!("Ada"));
    member
        .get("/api/organizations/acme/workspaces")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.slug", json!("acme-uk"))
        .assert_json_path("data.0.role", json!("member"))
        .assert_json_path("data.0.organization_id", json!(acme.id));
    member
        .patch("/api/organizations/acme")
        .json(&json!({ "name": "Mine" }))
        .send()
        .await
        .assert_error(403, None);
    member
        .post("/api/organizations/acme/members")
        .json(&json!({ "user_id": ken }))
        .send()
        .await
        .assert_error(403, None);

    // An admin manages members, but not ownership or deletion.
    let admin = world.as_user(grace);
    admin
        .post("/api/organizations/acme/members")
        .json(&json!({ "user_id": ken, "role": "partner", "workspaces": [de.id] }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("role", json!("partner"))
        .assert_json_path("workspaces", json!([de.id]));
    admin
        .post("/api/organizations/acme/members")
        .json(&json!({ "user_id": ken }))
        .send()
        .await
        .assert_error(409, "The user is already a member of this organization.");
    admin
        .patch(&format!("/api/organizations/acme/members/{ken}"))
        .json(&json!({ "workspaces": "all" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("workspaces", json!("all"))
        .assert_json_path("role", json!("partner"));
    admin
        .patch(&format!("/api/organizations/acme/members/{ada}"))
        .json(&json!({ "role": "admin" }))
        .send()
        .await
        .assert_status(409);
    admin
        .patch(&format!("/api/organizations/acme/members/{ken}"))
        .json(&json!({ "role": "owner" }))
        .send()
        .await
        .assert_validation_error("role");
    admin
        .delete("/api/organizations/acme")
        .send()
        .await
        .assert_error(403, None);
    admin
        .post("/api/organizations/acme/transfer")
        .json(&json!({ "user_id": grace }))
        .send()
        .await
        .assert_error(403, None);
    admin
        .delete(&format!("/api/organizations/acme/members/{ken}"))
        .send()
        .await
        .assert_status(204);

    // The owner transfers; members leave; the owner can't.
    let owner = world.as_user(ada);
    owner
        .post("/api/organizations/acme/leave")
        .send()
        .await
        .assert_status(409);
    member
        .post("/api/organizations/acme/leave")
        .send()
        .await
        .assert_status(204);
    owner
        .post("/api/organizations/acme/transfer")
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
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let ken = world.user("Ken", "ken@example.com").await;
    let acme = world.organization(ada, "Acme").await;
    let uk = world.workspace(ada, "Acme UK").await;
    let de = world.workspace(ada, "Acme DE").await;
    world.attach(acme.id, uk.id).await;
    world.attach(acme.id, de.id).await;
    world
        .grant(acme.id, grace, "admin", WorkspaceSelector::only([uk.id]))
        .await;
    let admin = world.as_user(grace);

    // Not themselves to everything, nor others to more than they reach.
    admin
        .patch(&format!("/api/organizations/acme/members/{grace}"))
        .json(&json!({ "workspaces": "all" }))
        .send()
        .await
        .assert_error(403, "You can only grant workspaces you reach.");
    admin
        .post("/api/organizations/acme/members")
        .json(&json!({ "user_id": ken, "workspaces": [de.id] }))
        .send()
        .await
        .assert_error(403, "You can only grant workspaces you reach.");
    admin
        .post("/api/organizations/acme/members")
        .json(&json!({ "user_id": ken }))
        .send()
        .await
        .assert_error(403, "You can only grant workspaces you reach.");
    admin
        .post("/api/organizations/acme/members")
        .json(&json!({ "user_id": ken, "workspaces": [uk.id] }))
        .send()
        .await
        .assert_status(201);
    // Nor change members who reach more than they do.
    admin
        .delete(&format!("/api/organizations/acme/members/{ada}"))
        .send()
        .await
        .assert_error(403, "You can only grant workspaces you reach.");
    // And they see only their workspaces.
    admin
        .get("/api/organizations/acme/workspaces")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.__type", json!("Workspace"))
        .assert_json_path("data.0.id", json!(uk.id))
        .assert_json_path("data.0.organization_id", json!(acme.id))
        .assert_json_path("data.0.role", json!("admin"))
        .json_value();
    let listed = admin
        .get("/api/organizations/acme/workspaces")
        .send()
        .await
        .json_value();
    assert_eq!(listed["data"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn workspaces_are_attached_and_detached_over_http() {
    let world = World::with(org_builder()).await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let acme = world.organization(ada, "Acme").await;
    let mine = world.workspace(ada, "Acme UK").await;
    let theirs = world.workspace(grace, "Grace Co").await;
    world
        .grant(acme.id, grace, "admin", WorkspaceSelector::All)
        .await;
    let owner = world.as_user(ada);

    owner
        .post("/api/organizations/acme/workspaces")
        .json(&json!({ "workspace": "acme-uk" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("organization_id", json!(acme.id));
    // Attaching takes the workspace's owner.
    owner
        .post("/api/organizations/acme/workspaces")
        .json(&json!({ "workspace": theirs.id }))
        .send()
        .await
        .assert_error(
            403,
            "Only the workspace's owner can attach it to an organization.",
        );
    world
        .as_user(grace)
        .post("/api/organizations/acme/workspaces")
        .json(&json!({ "workspace": "grace-co" }))
        .send()
        .await
        .assert_status(200);
    owner
        .post("/api/organizations/acme/workspaces")
        .json(&json!({ "workspace": "nope" }))
        .send()
        .await
        .assert_error(404, "Workspace not found.");
    owner
        .get("/api/workspaces/grace-co")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("role", json!("owner"));
    owner
        .delete(&format!("/api/organizations/acme/workspaces/{}", mine.id))
        .send()
        .await
        .assert_status(204);
    owner
        .delete("/api/organizations/acme/workspaces/acme-uk")
        .send()
        .await
        .assert_error(404, "The workspace is not part of this organization.");
}

#[tokio::test]
async fn access_requests_over_http() {
    let world = World::with(org_builder()).await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let linus = world.user("Linus", "linus@example.com").await;
    let acme = world.organization(ada, "Acme").await;
    let uk = world.workspace(ada, "Acme UK").await;
    world.attach(acme.id, uk.id).await;

    let asking = world.as_user(grace);
    let request = asking
        .post("/api/organizations/acme/access-requests")
        .json(&json!({ "workspace_id": uk.id, "message": "UK lead" }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("__type", json!("AccessRequest"))
        .assert_json_path("status", json!("pending"))
        .json_value();
    let id = request["id"].as_str().unwrap().to_owned();
    asking
        .post("/api/organizations/acme/access-requests")
        .json(&json!({}))
        .send()
        .await
        .assert_error(409, "An access request is already pending.");
    // Requesters aren't members: they can't read the queue.
    asking
        .get("/api/organizations/acme/access-requests")
        .send()
        .await
        .assert_error(403, "You are not a member of this organization.");

    let owner = world.as_user(ada);
    owner
        .get("/api/organizations/acme/access-requests")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.user.email", json!("grace@example.com"))
        .assert_json_path("data.0.message", json!("UK lead"));
    owner
        .post(&format!(
            "/api/organizations/acme/access-requests/{id}/approve"
        ))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("request.status", json!("approved"))
        .assert_json_path("member.role", json!("member"))
        .assert_json_path("member.workspaces", json!([uk.id]));
    owner
        .post(&format!(
            "/api/organizations/acme/access-requests/{id}/decline"
        ))
        .send()
        .await
        .assert_error(409, "The access request has already been decided.");
    asking
        .get("/api/workspaces/acme-uk")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("role", json!("member"));

    let other = world
        .as_user(linus)
        .post("/api/organizations/acme/access-requests")
        .json(&json!({}))
        .send()
        .await
        .assert_status(201)
        .json_value();
    owner
        .post(&format!(
            "/api/organizations/acme/access-requests/{}/decline",
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
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let linus = world.user("Linus", "linus@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    world.join(acme.id, grace, "admin").await;
    world.join(acme.id, linus, "member").await;
    let owner = world.as_user(ada);

    owner
        .post("/api/workspaces/acme/partitions")
        .json(&json!({ "name": "acme.co.uk" }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("__type", json!("WorkspacePartition"))
        .assert_json_path("slug", json!("acme-co-uk"));
    let de = world.partition(acme.id, "acme.de").await;
    owner
        .patch("/api/workspaces/acme/partitions/acme-de")
        .json(&json!({ "name": "acme.de (EU)" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!("acme.de (EU)"));
    world
        .as_user(linus)
        .post("/api/workspaces/acme/partitions")
        .json(&json!({ "name": "x" }))
        .send()
        .await
        .assert_error(403, None);

    // Limit Linus to one partition; he lists only that one.
    let uk = world
        .partitions()
        .await
        .resolve(acme.id, "acme-co-uk")
        .await
        .unwrap();
    owner
        .put(&format!("/api/workspaces/acme/members/{linus}/partitions"))
        .json(&json!({ "partitions": [uk.id] }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("partitions", json!([uk.id]));
    owner
        .get(&format!("/api/workspaces/acme/members/{linus}/partitions"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("partitions", json!([uk.id]));
    world
        .as_user(linus)
        .get("/api/workspaces/acme/partitions")
        .send()
        .await
        .assert_status(200)
        .assert_json_path(
            "data",
            json!([{
                "__type": "WorkspacePartition",
                "id": uk.id,
                "workspace_id": acme.id,
                "name": "acme.co.uk",
                "slug": "acme-co-uk",
                "created_at": uk.created_at,
                "updated_at": uk.updated_at,
            }]),
        );

    // A partition-limited admin can't widen anyone, themselves included,
    // or manage partitions.
    world
        .limit_partitions(acme.id, grace, PartitionSelector::only([uk.id]))
        .await;
    let admin = world.as_user(grace);
    admin
        .put(&format!("/api/workspaces/acme/members/{grace}/partitions"))
        .json(&json!({ "partitions": "all" }))
        .send()
        .await
        .assert_error(403, "You can only grant partitions you see.");
    admin
        .put(&format!("/api/workspaces/acme/members/{linus}/partitions"))
        .json(&json!({ "partitions": [de.id] }))
        .send()
        .await
        .assert_error(403, "You can only grant partitions you see.");
    admin
        .delete(&format!("/api/workspaces/acme/partitions/{}", uk.id))
        .send()
        .await
        .assert_error(
            403,
            "Only members who see every partition can manage partitions.",
        );

    owner
        .delete(&format!("/api/workspaces/acme/partitions/{}", de.id))
        .send()
        .await
        .assert_status(204);
    owner
        .delete(&format!("/api/workspaces/acme/partitions/{}", de.id))
        .send()
        .await
        .assert_error(404, "Partition not found.");
}

#[tokio::test]
async fn the_routes_are_off_with_the_features() {
    let world = World::with(builder()).await;
    let ada = world.user("Ada", "ada@example.com").await;
    world.workspace(ada, "Acme").await;
    let client = world.as_user(ada);
    client
        .get("/api/organizations")
        .send()
        .await
        .assert_status(404);
    client
        .get("/api/workspaces/acme/partitions")
        .send()
        .await
        .assert_status(404);
    // Workspace resources still carry `organization_id`.
    client
        .get("/api/workspaces/acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("organization_id", json!(null));
}
