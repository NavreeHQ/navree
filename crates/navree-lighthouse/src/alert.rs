//! Alerts: [`AlertKind`], [`Alert`], the [`AlertChannel`] trait, the
//! built-in email, Slack and webhook channels, and the `lighthouse.alert`
//! job ([`DeliverAlert`]).

use std::fmt;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use hmac::{KeyInit as _, Mac as _};
use navree_contracts::{Backoff, Job, JobContext, JobOptions};
use navree_http_client::HttpClient;
use navree_kernel::{
    BoxError, BoxFuture, Factory, Injectable, ModuleBuilder, ProviderKey, Resolver,
};
use navree_mail::{Mailer, Message};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::model::Issue;

/// What an alert is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    /// An issue was opened for the first time.
    NewIssue,
    /// A resolved issue occurred again.
    Regression,
    /// An issue occurred more than `alerts.spike.occurrences` times within
    /// `alerts.spike.minutes`.
    Spike,
    /// A job moved to the failed jobs.
    FailedJob,
    /// A queue's oldest job has waited longer than its threshold (raised by
    /// a queue monitor through [`Lighthouse::raise_alert`](crate::Lighthouse::raise_alert)).
    LongWait,
}

impl AlertKind {
    /// Every kind.
    pub const ALL: [AlertKind; 5] = [
        AlertKind::NewIssue,
        AlertKind::Regression,
        AlertKind::Spike,
        AlertKind::FailedJob,
        AlertKind::LongWait,
    ];

    /// The snake_case name (`new_issue`).
    pub fn as_str(self) -> &'static str {
        match self {
            AlertKind::NewIssue => "new_issue",
            AlertKind::Regression => "regression",
            AlertKind::Spike => "spike",
            AlertKind::FailedJob => "failed_job",
            AlertKind::LongWait => "long_wait",
        }
    }

    /// Parses the snake_case name.
    pub fn parse(text: &str) -> Option<AlertKind> {
        AlertKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == text)
    }

    /// A label for people (`New issue`).
    pub fn label(self) -> &'static str {
        match self {
            AlertKind::NewIssue => "New issue",
            AlertKind::Regression => "Regression",
            AlertKind::Spike => "Spike",
            AlertKind::FailedJob => "Failed job",
            AlertKind::LongWait => "Long wait",
        }
    }
}

impl fmt::Display for AlertKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An alert: its kind and the issue it is about, with the environment and
/// the release it happened in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    /// The alert's id (also the webhook message id).
    pub id: Uuid,
    /// What happened.
    pub kind: AlertKind,
    /// The issue.
    pub issue: Issue,
    /// The environment label.
    pub environment: String,
    /// The release label.
    pub release: String,
    /// When it was raised.
    pub at: DateTime<Utc>,
    /// Whether it is a test alert (`lighthouse:test-alerts`).
    #[serde(default)]
    pub test: bool,
}

impl Alert {
    /// The JSON body of a webhook alert:
    /// `{"type": "lighthouse.<kind>", "issue": {..}, "environment", "release"}`.
    pub fn payload(&self) -> Value {
        let mut body = json!({
            "type": format!("lighthouse.{}", self.kind),
            "issue": self.issue,
            "environment": self.environment,
            "release": self.release,
        });
        if self.test {
            body["test"] = Value::Bool(true);
        }
        body
    }

    /// A one-line summary: `New issue: <title> (<culprit>)`.
    pub fn summary(&self) -> String {
        format!(
            "{}{}: {} ({})",
            if self.test { "[test] " } else { "" },
            self.kind.label(),
            self.issue.title,
            self.issue.culprit
        )
    }
}

/// Sends alerts somewhere. Register yours with
/// [`ModuleBuilderLighthouseExt::alert_channel`]; the built-in channels are
/// `email`, `slack` and `webhook`.
pub trait AlertChannel: Send + Sync + 'static {
    /// The channel's name (`lighthouse:test-alerts <name>`, the
    /// `lighthouse_alerts.channel` column).
    fn name(&self) -> &str;

    /// Whether the channel wants this alert.
    fn wants(&self, alert: &Alert) -> bool;

    /// Delivers the alert; an error is retried with backoff.
    fn send<'a>(&'a self, alert: &'a Alert) -> BoxFuture<'a, Result<(), BoxError>>;
}

/// Registers alert channels on a [`ModuleBuilder`].
pub trait ModuleBuilderLighthouseExt {
    /// Registers provider `C` (built by the container) as an alert channel.
    fn alert_channel<C: AlertChannel + Injectable>(&mut self) -> &mut Self;
}

impl ModuleBuilderLighthouseExt for ModuleBuilder {
    fn alert_channel<C: AlertChannel + Injectable>(&mut self) -> &mut Self {
        self.provider::<C>();
        self.multi(
            Factory::<dyn AlertChannel>::new(|r: Resolver| async move {
                Ok(r.get::<C>().await? as Arc<dyn AlertChannel>)
            })
            .deps(vec![ProviderKey::of::<C>()]),
        )
    }
}

