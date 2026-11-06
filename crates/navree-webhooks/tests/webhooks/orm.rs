//! The webhook tables as navree-orm models: endpoints, the delivery log
//! and its attempts read back as models, queried with typed columns.

use http::Method;
use navree_db::Db;
use navree_http_client::FakeResponse;
use navree_orm::Model as _;
use navree_webhooks::NewEndpoint;
use navree_webhooks::entity::{attempt, delivery, endpoint};

use crate::support::{deal_won, harness};

#[tokio::test]
async fn endpoints_deliveries_and_attempts_are_orm_models() {
    let h = harness(|b| b).await;
    h.server.on(
        Method::POST,
        "https://hooks.lakhotia.io/*",
        FakeResponse::new(204),
    );
    let created = h
        .endpoint(NewEndpoint::new("https://hooks.lakhotia.io/navree").events(["deal.won"]))
        .await;
    h.emit(deal_won(None)).await;
    h.run_queue().await;
    let db = h.app.get::<Db>().await;

    let found = endpoint::Model::find(&db, created.endpoint.id)
        .await
        .unwrap()
        .expect("the endpoint");
    assert_eq!(found.url, "https://hooks.lakhotia.io/navree");
    assert!(
        found.to_value().unwrap().get("secret").is_none(),
        "secrets never serialize"
    );
    assert_eq!(
        endpoint::Model::query()
            .where_(endpoint::Model::ENABLED.eq(true))
            .count(&db)
            .await
            .unwrap(),
        1
    );

    let deliveries = delivery::Model::query()
        .where_(delivery::Model::ENDPOINT_ID.eq(created.endpoint.id))
        .where_(delivery::Model::EVENT_TYPE.eq("deal.won"))
        .get(&db)
        .await
        .unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].status, "succeeded");
    let attempts = attempt::Model::query()
        .where_(attempt::Model::DELIVERY_ID.eq(deliveries[0].id))
        .get(&db)
        .await
        .unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].status_code, Some(204));
    assert!(attempts[0].succeeded);
}
