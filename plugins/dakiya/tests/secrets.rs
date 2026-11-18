//! Ensures the Dakiya API key and URL credentials never appear in errors, `Debug` or logs.

use std::sync::{Arc, Mutex};

use http::Method;
use navree::contracts::Secret;
use navree::db::{DatabaseModule, PageQuery};
use navree::http_client::{FakeResponse, HttpClient, HttpClientModule};
use navree::mail::{
    Deliveries, DeliveryFilter, DeliveryStatus, MailError, MailModule, Mailer, Message,
};
use navree::testing::TestApp;
use navree::view::ViewModule;
use navree_plugin_dakiya::{DakiyaModule, DakiyaSettings, DakiyaTransport};
use serde_json::json;

#[derive(Clone, Default)]
struct AllLogs(Arc<Mutex<Vec<String>>>);

impl AllLogs {
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

impl tracing::Subscriber for AllLogs {
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

fn sample_mail() -> Message {
    Message::new()
        .from("Harpreet Sandhu <harpreet@ludhiana.example>")
        .to("Meera Sharma <meera@ludhiana.example>")
        .subject("Rent receipt, ₹25,000")
        .text("Received ₹25,000 for Model Town, Ludhiana.")
}

#[tokio::test]
async fn a_provider_that_echoes_the_key_does_not_put_it_in_the_error() {
    let key = "dakiya key/ludhiana+1=\"punjab\"";
    let pct_upper = "dakiya%20key%2Fludhiana%2B1%3D%22punjab%22";
    let pct_lower = "dakiya%20key%2fludhiana%2b1%3d%22punjab%22";
    let form_upper = "dakiya+key%2Fludhiana%2B1%3D%22punjab%22";
    let form_lower = "dakiya+key%2fludhiana%2b1%3d%22punjab%22";
    let json_escaped = r#"dakiya key/ludhiana+1=\"punjab\""#;
    let json_quoted = format!("\"{json_escaped}\"");

    let echoed = format!(
        "bad key {key} | \"{key}\" | {json_quoted} | {pct_upper} | {pct_lower} | {form_upper} | {form_lower} | ₹{}",
        "ਸ".repeat(220)
    );

    let (client, server) = HttpClient::fake();
    server.on_sequence(
        Method::POST,
        "https://api.dakiya.example/v1/send",
        [
            FakeResponse::new(401).json(&json!({ "error": echoed })),
            FakeResponse::new(401)
                .text(format!("{}bad key {key} and {pct_lower}", "₹".repeat(196))),
        ],
    );

    let app = TestApp::builder()
        .temp_database()
        .config("mail.from", "Harpreet Sandhu <harpreet@ludhiana.example>")
        .config("mail.transport", "dakiya")
        .config("mail.delivery.enabled", true)
        .config("mail.delivery.routes", false)
        .config("mail.dakiya.url", "https://api.dakiya.example/v1/send")
        .config("mail.dakiya.api_key", key)
        .module(DatabaseModule::from_config())
        .module(HttpClientModule::from_config())
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(DakiyaModule)
        .override_value(Arc::new(client))
        .build()
        .await;

    let mailer = app.get::<Mailer>().await;
    for _ in 0..2 {
        let error = mailer
            .send(&sample_mail())
            .await
            .expect_err("401 fails the send");
        let display = error.to_string();
        let debug = format!("{error:?}");
        assert!(
            display.contains("mail provider `dakiya` answered 401"),
            "{display}"
        );
        if let MailError::Provider { message, .. } = &error {
            assert!(
                message.chars().count() <= 200,
                "message exceeds 200 chars: {}",
                message.chars().count()
            );
        } else {
            panic!("expected MailError::Provider, got {debug}");
        }

        for surface in [&display, &debug] {
            for leaked in [
                key,
                pct_upper,
                pct_lower,
                form_upper,
                form_lower,
                json_escaped,
                json_quoted.as_str(),
                "dakiya key",
                "ludhiana+1",
                "punjab",
            ] {
                assert!(!surface.contains(leaked), "leaked {leaked:?} in {surface}");
            }
        }
    }

    let failed = app
        .get::<Deliveries>()
        .await
        .list(
            &DeliveryFilter {
                status: Some(DeliveryStatus::Failed),
                ..DeliveryFilter::default()
            },
            PageQuery::new(1, 10),
        )
        .await
        .expect("delivery log");
    assert_eq!(failed.data.len(), 2);
    for row in &failed.data {
        let logged = row.error.as_deref().expect("failed row carries error");
        assert!(
            logged.contains("mail provider `dakiya` answered 401"),
            "{logged}"
        );
        for leaked in [
            key,
            pct_upper,
            pct_lower,
            form_upper,
            form_lower,
            json_escaped,
            json_quoted.as_str(),
            "dakiya key",
            "punjab",
        ] {
            assert!(!logged.contains(leaked), "leaked {leaked:?} in {logged}");
        }
    }

    app.shutdown().await.expect("clean shutdown");
}

#[tokio::test]
async fn a_wrongly_typed_key_does_not_appear_in_the_boot_error() {
    let error = TestApp::builder()
        .config("mail.from", "Harpreet Sandhu <harpreet@ludhiana.example>")
        .config("mail.transport", "dakiya")
        .config("mail.dakiya.url", "https://api.dakiya.example/v1/send")
        .config("mail.dakiya.api_key", 98140123456789_i64)
        .module(HttpClientModule::from_config())
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(DakiyaModule)
        .try_build()
        .await
        .expect_err("numeric api_key fails the boot");

    let display = error.to_string();
    let debug = format!("{error:?}");
    assert!(
        display.contains("mail.dakiya.api_key must be text"),
        "{display}"
    );
    for surface in [&display, &debug] {
        assert!(!surface.contains("98140123456789"), "{surface}");
        assert!(!surface.contains("981401"), "{surface}");
    }
}

#[tokio::test]
async fn a_bad_url_is_named_by_its_key_and_never_printed() {
    for bad in [
        "ftp://harpreet:dak-word@files.ludhiana.example",
        "http://files.ludhiana.example/v1/send",
        "http://localhost.ludhiana.example/v1/send",
        "http://localhost:dak-word@files.ludhiana.example/v1/send",
    ] {
        let error = TestApp::builder()
            .config("mail.from", "Harpreet Sandhu <harpreet@ludhiana.example>")
            .config("mail.transport", "dakiya")
            .config("mail.dakiya.url", bad)
            .config("mail.dakiya.api_key", "test-only-key-ludhiana")
            .module(HttpClientModule::from_config())
            .module(ViewModule::from_config())
            .module(MailModule::from_config())
            .module(DakiyaModule)
            .try_build()
            .await
            .expect_err("bad URL fails the boot");

        let display = error.to_string();
        let debug = format!("{error:?}");
        assert!(
            display.contains("mail.dakiya.url must start with https://"),
            "{display}"
        );
        for surface in [&display, &debug] {
            for leaked in [
                bad,
                "ftp://",
                "harpreet",
                "dak-word",
                "files.ludhiana.example",
                "test-only-key-ludhiana",
            ] {
                assert!(!surface.contains(leaked), "leaked {leaked:?} in {surface}");
            }
        }
    }

    for local in [
        "http://localhost:8080/v1/send",
        "http://127.0.0.1:8080/v1/send",
    ] {
        let (client, _) = HttpClient::fake();
        let app = TestApp::builder()
            .config("mail.from", "Harpreet Sandhu <harpreet@ludhiana.example>")
            .config("mail.transport", "dakiya")
            .config("mail.dakiya.url", local)
            .config("mail.dakiya.api_key", "test-only-key-ludhiana")
            .module(HttpClientModule::from_config())
            .module(ViewModule::from_config())
            .module(MailModule::from_config())
            .module(DakiyaModule)
            .override_value(Arc::new(client))
            .build()
            .await;
        app.shutdown().await.expect("clean shutdown");
    }

    let missing_key = TestApp::builder()
        .config("mail.from", "Harpreet Sandhu <harpreet@ludhiana.example>")
        .config("mail.transport", "dakiya")
        .config("mail.dakiya.url", "https://api.dakiya.example/v1/send")
        .config("mail.dakiya.api_key", "")
        .module(HttpClientModule::from_config())
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(DakiyaModule)
        .try_build()
        .await
        .expect_err("empty api_key fails the boot");
    assert!(
        missing_key
            .to_string()
            .contains("mail.dakiya.api_key is required"),
        "{missing_key}"
    );
}

#[tokio::test]
async fn a_misspelt_setting_fails_the_boot_naming_the_key_and_not_the_value() {
    let error = TestApp::builder()
        .config("mail.from", "Harpreet Sandhu <harpreet@ludhiana.example>")
        .config("mail.transport", "dakiya")
        .config("mail.dakiya.url", "https://api.dakiya.example/v1/send")
        .config("mail.dakiya.api_key", "test-only-key-ludhiana")
        .config("mail.dakiya.apikey", "dakiya-test-placeholder")
        .module(HttpClientModule::from_config())
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(DakiyaModule)
        .try_build()
        .await
        .expect_err("misspelt setting fails the boot");

    let display = error.to_string();
    let debug = format!("{error:?}");
    assert!(display.contains("unknown field"), "{display}");
    assert!(display.contains("apikey"), "{display}");
    for surface in [&display, &debug] {
        assert!(!surface.contains("dakiya-test-placeholder"), "{surface}");
        assert!(!surface.contains("test-only-key-ludhiana"), "{surface}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn the_key_never_reaches_a_debug_print_or_a_log_line() {
    let logs = AllLogs::default();
    let _guard = logs.install();

    let key = "test-only-key-ludhiana";
    let (client, server) = HttpClient::fake();
    server.on_sequence(
        Method::POST,
        "https://api.dakiya.example/*",
        [
            FakeResponse::new(200).json(&json!({ "id": "dak-ludhiana-2502" })),
            FakeResponse::new(401).json(&json!({ "error": format!("bad key {key}") })),
        ],
    );
    server.on(
        Method::GET,
        "https://api.dakiya.example/v1/ping",
        FakeResponse::new(200).json(&json!({ "ok": true })),
    );

    let settings = DakiyaSettings {
        url: "https://harpreet:dak-word@api.dakiya.example/v1/send?secret_token=dak-query"
            .to_owned(),
        api_key: Secret::new(key.to_owned()),
        sender_name: Some("Ludhiana Desk".to_owned()),
    };
    let direct = DakiyaTransport::new(client.clone(), settings.clone())
        .expect("transport builds from https URL");
    let transport_debug = format!("{direct:?}");
    assert!(
        transport_debug.contains("DakiyaTransport"),
        "{transport_debug}"
    );
    assert!(
        transport_debug.contains("api.dakiya.example"),
        "{transport_debug}"
    );
    for leaked in [
        key,
        "harpreet",
        "dak-word",
        "/v1/send",
        "secret_token",
        "dak-query",
    ] {
        assert!(
            !transport_debug.contains(leaked),
            "leaked {leaked:?} in {transport_debug}"
        );
    }

    let settings_debug = format!("{settings:?}");
    for leaked in [
        key,
        "harpreet",
        "dak-word",
        "/v1/send",
        "secret_token",
        "dak-query",
    ] {
        assert!(
            !settings_debug.contains(leaked),
            "leaked {leaked:?} in {settings_debug}"
        );
    }

    let app = TestApp::builder()
        .config("mail.from", "Harpreet Sandhu <harpreet@ludhiana.example>")
        .config("mail.transport", "dakiya")
        .config(
            "mail.dakiya.url",
            "https://harpreet:dak-word@api.dakiya.example/v1/send?secret_token=dak-query",
        )
        .config("mail.dakiya.api_key", key)
        .module(HttpClientModule::from_config())
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(DakiyaModule)
        .override_value(Arc::new(client))
        .build()
        .await;

    let mailer = app.get::<Mailer>().await;
    mailer
        .send(&sample_mail())
        .await
        .expect("first send succeeds");
    let _ = mailer
        .send(&sample_mail())
        .await
        .expect_err("second send fails");
    mailer
        .transport()
        .check()
        .await
        .expect("ping check succeeds");
    let dyn_debug = format!("{:?}", mailer.transport());
    assert!(!dyn_debug.contains(key), "{dyn_debug}");

    for line in logs.all() {
        for leaked in [key, "dak-word", "dak-query"] {
            assert!(
                !line.contains(leaked),
                "leaked {leaked:?} in log line {line}"
            );
        }
    }

    app.shutdown().await.expect("clean shutdown");
}