/// Email alerts through the app's `Mailer`.
pub(crate) struct EmailChannel {
    pub(crate) to: Vec<String>,
    pub(crate) on: Vec<AlertKind>,
    pub(crate) mailer: Option<Arc<Mailer>>,
}

impl AlertChannel for EmailChannel {
    fn name(&self) -> &str {
        "email"
    }

    fn wants(&self, alert: &Alert) -> bool {
        self.on.contains(&alert.kind)
    }

    fn send<'a>(&'a self, alert: &'a Alert) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let mailer = self
                .mailer
                .as_ref()
                .ok_or("email alerts need MailModule (and ViewModule) in the app")?;
            let mut message = Message::new()
                .subject(format!("[Lighthouse] {}", alert.summary()))
                .text(email_text(alert))
                .html(email_html(alert));
            for to in &self.to {
                message = message.to(to.clone());
            }
            mailer.send_message(message).await?;
            Ok(())
        })
    }
}

fn email_text(alert: &Alert) -> String {
    let issue = &alert.issue;
    format!(
        "{kind}: {title}\n\nWhere: {culprit}\nKind: {issue_kind}\nOccurrences: {count}\nEnvironment: {environment}\nRelease: {release}\n\nOpen in Lighthouse: {url}\n",
        kind = alert.kind.label(),
        title = issue.title,
        culprit = issue.culprit,
        issue_kind = issue.kind.label(),
        count = issue.count,
        environment = alert.environment,
        release = alert.release,
        url = issue.url,
    )
}

fn email_html(alert: &Alert) -> String {
    let issue = &alert.issue;
    format!(
        "<p><strong>{kind}</strong>: {title}</p><table>\
         <tr><td>Where</td><td>{culprit}</td></tr>\
         <tr><td>Kind</td><td>{issue_kind}</td></tr>\
         <tr><td>Occurrences</td><td>{count}</td></tr>\
         <tr><td>Environment</td><td>{environment}</td></tr>\
         <tr><td>Release</td><td>{release}</td></tr></table>\
         <p><a href=\"{url}\">Open in Lighthouse</a></p>",
        kind = escape(alert.kind.label()),
        title = escape(&issue.title),
        culprit = escape(&issue.culprit),
        issue_kind = escape(issue.kind.label()),
        count = issue.count,
        environment = escape(&alert.environment),
        release = escape(&alert.release),
        url = escape(&issue.url),
    )
}

/// HTML-escapes text.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Slack alerts through an incoming webhook.
pub(crate) struct SlackChannel {
    pub(crate) url: String,
    pub(crate) on: Vec<AlertKind>,
    pub(crate) client: HttpClient,
}

/// The Slack message of an alert: the title, the route or job, the count
/// and an **Open in Lighthouse** button.
pub(crate) fn slack_message(alert: &Alert) -> Value {
    let issue = &alert.issue;
    let text = format!(
        "*{}*: {}\n`{}` · {} occurrence{} · {} · {}",
        alert.kind.label(),
        issue.title,
        issue.culprit,
        issue.count,
        if issue.count == 1 { "" } else { "s" },
        alert.environment,
        alert.release,
    );
    json!({
        "text": alert.summary(),
        "blocks": [
            { "type": "section", "text": { "type": "mrkdwn", "text": text } },
            {
                "type": "actions",
                "elements": [{
                    "type": "button",
                    "text": { "type": "plain_text", "text": "Open in Lighthouse" },
                    "url": issue.url,
                }]
            }
        ]
    })
}

impl AlertChannel for SlackChannel {
    fn name(&self) -> &str {
        "slack"
    }

    fn wants(&self, alert: &Alert) -> bool {
        self.on.contains(&alert.kind)
    }

    fn send<'a>(&'a self, alert: &'a Alert) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let response = self
                .client
                .post(&self.url)
                .json(&slack_message(alert))
                .send()
                .await?;
            if !(200..300).contains(&response.status().as_u16()) {
                return Err(format!("Slack answered {}", response.status()).into());
            }
            Ok(())
        })
    }
}

/// Signed webhook alerts (Standard Webhooks).
pub(crate) struct WebhookChannel {
    pub(crate) url: String,
    pub(crate) key: Vec<u8>,
    pub(crate) on: Vec<AlertKind>,
    pub(crate) client: HttpClient,
}

/// The HMAC key of a webhook secret: a `whsec_` secret is base64-decoded
/// (as Standard Webhooks secrets are); any other secret is used as bytes.
pub(crate) fn webhook_key(secret: &str) -> Vec<u8> {
    match secret.strip_prefix("whsec_") {
        Some(encoded) => STANDARD
            .decode(encoded)
            .unwrap_or_else(|_| secret.as_bytes().to_vec()),
        None => secret.as_bytes().to_vec(),
    }
}

