//! The `/api/directory` routes: connections, SCIM tokens, syncs queued as
//! operations, runs, changes, synced users and groups, approvals, and
//! their refusals.

use navree_directory::{DirectoryGroup, DirectoryUser, RunStatus};
use navree_queue::{OperationStatus, Operations, Queue, QueueDriver, QueueModule, QueueOptions};
use navree_testing::TestAppBuilder;
use serde_json::json;
use uuid::Uuid;

use crate::support::{ADMIN, Harness, OTHER, harness, principal, with_routes};

fn with_queue(builder: TestAppBuilder) -> TestAppBuilder {
    with_routes(builder).module(QueueModule::for_root(QueueOptions {
        driver: QueueDriver::Database,
        ..QueueOptions::default()
    }))
}

async fn run_queue(h: &Harness) {
    let stats = h.app.get::<Queue>().await.run_pending().await.unwrap();
    assert_eq!(stats.failed, 0, "{stats:?}");
}

#[tokio::test]
async fn admins_manage_scim_connections() {
    let h = harness(with_queue).await;
    let admin = h.app.acting_as(principal(ADMIN));
    admin
        .get("/api/directory/sources")
        .send()
        .await
        .assert_status(200)
        .assert_json_path(
            "data",
            json!(["scim", "google", "ldap", "microsoft", "okta"]),
        );

    let created = admin
        .post("/api/directory/connections")
        .json(&json!({ "name": "Agra Entra ID", "source": "scim" }))
        .send()
        .await
        .assert_status(201);
    let body = created.json_value();
    let id = body["connection"]["id"].as_str().unwrap().to_owned();
    let token = body["scim_token"].as_str().unwrap().to_owned();
    assert!(token.starts_with("nvscim_"));
    assert_eq!(body["scim_url"], json!("/scim/v2"));
    assert_eq!(body["connection"]["created_by"], json!(ADMIN));
    assert_eq!(body["connection"]["sync_interval_mins"], json!(0));
    assert!(body["connection"].get("secret").is_none());
    assert_eq!(
        created.header("location"),
        Some(format!("/api/directory/connections/{id}").as_str())
    );

    admin
        .get("/api/directory/connections")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.0.id", json!(id))
        .assert_json_path("data.0.scim_token_hint", json!(&token[..11]));
    admin
        .patch(&format!("/api/directory/connections/{id}"))
        .json(&json!({ "name": "Entra ID" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("name", json!("Entra ID"));

    let rotated = admin
        .post(&format!("/api/directory/connections/{id}/token"))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let fresh = rotated["token"].as_str().unwrap();
    assert_ne!(fresh, token);
    h.app
        .client()
        .get("/scim/v2/Users")
        .bearer(fresh)
        .send()
        .await
        .assert_status(200);

    admin
        .post(&format!("/api/directory/connections/{id}/sync"))
        .send()
        .await
        .assert_error_code(400, "directory_push_only");
    admin
        .delete(&format!("/api/directory/connections/{id}"))
        .send()
        .await
        .assert_status(204);
    admin
        .get(&format!("/api/directory/connections/{id}"))
        .send()
        .await
        .assert_error_code(404, "directory_connection_not_found");
    h.app
        .client()
        .get("/scim/v2/Users")
        .bearer(fresh)
        .send()
        .await
        .assert_status(401);
}

#[tokio::test]
async fn only_managers_reach_the_routes() {
    let h = harness(with_queue).await;
    h.app
        .acting_as(principal(OTHER))
        .get("/api/directory/connections")
        .send()
        .await
        .assert_status(403);
    h.app
        .client()
        .get("/api/directory/connections")
        .send()
        .await
        .assert_status(401);
}

#[tokio::test]
async fn bad_input_gets_its_code() {
    let h = harness(with_queue).await;
    let admin = h.app.acting_as(principal(ADMIN));
    admin
        .post("/api/directory/connections")
        .json(&json!({ "name": "x", "source": "Nope" }))
        .send()
        .await
        .assert_error_code(400, "unknown_directory_source");
    admin
        .post("/api/directory/connections")
        .json(&json!({ "name": "", "source": "ldap" }))
        .send()
        .await
        .assert_error_code(400, "invalid_directory_settings");
    admin
        .get(&format!("/api/directory/runs/{}", Uuid::now_v7()))
        .send()
        .await
        .assert_error_code(404, "directory_run_not_found");
}

#[tokio::test]
async fn syncs_run_as_operations_and_report_their_changes() {
    let h = harness(with_queue).await;
    h.fake()
        .user(DirectoryUser::new("u1", "anu@acme.test").email("anu@acme.test"))
        .user(DirectoryUser::new("u2", "gauri@acme.test"))
        .group(DirectoryGroup::new("g1", "Engineering").members(["u1", "u2"]));
    let admin = h.app.acting_as(principal(ADMIN));
    let id = admin
        .post("/api/directory/connections")
        .json(&json!({ "name": "Agra AD", "source": "ldap", "sync_interval_mins": 30 }))
        .send()
        .await
        .assert_status(201)
        .json_value()["connection"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let queued = admin
        .post(&format!("/api/directory/connections/{id}/sync"))
        .json(&json!({ "dry_run": true }))
        .send()
        .await
        .assert_status(202)
        .assert_json_path("status", json!("queued"))
        .assert_json_path("dry_run", json!(true))
        .json_value();
    let run_id = queued["id"].as_str().unwrap().to_owned();
    let operation: Uuid = queued["operation_id"].as_str().unwrap().parse().unwrap();
    admin
        .post(&format!("/api/directory/connections/{id}/sync"))
        .send()
        .await
        .assert_error_code(409, "directory_sync_running");

    run_queue(&h).await;
    let operations = h.app.get::<Operations>().await;
    let record = operations.find(operation).await.unwrap().unwrap();
    assert_eq!(record.status, OperationStatus::Succeeded, "{record:?}");
    assert_eq!(record.kind, "directory.sync");
    assert_eq!(record.owner_id, Some(ADMIN));
    admin
        .get(&format!("/api/directory/runs/{run_id}"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("status", json!("succeeded"))
        .assert_json_path("summary.users_created", json!(2));
    admin
        .get(&format!("/api/directory/runs/{run_id}/changes?per_page=2"))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("meta.total", json!(3))
        .assert_json_path("meta.last_page", json!(2))
        .assert_json_path("data.0.status", json!("planned"))
        .assert_json_path("data.0.label", json!("anu@acme.test"));

    // The real sync, through the queue.
    let run = admin
        .post(&format!("/api/directory/connections/{id}/sync"))
        .send()
        .await
        .assert_status(202)
        .json_value();
    run_queue(&h).await;
    let run_id = run["id"].as_str().unwrap();
    let operation: Uuid = run["operation_id"].as_str().unwrap().parse().unwrap();
    let record = operations.find(operation).await.unwrap().unwrap();
    assert_eq!(record.progress.percent, 100, "{record:?}");
    admin
        .get(&format!("/api/directory/connections/{id}/runs"))
        .send()
        .await
        .assert_json_path("data.0.id", json!(run_id))
        .assert_json_path("data.0.done", json!(3));
    admin
        .get(&format!(
            "/api/directory/connections/{id}/users?per_page=1&page=2"
        ))
        .send()
        .await
        .assert_status(200)
        .assert_json_path(
            "meta",
            json!({ "current_page": 2, "per_page": 1, "total": 2, "last_page": 2 }),
        )
        .assert_json_path("data.0.user.user_name", json!("gauri@acme.test"))
        .assert_json_path("data.0.local_id", json!("local-u2"));
    admin
        .get(&format!("/api/directory/connections/{id}/groups"))
        .send()
        .await
        .assert_json_path("data.0.group.members", json!(["u1", "u2"]));
    assert_eq!(h.log.calls().len(), 3);
}

#[tokio::test]
async fn a_held_run_is_approved() {
    let h = harness(with_queue).await;
    let people: Vec<DirectoryUser> = (0..20)
        .map(|n| DirectoryUser::new(format!("u{n}"), format!("p{n}@acme.test")))
        .collect();
    h.fake().users(people.clone());
    let id = h.connection("ldap").await;
    let directory = h.directory().await;
    directory
        .sync_now(id, navree_directory::SyncOptions::default())
        .await
        .unwrap();
    h.fake().users(people[..5].to_vec());
    let held = directory
        .sync_now(id, navree_directory::SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(held.status, RunStatus::Held);

    let admin = h.app.acting_as(principal(ADMIN));
    let approved = admin
        .post(&format!("/api/directory/runs/{}/approve", held.id))
        .send()
        .await
        .assert_status(202)
        .assert_json_path("force", json!(true))
        .json_value();
    run_queue(&h).await;
    let run = directory
        .run(approved["id"].as_str().unwrap().parse().unwrap())
        .await
        .unwrap();
    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(run.summary.users_deactivated, 15);
    admin
        .post(&format!("/api/directory/runs/{}/approve", run.id))
        .send()
        .await
        .assert_error_code(409, "directory_run_not_held");
}

#[tokio::test]
async fn the_commands_sync_list_and_rotate() {
    let h = harness(with_queue).await;
    h.fake().user(DirectoryUser::new("u1", "anu@acme.test"));
    let id = h.connection("ldap").await;
    h.app
        .run_command("directory:sync", &[&id.to_string(), "--dry-run"])
        .await
        .unwrap();
    h.app
        .run_command("directory:sync", &["--all"])
        .await
        .unwrap();
    assert_eq!(h.directory().await.users(id).await.unwrap().len(), 1);
    h.app
        .run_command("directory:sync", &["--due"])
        .await
        .unwrap();
    h.app.run_command("directory:runs", &[]).await.unwrap();
    h.app
        .run_command("directory:runs", &[&id.to_string(), "--limit", "1"])
        .await
        .unwrap();
    h.app
        .run_command("directory:connections", &[])
        .await
        .unwrap();
    let scim = h.scim(navree_directory::Owner::App).await;
    h.app
        .run_command("directory:token", &[&scim.connection.id.to_string()])
        .await
        .unwrap();
    assert!(h.app.run_command("directory:sync", &[]).await.is_err());
    assert!(
        h.app
            .run_command("directory:sync", &["--nope"])
            .await
            .is_err()
    );
    assert!(h.app.run_command("directory:token", &[]).await.is_err());
    h.fake().fail_next_pull("down");
    assert!(
        h.app
            .run_command("directory:sync", &[&id.to_string()])
            .await
            .is_err()
    );
    let runs = h.directory().await.runs(id, 10).await.unwrap();
    assert_eq!(runs[0].status, RunStatus::Failed);
    assert_eq!(runs[0].trigger, navree_directory::RunTrigger::Command);
}
