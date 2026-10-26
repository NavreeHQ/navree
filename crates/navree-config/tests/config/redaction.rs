//! Redacted views of the configuration, safe to log.

use navree_config::{Config, ConfigSources};
use serde_json::json;

fn config() -> Config {
    let dir = tempfile::tempdir().expect("tempdir");
    let toml = dir.path().join("app.toml");
    std::fs::write(
        &toml,
        r#"
name = "Navree"

[database]
url = "postgres://db/app"
pool = 5
password = "hunter2"

[app]
key = "base64:aGVsbG8="

[auth]
api_token = "tok_123"
CLIENT_SECRET = "sec_456"

[http]
Authorization = "Bearer xyz"
cors = { origins = ["http://localhost:3000"] }

[session]
cookie = "navree_session"

[api_keys]
stripe = "sk_live_789"

[log]
level = "info"
redact = ["password", "token"]
"#,
    )
    .expect("write fixture");
    Config::load(&ConfigSources::new().file(&toml)).expect("load")
}

#[test]
fn redacted_masks_secret_keys() {
    assert_eq!(
        config().redacted(),
        json!({
            "name": "Navree",
            "database": {
                "url": "postgres://db/app",
                "pool": 5,
                "password": "[redacted]",
            },
            "app": { "key": "[redacted]" },
            "auth": { "api_token": "[redacted]", "CLIENT_SECRET": "[redacted]" },
            "http": {
                "Authorization": "[redacted]",
                "cors": { "origins": ["http://localhost:3000"] },
            },
            "session": { "cookie": "[redacted]" },
            // A secret-looking key masks its whole subtree.
            "api_keys": "[redacted]",
            "log": { "level": "info", "redact": ["password", "token"] },
        })
    );
}

#[test]
fn redacted_masks_passwords_in_urls() {
    let config = Config::load(
        &ConfigSources::new()
            .set(
                "database.url",
                "postgres://navree:hunter2@db:5432/app?sslmode=require",
            )
            .set("cache.url", "redis://:hunter2@cache:6379/0")
            .set("queue.url", "amqp://guest@mq/")
            .set("site", "https://example.com/users/a@b:c")
            .set("note", "user:hunter2@host is not a URL"),
    )
    .expect("load");
    assert_eq!(
        config.redacted(),
        json!({
            "database": { "url": "postgres://navree:[redacted]@db:5432/app?sslmode=require" },
            "cache": { "url": "redis://:[redacted]@cache:6379/0" },
            "queue": { "url": "amqp://guest@mq/" },
            "site": "https://example.com/users/a@b:c",
            "note": "user:hunter2@host is not a URL",
        })
    );
}

#[test]
fn debug_output_is_redacted() {
    let debug = format!("{:?}", config());
    assert!(debug.contains("postgres://db/app"), "{debug}");
    for secret in ["hunter2", "aGVsbG8", "tok_123", "sec_456", "xyz", "sk_live"] {
        assert!(!debug.contains(secret), "{secret} leaked: {debug}");
    }
}
