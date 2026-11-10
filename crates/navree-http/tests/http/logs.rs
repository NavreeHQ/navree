//! The request log line and error logs.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use http::Method;
use navree_http::body::Body;
use navree_http::{AppError, RouteDef};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

use crate::support::{Setup, capture_logs};

/// One captured event: its target and fields (`message` included).
type Captured = (String, BTreeMap<String, String>);

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Captured>>>);

impl Capture {
    fn events(&self, target: &str) -> Vec<BTreeMap<String, String>> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, _)| t == target)
            .map(|(_, fields)| fields.clone())
            .collect()
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
        self.0
            .lock()
            .unwrap()
            .push((event.metadata().target().to_owned(), fields.0));
    }
}

const ALICE: &str = "0192f0c1-0000-7000-8000-00000000000a";

// `#[tokio::test]` runs on one thread, so a thread-local default subscriber
// sees every event of the request.
#[tokio::test]
async fn request_log_line_has_route_status_user_and_request_id() {
    let capture = Capture::default();
    let _guard = capture_logs(tracing_subscriber::registry().with(capture.clone()));
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/deals/{id}", || async { "deal" }).authenticated(),
    ])
    .authenticated()
    .boot()
    .await;

    let response = app
        .send(
            http::Request::get("/api/deals/42")
                .header("x-user-id", ALICE)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    app.get("/api/unknown").await;

    let lines = capture.events("navree::request");
    assert_eq!(lines.len(), 2, "{lines:?}");
    let line = &lines[0];
    assert_eq!(line["method"], "GET");
    assert_eq!(
        line["route"], "/api/deals/{id}",
        "the route template, not the path"
    );
    assert_eq!(line["status"], "200");
    assert_eq!(line["user_id"], ALICE);
    assert_eq!(line["request_id"], response.header("x-request-id").unwrap());
    assert_eq!(line["ip"], "0.0.0.0");
    assert!(line["latency_ms"].parse::<f64>().is_ok(), "{line:?}");

    let unmatched = &lines[1];
    assert_eq!(
        unmatched["route"], "unmatched",
        "no raw paths in route labels"
    );
    assert_eq!(unmatched["status"], "404");
    assert!(!unmatched.contains_key("user_id"));
}

#[tokio::test]
async fn server_errors_are_logged_with_their_cause() {
    let capture = Capture::default();
    let _guard = capture_logs(tracing_subscriber::registry().with(capture.clone()));
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/broken", || async {
            Err::<(), AppError>(std::io::Error::other("connection reset by database").into())
        })
        .public(),
        RouteDef::on(Method::GET, "/panic", || async {
            if true {
                panic!("index out of bounds");
            }
        })
        .public(),
        RouteDef::on(Method::GET, "/missing", || async { AppError::not_found() }).public(),
    ])
    .boot()
    .await;

    let response = app.get("/api/broken").await;
    assert!(!response.text().contains("connection reset"));
    app.get("/api/panic").await;
    app.get("/api/missing").await;

    let errors = capture.events("navree::error");
    assert_eq!(
        errors.len(),
        2,
        "client errors are not error logs: {errors:?}"
    );
    assert!(
        errors[0]["error"].contains("connection reset by database"),
        "{errors:?}"
    );
    assert_eq!(
        errors[0]["request_id"],
        response.header("x-request-id").unwrap()
    );
    assert!(
        errors[1]["panic"].contains("index out of bounds"),
        "{errors:?}"
    );
}

/// Records, for every event, the fields of the `request` span it happened in.
#[derive(Clone, Default)]
struct RequestSpanFields(Arc<Mutex<Vec<Captured>>>);

impl<S> Layer<S> for RequestSpanFields
where
    S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(fields);
        }
    }

    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        ctx: Context<'_, S>,
    ) {
        if let Some(span) = ctx.span(id)
            && let Some(fields) = span.extensions_mut().get_mut::<Fields>()
        {
            values.record(fields);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let request = ctx
            .event_scope(event)
            .into_iter()
            .flatten()
            .find(|span| span.name() == "request");
        if let Some(span) = request
            && let Some(fields) = span.extensions().get::<Fields>()
        {
            self.0
                .lock()
                .unwrap()
                .push((event.metadata().target().to_owned(), fields.0.clone()));
        }
    }
}

#[tokio::test]
async fn handler_logs_carry_the_request_id_and_user_id() {
    let capture = RequestSpanFields::default();
    let _guard = capture_logs(tracing_subscriber::registry().with(capture.clone()));
    let app = Setup::new(vec![
        RouteDef::on(Method::GET, "/work", || async {
            tracing::info!(target: "test::handler", "working");
            "done"
        })
        .authenticated(),
    ])
    .authenticated()
    .boot()
    .await;

    let response = app
        .send(
            http::Request::get("/api/work")
                .header("x-user-id", ALICE)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.text(), "done");

    let events = capture.0.lock().unwrap().clone();
    let (_, span) = events
        .iter()
        .find(|(target, _)| target == "test::handler")
        .unwrap_or_else(|| panic!("handler event inside the request span: {events:?}"));
    assert_eq!(span["user_id"], ALICE, "{span:?}");
    assert_eq!(span["request_id"], response.header("x-request-id").unwrap());
}
