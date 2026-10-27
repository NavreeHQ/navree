//! The operations routes, their SSE stream and the private channel
//! `operations.{id}`.

use std::sync::Arc;
use std::time::Duration;

use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::Secret;
use navree_db::{DatabaseModule, Db};
use navree_http::{HttpModule, route_table};
use navree_kernel::Navree;
use navree_queue::{Operation, OperationRecord, QueueDriver, QueueModule};
use navree_realtime::{RealtimeModule, RealtimeOptions};
use navree_testing::{principal, temp_sqlite_url};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{Grant, GrantsModule, Harness, harness_using, one_worker};

async fn app_with_grants(grants: Vec<Arc<Grant>>) -> Harness {
    harness_using(one_worker(QueueDriver::Memory), |builder| {
        builder
            .config("http.per_ip_rate_limit", Value::Null)
            .module(GrantsModule(grants))
    })
    .await
}

async fn start(h: &Harness, operation: Operation) -> OperationRecord {
    let db = h.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let record = h.operations.start(&uow, operation).await.unwrap();
    uow.commit().await.unwrap();
    record
}

#[tokio::test]
async fn the_owner_and_permission_holders_see_an_operation() {
    let owner = Uuid::now_v7();
    let viewer = Uuid::now_v7();
    let stranger = Uuid::now_v7();
    let h = app_with_grants(vec![Arc::new(Grant {
        user: viewer,
        permission: "operations.view",
        workspace: None,
    })])
    .await;
    let op = start(&h, Operation::new("deals.export").owner(owner).total(10)).await;
    h.operations
        .progress(op.id, 4, 10, "Exporting")
        .await
        .unwrap();
    let path = format!("/api/operations/{}", op.id);

    let body = h
        .app
        .acting_as(principal(owner))
        .get(&path)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("status", json!("running"))
        .assert_json_path("kind", json!("deals.export"))
        .assert_json_path(
            "progress",
            json!({"done": 4, "total": 10, "percent": 40, "note": "Exporting"}),
        )
        .json_value();
    assert_eq!(body["id"], json!(op.id));
    assert_eq!(body["owner_id"], json!(owner));
    assert_eq!(body["result"], Value::Null);
    for key in [
        "workspace_id",
        "batch_id",
        "error",
        "created_at",
        "started_at",
        "heartbeat_at",
        "finished_at",
        "updated_at",
    ] {
        assert!(body.get(key).is_some(), "{key} missing from {body}");
    }

    h.app
        .acting_as(principal(viewer))
        .get(&path)
        .send()
        .await
        .assert_status(200);
    // Everyone else gets a 404, as for an operation that does not exist.
    h.app
        .acting_as(principal(stranger))
        .get(&path)
        .send()
        .await
        .assert_error(404, None);
    h.app
        .acting_as(principal(owner))
        .get(&format!("/api/operations/{}", Uuid::now_v7()))
        .send()
        .await
        .assert_error(404, None);
    h.app
        .acting_as(principal(owner))
        .get("/api/operations/not-an-id")
        .send()
        .await
        .assert_error(404, None);
    h.app
        .client()
        .get(&path)
        .send()
        .await
        .assert_error(401, None);
}

#[tokio::test]
async fn the_index_lists_the_users_own_operations() {
    let owner = Uuid::now_v7();
    let h = app_with_grants(Vec::new()).await;
    let first = start(&h, Operation::new("first").owner(owner)).await;
    h.advance(1);
    let second = start(&h, Operation::new("second").owner(owner)).await;
    start(&h, Operation::new("other").owner(Uuid::now_v7())).await;

    let body = h
        .app
        .acting_as(principal(owner))
        .get("/api/operations")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let ids: Vec<&Value> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|op| &op["id"])
        .collect();
    assert_eq!(ids, [&json!(second.id), &json!(first.id)]);

    let body = h
        .app
        .acting_as(principal(owner))
        .get("/api/operations")
        .query(&[("limit", "1")])
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn cancel_needs_the_owner_or_the_cancel_permission() {
    let owner = Uuid::now_v7();
    let viewer = Uuid::now_v7();
    let canceller = Uuid::now_v7();
    let h = app_with_grants(vec![
        Arc::new(Grant {
            user: viewer,
            permission: "operations.view",
            workspace: None,
        }),
        Arc::new(Grant {
            user: canceller,
            permission: "operations.cancel",
            workspace: None,
        }),
    ])
    .await;
    let op = start(&h, Operation::new("export").owner(owner)).await;
    let path = format!("/api/operations/{}", op.id);

    // May see it, may not cancel it.
    h.app
        .acting_as(principal(viewer))
        .delete(&path)
        .send()
        .await
        .assert_error(403, None);
    // May not even see it.
    h.app
        .acting_as(principal(canceller))
        .delete(&path)
        .send()
        .await
        .assert_error(404, None);
    h.app
        .acting_as(principal(owner))
        .delete(&path)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("status", json!("cancelled"));
    h.app
        .acting_as(principal(owner))
        .delete(&path)
        .send()
        .await
        .assert_error(409, "The operation has already finished.")
        .assert_error_code(409, "operation_finished");
}

