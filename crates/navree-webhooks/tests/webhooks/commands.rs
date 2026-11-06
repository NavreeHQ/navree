//! The commands: `webhooks:endpoints`, `webhooks:test`, `webhooks:retry`
//! and `webhooks:prune`.

use std::time::Duration;

use http::Method;
use navree_http_client::FakeResponse;
use navree_webhooks::{DeliveryStatus, NewEndpoint};
use uuid::Uuid;

use crate::support::{deal_won, harness};

const URL: &str = "https://hooks.lakhotia.io/navree";

#[tokio::test]
async fn endpoints_and_test_commands_run() {
    let h = harness(|b| b).await;
    h.server.on(
        Method::POST,
        "https://hooks.lakhotia.io/*",
        FakeResponse::new(200),
    );
    let created = h.endpoint(NewEndpoint::new(URL).events(["*"])).await;

    h.app.run_command("webhooks:endpoints", &[]).await.unwrap();
    h.app
        .run_command("webhooks:test", &[&created.endpoint.id.to_string()])
        .await
        .unwrap();
    h.server.assert_sent_count(1);

    assert!(h.app.run_command("webhooks:test", &[]).await.is_err());
    assert!(h.app.run_command("webhooks:test", &["nope"]).await.is_err());
    let unknown = h
        .app
        .run_command("webhooks:test", &[&Uuid::now_v7().to_string()])
        .await
        .unwrap_err();
    assert!(unknown.to_string().contains("not found"), "{unknown}");
    // `--workspace` needs WorkspaceModule.
    assert!(
        h.app
            .run_command("webhooks:endpoints", &["--workspace", "agra"])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_failing_test_delivery_fails_the_command() {
    let h = harness(|b| b).await;
    h.server.on(
        Method::POST,
        "https://hooks.lakhotia.io/*",
        FakeResponse::new(500),
    );
    let created = h.endpoint(NewEndpoint::new(URL).events(["*"])).await;
    assert!(
        h.app
            .run_command("webhooks:test", &[&created.endpoint.id.to_string()])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn retry_queues_one_or_every_failed_delivery() {
    let h = harness(|b| b).await;
    h.server.on_sequence(
        Method::POST,
        "https://hooks.lakhotia.io/*",
        [
            FakeResponse::new(410),
            FakeResponse::new(410),
            FakeResponse::new(200),
        ],
    );
    let created = h.endpoint(NewEndpoint::new(URL).events(["*"])).await;
    h.emit(deal_won(None)).await;
    h.run_queue().await;
    let first = h.last_delivery(created.endpoint.id).await;
    assert_eq!(first.status, DeliveryStatus::Failed);

    // Re-enable, then retry it by id: it fails again (410).
    let uow = h.app.get::<navree_db::Db>().await.uow(None, None).await;
    h.webhooks()
        .await
        .enable(&uow, created.endpoint.id)
        .await
        .unwrap();
    uow.commit().await.unwrap();
    h.app
        .run_command("webhooks:retry", &[&first.id.to_string()])
        .await
        .unwrap();
    h.run_queue().await;
    h.server.assert_sent_count(2);

    let uow = h.app.get::<navree_db::Db>().await.uow(None, None).await;
    h.webhooks()
        .await
        .enable(&uow, created.endpoint.id)
        .await
        .unwrap();
    uow.commit().await.unwrap();
    h.app
        .run_command("webhooks:retry", &["failed"])
        .await
        .unwrap();
    h.run_queue().await;
    h.server.assert_sent_count(3);
    assert_eq!(
        h.webhooks().await.delivery(first.id).await.unwrap().status,
        DeliveryStatus::Succeeded
    );
    assert!(h.app.run_command("webhooks:retry", &[]).await.is_err());
}

#[tokio::test]
async fn prune_deletes_old_deliveries_and_receipts() {
    let h = harness(|b| b).await;
    h.server.on(
        Method::POST,
        "https://hooks.lakhotia.io/*",
        FakeResponse::new(200),
    );
    let created = h.endpoint(NewEndpoint::new(URL).events(["*"])).await;
    h.emit(deal_won(None)).await;
    h.run_queue().await;
    let old = h.last_delivery(created.endpoint.id).await;

    h.clock.advance(Duration::from_secs(10 * 86_400));
    h.emit(deal_won(None)).await;
    h.run_queue().await;

    h.app
        .run_command("webhooks:prune", &["--days", "7"])
        .await
        .unwrap();
    let webhooks = h.webhooks().await;
    assert!(webhooks.delivery(old.id).await.is_err());
    let page = webhooks
        .deliveries(created.endpoint.id, Default::default())
        .await
        .unwrap();
    assert_eq!(page.meta.total, 1);

    // The default keeps `webhooks.retain_days` (30).
    h.app.run_command("webhooks:prune", &[]).await.unwrap();
    assert_eq!(
        webhooks
            .deliveries(created.endpoint.id, Default::default())
            .await
            .unwrap()
            .meta
            .total,
        1
    );
    assert!(
        h.app
            .run_command("webhooks:prune", &["--days", "soon"])
            .await
            .is_err()
    );
}
