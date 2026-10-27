//! The alert pipeline: which alerts to raise after a batch, the cooldown,
//! one `lighthouse_alerts` row per channel, and delivery (a
//! `lighthouse.alert` job when the app has a queue, at once otherwise).

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use chrono::{TimeDelta, Utc};
use navree_contracts::Dispatcher;
use navree_http_client::{HttpClient, HttpClientOptions};
use navree_kernel::{AppContext, BoxError};
use navree_mail::Mailer;
use uuid::Uuid;

use crate::alert::{
    Alert, AlertChannel, AlertKind, DeliverAlert, EmailChannel, SlackChannel, WebhookChannel,
    webhook_key,
};
use crate::model::{Issue, IssueKind, IssueStatus, Level};
use crate::options::{DEFAULT_ALERT_KINDS, LighthouseOptions};
use crate::store::{Candidate, LighthouseError, LighthouseStore};

/// How long one delivery may take when it is sent at once.
const SEND_TIMEOUT: Duration = Duration::from_secs(30);

/// Raises and delivers alerts.
pub(crate) struct Alerter {
    store: LighthouseStore,
    options: Arc<LighthouseOptions>,
    environment: String,
    release: String,
    channels: OnceLock<Vec<Arc<dyn AlertChannel>>>,
    dispatcher: OnceLock<Option<Arc<dyn Dispatcher>>>,
}

/// What happened to one channel's delivery of a test alert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelResult {
    /// The channel's name.
    pub channel: String,
    /// `Ok` when it was delivered, the error otherwise.
    pub result: Result<(), String>,
}

impl Alerter {
    pub(crate) fn new(
        store: LighthouseStore,
        options: Arc<LighthouseOptions>,
        environment: String,
        release: String,
    ) -> Alerter {
        Alerter {
            store,
            options,
            environment,
            release,
            channels: OnceLock::new(),
            dispatcher: OnceLock::new(),
        }
    }

    /// Builds the channels (the configured email, Slack and webhook ones,
    /// then every registered `dyn AlertChannel`) and finds the queue.
    pub(crate) async fn init(&self, ctx: &AppContext) -> Result<(), BoxError> {
        let mut channels: Vec<Arc<dyn AlertChannel>> = Vec::new();
        let alerts = &self.options.alerts;
        let kinds =
            |on: &Option<Vec<AlertKind>>| on.clone().unwrap_or(DEFAULT_ALERT_KINDS.to_vec());
        if !alerts.email.to.is_empty() {
            channels.push(Arc::new(EmailChannel {
                to: alerts.email.to.clone(),
                on: kinds(&alerts.email.on),
                mailer: ctx.try_get::<Mailer>().await,
            }));
        }
        let needs_client = alerts.slack.webhook_url.is_some() || alerts.webhook.url.is_some();
        if needs_client {
            let client = match ctx.try_get::<HttpClient>().await {
                Some(client) => (*client).clone(),
                None => HttpClient::new(HttpClientOptions::default())?,
            };
            if let Some(url) = &alerts.slack.webhook_url {
                channels.push(Arc::new(SlackChannel {
                    url: url.expose().clone(),
                    on: kinds(&alerts.slack.on),
                    client: client.clone(),
                }));
            }
            if let (Some(url), Some(secret)) = (&alerts.webhook.url, &alerts.webhook.secret) {
                channels.push(Arc::new(WebhookChannel {
                    url: url.clone(),
                    key: webhook_key(secret.expose()),
                    on: kinds(&alerts.webhook.on),
                    client,
                }));
            }
        }
        channels.extend(ctx.get_all::<dyn AlertChannel>().await?);
        let _ = self.channels.set(channels);
        let _ = self.dispatcher.set(ctx.try_get::<dyn Dispatcher>().await);
        Ok(())
    }

    /// The channels (none before [`init`](Self::init)).
    pub(crate) fn channels(&self) -> &[Arc<dyn AlertChannel>] {
        self.channels.get().map_or(&[], Vec::as_slice)
    }

    fn dispatcher(&self) -> Option<&Arc<dyn Dispatcher>> {
        self.dispatcher.get().and_then(Option::as_ref)
    }

    /// Raises the alerts a batch asks for; errors are logged.
    pub(crate) async fn consider(&self, candidates: Vec<Candidate>) {
        for candidate in candidates {
            let issue = match self.store.issue_for_alert(&candidate.issue).await {
                Ok(issue) => issue,
                Err(error) => {
                    tracing::warn!(target: "navree::lighthouse", %error, "could not read an issue to alert about");
                    continue;
                }
            };
            let alert = self.alert(candidate.kind, issue, false);
            if let Err(error) = self.raise(alert).await {
                tracing::warn!(target: "navree::lighthouse", %error, "could not raise an alert");
            }
        }
    }

