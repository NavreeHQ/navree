//! A `LoggerModule` app with a telemetry sink receives the log lines written
//! outside any request or job (boot, background tasks) while it runs. Its
//! own test binary: it installs the process-wide subscriber.

use navree_kernel::Navree;
use navree_log::{LogOptions, LoggerModule};
use navree_telemetry::TelemetryRecorder;

#[tokio::test]
async fn background_lines_reach_the_app_until_it_shuts_down() {
    let recorder = TelemetryRecorder::new();
    let app = Navree::app()
        .module(LoggerModule::for_root(LogOptions::default()))
        .module(recorder.module())
        .boot()
        .await
        .expect("boots");

    tracing::warn!(target: "crm::sync", "background line");
    let logs = recorder.logs();
    assert!(
        logs.iter().any(|log| log.message == "background line"),
        "{logs:?}"
    );

    app.shutdown().await.expect("shuts down");
    recorder.clear();
    tracing::warn!(target: "crm::sync", "after shutdown");
    assert!(recorder.logs().is_empty());
}
