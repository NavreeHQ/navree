//! Every public item, config key, route, command, table, job and event
//! named on `content/docs/packages/webhooks.mdx`, so a rename fails here.

use std::collections::BTreeSet;
use std::sync::Arc;

use navree_contracts::{FixedClock, Job};
use navree_db::Db;
use navree_db::{Backend, Executor, Statement};
use navree_events::Event;
use navree_http_client::HttpClient;
use navree_webhooks::{
    Attempt, AwsSns, CreatedEndpoint, Deliver, Delivery, DeliveryStatus, Endpoint, EndpointChanges,
    EventType, FanOut, GitHub, GooglePubSub, GraphNotifications, Handshake, Hmac, HmacAlgorithm,
    IncomingOptions, IncomingRequest, MAX_ATTEMPTS, Mailgun, MicrosoftGraph,
    ModuleBuilderWebhooksExt, NewEndpoint, Paddle, Postmark, PubSubMessage, PubSubPush,
    RETRY_SCHEDULE, SenderKeys, SignatureEncoding, Slack, SnsMessage, SourceOptions,
    StandardWebhooks, Stripe, Tolerance, Twilio, Verdict, Verified, VerifiedWebhook, VerifyError,
    WebhookContext, WebhookDeliveryFailed, WebhookEndpointDisabled, WebhookEvent, WebhookSource,
    WebhookVerifier, Webhooks, WebhooksError, WebhooksModule, WebhooksOptions,
};

use crate::support::{DealWon, builder, start};

/// The page's custom verifier: a `?challenge=` handshake, then GitHub's
/// scheme.
pub struct Partner;

impl WebhookVerifier for Partner {
    async fn verify_incoming(request: &IncomingRequest<'_>) -> Result<Verdict, VerifyError> {
        if let Some(challenge) = request.query("challenge") {
            return Ok(Verdict::Handshake(Handshake::text(challenge)));
        }
        GitHub::verify(&request.source().secret, request.headers(), request.body())
            .map(Verdict::Verified)
    }
}

impl WebhookSource for Partner {
    const NAME: &'static str = "partner";
    type Verifier = Partner;
}

#[tokio::test]
async fn the_documented_custom_verifier_answers_its_handshake_and_verifies() {
    let source = SourceOptions::new("partner-secret");
    let headers = http::HeaderMap::new();
    let challenge = IncomingRequest::new(&source, &headers, b"")
        .with_url("https://api.acme.example/hooks/partner?challenge=abc");
    assert_eq!(
        Partner::verify_incoming(&challenge).await,
        Ok(Verdict::Handshake(Handshake::text("abc")))
    );
    let mut signed = http::HeaderMap::new();
    signed.insert(
        "x-hub-signature-256",
        GitHub::sign("partner-secret", b"{}").parse().unwrap(),
    );
    let delivery = IncomingRequest::new(&source, &signed, b"{}");
    assert!(matches!(
        Partner::verify_incoming(&delivery).await,
        Ok(Verdict::Verified(_))
    ));
}

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

[webhooks.incoming.aws_sns]
topics = ["arn:aws:sns:eu-west-1:123456789012:ses-events"]
auto_confirm = true

[webhooks.incoming.google_pubsub]
service_account = "gmail-push@acme-crm.iam.gserviceaccount.com"
audience = "https://api.acme.example/api/v1/mail/gmail-push"

[webhooks.incoming.twilio]
secret = "set-in-env"
url = "https://api.acme.example/api/v1/sms/status"
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
    let sns = options.incoming.source("aws_sns").unwrap();
    assert_eq!(sns.topics.len(), 1);
    assert!(sns.auto_confirm);
    let pubsub = options.incoming.source("google_pubsub").unwrap();
    assert!(pubsub.service_account.is_some() && pubsub.audience.is_some());
    assert!(options.incoming.source("twilio").unwrap().url.is_some());
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
    // The other built-in sources.
    assert_eq!(source::<AwsSns>(), "aws_sns");
    assert_eq!(source::<GooglePubSub>(), "google_pubsub");
    assert_eq!(source::<MicrosoftGraph>(), "microsoft_graph");
    assert_eq!(source::<Twilio>(), "twilio");
    assert_eq!(source::<Slack>(), "slack");
    assert_eq!(source::<Mailgun>(), "mailgun");
    assert_eq!(source::<Postmark>(), "postmark");
    assert_eq!(source::<Partner>(), "partner");
    let _ = VerifiedWebhook::<Twilio>::form::<serde_json::Value>;
    let _ = Twilio::sign;
    let _ = Twilio::verify;
    let _ = Slack::sign;
    let _ = Slack::verify;
    let _ = Mailgun::sign;
    let _ = Postmark::authorization;
    let _ = SnsMessage::string_to_sign;
    let _ = SnsMessage::message_json::<serde_json::Value>;
    let _ = |message: &SnsMessage| message.subscribe_url.clone();
    let _ = PubSubMessage::data_json::<serde_json::Value>;
    let _: Option<(
        PubSubPush,
        GraphNotifications,
        SenderKeys,
        IncomingRequest<'_>,
    )> = None;
    assert_eq!(
        GooglePubSub::KEYS_URL,
        "https://www.googleapis.com/oauth2/v3/certs"
    );
    assert_eq!(
        AwsSns::missing_setting(&SourceOptions::default()),
        Some("topics")
    );
    assert_eq!(
        GooglePubSub::missing_setting(&SourceOptions::default()),
        Some("service_account")
    );
    let _ = Verdict::Handshake(Handshake::text("challenge"));
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
    assert_eq!(
        navree_webhooks::INVALID_SIGNATURE_CODE,
        "invalid_webhook_signature"
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
    let sql = match db.backend() {
        Backend::Sqlite => {
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'webhook_%' ORDER BY name"
        }
        Backend::Postgres => {
            "SELECT tablename::text AS name FROM pg_tables WHERE schemaname = current_schema() AND tablename LIKE 'webhook_%' ORDER BY tablename"
        }
        Backend::MySql => {
            "SELECT CAST(table_name AS CHAR) AS name FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name LIKE 'webhook_%' ORDER BY table_name"
        }
    };
    let tables: Vec<String> = db
        .fetch_all(&Statement::text(sql))
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String>("name").unwrap())
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
