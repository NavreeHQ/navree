//! Observability: one log line at target `navree::mcp` per call (never with
//! arguments or results), `mcp_requests_total` and
//! `mcp_request_duration_seconds`.
//!
//! The test binary gets a global JSON subscriber and a global metrics
//! recorder, once; other tests' calls land there too, so the assertions
//! pick this test's own lines by user id and count metrics as "at least".

use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use metrics::{
    Counter, CounterFn, Gauge, Histogram, HistogramFn, Key, KeyName, Metadata, Recorder,
    SharedString, Unit,
};
use navree_mcp::testing::McpTestClient;
use serde_json::{Value, json};
use tracing_subscriber::fmt::MakeWriter;

use crate::support::{app, user};

/// Everything the global subscriber wrote.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    fn lines(&self) -> Vec<Value> {
        let bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&bytes)
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    fn text(&self) -> String {
        let bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl io::Write for Captured {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Counter values and histogram sample counts, by `name{label=value,..}`.
#[derive(Clone, Default)]
struct Recorded(Arc<Mutex<BTreeMap<String, u64>>>);

impl Recorded {
    fn add(&self, key: &str, value: u64) {
        *self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key.to_owned())
            .or_default() += value;
    }

    fn get(&self, key: &str) -> u64 {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .copied()
            .unwrap_or(0)
    }
}

/// One counter or histogram of [`TestRecorder`].
struct Series {
    key: String,
    recorded: Recorded,
}

impl CounterFn for Series {
    fn increment(&self, value: u64) {
        self.recorded.add(&self.key, value);
    }

    fn absolute(&self, value: u64) {
        self.recorded.add(&self.key, value);
    }
}

impl HistogramFn for Series {
    fn record(&self, _value: f64) {
        self.recorded.add(&self.key, 1);
    }
}

struct TestRecorder(Recorded);

impl TestRecorder {
    fn series(&self, key: &Key) -> Arc<Series> {
        let labels: Vec<String> = key
            .labels()
            .map(|label| format!("{}={}", label.key(), label.value()))
            .collect();
        Arc::new(Series {
            key: format!("{}{{{}}}", key.name(), labels.join(",")),
            recorded: self.0.clone(),
        })
    }
}

impl Recorder for TestRecorder {
    fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
        Counter::from_arc(self.series(key))
    }

    fn register_gauge(&self, _: &Key, _: &Metadata<'_>) -> Gauge {
        Gauge::noop()
    }

    fn register_histogram(&self, key: &Key, _: &Metadata<'_>) -> Histogram {
        Histogram::from_arc(self.series(key))
    }
}

/// The process-wide subscriber and recorder.
fn installed() -> &'static (Captured, Recorded) {
    static INSTALLED: OnceLock<(Captured, Recorded)> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let captured = Captured::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::INFO)
            .with_writer(captured.clone())
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("no other global subscriber");
        let recorded = Recorded::default();
        metrics::set_global_recorder(TestRecorder(recorded.clone()))
            .expect("no other global recorder");
        (captured, recorded)
    })
}

#[tokio::test]
async fn every_call_is_logged_and_counted_without_its_arguments() {
    let (logs, metrics) = installed();
    let app = app().await;
    let caller = user();
    let mcp = McpTestClient::new(app.acting_as(caller.clone()))
        .initialize()
        .await;

    mcp.call_tool("whoami", json!({})).await.assert_ok();
    mcp.call_tool(
        "create_deal",
        json!({ "title": "Confidential merger 7f3a", "amount_cents": 100 }),
    )
    .await
    .assert_ok();
    mcp.call_tool("explode", json!({})).await.assert_error(500, None);
    mcp.call_tool("not_a_tool", json!({ "x": 1 })).await;
    mcp.read_resource("crm://notes/kickoff").await.assert_ok();
    mcp.get_prompt("summarize", json!({ "days": 7 }))
        .await
        .assert_ok();

    let user_id = caller.user_id.to_string();
    let calls: Vec<Value> = logs
        .lines()
        .into_iter()
        .filter(|line| line["target"] == "navree::mcp" && line["fields"]["user_id"] == user_id)
        .map(|line| line["fields"].clone())
        .collect();
    let seen: Vec<(String, String, String)> = calls
        .iter()
        .map(|fields| {
            (
                fields["method"].as_str().unwrap_or_default().to_owned(),
                fields["name"].as_str().unwrap_or_default().to_owned(),
                fields["outcome"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let expected = [
        ("tools/call", "whoami", "ok"),
        ("tools/call", "create_deal", "ok"),
        ("tools/call", "explode", "error"),
        ("tools/call", "unknown", "unknown"),
        ("resources/read", "note", "ok"),
        ("prompts/get", "summarize", "ok"),
    ]
    .map(|(m, n, o)| (m.to_owned(), n.to_owned(), o.to_owned()));
    assert_eq!(seen, expected, "{calls:#?}");
    for fields in &calls {
        assert_eq!(fields["message"], "mcp call");
        assert!(fields["duration_ms"].is_u64(), "{fields}");
        assert!(
            fields["request_id"].as_str().is_some_and(|id| !id.is_empty()),
            "{fields}"
        );
        assert_eq!(fields["client"], "navree-test", "{fields}");
    }
    assert!(
        !logs.text().contains("Confidential merger 7f3a"),
        "arguments are never logged"
    );

    for key in [
        "mcp_requests_total{method=tools/call,name=whoami,outcome=ok}",
        "mcp_requests_total{method=tools/call,name=explode,outcome=error}",
        "mcp_requests_total{method=tools/call,name=unknown,outcome=unknown}",
        "mcp_requests_total{method=resources/read,name=note,outcome=ok}",
        "mcp_requests_total{method=prompts/get,name=summarize,outcome=ok}",
        "mcp_request_duration_seconds{method=tools/call,name=whoami}",
        "mcp_request_duration_seconds{method=prompts/get,name=summarize}",
    ] {
        assert!(metrics.get(key) >= 1, "{key} was not recorded");
    }
}
