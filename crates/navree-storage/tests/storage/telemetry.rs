//! File operations reported as telemetry: the disk, the operation, the path
//! and the bytes, never the contents.

use std::sync::Arc;

use navree_storage::StorageError;
use navree_telemetry::{Outcome, StorageOperation, Telemetry, TelemetryContext, TelemetryRecorder};

use crate::support::storage;

#[tokio::test]
async fn disk_operations_are_reported_with_their_bytes_and_outcome() {
    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    let root = tempfile::tempdir().unwrap();
    let disk = storage(root.path()).disk("local").unwrap();

    TelemetryContext::new(telemetry)
        .scope(async {
            disk.put("kyc/pan.txt", "ABCDE1234F").await.unwrap();
            let bytes = disk.get("kyc/pan.txt").await.unwrap();
            assert_eq!(bytes, "ABCDE1234F");
            disk.files("kyc").await.unwrap();
            let missing = disk.get("kyc/gone.txt").await.unwrap_err();
            assert!(matches!(missing, StorageError::NotFound { .. }));
            disk.delete("kyc/pan.txt").await.unwrap();
        })
        .await;

    let ops = recorder.storage_ops();
    let steps: Vec<(StorageOperation, &str, Outcome)> = ops
        .iter()
        .map(|op| (op.operation, op.path.trim_end_matches('/'), op.outcome))
        .collect();
    assert_eq!(
        steps,
        [
            (StorageOperation::Write, "kyc/pan.txt", Outcome::Success),
            (StorageOperation::Read, "kyc/pan.txt", Outcome::Success),
            (StorageOperation::List, "kyc", Outcome::Success),
            (StorageOperation::Read, "kyc/gone.txt", Outcome::Failure),
            (StorageOperation::Delete, "kyc/pan.txt", Outcome::Success),
        ]
    );
    assert_eq!(ops[0].disk, "local");
    assert_eq!(ops[0].driver, "local");
    assert_eq!(ops[0].bytes, Some(10));
    assert_eq!(ops[1].bytes, Some(10));
    assert_eq!(ops[3].error.as_deref(), Some("not_found"));
    let text = serde_json::to_string(&ops).unwrap();
    assert!(
        !text.contains("ABCDE1234F"),
        "the contents stay out: {text}"
    );
}

#[tokio::test]
async fn a_streamed_write_reports_the_bytes_written() {
    let recorder = TelemetryRecorder::new();
    let telemetry = Telemetry::with_sinks(vec![Arc::new(recorder.clone())]);
    let root = tempfile::tempdir().unwrap();
    let disk = storage(root.path()).disk("memory").unwrap();
    let chunks = futures_util::stream::iter([
        Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"abc")),
        Ok(bytes::Bytes::from_static(b"de")),
    ]);

    TelemetryContext::new(telemetry)
        .scope(async {
            disk.put_stream("a/b.bin", chunks).await.unwrap();
        })
        .await;

    let op = &recorder.storage_ops()[0];
    assert_eq!(op.operation, StorageOperation::Write);
    assert_eq!(op.disk, "memory");
    assert_eq!(op.bytes, Some(5));
}
