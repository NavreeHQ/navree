//! The examples of `content/docs/packages/webhooks.mdx`, written as an app
//! writes them (the facade, the prelude and the macros), compiled and run.

use http::Method;
use navree::BoxFuture;
use navree::db::Db;
use navree::events::{Event, EventBus};
use navree::http_client::{FakeResponse, HttpClient};
use navree::prelude::*;
use navree::queue::Queue;
use navree::testing::TestApp;
use navree::webhooks::{
    AwsSns, GitHub, NewEndpoint, SnsMessage, StandardWebhooks, VerifiedWebhook, WebhookContext,
    WebhookEvent, Webhooks,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// A deal moved to won.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DealWon {
    deal_id: Uuid,
    workspace_id: Option<Uuid>,
    won_by: Uuid,
}

impl Event for DealWon {
    const NAME: &'static str = "deal.won";
}

impl WebhookEvent for DealWon {
    const DESCRIPTION: &'static str = "A deal moved to won.";

    /// The workspace whose endpoints receive it.
    fn workspace_id(&self) -> Option<Uuid> {
        self.workspace_id
    }

    /// What goes in `data`.
    fn payload<'a>(&'a self, ctx: &'a WebhookContext) -> BoxFuture<'a, Result<Value, BoxError>> {
        Box::pin(async move {
            let _db = ctx.db();
            Ok(json!({ "deal": { "id": self.deal_id }, "won_by": self.won_by }))
        })
    }
}

/// Makes `DealWon` a webhook and registers `export.ready`.
#[module(imports = [WebhooksModule::for_feature::<DealWon>()], register = Self::wire)]
pub struct DealsModule;

impl DealsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.webhook_type("export.ready", "A CSV export finished.");
    }
}

#[derive(Debug, Deserialize)]
struct IssueEvent {
    action: String,
}

/// Receives GitHub webhooks.
#[injectable]
pub struct GitHubHookController;

