//! Sending, failover and readiness checks of `DakiyaTransport` over `HttpClient::fake`.

use std::sync::{Arc, Mutex};

use http::Method;
use navree::db::{DatabaseModule, PageQuery};
use navree::http_client::{FakeResponse, HttpClient, HttpClientModule};
use navree::mail::{Deliveries, DeliveryFilter, DeliveryStatus, MailModule, Mailer, Message};
use navree::testing::TestApp;
use navree::view::ViewModule;
use navree_plugin_dakiya::DakiyaModule;
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct LogLines(Arc<Mutex<Vec<String>>>);

impl LogLines {
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(self.clone())
    }

    fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

#[derive(Default)]
struct Fields(Vec<String>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.push(format!("{}={value:?}", field.name()));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.push(format!("{}={value}", field.name()));
    }
}

impl tracing::Subscriber for LogLines {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, attrs: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        if !fields.0.is_empty() {
            self.0.lock().unwrap().push(fields.0.join(" "));
        }
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        if !fields.0.is_empty() {
            self.0.lock().unwrap().push(fields.0.join(" "));
        }
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.0.lock().unwrap().push(fields.0.join(" "));
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

fn rent_receipt() -> Message {
    Message::new()
        .from("Harpreet Sandhu <harpreet@ludhiana.example>")
        .to("Meera Sharma <meera@ludhiana.example>")
        .cc("Simran Kaur <simran@ludhiana.example>")
        .bcc("Gurpreet Singh <gurpreet@ludhiana.example>")
        .subject("Rent receipt, ₹25,000")
        .html("<p>Received ₹25,000 for Model Town, Ludhiana (IST).</p>")
        .text("Received ₹25,000 for Model Town, Ludhiana (IST).")
}

#[tokio::test]
async fn a_message_is_posted_to_the_api_with_the_bearer_key_and_the_provider_id_is_kept() {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::POST,
        "https://api.dakiya.example/v1/send",
        FakeResponse::new(200).json(&json!({ "id": "dak-ludhiana-2501" })),
    );

    let app = TestApp::builder()
        .temp_database()
        .config("mail.from", "Harpreet Sandhu <harpreet@ludhiana.example>")
        .config("mail.transport", "dakiya")
        .config("mail.delivery.enabled", true)
        .config("mail.delivery.routes", false)
        .config("mail.dakiya.url", "https://api.dakiya.example/v1/send")
        .config("mail.dakiya.api_key", "test-only-key-ludhiana")
        .module(DatabaseModule::from_config())
        .module(HttpClientModule::from_config())
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(DakiyaModule)
        .override_value(Arc::new(client))
        .build()
        .await;

    let mailer = app.get::<Mailer>().await;
    let report = mailer
        .deliver(&rent_receipt())
        .await
        .expect("mail is delivered");
    assert_eq!(report.status, DeliveryStatus::Sent);
    assert_eq!(report.transport, Some("dakiya"));
    assert_eq!(report.provider_id.as_deref(), Some("dak-ludhiana-2501"));

    let recorded = server.recorded();
    assert_eq!(recorded.len(), 1);
    let sent_req = &recorded[0];
    assert_eq!(sent_req.method(), Method::POST);
    assert_eq!(
        sent_req.url().as_str(),
        "https://api.dakiya.example/v1/send"
    );
    assert_eq!(
        sent_req.header("authorization"),
        Some("Bearer test-only-key-ludhiana")
    );

    let body: Value = sent_req.json().expect("JSON request body");
    assert_eq!(body["from"], "Harpreet Sandhu <harpreet@ludhiana.example>");
    assert_eq!(body["to"], json!(["Meera Sharma <meera@ludhiana.example>"]));
    assert_eq!(body["cc"], json!(["Simran Kaur <simran@ludhiana.example>"]));
    assert_eq!(
        body["bcc"],
        json!(["Gurpreet Singh <gurpreet@ludhiana.example>"])
    );
    assert_eq!(body["subject"], "Rent receipt, ₹25,000");
    assert!(
        body["html"]
            .as_str()
            .is_some_and(|html| html.contains("₹25,000"))
    );
    assert_eq!(
        body["text"],
        "Received ₹25,000 for Model Town, Ludhiana (IST)."
    );

