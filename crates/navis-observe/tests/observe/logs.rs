//! Logs written for operators: failing checks and rejected metrics tokens.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use http::StatusCode;
use navis_contracts::Secret;
use navis_observe::ObserveOptions;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

use crate::support::{Behaviour, FakeCheck, TestApp, app_with};

/// One captured event: its target, level and fields (`message` included).
type Captured = (String, Level, BTreeMap<String, String>);

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Captured>>>);

impl Capture {
    fn events(&self, target: &str) -> Vec<(Level, BTreeMap<String, String>)> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, _, _)| t == target)
            .map(|(_, level, fields)| (*level, fields.clone()))
            .collect()
    }

    fn all_text(&self) -> String {
        format!("{:?}", self.0.lock().unwrap())
    }
}

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }
}

impl<S: Subscriber> Layer<S> for Capture {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.0.lock().unwrap().push((
            event.metadata().target().to_owned(),
            *event.metadata().level(),
            fields.0,
        ));
    }
}

// `#[tokio::test]` runs on one thread, so a thread-local default subscriber
// sees every event of the request.
#[tokio::test]
async fn down_checks_are_logged_as_warnings() {
    let capture = Capture::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
    let app = TestApp::boot(app_with(
        ObserveOptions::default(),
        vec![
            FakeCheck::new("database", Behaviour::Up),
            FakeCheck::new("cache", Behaviour::Down("unreachable")),
        ],
    ))
    .await;

    assert_eq!(
        app.get("/health/ready").await.status,
        StatusCode::SERVICE_UNAVAILABLE
    );

    let events = capture.events("navis::health");
    assert_eq!(events.len(), 1, "only the down check: {events:?}");
    let (level, fields) = &events[0];
    assert_eq!(*level, Level::WARN);
    assert_eq!(fields["check"], "cache");
    assert_eq!(fields["error"], "unreachable");
}

#[tokio::test]
async fn rejected_metrics_token_is_logged_without_the_token() {
    let capture = Capture::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
    let options = ObserveOptions {
        metrics_token: Some(Secret::new("expected-token".to_owned())),
        ..ObserveOptions::default()
    };
    let app = TestApp::boot(app_with(options, vec![])).await;

    let response = app
        .get_with_auth("/metrics", "Bearer guessed-token-123")
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);

    let events = capture.events("navis::security");
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].0, Level::WARN);
    let text = capture.all_text();
    assert!(!text.contains("guessed-token-123"), "{text}");
    assert!(!text.contains("expected-token"), "{text}");
}
