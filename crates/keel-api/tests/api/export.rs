//! The `openapi:export [path]` command.

use crate::support::{boot, document};

#[tokio::test]
async fn openapi_export_writes_file() {
    let app = boot().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("web").join("openapi.json");

    app.run_command("openapi:export", vec![path.display().to_string()])
        .await
        .expect("export succeeds");

    let written = std::fs::read_to_string(&path).expect("file written, parent created");
    assert!(written.ends_with('\n'), "ends with a newline");
    assert!(written.contains("\n  \"openapi\""), "pretty-printed");
    let exported: serde_json::Value = serde_json::from_str(&written).unwrap();
    assert_eq!(exported, document(&app).await);
    app.shutdown().await.unwrap();
}
