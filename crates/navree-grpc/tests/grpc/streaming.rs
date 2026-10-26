//! Server, client and bidirectional streaming; streams end with
//! `UNAVAILABLE` on shutdown and with `DEADLINE_EXCEEDED` at the client's
//! deadline.

use std::time::Duration;

use navree_grpc::Code;
use navree_grpc::testing::TestChannel;
use navree_testing::principal;
use uuid::Uuid;

use crate::support::pb::deals::deals_client::DealsClient;
use crate::support::pb::deals::{Deal, DealEvent, MoveDealRequest, WatchDealsRequest};
use crate::support::{deals_app, insert_deal};

#[tokio::test]
async fn server_streams_carry_events_and_check_access_when_they_open() {
    let app = deals_app().build().await;
    let rep = Uuid::now_v7();
    let deal = insert_deal(&app, "Acme", rep).await;

    let mut guest = DealsClient::new(TestChannel::new(&app));
    let status = guest.watch_deals(WatchDealsRequest {}).await.unwrap_err();
    assert_eq!(status.code(), Code::Unauthenticated);

    let mut owner = DealsClient::new(TestChannel::new(&app).acting_as(principal(rep)));
    let mut events = owner
        .watch_deals(WatchDealsRequest {})
        .await
        .unwrap()
        .into_inner();
    owner
        .move_deal(MoveDealRequest {
            id: deal.id.to_string(),
            stage: "won".into(),
            position: None,
            lost_reason: None,
        })
        .await
        .unwrap();
    let event = tokio::time::timeout(Duration::from_secs(5), events.message())
        .await
        .expect("an event arrives")
        .unwrap()
        .unwrap();
    assert_eq!(
        event,
        DealEvent {
            deal_id: deal.id.to_string(),
            stage: "won".into(),
        }
    );
}

#[tokio::test]
async fn client_and_bidirectional_streams() {
    let app = deals_app().build().await;
    let mut client = DealsClient::new(TestChannel::new(&app).acting_as(principal(Uuid::now_v7())));

    let deals = (0..3).map(|i| Deal {
        title: format!("Deal {i}"),
        ..Deal::default()
    });
    let imported = client
        .import_deals(tokio_stream::iter(deals))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(imported.imported, 3);

    let status = client
        .import_deals(tokio_stream::iter([Deal::default()]))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);

    let sent = vec![
        DealEvent {
            deal_id: "1".into(),
            stage: "won".into(),
        },
        DealEvent {
            deal_id: "2".into(),
            stage: "lost".into(),
        },
    ];
    let mut echoed = client
        .echo(tokio_stream::iter(sent.clone()))
        .await
        .unwrap()
        .into_inner();
    let mut received = Vec::new();
    while let Some(event) = echoed.message().await.unwrap() {
        received.push(event);
    }
    assert_eq!(received, sent);
}

#[tokio::test]
async fn open_streams_end_with_unavailable_on_shutdown() {
    let app = deals_app().build().await;
    let mut owner = DealsClient::new(TestChannel::new(&app).acting_as(principal(Uuid::now_v7())));
    let mut events = owner
        .watch_deals(WatchDealsRequest {})
        .await
        .unwrap()
        .into_inner();

    app.ctx().shutdown_token().cancel();
    let ended = tokio::time::timeout(Duration::from_secs(5), events.message())
        .await
        .expect("the stream ends");
    let status = ended.unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);
    assert_eq!(status.message(), "the server is shutting down");
}

#[tokio::test]
async fn streams_end_at_the_clients_deadline() {
    let app = deals_app().build().await;
    let mut owner = DealsClient::new(TestChannel::new(&app).acting_as(principal(Uuid::now_v7())));
    let mut request = tonic::Request::new(WatchDealsRequest {});
    request.set_timeout(Duration::from_millis(200));
    let mut events = owner.watch_deals(request).await.unwrap().into_inner();
    let ended = tokio::time::timeout(Duration::from_secs(5), events.message())
        .await
        .expect("the stream ends");
    assert_eq!(ended.unwrap_err().code(), Code::DeadlineExceeded);
}
