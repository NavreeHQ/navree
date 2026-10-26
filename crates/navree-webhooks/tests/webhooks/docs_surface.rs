//! Every public item, config key, route, command, table, job and event
//! named on `content/docs/packages/webhooks.mdx`, so a rename fails here.

use std::collections::BTreeSet;
use std::sync::Arc;

use navree_contracts::{FixedClock, Job};
use navree_db::Db;
use navree_events::Event;
use navree_http_client::HttpClient;
use navree_webhooks::{
    Attempt, CreatedEndpoint, Deliver, Delivery, DeliveryStatus, Endpoint, EndpointChanges,
    EventType, FanOut, GitHub, Hmac, HmacAlgorithm, IncomingOptions, MAX_ATTEMPTS,
    ModuleBuilderWebhooksExt, NewEndpoint, Paddle, RETRY_SCHEDULE, SignatureEncoding,
    SourceOptions, StandardWebhooks, Stripe, Tolerance, Verified, VerifiedWebhook, VerifyError,
    WebhookContext, WebhookDeliveryFailed, WebhookEndpointDisabled, WebhookEvent, WebhookSource,
    WebhookVerifier, Webhooks, WebhooksError, WebhooksModule, WebhooksOptions,
};
use sea_orm::{ConnectionTrait, Statement};

use crate::support::{DealWon, builder, start};

/// The page's configuration blocks, merged.
const DOCUMENTED_TOML: &str = r#"
[webhooks]
queue = "webhooks"
timeout_secs = 15
require_https = true
disable_after_days = 5
max_payload_bytes = 262144
retain_days = 30
routes = true

[webhooks.incoming]
tolerance_secs = 300
max_body_bytes = 1048576
receipt_days = 30

[webhooks.incoming.calendly]
secret = "set-in-env"
header = "Calendly-Webhook-Signature"
algorithm = "sha256"
encoding = "hex"
format = "t={timestamp},v1={signature}"
signed = "{timestamp}.{body}"
tolerance_secs = 180
timestamp_header = "X-Timestamp"
id_header = "X-Id"
event_header = "X-Event"
"#;

#[test]
fn the_documented_configuration_parses_and_validates() {
    let parsed: toml::Table = DOCUMENTED_TOML.parse().unwrap();
    let section = serde_json::to_value(&parsed["webhooks"]).unwrap();
    let options: WebhooksOptions = serde_json::from_value(section).unwrap();
    assert_eq!(options.queue, "webhooks");
    assert_eq!(options.incoming.tolerance_secs, 300);
    let calendly = options.incoming.source("calendly").unwrap();
    assert_eq!(calendly.algorithm, HmacAlgorithm::Sha256);
    assert_eq!(calendly.encoding, SignatureEncoding::Hex);
    assert_eq!(calendly.tolerance_secs, Some(180));
    assert_eq!(navree_config::ConfigSection::validate(&options), Ok(()));
    let _: IncomingOptions = options.incoming.clone();
    let _: SourceOptions = SourceOptions::new("s");
}

