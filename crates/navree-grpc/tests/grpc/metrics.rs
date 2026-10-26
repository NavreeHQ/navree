//! Every finished call is counted in the app's metrics:
//! `grpc_server_handled_total` and `grpc_server_handling_seconds`, by
//! service, method and code.

use navree_grpc::Code;
use navree_grpc::testing::TestChannel;
use navree_observe::{ObserveModule, ObserveOptions};

use crate::support::pb::ping::PingRequest;
use crate::support::pb::ping::ping_client::PingClient;
use crate::support::ping_app;

fn ping(message: &str) -> PingRequest {
    PingRequest {
        message: message.to_owned(),
        sleep_ms: 0,
    }
}

/// Whether `body` has a sample of `name` with every label of `labels`.
fn has_sample(body: &str, name: &str, labels: &[&str]) -> bool {
    body.lines().any(|line| {
        line.starts_with(name)
            && line[name.len()..].starts_with(['{', '_'])
            && labels.iter().all(|label| line.contains(label))
    })
}

#[tokio::test]
async fn finished_calls_are_counted_by_service_method_and_code() {
    let app = ping_app()
        .module(ObserveModule::for_root(ObserveOptions::default()))
        .build()
        .await;
    let mut client = PingClient::new(TestChannel::new(&app));
    client.ping(ping("hello")).await.unwrap();
    let status = client.ping(ping("conflict")).await.unwrap_err();
    assert_eq!(status.code(), Code::AlreadyExists);

    let body = app
        .client()
        .get("/metrics")
        .send()
        .await
        .assert_status(200)
        .text();
    let call = [r#"service="navree.test.v1.Ping""#, r#"method="Ping""#];
    for code in ["OK", "ALREADY_EXISTS"] {
        let code = format!(r#"code="{code}""#);
        let labels = [call[0], call[1], code.as_str()];
        assert!(
            has_sample(&body, "grpc_server_handled_total", &labels),
            "{code} is counted: {body}"
        );
        assert!(
            has_sample(&body, "grpc_server_handling_seconds", &labels),
            "{code} is timed: {body}"
        );
    }

    app.shutdown().await.unwrap();
}