    let logged = app
        .get::<Deliveries>()
        .await
        .find(report.delivery_id.expect("delivery id"))
        .await
        .expect("delivery row");
    assert_eq!(logged.transport, "dakiya");
    assert_eq!(logged.provider_id.as_deref(), Some("dak-ludhiana-2501"));

    app.shutdown().await.expect("clean shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn a_failover_falls_through_to_the_log_transport_when_the_api_is_down() {
    let logs = LogLines::default();
    let _guard = logs.install();

    let (client, server) = HttpClient::fake();
    server.on(
        Method::POST,
        "https://api.dakiya.example/v1/send",
        FakeResponse::new(503).json(&json!({ "error": "gateway maintenance in Ludhiana" })),
    );

    let app = TestApp::builder()
        .temp_database()
        .config("mail.from", "Harpreet Sandhu <harpreet@ludhiana.example>")
        .config(
            "mail.transport",
            json!({ "type": "failover", "transports": ["dakiya", "log"] }),
        )
        .config("mail.delivery.enabled", true)
        .config("mail.delivery.routes", false)
        .config("mail.dakiya.url", "https://api.dakiya.example/v1/send")
        .config("mail.dakiya.api_key", "test-only-key-ludhiana")
        .module(DatabaseModule::from_config())
        .module(HttpClientModule::from_config())
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(DakiyaModule)
        .override_value(Arc::new(client))
        .build()
        .await;

    let mailer = app.get::<Mailer>().await;
    let report = mailer
        .deliver(&rent_receipt())
        .await
        .expect("failover delivers through log");
    assert_eq!(report.status, DeliveryStatus::Sent);
    assert_eq!(report.transport, Some("log"));
    server.assert_sent_count(1);

    let deliveries = app
        .get::<Deliveries>()
        .await
        .list(&DeliveryFilter::default(), PageQuery::new(1, 10))
        .await
        .expect("delivery rows");
    assert_eq!(deliveries.data.len(), 1);
    assert_eq!(deliveries.data[0].status, DeliveryStatus::Sent);
    assert_eq!(deliveries.data[0].transport, "log");

    let lines = logs.all();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("dakiya") && line.contains("503")),
        "failover log names dakiya: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("log transport")),
        "delivery log names log: {lines:?}"
    );

    app.shutdown().await.expect("clean shutdown");
}

#[tokio::test]
async fn check_reads_the_ping_path_with_the_key_and_sends_nothing() {
    let (client, server) = HttpClient::fake();
    server.on(
        Method::GET,
        "https://api.dakiya.example/v1/ping",
        FakeResponse::new(200).json(&json!({ "ok": true })),
    );

    let app = TestApp::builder()
        .config("mail.from", "Harpreet Sandhu <harpreet@ludhiana.example>")
        .config("mail.transport", "dakiya")
        .config("mail.dakiya.url", "https://api.dakiya.example/v1/send")
        .config("mail.dakiya.api_key", "test-only-key-ludhiana")
        .module(HttpClientModule::from_config())
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(DakiyaModule)
        .override_value(Arc::new(client))
        .build()
        .await;

    let mailer = app.get::<Mailer>().await;
    mailer
        .transport()
        .check()
        .await
        .expect("ping check succeeds");

    let recorded = server.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].method(), Method::GET);
    assert_eq!(
        recorded[0].url().as_str(),
        "https://api.dakiya.example/v1/ping"
    );
    assert_eq!(
        recorded[0].header("authorization"),
        Some("Bearer test-only-key-ludhiana")
    );
    assert!(recorded[0].body().is_empty());

    app.shutdown().await.expect("clean shutdown");
}