#[controller("/integrations/github", version = "1")]
impl GitHubHookController {
    /// `POST /api/v1/integrations/github`.
    #[post("/")]
    #[public]
    async fn receive(&self, uow: Uow, hook: VerifiedWebhook<GitHub>) -> Result<Json<Value>> {
        if !hook.mark_processed(&uow).await? {
            return Ok(Json(json!({ "handled": false })));
        }
        let mut action = None;
        if hook.event_type() == Some("issues") {
            let event: IssueEvent = hook.json()?;
            action = Some(event.action);
        }
        Ok(Json(json!({ "handled": true, "action": action })))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SesEvent {
    event_type: String,
    mail: SesMail,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SesMail {
    message_id: String,
}

/// Receives SES events through SNS.
#[injectable]
pub struct SesEventsController;

#[controller("/mail/ses-events", version = "1")]
impl SesEventsController {
    /// `POST /api/v1/mail/ses-events`.
    #[post("/")]
    #[public]
    async fn receive(&self, uow: Uow, hook: VerifiedWebhook<AwsSns>) -> Result<Json<Value>> {
        if !hook.mark_processed(&uow).await? {
            return Ok(Json(json!({ "handled": false })));
        }
        let message: SnsMessage = hook.json()?;
        let event: SesEvent = message
            .message_json()
            .map_err(|error| AppError::bad_request(error.to_string()))?;
        Ok(Json(json!({
            "handled": true,
            "event": event.event_type,
            "mail": event.mail.message_id,
        })))
    }
}

/// The GitHub and SES integrations.
#[module(controllers = [GitHubHookController, SesEventsController])]
pub struct IntegrationsModule;

async fn app(client: HttpClient) -> TestApp {
    TestApp::builder()
        .temp_database()
        .config("queue.driver", "memory")
        .config("webhooks.routes", false)
        .config("webhooks.incoming.github.secret", "test-secret")
        .config(
            "webhooks.incoming.aws_sns.topics",
            "arn:aws:sns:us-east-1:123456789012:ses-events",
        )
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(QueueModule::from_config())
        .module(HttpClientModule::from_config())
        .module(WebhooksModule::from_config())
        .module(DealsModule)
        .module(IntegrationsModule)
        .override_value(Arc::new(client))
        .build()
        .await
}

fn a_won_deal() -> DealWon {
    DealWon {
        deal_id: Uuid::now_v7(),
        workspace_id: None,
        won_by: Uuid::now_v7(),
    }
}

#[tokio::test]
async fn winning_a_deal_notifies_subscribed_endpoints() {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::POST,
        "https://hooks.lovelace.io/*",
        FakeResponse::new(204),
    );
    let app = app(client).await;

    let uow = app.get::<Db>().await.uow(None, None).await;
    let created = app
        .get::<Webhooks>()
        .await
        .create_endpoint(
            &uow,
            NewEndpoint::new("https://hooks.lovelace.io/navree").events(["deal.won"]),
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();

    app.get::<EventBus>()
        .await
        .emit(a_won_deal())
        .await
        .unwrap();
    app.get::<Queue>().await.run_pending().await.unwrap();

    server.assert_sent_count(1);
    let request = server.recorded().pop().unwrap();
    StandardWebhooks::verify(&created.secret, request.headers(), request.body(), None).unwrap();
    assert_eq!(
        request.json::<serde_json::Value>().unwrap()["type"],
        "deal.won"
    );
}

#[tokio::test]
async fn webhooks_that_are_not_events_are_dispatched_by_name() {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::POST,
        "https://hooks.lovelace.io/*",
        FakeResponse::new(200),
    );
    let app = app(client).await;
    let webhooks = app.get::<Webhooks>().await;
    let workspace_id = Uuid::now_v7();
    let uow = app.get::<Db>().await.uow(None, None).await;
    webhooks
        .create_endpoint(
            &uow,
            NewEndpoint::new("https://hooks.lovelace.io/exports")
                .events(["export.ready"])
                .workspace(workspace_id),
        )
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let message_id = webhooks
        .dispatch_json(
            "export.ready",
            json!({ "export_id": 7, "url": "https://files/7.csv" }),
        )
        .workspace(workspace_id)
        .send()
        .await
        .unwrap();
    app.get::<Queue>().await.run_pending().await.unwrap();

    let request = server.recorded().pop().unwrap();
    assert_eq!(request.header("webhook-id"), Some(message_id.as_str()));
}

#[tokio::test]
async fn a_verified_github_webhook_reaches_the_handler_once() {
    let (client, _server) = HttpClient::fake();
    let app = app(client).await;
    let body = serde_json::to_vec(&json!({
        "action": "opened",
        "issue": { "number": 7, "title": "Export fails" },
    }))
    .unwrap();
    let send = |signature: String| {
        app.client()
            .post("/api/v1/integrations/github")
            .header("X-GitHub-Event", "issues")
            .header("X-GitHub-Delivery", "72d3162e-cc78-11e3-81ab-4c9367dc0958")
            .header("X-Hub-Signature-256", signature)
            .header("Content-Type", "application/json")
            .body(body.clone())
            .send()
    };

    send(GitHub::sign("test-secret", &body))
        .await
        .assert_status(200)
        .assert_json_path("handled", json!(true))
        .assert_json_path("action", json!("opened"));
    send(GitHub::sign("test-secret", &body))
        .await
        .assert_status(200)
        .assert_json_path("handled", json!(false));
    send(GitHub::sign("another-secret", &body))
        .await
        .assert_error(400, "Invalid webhook signature");
}

/// A fixture of navree-webhooks' `tests/fixtures/sources` (an SNS signing
/// certificate and messages signed with its key by OpenSSL).
fn sns_fixture(name: &str) -> String {
    let path = format!(
        "{}/../navree-webhooks/tests/fixtures/sources/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[tokio::test]
async fn ses_events_arrive_verified_through_sns() {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::GET,
        "https://sns.us-east-1.amazonaws.com/SimpleNotificationService-*.pem",
        FakeResponse::new(200).text(sns_fixture("signer-cert.pem")),
    );
    let app = app(client).await;
    let messages: Value = serde_json::from_str(&sns_fixture("sns-messages.json")).unwrap();
    let notification = serde_json::to_vec(&messages["notification_v1"]).unwrap();
    let send = |body: Vec<u8>| {
        app.client()
            .post("/api/v1/mail/ses-events")
            .header("x-amz-sns-message-type", "Notification")
            .header("content-type", "text/plain; charset=UTF-8")
            .body(body)
            .send()
    };

    send(notification.clone())
        .await
        .assert_status(200)
        .assert_json_path("handled", json!(true))
        .assert_json_path("event", json!("Bounce"))
        .assert_json_path("mail", json!("0100018c-9a7f-4f0e-b2a1-000000000000"));
    send(notification.clone())
        .await
        .assert_status(200)
        .assert_json_path("handled", json!(false));
    let forged = String::from_utf8(notification)
        .unwrap()
        .replace("ses-events", "other-topic");
    send(forged.into_bytes())
        .await
        .assert_error(400, "Invalid webhook signature");
}
