//! The delivery log is time series: `webhook_deliveries` and
//! `webhook_attempts` are TimescaleDB hypertables on `created_at` (see the
//! facade's `time_series` suite), regular tables with their `created_at`
//! indexes here. Attempts reference their endpoint, not their delivery, so
//! both tables can be hypertables; pruning takes a delivery's attempts with
//! it.

use std::time::Duration;

use chrono::TimeDelta;
use http::Method;
use navree_db::navree_schema::{IndexKind, ReferentialAction, Schema};
use navree_db::{Db, MIGRATIONS_TABLE};
use navree_http_client::FakeResponse;
use navree_webhooks::NewEndpoint;
use navree_webhooks::entity::attempt;
use sea_orm::{ActiveModelTrait, ConnectionTrait, EntityTrait, PaginatorTrait, Set, Statement};
use uuid::Uuid;

use crate::support::{deal_won, harness, start};

const URL: &str = "https://hooks.lovelace.io/navree";

#[tokio::test]
async fn the_delivery_log_is_declared_time_series() {
    let h = harness(|b| b).await;
    let db = h.app.get::<Db>().await;
    let applied: Vec<String> = db
        .conn()
        .query_all_raw(Statement::from_string(
            db.backend(),
            format!("SELECT name FROM {MIGRATIONS_TABLE} WHERE module = 'webhooks' ORDER BY name"),
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .collect();
    assert_eq!(
        applied,
        [
            "m20261026_000001_create_webhook_tables",
            "m20261027_000001_convert_webhook_deliveries_to_hypertables"
        ]
    );
    let schema = Schema::new(&db);
    let foreign_keys = schema.foreign_keys("webhook_attempts").await.unwrap();
    assert_eq!(foreign_keys.len(), 1, "{foreign_keys:?}");
    assert_eq!(foreign_keys[0].columns, ["endpoint_id"]);
    assert_eq!(foreign_keys[0].foreign_table, "webhook_endpoints");
    assert_eq!(foreign_keys[0].foreign_columns, ["id"]);
    assert_eq!(foreign_keys[0].on_delete, Some(ReferentialAction::Cascade));
    let deliveries = schema.foreign_keys("webhook_deliveries").await.unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].foreign_table, "webhook_endpoints");
    for table in ["webhook_deliveries", "webhook_attempts"] {
        assert!(!schema.is_hypertable(table).await.unwrap());
        assert!(
            schema
                .has_index(table, ["created_at"], Some(IndexKind::Index))
                .await
                .unwrap(),
            "{table} is indexed by time"
        );
    }
    assert!(
        schema
            .has_index("webhook_attempts", ["delivery_id"], Some(IndexKind::Index))
            .await
            .unwrap(),
        "a delivery's attempts are still found by its id"
    );
}

#[tokio::test]
async fn pruning_takes_a_delivery_s_attempts_however_late() {
    let h = harness(|b| b).await;
    h.server.on(
        Method::POST,
        "https://hooks.lovelace.io/*",
        FakeResponse::new(200),
    );
    let created = h.endpoint(NewEndpoint::new(URL).events(["*"])).await;
    h.emit(deal_won(None)).await;
    h.run_queue().await;
    let old = h.last_delivery(created.endpoint.id).await;
    // An attempt made nine days after its delivery, as a late retry is.
    let db = h.app.get::<Db>().await;
    attempt::ActiveModel {
        id: Set(Uuid::now_v7()),
        delivery_id: Set(old.id),
        endpoint_id: Set(created.endpoint.id),
        attempt: Set(2),
        status_code: Set(Some(200)),
        succeeded: Set(true),
        response_body: Set(None),
        error: Set(None),
        duration_ms: Set(12),
        created_at: Set(start() + TimeDelta::days(9)),
    }
    .insert(db.conn())
    .await
    .unwrap();
    h.clock.advance(Duration::from_secs(10 * 86_400));
    h.emit(deal_won(None)).await;
    h.run_queue().await;
    let new = h.last_delivery(created.endpoint.id).await;
    assert_eq!(attempt::Entity::find().count(db.conn()).await.unwrap(), 3);

    // A week back from day 10: the first delivery goes, with both its
    // attempts (the second one is newer than the cutoff).
    let pruned = h.webhooks().await.prune(Some(7)).await.unwrap();
    assert_eq!(pruned.deliveries, 1);
    let webhooks = h.webhooks().await;
    assert!(webhooks.delivery(old.id).await.is_err());
    assert!(webhooks.attempts(old.id).await.unwrap().is_empty());
    assert_eq!(webhooks.attempts(new.id).await.unwrap().len(), 1);
    assert_eq!(attempt::Entity::find().count(db.conn()).await.unwrap(), 1);
}

#[tokio::test]
async fn deleting_an_endpoint_deletes_its_deliveries_and_attempts() {
    let h = harness(|b| b).await;
    h.server.on(
        Method::POST,
        "https://hooks.lovelace.io/*",
        FakeResponse::new(200),
    );
    let created = h.endpoint(NewEndpoint::new(URL).events(["*"])).await;
    let kept = h
        .endpoint(NewEndpoint::new("https://hooks.lovelace.io/other").events(["*"]))
        .await;
    h.emit(deal_won(None)).await;
    h.run_queue().await;
    let db = h.app.get::<Db>().await;
    assert_eq!(attempt::Entity::find().count(db.conn()).await.unwrap(), 2);

    let uow = db.uow(None, None).await;
    h.webhooks()
        .await
        .delete_endpoint(&uow, created.endpoint.id)
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let left = attempt::Entity::find().all(db.conn()).await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].endpoint_id, kept.endpoint.id);
    assert_eq!(
        navree_webhooks::entity::delivery::Entity::find()
            .count(db.conn())
            .await
            .unwrap(),
        1
    );
}
