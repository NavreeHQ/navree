//! What the Settings page shows of the options that cannot change from the
//! dashboard: read from the options themselves, grouped, with how each is
//! set. A secret is only ever shown as set or not set.

use serde_json::{Value, json};

use crate::alert::AlertKind;
use crate::normalize::url_host;
use crate::options::{LighthouseOptions, StoreKind};

/// One read-only option.
fn item(key: &str, label: &str, value: Value) -> Value {
    json!({
        "key": format!("lighthouse.{key}"),
        "label": label,
        "value": value,
        "secret": false,
        "env": format!("APP_LIGHTHOUSE__{}", key.replace('.', "__").to_uppercase()),
    })
}

/// A secret option: whether it is set, never what it is.
fn secret(key: &str, label: &str, set: bool) -> Value {
    json!({
        "key": format!("lighthouse.{key}"),
        "label": label,
        "value": if set { "set" } else { "not set" },
        "secret": true,
        "set": set,
        "env": format!("APP_LIGHTHOUSE__{}", key.replace('.', "__").to_uppercase()),
    })
}

fn list(values: &[String]) -> Value {
    if values.is_empty() {
        json!("none")
    } else {
        json!(values.join(", "))
    }
}

fn kinds(on: &Option<Vec<AlertKind>>) -> Value {
    match on {
        Some(kinds) => json!(
            kinds
                .iter()
                .map(|kind| kind.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        None => json!("new_issue, regression, spike (the default)"),
    }
}

/// The options that are set in code or configuration, in groups, with the
/// value the app booted with.
pub(crate) fn read_only(options: &LighthouseOptions) -> Vec<Value> {
    let alerts = &options.alerts;
    let webhook_host = alerts.webhook.url.as_deref().map(url_host);
    vec![
        json!({
            "group": "Where it runs",
            "items": [
                item("enabled", "Recording", json!(if options.enabled { "on" } else { "off" })),
                item("store", "Store", json!(match options.store {
                    StoreKind::Database => "database",
                    StoreKind::Off => "off",
                })),
                secret("database_url", "Separate database", options.database_url.is_some()),
                item("dashboard", "Dashboard and JSON API", json!(if options.dashboard { "mounted" } else { "off" })),
                item("mcp", "MCP tools", json!(if options.mcp { "offered" } else { "off" })),
                item("path", "Mounted at", json!(options.mount_path())),
                item("url", "Public URL for links in alerts", json!(options.url.clone().unwrap_or_else(|| "the app URL and the path".to_owned()))),
                item("environment", "Environment", json!(options.environment_label())),
                item("release", "Release label", json!(options.release.clone().filter(|r| !r.trim().is_empty()).unwrap_or_else(|| "from app.version, else none".to_owned()))),
                item("server", "Server label", json!(options.server_label())),
            ],
        }),
        json!({
            "group": "What is recorded",
            "items": [
                item("buffer", "Details waiting to be written", json!(options.buffer)),
                item("prune_at", "Daily prune (UTC)", json!(if options.prune_at.trim().is_empty() { "not scheduled" } else { options.prune_at.trim() })),
                item("capture.request_headers", "Request headers (masked)", json!(options.capture.request_headers)),
                item("capture.request_body", "JSON request bodies (masked)", json!(options.capture.request_body)),
                item("capture.ip", "Client address", json!(options.capture.ip)),
                item("redact", "Extra names masked", list(&options.redact)),
                item("ignore.routes", "Routes never recorded", list(&options.ignore.routes)),
                item("ignore.jobs", "Jobs never recorded", list(&options.ignore.jobs)),
            ],
        }),
        json!({
            "group": "Alert channels",
            "items": [
                item("alerts.email.to", "Email recipients", list(&alerts.email.to)),
                item("alerts.email.on", "Email kinds", kinds(&alerts.email.on)),
                secret("alerts.slack.webhook_url", "Slack webhook URL", alerts.slack.webhook_url.is_some()),
                item("alerts.slack.on", "Slack kinds", kinds(&alerts.slack.on)),
                item("alerts.webhook.url", "Webhook host", json!(webhook_host.unwrap_or_else(|| "none".to_owned()))),
                secret("alerts.webhook.secret", "Webhook signing secret", alerts.webhook.secret.is_some()),
                item("alerts.webhook.on", "Webhook kinds", kinds(&alerts.webhook.on)),
            ],
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_only_ever_set_or_not_set() {
        let options: LighthouseOptions = serde_json::from_value(json!({
            "alerts": {
                "slack": { "webhook_url": "https://hooks.slack.test/services/T0B0/ZZTOPPATH" },
                "webhook": { "url": "https://ops.test/hook?token=T", "secret": "x".repeat(40) },
            },
            "database_url": "postgres://user:pw@host/db",
        }))
        .unwrap();
        let text = serde_json::to_string(&read_only(&options)).unwrap();
        for hidden in ["ZZTOPPATH", "token=T", "pw@host", &"x".repeat(40)] {
            assert!(!text.contains(hidden), "{hidden}");
        }
        assert!(text.contains("ops.test"));
        assert!(text.contains("APP_LIGHTHOUSE__ALERTS__SLACK__WEBHOOK_URL"));
    }
}