/// The Standard Webhooks signature (`v1,<base64>`) of `body` sent as
/// message `id` at `timestamp`: HMAC-SHA256 over `id.timestamp.body`.
pub fn webhook_signature(key: &[u8], id: &str, timestamp: i64, body: &[u8]) -> String {
    let mut mac =
        hmac::Hmac::<sha2::Sha256>::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(id.as_bytes());
    mac.update(b".");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()))
}

impl AlertChannel for WebhookChannel {
    fn name(&self) -> &str {
        "webhook"
    }

    fn wants(&self, alert: &Alert) -> bool {
        self.on.contains(&alert.kind)
    }

    fn send<'a>(&'a self, alert: &'a Alert) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let body = serde_json::to_vec(&alert.payload())?;
            let id = format!("msg_{}", alert.id.simple());
            let timestamp = Utc::now().timestamp();
            let signature = webhook_signature(&self.key, &id, timestamp, &body);
            let response = self
                .client
                .post(&self.url)
                .header("content-type", "application/json")
                .header("webhook-id", &id)
                .header("webhook-timestamp", &timestamp.to_string())
                .header("webhook-signature", &signature)
                .body(body)
                .send()
                .await?;
            if !(200..300).contains(&response.status().as_u16()) {
                return Err(format!("the webhook answered {}", response.status()).into());
            }
            Ok(())
        })
    }
}

/// The `lighthouse.alert` job: delivers one stored alert through its
/// channel, retried with backoff (5 attempts, from 10 seconds up to 10
/// minutes apart).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeliverAlert {
    /// The `lighthouse_alerts` row.
    pub alert_id: Uuid,
}

/// Attempts of a `lighthouse.alert` job.
pub(crate) const ALERT_ATTEMPTS: u32 = 5;

impl Job for DeliverAlert {
    const NAME: &'static str = "lighthouse.alert";

    fn options() -> JobOptions {
        JobOptions {
            max_attempts: ALERT_ATTEMPTS,
            backoff: Backoff::Exponential {
                base_secs: 10,
                max_secs: 600,
            },
            timeout_secs: 60,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let lighthouse = ctx.app.get::<crate::Lighthouse>().await?;
        lighthouse
            .deliver_stored_alert(self.alert_id, ctx.attempt, ALERT_ATTEMPTS)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{IssueKind, IssueStatus, Level};

    fn alert() -> Alert {
        Alert {
            id: Uuid::nil(),
            kind: AlertKind::NewIssue,
            issue: Issue {
                id: Uuid::nil(),
                title: "cannot charge invoice: card declined".into(),
                kind: IssueKind::Exception,
                level: Level::Error,
                status: IssueStatus::Open,
                culprit: "POST /api/v1/invoices/{id}/charge".into(),
                count: 1,
                users: 1,
                first_seen: DateTime::UNIX_EPOCH,
                last_seen: DateTime::UNIX_EPOCH,
                url: "https://api.crm.example.com/_lighthouse/issues/x".into(),
            },
            environment: "production".into(),
            release: "20261026-091512-3f2a9c1".into(),
            at: DateTime::UNIX_EPOCH,
            test: false,
        }
    }

    #[test]
    fn the_standard_webhooks_test_vector_verifies() {
        let key = webhook_key("whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw");
        assert_eq!(
            webhook_signature(
                &key,
                "msg_p5jXN8AQM9LWM0D4loKWxJek",
                1_614_265_330,
                br#"{"test": 2432232314}"#
            ),
            "v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE="
        );
        assert_eq!(webhook_key("plain-secret"), b"plain-secret");
    }

    #[test]
    fn the_payload_matches_the_documented_shape() {
        let payload = alert().payload();
        assert_eq!(payload["type"], "lighthouse.new_issue");
        assert_eq!(payload["issue"]["kind"], "exception");
        assert_eq!(payload["issue"]["status"], "open");
        assert_eq!(payload["issue"]["level"], "error");
        assert_eq!(
            payload["issue"]["culprit"],
            "POST /api/v1/invoices/{id}/charge"
        );
        assert_eq!(payload["environment"], "production");
        assert_eq!(payload["release"], "20261026-091512-3f2a9c1");
        assert!(payload.get("test").is_none());
        for key in [
            "id",
            "title",
            "kind",
            "level",
            "status",
            "culprit",
            "count",
            "users",
            "first_seen",
            "last_seen",
            "url",
        ] {
            assert!(payload["issue"].get(key).is_some(), "{key}");
        }
    }

    #[test]
    fn slack_messages_have_a_button() {
        let message = slack_message(&alert());
        assert_eq!(
            message["blocks"][1]["elements"][0]["url"],
            "https://api.crm.example.com/_lighthouse/issues/x"
        );
        assert!(message["text"].as_str().unwrap().starts_with("New issue: "));
    }

    #[test]
    fn kinds_round_trip_and_escape_works() {
        for kind in AlertKind::ALL {
            assert_eq!(AlertKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(
            escape("<a href='x'>&</a>"),
            "&lt;a href=&#39;x&#39;&gt;&amp;&lt;/a&gt;"
        );
        assert!(email_text(&alert()).contains("Open in Lighthouse: https://"));
    }
}
