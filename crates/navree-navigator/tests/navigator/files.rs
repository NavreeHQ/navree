//! read_config, read_logs and last_error: what the app's files say.

use serde_json::json;

use crate::support::{Fixture, call, refused, write};

#[tokio::test]
async fn the_configuration_is_merged_and_redacted() {
    let fixture = Fixture::new(
        "[database]\nurl = \"postgres://navree:s3cret@127.0.0.1/crm\"\n\n[mail]\nhost = \"smtp.test\"\npassword = \"hunter2\"\n\n[queue]\nworkers = 4\n",
    );
    write(&fixture.root().join(".env"), "APP_KEY=base64:c2VjcmV0\nAPP_QUEUE__DRIVER=database\n");
    let navigator = fixture.navigator();
    let config = call(&navigator, "read_config", json!({})).await;
    assert_eq!(config["mail"]["host"], "smtp.test");
    assert_eq!(config["mail"]["password"], "[redacted]");
    assert_eq!(config["key"], "[redacted]");
    assert_eq!(config["database"]["url"], "postgres://navree:[redacted]@127.0.0.1/crm");
    assert_eq!(config["queue"]["driver"], "database", ".env is read");
    let text = config.to_string();
    assert!(!text.contains("hunter2") && !text.contains("s3cret") && !text.contains("c2VjcmV0"), "{text}");

    let queue = call(&navigator, "read_config", json!({"key": "queue"})).await;
    assert_eq!(queue, json!({"workers": 4, "driver": "database"}));
    let host = call(&navigator, "read_config", json!({"key": "mail.host"})).await;
    assert_eq!(host, "smtp.test");
    let message = refused(&navigator, "read_config", json!({"key": "nope"})).await;
    assert!(message.contains("no `nope`") && message.contains("database"), "{message}");
}

#[tokio::test]
async fn dock_settings_reach_the_configuration() {
    let fixture = Fixture::new("");
    write(&fixture.root().join("dock.toml"), "[services.postgres]\n");
    if std::env::var_os("APP_DATABASE__URL").is_some() {
        return; // the environment wins over Dock's settings
    }
    let url = call(&fixture.navigator(), "read_config", json!({"key": "database.url"})).await;
    assert_eq!(url, "postgres://navree:[redacted]@127.0.0.1:5432/crm");
}

const LOG_TOML: &str = "[log]\nfile = { path = \"storage/logs/navree.log\", rotation = \"daily\", keep_days = 7 }\n";

#[tokio::test]
async fn without_a_log_file_the_tools_say_how_to_turn_it_on() {
    let fixture = Fixture::new("");
    for tool in ["read_logs", "last_error"] {
        let message = refused(&fixture.navigator(), tool, json!({})).await;
        assert!(message.contains("[log]") && message.contains("storage/logs/navree.log"), "{message}");
    }
    let fixture = Fixture::new(LOG_TOML);
    let message = refused(&fixture.navigator(), "read_logs", json!({})).await;
    assert!(message.contains("doesn't exist yet"), "{message}");
}

#[tokio::test]
async fn logs_are_read_from_the_newest_rotated_files() {
    let fixture = Fixture::new(LOG_TOML);
    let logs = fixture.root().join("storage/logs");
    write(
        &logs.join("navree.2026-10-26.log"),
        "2026-10-26T23:59:00Z  INFO navree::http: yesterday\n",
    );
    write(
        &logs.join("navree.2026-10-27.log"),
        &[
            r#"{"timestamp":"2026-10-27T09:12:00Z","level":"INFO","target":"navree::http","fields":{"message":"listening"}}"#,
            r#"{"timestamp":"2026-10-27T09:12:01Z","level":"ERROR","target":"navree::http","fields":{"message":"handler failed","error":"no such column: deals.expected_close_date"},"spans":[{"name":"request","request_id":"0199c4b1"}]}"#,
            r#"{"timestamp":"2026-10-27T09:12:01.1Z","level":"INFO","target":"navree::request","fields":{"message":"request completed","method":"GET","route":"/api/v1/deals","status":500,"request_id":"0199c4b1"}}"#,
        ]
        .join("\n"),
    );
    let navigator = fixture.navigator();
    let answer = call(&navigator, "read_logs", json!({})).await;
    let messages: Vec<&str> = answer["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["message"].as_str().unwrap())
        .collect();
    assert_eq!(messages, ["yesterday", "listening", "handler failed", "request completed"]);
    assert!(answer["file"].as_str().unwrap().ends_with("navree.2026-10-27.log"));

    let errors = call(&navigator, "read_logs", json!({"level": "error"})).await;
    assert_eq!(errors["lines"].as_array().unwrap().len(), 1);
    let message = refused(&navigator, "read_logs", json!({"level": "loud"})).await;
    assert!(message.contains("invalid arguments") || message.contains("loud"), "{message}");

    let last = call(&navigator, "last_error", json!({})).await;
    assert_eq!(last["request_id"], "0199c4b1");
    assert_eq!(last["error"]["fields"]["error"], "no such column: deals.expected_close_date");
    assert_eq!(last["request"], json!({"method": "GET", "route": "/api/v1/deals", "status": 500}));
    assert_eq!(last["lines"].as_array().unwrap().len(), 2);
}