    /// An alert of `kind` about `issue`, now.
    pub(crate) fn alert(&self, kind: AlertKind, issue: Issue, test: bool) -> Alert {
        Alert {
            id: Uuid::now_v7(),
            kind,
            issue,
            environment: self.environment.clone(),
            release: self.release.clone(),
            at: Utc::now(),
            test,
        }
    }

    /// Raises `alert` unless one of its kind about its issue was raised
    /// within `alerts.cooldown_minutes`, or its issue is ignored or at
    /// level `info`: one `lighthouse_alerts` row per channel that wants it,
    /// each delivered through the queue, or at once without one. Returns
    /// the number of channels it went to.
    pub(crate) async fn raise(&self, alert: Alert) -> Result<usize, LighthouseError> {
        if alert.issue.status == IssueStatus::Ignored || alert.issue.level == Level::Info {
            return Ok(0);
        }
        let cooldown = TimeDelta::minutes(i64::from(self.options.alerts.cooldown_minutes));
        let subject = alert.issue.id.to_string();
        if self
            .store
            .alerted_since(&subject, alert.kind, alert.at - cooldown)
            .await?
        {
            return Ok(0);
        }
        let mut sent = 0;
        for channel in self.channels() {
            if !channel.wants(&alert) {
                continue;
            }
            let id = Uuid::now_v7();
            self.store.insert_alert(id, &alert, channel.name()).await?;
            sent += 1;
            if let Some(dispatcher) = self.dispatcher() {
                match dispatcher.dispatch(DeliverAlert { alert_id: id }).await {
                    Ok(_) => continue,
                    Err(error) => tracing::warn!(
                        target: "navree::lighthouse",
                        %error,
                        "could not queue an alert; sending it now"
                    ),
                }
            }
            let result = send(channel.as_ref(), &alert).await;
            self.store
                .record_delivery(id, result.map_err(|e| e.to_string()), 1, true)
                .await?;
        }
        Ok(sent)
    }

    /// Delivers the stored alert `id` (the `lighthouse.alert` job): attempt
    /// `attempt` of `max`. An error is returned so the queue retries it.
    pub(crate) async fn deliver_stored(
        &self,
        id: Uuid,
        attempt: u32,
        max: u32,
    ) -> Result<(), BoxError> {
        let Some(row) = self.store.alert_row(id).await? else {
            return Ok(());
        };
        if row.status == "delivered" {
            return Ok(());
        }
        let alert: Alert = serde_json::from_str(&row.payload)?;
        let Some(channel) = self.channels().iter().find(|c| c.name() == row.channel) else {
            let error = format!("no alert channel named `{}`", row.channel);
            self.store
                .record_delivery(id, Err(error.clone()), attempt as i32, true)
                .await?;
            return Ok(());
        };
        let result = send(channel.as_ref(), &alert).await;
        let last = attempt >= max.max(1);
        self.store
            .record_delivery(
                id,
                result.as_ref().map(|_| ()).map_err(ToString::to_string),
                i32::try_from(attempt).unwrap_or(i32::MAX),
                last,
            )
            .await?;
        result
    }

    /// Sends a test alert through every channel, or the one named
    /// `channel`, and reports each result. Nothing is stored.
    pub(crate) async fn test(
        &self,
        channel: Option<&str>,
    ) -> Result<Vec<ChannelResult>, LighthouseError> {
        let channels: Vec<&Arc<dyn AlertChannel>> = self
            .channels()
            .iter()
            .filter(|c| channel.is_none_or(|name| c.name() == name))
            .collect();
        if let Some(name) = channel
            && channels.is_empty()
        {
            let names: Vec<&str> = self.channels().iter().map(|c| c.name()).collect();
            return Err(LighthouseError::Other(format!(
                "no alert channel named `{name}` (configured: {})",
                if names.is_empty() {
                    "none".to_owned()
                } else {
                    names.join(", ")
                }
            )));
        }
        let now = Utc::now();
        let issue = Issue {
            id: Uuid::nil(),
            title: "Test alert from Lighthouse".to_owned(),
            kind: IssueKind::Exception,
            level: Level::Error,
            status: IssueStatus::Open,
            culprit: "lighthouse:test-alerts".to_owned(),
            count: 1,
            users: 0,
            first_seen: now,
            last_seen: now,
            url: self.store.base_url().to_owned(),
        };
        let alert = self.alert(AlertKind::NewIssue, issue, true);
        let mut results = Vec::new();
        for channel in channels {
            results.push(ChannelResult {
                channel: channel.name().to_owned(),
                result: send(channel.as_ref(), &alert)
                    .await
                    .map_err(|error| error.to_string()),
            });
        }
        Ok(results)
    }
}

/// Sends `alert` through `channel`, within [`SEND_TIMEOUT`].
async fn send(channel: &dyn AlertChannel, alert: &Alert) -> Result<(), BoxError> {
    match tokio::time::timeout(SEND_TIMEOUT, channel.send(alert)).await {
        Ok(result) => result,
        Err(_) => Err(format!("timed out after {}s", SEND_TIMEOUT.as_secs()).into()),
    }
}