#[test]
fn the_documented_names_exist() {
    // The module.
    let _ = WebhooksModule::from_config;
    let _ = WebhooksModule::for_root;
    let _ = WebhooksModule::for_feature::<DealWon>;
    let _ = |m: &mut navree_kernel::ModuleBuilder| {
        m.webhook_type("export.ready", "A CSV export finished.");
    };
    let _ = <navree_kernel::ModuleBuilder as ModuleBuilderWebhooksExt>::webhook_event::<DealWon>;
    // Events that become webhooks.
    fn is_webhook_event<E: WebhookEvent>() {}
    is_webhook_event::<DealWon>();
    let _ = WebhookContext::conn;
    let _ = WebhookContext::db;
    let _ = WebhookContext::app;
    // The service.
    let _ = Webhooks::create_endpoint;
    let _ = Webhooks::update_endpoint;
    let _ = Webhooks::delete_endpoint;
    let _ = Webhooks::rotate_secret;
    let _ = Webhooks::enable;
    let _ = Webhooks::disable;
    let _ = Webhooks::endpoint;
    let _ = Webhooks::endpoints;
    let _ = Webhooks::send_test;
    let _ = Webhooks::deliveries;
    let _ = Webhooks::delivery;
    let _ = Webhooks::attempts;
    let _ = Webhooks::retry;
    let _ = Webhooks::retry_failed;
    let _ = Webhooks::prune;
    let _ = Webhooks::event_types;
    let _ = |w: &Webhooks| {
        let _ = w
            .dispatch_json("export.ready", serde_json::json!({}))
            .workspace(uuid::Uuid::nil());
    };
    let _ = NewEndpoint::new("https://x")
        .events(["deal.won"])
        .workspace(uuid::Uuid::nil())
        .description("d")
        .header("X-Team", "revenue");
    let _ = EndpointChanges::new().url("https://x").events(["*"]);
    let _: Option<(Endpoint, CreatedEndpoint, Delivery, Attempt, EventType)> = None;
    let _ = [
        DeliveryStatus::Pending,
        DeliveryStatus::Succeeded,
        DeliveryStatus::Failed,
    ];
    let _ = WebhooksError::NotRetryable(DeliveryStatus::Pending);
    assert_eq!(RETRY_SCHEDULE.len() as u32, MAX_ATTEMPTS);
    // Incoming.
    let _ = VerifiedWebhook::<GitHub>::json::<serde_json::Value>;
    let _ = VerifiedWebhook::<GitHub>::body;
    let _ = VerifiedWebhook::<GitHub>::headers;
    let _ = VerifiedWebhook::<GitHub>::id;
    let _ = VerifiedWebhook::<GitHub>::event_type;
    let _ = VerifiedWebhook::<GitHub>::timestamp;
    let _ = VerifiedWebhook::<GitHub>::mark_processed;
    fn source<S: WebhookSource>() -> &'static str {
        S::NAME
    }
    assert_eq!(source::<Stripe>(), "stripe");
    assert_eq!(source::<GitHub>(), "github");
    assert_eq!(source::<Paddle>(), "paddle");
    assert_eq!(source::<StandardWebhooks>(), "standard");
    fn verifier<V: WebhookVerifier>() {}
    verifier::<Hmac>();
    let _ = Stripe::sign;
    let _ = Stripe::verify;
    let _ = GitHub::sign;
    let _ = GitHub::verify;
    let _ = Paddle::sign;
    let _ = Paddle::verify;
    let _ = StandardWebhooks::sign;
    let _ = StandardWebhooks::verify;
    let _ = Hmac::sign;
    let _ = Hmac::verify;
    let _ = Tolerance::secs(300);
    let _ = Tolerance::at(start(), 300);
    let _: Option<(Verified, VerifyError)> = None;
    // Jobs and events.
    assert_eq!(FanOut::NAME, "webhooks.fan_out");
    assert_eq!(Deliver::NAME, "webhooks.deliver");
    assert_eq!(WebhookDeliveryFailed::NAME, "webhook.delivery_failed");
    assert_eq!(WebhookEndpointDisabled::NAME, "webhook.endpoint_disabled");
    assert_eq!(navree_webhooks::USER_AGENT_VALUE, "Navree-Webhooks/1");
    assert_eq!(navree_webhooks::TEST_EVENT_TYPE, "webhook.test");
    assert_eq!(navree_webhooks::MANAGE_PERMISSION, "webhooks.manage");
    assert_eq!(
        navree_webhooks::INVALID_SIGNATURE,
        "Invalid webhook signature"
    );
}

#[tokio::test]
async fn the_documented_routes_commands_and_tables_exist() {
    let (client, _server) = HttpClient::fake();
    let app = builder(client, Arc::new(FixedClock::new(start())))
        .config("webhooks.routes", true)
        .module(navree_access::AccessModule::for_root(
            navree_access::AccessOptions::default(),
        ))
        .build()
        .await;

    let routes: BTreeSet<String> = navree_http::route_table(app.ctx())
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.full_path.starts_with("/api/webhooks"))
        .map(|r| format!("{} {}", r.method, r.full_path))
        .collect();
    let documented: BTreeSet<String> = [
        "GET /api/webhooks/event-types",
        "GET /api/webhooks/endpoints",
        "POST /api/webhooks/endpoints",
        "GET /api/webhooks/endpoints/{id}",
        "PATCH /api/webhooks/endpoints/{id}",
        "DELETE /api/webhooks/endpoints/{id}",
        "POST /api/webhooks/endpoints/{id}/rotate-secret",
        "POST /api/webhooks/endpoints/{id}/test",
        "GET /api/webhooks/endpoints/{id}/deliveries",
        "POST /api/webhooks/deliveries/{id}/retry",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(routes, documented);

    for (command, args) in [
        ("webhooks:endpoints", vec![]),
        ("webhooks:retry", vec!["failed"]),
        ("webhooks:prune", vec!["--days", "30"]),
    ] {
        app.run_command(command, &args).await.unwrap();
    }
    assert!(app.run_command("webhooks:test", &[]).await.is_err());

    let db = app.get::<Db>().await;
    let rows = db
        .conn()
        .query_all_raw(Statement::from_string(
            db.backend(),
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'webhook_%' ORDER BY name",
        ))
        .await
        .unwrap();
    let tables: Vec<String> = rows
        .iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .collect();
    assert_eq!(
        tables,
        [
            "webhook_attempts",
            "webhook_deliveries",
            "webhook_endpoints",
            "webhook_receipts"
        ]
    );
    let _: Option<VerifiedWebhook<GitHub>> = None;
}