#[tokio::test]
async fn the_event_stream_sends_the_operation_until_it_finishes() {
    let owner = Uuid::now_v7();
    let h = app_with_grants(Vec::new()).await;

    // A finished operation: one event, then the stream ends.
    let done = start(&h, Operation::new("export").owner(owner)).await;
    h.operations
        .complete(done.id, json!({"url": "/exports/1.csv"}))
        .await
        .unwrap();
    let response = h
        .app
        .acting_as(principal(owner))
        .get(&format!("/api/operations/{}/events", done.id))
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        response.header("content-type"),
        Some("text/event-stream"),
        "{:?}",
        response.headers()
    );
    let finished = events(&response.text());
    assert_eq!(finished.len(), 1, "{finished:?}");
    assert_eq!(finished[0]["status"], "succeeded");
    assert_eq!(finished[0]["result"], json!({"url": "/exports/1.csv"}));

    // A running operation: an event now and after each change.
    let live = start(&h, Operation::new("export").owner(owner).total(2)).await;
    let operations = h.operations.clone();
    let id = live.id;
    let worker = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        operations.progress(id, 1, 2, "Half").await.unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        operations.complete(id, json!({"rows": 2})).await.unwrap();
    });
    let response = tokio::time::timeout(
        Duration::from_secs(10),
        h.app
            .acting_as(principal(owner))
            .get(&format!("/api/operations/{id}/events"))
            .send(),
    )
    .await
    .expect("the stream ends once the operation finishes")
    .assert_status(200);
    worker.await.unwrap();
    let live = events(&response.text());
    let statuses: Vec<&str> = live
        .iter()
        .map(|event| event["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["pending", "running", "succeeded"]);
    assert_eq!(live[1]["progress"]["percent"], 50);

    // Not visible: a 404 before any event.
    h.app
        .acting_as(principal(Uuid::now_v7()))
        .get(&format!("/api/operations/{id}/events"))
        .send()
        .await
        .assert_error(404, None);
}

/// The `data:` payloads of the `operation` events in an SSE body.
fn events(body: &str) -> Vec<Value> {
    body.split("\n\n")
        .filter(|block| block.lines().any(|line| line == "event: operation"))
        .map(|block| {
            let data: String = block
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .collect();
            serde_json::from_str(&data).unwrap_or_else(|e| panic!("{e}: {block}"))
        })
        .collect()
}

#[tokio::test]
async fn routes_can_be_turned_off() {
    let owner = Uuid::now_v7();
    let mut options = one_worker(QueueDriver::Memory);
    options.operations.routes = false;
    let h = harness_using(options, |builder| builder).await;
    let op = start(&h, Operation::new("export").owner(owner)).await;
    h.app
        .acting_as(principal(owner))
        .get(&format!("/api/operations/{}", op.id))
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn routes_need_an_authenticator_and_the_database() {
    let paths = |routes: Vec<navree_http::RouteMeta>| -> Vec<String> {
        routes
            .into_iter()
            .map(|route| route.full_path)
            .filter(|path| path.starts_with("/api/operations"))
            .collect()
    };
    // No authenticator: nothing is mounted, and the app boots.
    let (url, _guard) = temp_sqlite_url();
    let app = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new()
                .set("database.url", url)
                .set("database.migrate_on_boot", true),
        ))
        .module(HttpModule)
        .module(DatabaseModule)
        .module(QueueModule::from_config())
        .boot()
        .await
        .expect("boots");
    assert!(paths(route_table(app.ctx()).await.unwrap()).is_empty());

    // The test app has one.
    let h = app_with_grants(Vec::new()).await;
    let mut mounted = paths(route_table(h.app.ctx()).await.unwrap());
    mounted.sort();
    mounted.dedup();
    assert_eq!(
        mounted,
        [
            "/api/operations",
            "/api/operations/{id}",
            "/api/operations/{id}/events"
        ]
    );
}

#[tokio::test]
async fn the_private_channel_is_for_those_who_may_see_the_operation() {
    let owner = Uuid::now_v7();
    let stranger = Uuid::now_v7();
    let h = harness_using(one_worker(QueueDriver::Memory), |builder| {
        builder.module(RealtimeModule::for_root(RealtimeOptions {
            app_id: "test".to_owned(),
            key: "test-key".to_owned(),
            secret: Secret::new("test-secret".to_owned()),
            ..RealtimeOptions::default()
        }))
    })
    .await;
    let op = start(&h, Operation::new("export").owner(owner)).await;
    let form = [
        ("socket_id", "1234.5678".to_owned()),
        ("channel_name", op.channel()),
    ];

    let reply = h
        .app
        .acting_as(principal(owner))
        .post("/broadcasting/auth")
        .form(&form)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert!(
        reply["auth"].as_str().unwrap().starts_with("test-key:"),
        "{reply}"
    );
    h.app
        .acting_as(principal(stranger))
        .post("/broadcasting/auth")
        .form(&form)
        .send()
        .await
        .assert_status(403);
    let unknown = [
        ("socket_id", "1234.5678".to_owned()),
        (
            "channel_name",
            format!("private-operations.{}", Uuid::now_v7()),
        ),
    ];
    h.app
        .acting_as(principal(owner))
        .post("/broadcasting/auth")
        .form(&unknown)
        .send()
        .await
        .assert_status(403);
}
