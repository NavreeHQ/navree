//! Mailboxes of connections (crate feature `mail`): Microsoft 365 and Gmail
//! mailboxes watched for new mail, each message received through
//! navree-mail's inbound pipeline.
//!
//! - Microsoft 365: a Graph subscription on the mailbox's inbox; Graph
//!   posts a notification per new message to `POST /api/mail/inbound/graph`
//!   (checked with the watch's `clientState`), and the message's MIME is
//!   read from `/users/{mailbox}/messages/{id}/$value`.
//! - Gmail: `users.watch` publishes changes to a Pub/Sub topic whose push
//!   subscription posts to `POST /api/mail/inbound/gmail?token=...`; the
//!   new messages are read from the mailbox's history since the watch's
//!   cursor, each in `format=raw`.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
use chrono::{DateTime, TimeDelta, Utc};
use http::{Method, StatusCode, header};
use navree_config::ConfigTable;
use navree_contracts::Secret;
use navree_db::MigrationSet;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaResult,
};
use navree_http::body::Bytes;
use navree_http::{ApiMeta, AppError, Controller, IntoResponse, Query, Response, RouteDef};
use navree_kernel::{
    AppContext, BoxError, BoxFuture, CommandSpec, Injectable, KernelError, ProviderKey, Resolver,
};
use navree_mail::{Inbound, Incoming, Received};
use navree_orm::{Model, Record};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::runtime::Runtime;
use crate::{
    CredentialKind, GoogleClient, GraphClient, Integrations, IntegrationsError, ProviderType,
};

/// The table of mailbox watches.
pub(crate) const MAILBOX_WATCHES_TABLE: &str = "integration_mailbox_watches";

/// The key a Graph subscription's `clientState` is derived with.
const CLIENT_STATE_PURPOSE: &str = "integrations-mailbox-client-state";

/// How long a Graph subscription on messages lasts (70 hours: Graph allows
/// a little under seven days).
const GRAPH_SUBSCRIPTION_MINUTES: i64 = 70 * 60;

/// The Gmail API.
const GMAIL_API: &str = "https://gmail.googleapis.com/gmail/v1";

/// The most messages one notification or sync reads.
const MAX_MESSAGES: usize = 500;

/// `integrations.mailboxes` (crate feature `mail`):
///
/// ```toml
/// [integrations.mailboxes]
/// graph_notification_url = "https://app.acme.test/api/mail/inbound/graph"
/// gmail_topic = "projects/acme-prod/topics/gmail-inbound"
/// gmail_token = "..."          # the Pub/Sub push endpoint's ?token=
/// renew_before_hours = 24      # integrations:mailboxes --renew renews watches expiring sooner
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct MailboxesOptions {
    /// The public URL of `POST /api/mail/inbound/graph`, which Graph posts
    /// notifications to. Needed to watch Microsoft 365 mailboxes; the
    /// route is mounted when it is set.
    pub graph_notification_url: Option<String>,
    /// The Pub/Sub topic Gmail publishes changes to
    /// (`projects/{project}/topics/{topic}`). Needed to watch Gmail
    /// mailboxes.
    pub gmail_topic: Option<String>,
    /// The secret in the push subscription's endpoint
    /// (`.../api/mail/inbound/gmail?token=...`); the route is mounted when
    /// it is set.
    pub gmail_token: Option<Secret<String>>,
    /// `integrations:mailboxes --renew` (scheduled hourly) renews the
    /// watches that expire within this many hours (default 24).
    pub renew_before_hours: u32,
}

impl Default for MailboxesOptions {
    fn default() -> Self {
        Self {
            graph_notification_url: None,
            gmail_topic: None,
            gmail_token: None,
            renew_before_hours: 24,
        }
    }
}

impl MailboxesOptions {
    /// Adds a message per problem, each naming its key.
    pub(crate) fn check(&self, errors: &mut Vec<String>) {
        if let Some(url) = &self.graph_notification_url {
            match url::Url::parse(url) {
                Ok(parsed) if parsed.scheme() == "https" || parsed.scheme() == "http" => {}
                _ => errors.push(format!(
                    "integrations.mailboxes.graph_notification_url must be an absolute http(s) URL, got `{url}`"
                )),
            }
        }
        if let Some(topic) = &self.gmail_topic {
            let parts: Vec<&str> = topic.split('/').collect();
            if parts.len() != 4
                || parts[0] != "projects"
                || parts[2] != "topics"
                || parts[1].is_empty()
                || parts[3].is_empty()
            {
                errors.push(format!(
                    "integrations.mailboxes.gmail_topic must be `projects/{{project}}/topics/{{topic}}`, got `{topic}`"
                ));
            }
        }
        if self
            .gmail_token
            .as_ref()
            .is_some_and(|token| token.expose().trim().is_empty())
        {
            errors.push("integrations.mailboxes.gmail_token must not be empty when set".to_owned());
        }
        if self.renew_before_hours == 0 {
            errors.push("integrations.mailboxes.renew_before_hours must be at least 1".to_owned());
        }
    }
}

/// A row of `integration_mailbox_watches`: one mailbox of a connection,
/// watched for new mail (unique per connection and mailbox).
#[derive(Model, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[model(table = "integration_mailbox_watches")]
pub struct MailboxWatch {
    /// The id (a UUID v7).
    pub id: Uuid,
    /// The connection whose token reads the mailbox.
    pub connection_id: Uuid,
    /// `microsoft` or `google`.
    pub provider: String,
    /// The mailbox's address, in lower case.
    pub mailbox: String,
    /// The Graph subscription id (Microsoft 365).
    pub subscription_id: Option<String>,
    /// The last history id read (Gmail).
    pub cursor: Option<String>,
    /// `active`, or `failing` after an error (see `last_error`).
    pub status: String,
    /// The last error.
    pub last_error: Option<String>,
    /// When the subscription or watch expires at the provider.
    pub expires_at: Option<DateTime<Utc>>,
    /// When a message last arrived through it.
    pub last_received_at: Option<DateTime<Utc>>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl fmt::Debug for MailboxWatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MailboxWatch")
            .field("id", &self.id)
            .field("connection_id", &self.connection_id)
            .field("provider", &self.provider)
            .field("mailbox", &self.mailbox)
            .field("status", &self.status)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// What [`MailboxWatches::renew_due`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RenewSummary {
    /// Watches renewed.
    pub renewed: usize,
    /// Watches whose renewal failed (now `failing`).
    pub failed: usize,
}

/// Watches the mailboxes of connections for new mail and hands each new
/// message to navree-mail's [`Inbound`]: provided by `IntegrationsModule`
/// with the crate feature `mail` (the app needs `MailModule` with
/// navree-mail's `inbound` feature).
///
/// The connection needs read access to mail: Graph's `Mail.Read`
/// application permission, or Gmail's
/// `https://www.googleapis.com/auth/gmail.readonly` (a service account with
/// domain-wide delegation reads every mailbox of its domain).
pub struct MailboxWatches {
    integrations: Arc<Integrations>,
}

impl MailboxWatches {
    /// The watches of `integrations`' connections.
    pub fn new(integrations: Arc<Integrations>) -> Self {
        Self { integrations }
    }

    fn rt(&self) -> Result<&Arc<Runtime>, IntegrationsError> {
        self.integrations.rt()
    }

    fn options(&self) -> Result<&MailboxesOptions, IntegrationsError> {
        Ok(&self.rt()?.options.mailboxes)
    }

    /// The app's inbound pipeline.
    async fn inbound(&self) -> Result<Arc<Inbound>, IntegrationsError> {
        let app = self
            .rt()?
            .app
            .upgrade()
            .ok_or(IntegrationsError::NotBound)?;
        app.try_get::<Inbound>().await.ok_or_else(|| {
            IntegrationsError::Config(
                "watching mailboxes needs MailModule with navree-mail's `inbound` feature"
                    .to_owned(),
            )
        })
    }

    /// The `clientState` of watch `id`'s Graph subscription: an HMAC of
    /// its id under a key derived from the application key.
    ///
    /// # Errors
    ///
    /// [`IntegrationsError::Crypto`] without an application key.
    pub fn client_state(&self, id: Uuid) -> Result<String, IntegrationsError> {
        let key = self.rt()?.encrypter.derive_key(CLIENT_STATE_PURPOSE)?;
        Ok(URL_SAFE_NO_PAD.encode(navree_crypto::hmac_sha256(&key, id.as_bytes())))
    }

    /// Every watch, oldest first.
    pub async fn all(&self) -> Result<Vec<MailboxWatch>, IntegrationsError> {
        let rt = self.rt()?;
        let rows = MailboxWatch::query()
            .order_by(MailboxWatch::CREATED_AT)
            .get(&rt.db)
            .await?;
        Ok(rows.into_iter().map(Record::into_inner).collect())
    }

    /// The watches of connection `connection`.
    pub async fn of_connection(
        &self,
        connection: Uuid,
    ) -> Result<Vec<MailboxWatch>, IntegrationsError> {
        let rt = self.rt()?;
        let rows = MailboxWatch::query()
            .where_(MailboxWatch::CONNECTION_ID.eq(connection))
            .order_by(MailboxWatch::MAILBOX)
            .get(&rt.db)
            .await?;
        Ok(rows.into_iter().map(Record::into_inner).collect())
    }

    /// The watch `id`.
    ///
    /// # Errors
    ///
    /// [`IntegrationsError::MailboxWatchNotFound`].
    pub async fn find(&self, id: Uuid) -> Result<MailboxWatch, IntegrationsError> {
        let rt = self.rt()?;
        MailboxWatch::find(&rt.db, id)
            .await?
            .map(Record::into_inner)
            .ok_or(IntegrationsError::MailboxWatchNotFound)
    }

    /// Starts watching `mailbox` with connection `connection` (a
    /// `microsoft` or `google` connection), or renews the watch when there
    /// is one: a Graph subscription on the inbox, or Gmail's `users.watch`
    /// on `INBOX`.
    ///
    /// # Errors
    ///
    /// [`IntegrationsError::Unsupported`] for other providers,
    /// [`IntegrationsError::Config`] without
    /// `integrations.mailboxes.graph_notification_url` (Microsoft) or
    /// `gmail_topic` (Google), and the provider's refusals.
    pub async fn watch(
        &self,
        connection: Uuid,
        mailbox: &str,
    ) -> Result<MailboxWatch, IntegrationsError> {
        let rt = self.rt()?.clone();
        let info = self.integrations.find(connection).await?;
        let provider = provider_type(&rt, &info.provider)?;
        let mailbox = mailbox.trim().to_ascii_lowercase();
        if mailbox.is_empty() || !mailbox.contains('@') {
            return Err(IntegrationsError::Config(format!(
                "`{mailbox}` is not a mailbox address"
            )));
        }
        let existing = MailboxWatch::query()
            .where_(MailboxWatch::CONNECTION_ID.eq(connection))
            .where_(MailboxWatch::MAILBOX.eq(mailbox.as_str()))
            .first(&rt.db)
            .await?;
        if let Some(existing) = existing {
            return self.renew(existing.id).await;
        }
        let now = rt.now();
        let mut watch = MailboxWatch {
            id: Uuid::now_v7(),
            connection_id: connection,
            provider: provider.as_str().to_owned(),
            mailbox,
            status: "active".to_owned(),
            created_at: now,
            updated_at: now,
            ..MailboxWatch::default()
        };
        match provider {
            ProviderType::Microsoft => {
                let (id, expires) = self.graph_subscribe(&watch).await?;
                watch.subscription_id = Some(id);
                watch.expires_at = expires;
            }
            ProviderType::Google => {
                let (history, expires) = self.gmail_watch(&watch).await?;
                watch.cursor = Some(history);
                watch.expires_at = expires;
            }
            _ => unreachable!("provider_type only returns Microsoft or Google"),
        }
        let saved = watch.clone();
        rt.housekeeping(|uow| async move {
            // Inserted as it is, with no change event.
            MailboxWatch::insert(&uow, [saved]).await?;
            uow.mark_modified();
            Ok(())
        })
        .await?;
        tracing::info!(target: "navree::integrations", watch = %watch.id, provider = %watch.provider, "mailbox watched");
        Ok(watch)
    }

    /// Renews watch `id` at the provider (a new expiry; Gmail's watch
    /// keeps its cursor).
    ///
    /// # Errors
    ///
    /// [`IntegrationsError::MailboxWatchNotFound`] and the provider's
    /// refusals (the watch turns `failing`).
    pub async fn renew(&self, id: Uuid) -> Result<MailboxWatch, IntegrationsError> {
        let rt = self.rt()?.clone();
        let mut watch = self.find(id).await?;
        let outcome = match watch.provider.as_str() {
            "microsoft" => match &watch.subscription_id {
                Some(subscription) => self
                    .graph_renew(&watch, subscription)
                    .await
                    .map(|expires| (watch.subscription_id.clone(), watch.cursor.clone(), expires)),
                None => self
                    .graph_subscribe(&watch)
                    .await
                    .map(|(id, expires)| (Some(id), None, expires)),
            },
            _ => self
                .gmail_watch(&watch)
                .await
                .map(|(history, expires)| (None, watch.cursor.clone().or(Some(history)), expires)),
        };
        match outcome {
            Ok((subscription, cursor, expires)) => {
                watch.subscription_id = subscription;
                watch.cursor = cursor;
                watch.expires_at = expires;
                watch.status = "active".to_owned();
                watch.last_error = None;
                watch.updated_at = rt.now();
                self.save(&watch).await?;
                Ok(watch)
            }
            Err(error) => {
                self.failing(&watch, &error).await;
                Err(error)
            }
        }
    }

    /// Renews every watch expiring within
    /// `integrations.mailboxes.renew_before_hours`, and the failing ones
    /// (what `integrations:mailboxes --renew` runs hourly).
    pub async fn renew_due(&self) -> Result<RenewSummary, IntegrationsError> {
        let rt = self.rt()?.clone();
        let horizon = rt.now() + TimeDelta::hours(i64::from(self.options()?.renew_before_hours));
        let mut summary = RenewSummary::default();
        for watch in self.all().await? {
            let due = watch.status != "active" || watch.expires_at.is_none_or(|at| at <= horizon);
            if !due {
                continue;
            }
            match self.renew(watch.id).await {
                Ok(_) => summary.renewed += 1,
                Err(error) => {
                    summary.failed += 1;
                    tracing::warn!(target: "navree::integrations", watch = %watch.id, %error, "renewing a mailbox watch failed");
                }
            }
        }
        Ok(summary)
    }

    /// Stops watching: deletes the Graph subscription or stops Gmail's
    /// watch (best effort), then the row.
    pub async fn unwatch(&self, id: Uuid) -> Result<(), IntegrationsError> {
        let rt = self.rt()?.clone();
        let watch = self.find(id).await?;
        let stopped = match watch.provider.as_str() {
            "microsoft" => match &watch.subscription_id {
                Some(subscription) => {
                    async {
                        let graph = self.graph(&watch).await?;
                        graph
                            .delete(format!("/subscriptions/{subscription}"))
                            .send()
                            .await?;
                        Ok::<(), IntegrationsError>(())
                    }
                    .await
                }
                None => Ok(()),
            },
            _ => {
                async {
                    let gmail = self.gmail(&watch).await?;
                    gmail
                        .post(format!("{GMAIL_API}/users/{}/stop", user_id(&watch)))
                        .send()
                        .await?;
                    Ok(())
                }
                .await
            }
        };
        if let Err(error) = stopped {
            tracing::warn!(target: "navree::integrations", watch = %id, %error, "could not stop the mailbox watch at the provider");
        }
        rt.housekeeping(|uow| async move {
            MailboxWatch::query()
                .where_(MailboxWatch::ID.eq(id))
                .delete(&uow)
                .await?;
            Ok(())
        })
        .await
    }

    /// Reads what arrived in watch `id`'s mailbox since its cursor (Gmail)
    /// or since its last message (Microsoft 365, the last day at most),
    /// and receives it. A catch-up after missed notifications; messages
    /// already received may come again, as with any provider retry.
    pub async fn sync(&self, id: Uuid) -> Result<Vec<Received>, IntegrationsError> {
        let watch = self.find(id).await?;
        match watch.provider.as_str() {
            "microsoft" => self.graph_sync(&watch).await,
            _ => self.gmail_history(&watch, None).await,
        }
    }

    // -- Microsoft Graph ------------------------------------------------------

    async fn graph(&self, watch: &MailboxWatch) -> Result<GraphClient, IntegrationsError> {
        let info = self.integrations.find(watch.connection_id).await?;
        Ok(self.integrations.client_of(&info).await?.into())
    }

    /// Creates the subscription: (id, expiry).
    async fn graph_subscribe(
        &self,
        watch: &MailboxWatch,
    ) -> Result<(String, Option<DateTime<Utc>>), IntegrationsError> {
        let rt = self.rt()?;
        let url = self
            .options()?
            .graph_notification_url
            .clone()
            .ok_or_else(|| {
                IntegrationsError::Config(
                    "integrations.mailboxes.graph_notification_url is not set".to_owned(),
                )
            })?;
        let expires = rt.now() + TimeDelta::minutes(GRAPH_SUBSCRIPTION_MINUTES);
        let graph = self.graph(watch).await?;
        let created: Value = graph
            .post("/subscriptions")
            .json(&json!({
                "changeType": "created",
                "notificationUrl": url,
                "lifecycleNotificationUrl": url,
                "resource": format!("users/{}/mailFolders('inbox')/messages", watch.mailbox),
                "expirationDateTime": expires.to_rfc3339(),
                "clientState": self.client_state(watch.id)?,
            }))
            .send_json()
            .await?;
        let id = created["id"]
            .as_str()
            .ok_or_else(|| api_error("Microsoft 365", "the subscription has no id"))?
            .to_owned();
        Ok((
            id,
            parse_time(&created["expirationDateTime"]).or(Some(expires)),
        ))
    }

    /// Extends the subscription: the new expiry.
    async fn graph_renew(
        &self,
        watch: &MailboxWatch,
        subscription: &str,
    ) -> Result<Option<DateTime<Utc>>, IntegrationsError> {
        let rt = self.rt()?;
        let expires = rt.now() + TimeDelta::minutes(GRAPH_SUBSCRIPTION_MINUTES);
        let graph = self.graph(watch).await?;
        let renewed: Value = graph
            .patch(format!("/subscriptions/{subscription}"))
            .json(&json!({ "expirationDateTime": expires.to_rfc3339() }))
            .send_json()
            .await?;
        Ok(parse_time(&renewed["expirationDateTime"]).or(Some(expires)))
    }

    /// Reads message `message` of the watch's mailbox as MIME and receives
    /// it.
    async fn graph_receive(
        &self,
        watch: &MailboxWatch,
        message: &str,
    ) -> Result<Received, IntegrationsError> {
        let graph = self.graph(watch).await?;
        let response = graph
            .get(format!(
                "/users/{}/messages/{message}/$value",
                watch.mailbox
            ))
            .send()
            .await?
            .error_for_status()?;
        let raw = response.bytes();
        self.receive(
            watch,
            Incoming::new("graph", raw)
                .provider_id(message)
                .recipient(watch.mailbox.clone()),
        )
        .await
    }

    /// Handles a Graph notification body (`{"value": [...]}`): each
    /// `created` change of a known subscription whose `clientState`
    /// matches is read and received; lifecycle notifications renew
    /// (`reauthorizationRequired`), catch up (`missed`) or mark the watch
    /// failing (`subscriptionRemoved`). Notifications with a wrong
    /// `clientState` are ignored.
    ///
    /// # Errors
    ///
    /// The first error reading or receiving a message (the route answers
    /// 500, so Graph sends the notification again).
    pub async fn graph_notifications(
        &self,
        payload: &Value,
    ) -> Result<Vec<Received>, IntegrationsError> {
        let rt = self.rt()?.clone();
        let mut received = Vec::new();
        for notification in payload["value"].as_array().into_iter().flatten() {
            let Some(subscription) = notification["subscriptionId"].as_str() else {
                continue;
            };
            let Some(watch) = MailboxWatch::query()
                .where_(MailboxWatch::SUBSCRIPTION_ID.eq(subscription))
                .first(&rt.db)
                .await?
                .map(Record::into_inner)
            else {
                tracing::warn!(target: "navree::integrations", subscription, "a Graph notification for an unknown subscription was ignored");
                continue;
            };
            let given = notification["clientState"].as_str().unwrap_or_default();
            let expected = self.client_state(watch.id)?;
            if !navree_crypto::constant_time_eq(given.as_bytes(), expected.as_bytes()) {
                tracing::warn!(target: "navree::integrations", watch = %watch.id, "a Graph notification with a wrong clientState was ignored");
                continue;
            }
            if let Some(event) = notification["lifecycleEvent"].as_str() {
                match event {
                    "reauthorizationRequired" => {
                        self.renew(watch.id).await?;
                    }
                    "missed" => received.extend(self.graph_sync(&watch).await?),
                    "subscriptionRemoved" => {
                        let mut watch = watch;
                        watch.subscription_id = None;
                        watch.status = "failing".to_owned();
                        watch.last_error = Some("Graph removed the subscription".to_owned());
                        watch.updated_at = rt.now();
                        self.save(&watch).await?;
                    }
                    other => {
                        tracing::debug!(target: "navree::integrations", event = other, "Graph lifecycle notification ignored");
                    }
                }
                continue;
            }
            if notification["changeType"].as_str() != Some("created") {
                continue;
            }
            let message = notification["resourceData"]["id"].as_str().or_else(|| {
                notification["resource"]
                    .as_str()
                    .and_then(|resource| resource.rsplit('/').next())
            });
            let Some(message) = message else {
                continue;
            };
            received.push(self.graph_receive(&watch, message).await?);
        }
        Ok(received)
    }

    /// Reads the inbox's messages since the last one received (at most a
    /// day back) and receives them.
    async fn graph_sync(&self, watch: &MailboxWatch) -> Result<Vec<Received>, IntegrationsError> {
        let rt = self.rt()?;
        let floor = rt.now() - TimeDelta::days(1);
        let since = watch
            .last_received_at
            .filter(|at| *at > floor)
            .unwrap_or(floor);
        let graph = self.graph(watch).await?;
        #[derive(Deserialize)]
        struct Item {
            id: String,
        }
        let items: Vec<Item> = graph
            .paginate(&format!(
                "/users/{}/mailFolders/inbox/messages?$select=id&$orderby=receivedDateTime&$filter=receivedDateTime ge {}",
                watch.mailbox,
                since.format("%Y-%m-%dT%H:%M:%SZ")
            ))
            .await?;
        let mut received = Vec::new();
        for item in items.into_iter().take(MAX_MESSAGES) {
            received.push(self.graph_receive(watch, &item.id).await?);
        }
        Ok(received)
    }

    // -- Gmail ---------------------------------------------------------------

    async fn gmail(&self, watch: &MailboxWatch) -> Result<GoogleClient, IntegrationsError> {
        let info = self.integrations.find(watch.connection_id).await?;
        let client: GoogleClient = self.integrations.client_of(&info).await?.into();
        Ok(if info.kind == CredentialKind::ServiceAccount {
            client.as_user(watch.mailbox.clone())
        } else {
            client
        })
    }

    /// Calls `users.watch`: (history id, expiry).
    async fn gmail_watch(
        &self,
        watch: &MailboxWatch,
    ) -> Result<(String, Option<DateTime<Utc>>), IntegrationsError> {
        let topic = self.options()?.gmail_topic.clone().ok_or_else(|| {
            IntegrationsError::Config("integrations.mailboxes.gmail_topic is not set".to_owned())
        })?;
        let gmail = self.gmail(watch).await?;
        let answer: Value = gmail
            .post(format!("{GMAIL_API}/users/{}/watch", user_id(watch)))
            .json(&json!({
                "topicName": topic,
                "labelIds": ["INBOX"],
                "labelFilterBehavior": "INCLUDE",
            }))
            .send_json()
            .await?;
        let history = text(&answer["historyId"])
            .ok_or_else(|| api_error("Google Workspace", "users.watch gave no historyId"))?;
        let expires = text(&answer["expiration"])
            .and_then(|ms| ms.parse::<i64>().ok())
            .and_then(DateTime::from_timestamp_millis);
        Ok((history, expires))
    }

    /// Handles a Pub/Sub push body: the mailbox in its data names the
    /// watch, whose history since its cursor is read and received.
    ///
    /// # Errors
    ///
    /// The first error reading or receiving a message (the route answers
    /// 500, so Pub/Sub pushes again).
    pub async fn gmail_push(&self, payload: &Value) -> Result<Vec<Received>, IntegrationsError> {
        let rt = self.rt()?.clone();
        let data = payload["message"]["data"].as_str().unwrap_or_default();
        let decoded = STANDARD
            .decode(data.trim())
            .or_else(|_| URL_SAFE.decode(data.trim()))
            .map_err(|_| api_error("Google Workspace", "the push message's data is not base64"))?;
        let change: Value = serde_json::from_slice(&decoded)
            .map_err(|_| api_error("Google Workspace", "the push message's data is not JSON"))?;
        let mailbox = change["emailAddress"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let watches = MailboxWatch::query()
            .where_(MailboxWatch::PROVIDER.eq("google"))
            .where_(MailboxWatch::MAILBOX.eq(mailbox.as_str()))
            .get(&rt.db)
            .await?;
        let mut received = Vec::new();
        for watch in watches {
            received.extend(
                self.gmail_history(&watch.into_inner(), text(&change["historyId"]))
                    .await?,
            );
        }
        Ok(received)
    }

    /// Reads the mailbox's history since the watch's cursor, receives each
    /// added message, and moves the cursor. A cursor Gmail no longer knows
    /// (404) restarts from `latest`.
    async fn gmail_history(
        &self,
        watch: &MailboxWatch,
        latest: Option<String>,
    ) -> Result<Vec<Received>, IntegrationsError> {
        let rt = self.rt()?.clone();
        let Some(cursor) = watch.cursor.clone().or_else(|| latest.clone()) else {
            return Ok(Vec::new());
        };
        let gmail = self.gmail(watch).await?;
        let mut ids: Vec<String> = Vec::new();
        let mut next_cursor = latest.clone();
        let mut page_token: Option<String> = None;
        loop {
            let mut request = gmail
                .get(format!("{GMAIL_API}/users/{}/history", user_id(watch)))
                .query(&[
                    ("startHistoryId", cursor.as_str()),
                    ("historyTypes", "messageAdded"),
                    ("labelId", "INBOX"),
                ]);
            if let Some(token) = &page_token {
                request = request.query(&[("pageToken", token.as_str())]);
            }
            let response = request.send().await?;
            if response.status().as_u16() == 404 {
                tracing::warn!(target: "navree::integrations", watch = %watch.id, "Gmail no longer knows the watch's cursor: restarting from the latest history");
                let mut watch = watch.clone();
                watch.cursor = latest;
                watch.updated_at = rt.now();
                self.save(&watch).await?;
                return Ok(Vec::new());
            }
            let page: Value = response.error_for_status()?.json()?;
            for entry in page["history"].as_array().into_iter().flatten() {
                for added in entry["messagesAdded"].as_array().into_iter().flatten() {
                    if let Some(id) = added["message"]["id"].as_str()
                        && !ids.iter().any(|known| known == id)
                    {
                        ids.push(id.to_owned());
                    }
                }
            }
            if let Some(history) = text(&page["historyId"]) {
                next_cursor = Some(history);
            }
            page_token = page["nextPageToken"].as_str().map(str::to_owned);
            if page_token.is_none() || ids.len() >= MAX_MESSAGES {
                break;
            }
        }
        let mut received = Vec::new();
        for id in ids.iter().take(MAX_MESSAGES) {
            let message: Value = gmail
                .get(format!(
                    "{GMAIL_API}/users/{}/messages/{id}",
                    user_id(watch)
                ))
                .query(&[("format", "raw")])
                .send_json()
                .await?;
            let raw = message["raw"].as_str().unwrap_or_default();
            let bytes = URL_SAFE_NO_PAD
                .decode(raw.trim_end_matches('='))
                .map_err(|_| api_error("Google Workspace", "a raw message is not base64url"))?;
            received.push(
                self.receive(
                    watch,
                    Incoming::new("gmail", bytes)
                        .provider_id(id.clone())
                        .recipient(watch.mailbox.clone()),
                )
                .await?,
            );
        }
        let mut watch = watch.clone();
        if next_cursor.is_some() {
            watch.cursor = next_cursor;
        }
        watch.updated_at = rt.now();
        self.save(&watch).await?;
        Ok(received)
    }

    // -- shared ---------------------------------------------------------------

    /// Hands one message to the inbound pipeline and notes its arrival.
    async fn receive(
        &self,
        watch: &MailboxWatch,
        incoming: Incoming,
    ) -> Result<Received, IntegrationsError> {
        let inbound = self.inbound().await?;
        let received = inbound.receive(incoming).await?;
        let rt = self.rt()?.clone();
        let (id, now) = (watch.id, rt.now());
        rt.housekeeping(|uow| async move {
            MailboxWatch::query()
                .where_(MailboxWatch::ID.eq(id))
                .update(
                    &uow,
                    vec![
                        MailboxWatch::LAST_RECEIVED_AT.set(Some(now)),
                        MailboxWatch::UPDATED_AT.set(now),
                    ],
                )
                .await?;
            Ok(())
        })
        .await?;
        Ok(received)
    }

    /// Writes `watch` back.
    async fn save(&self, watch: &MailboxWatch) -> Result<(), IntegrationsError> {
        let rt = self.rt()?.clone();
        let watch = watch.clone();
        rt.housekeeping(|uow| async move {
            MailboxWatch::query()
                .where_(MailboxWatch::ID.eq(watch.id))
                .update(
                    &uow,
                    vec![
                        MailboxWatch::SUBSCRIPTION_ID.set(watch.subscription_id),
                        MailboxWatch::CURSOR.set(watch.cursor),
                        MailboxWatch::STATUS.set(watch.status),
                        MailboxWatch::LAST_ERROR.set(watch.last_error),
                        MailboxWatch::EXPIRES_AT.set(watch.expires_at),
                        MailboxWatch::UPDATED_AT.set(watch.updated_at),
                    ],
                )
                .await?;
            Ok(())
        })
        .await
    }

    /// Marks `watch` failing with `error` (logged when that fails too).
    async fn failing(&self, watch: &MailboxWatch, error: &IntegrationsError) {
        let Ok(rt) = self.rt() else {
            return;
        };
        let mut failed = watch.clone();
        failed.status = "failing".to_owned();
        failed.last_error = Some(error.to_string());
        failed.updated_at = rt.now();
        if let Err(save) = self.save(&failed).await {
            tracing::warn!(target: "navree::integrations", watch = %watch.id, error = %save, "could not mark the mailbox watch failing");
        }
    }
}

impl fmt::Debug for MailboxWatches {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MailboxWatches").finish_non_exhaustive()
    }
}

/// `microsoft` or `google`, from the connection's provider key.
fn provider_type(rt: &Runtime, key: &str) -> Result<ProviderType, IntegrationsError> {
    let entry = rt.provider(key)?;
    match entry.kind {
        ProviderType::Microsoft | ProviderType::Google => Ok(entry.kind),
        _ => Err(IntegrationsError::Unsupported {
            provider: entry.name.clone(),
            kind: "mailbox watches",
        }),
    }
}

/// The Gmail user id of a watch: the mailbox (a service account acts as
/// it; an OAuth connection's own mailbox answers to its address too).
fn user_id(watch: &MailboxWatch) -> &str {
    &watch.mailbox
}

/// A JSON string or number as text.
fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn parse_time(value: &Value) -> Option<DateTime<Utc>> {
    value
        .as_str()
        .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
        .map(|time| time.with_timezone(&Utc))
}

fn api_error(provider: &str, error: &str) -> IntegrationsError {
    IntegrationsError::Api {
        provider: provider.to_owned(),
        error: error.to_owned(),
    }
}

// -- migration ----------------------------------------------------------------

/// The migration of `integration_mailbox_watches` (in the `integrations`
/// set, with the crate feature `mail`).
pub(crate) fn add_migrations(set: MigrationSet) -> MigrationSet {
    set.migration(navree_db::navree_schema::SchemaMigration::new(
        "m20261028_000001_create_integration_mailbox_watches_table",
        CreateMailboxWatches,
    ))
}

/// Creates `integration_mailbox_watches`: unique per connection and
/// mailbox, found by Graph subscription id and by mailbox.
struct CreateMailboxWatches;

fn uuid<'a>(t: &'a mut Blueprint, name: &str, mysql: bool) -> ColumnBuilder<'a> {
    if mysql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

impl Migration for CreateMailboxWatches {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create(MAILBOX_WATCHES_TABLE, |t| {
                uuid(t, "id", mysql).primary();
                uuid(t, "connection_id", mysql);
                t.string("provider").length(32);
                t.string("mailbox").length(320);
                t.string("subscription_id")
                    .nullable()
                    .index_named("integration_mailbox_watches_subscription_index");
                t.string("cursor").length(64).nullable();
                t.string("status").length(16).default("active");
                t.text("last_error").nullable();
                t.timestamp_tz("expires_at").nullable();
                t.timestamp_tz("last_received_at").nullable();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.unique(["connection_id", "mailbox"])
                    .name("integration_mailbox_watches_mailbox_unique");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists(MAILBOX_WATCHES_TABLE).await
    }
}

// -- routes -------------------------------------------------------------------

/// The routes Microsoft Graph and Gmail's Pub/Sub push subscription post
/// to, under `{http.api_prefix}/mail/inbound` (crate feature `mail`):
///
/// - `POST /mail/inbound/graph` (mounted with
///   `integrations.mailboxes.graph_notification_url`): echoes Graph's
///   `validationToken`, then handles notifications
///   ([`MailboxWatches::graph_notifications`]) and answers 202;
/// - `POST /mail/inbound/gmail?token=..` (mounted with
///   `integrations.mailboxes.gmail_token`): a Pub/Sub push
///   ([`MailboxWatches::gmail_push`]), answered 204; a wrong token is 401
///   `invalid_signature`.
///
/// A failure reading or receiving a message is a 500, so the provider
/// sends the notification again.
pub struct MailboxesController {
    watches: Arc<MailboxWatches>,
    options: MailboxesOptions,
}

impl Injectable for MailboxesController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<MailboxWatches>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let options = r
                .get::<crate::IntegrationsOptions>()
                .await?
                .mailboxes
                .clone();
            Ok(MailboxesController {
                watches: r.get::<MailboxWatches>().await?,
                options,
            })
        })
    }
}

fn api(summary: &str) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Mail".to_owned()],
        responses: vec![400, 401],
        ..ApiMeta::default()
    }
}

impl Controller for MailboxesController {
    fn prefix(&self) -> &'static str {
        "/mail/inbound"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let mut routes = Vec::new();
        if self.options.graph_notification_url.is_some() {
            let watches = self.watches.clone();
            let graph = move |Query(query): Query<HashMap<String, String>>, body: Bytes| {
                let watches = watches.clone();
                async move {
                    if let Some(token) = query.get("validationToken") {
                        return Ok::<Response, AppError>(
                            (
                                StatusCode::OK,
                                [(header::CONTENT_TYPE, "text/plain")],
                                token.clone(),
                            )
                                .into_response(),
                        );
                    }
                    let payload: Value = serde_json::from_slice(&body)
                        .map_err(|_| AppError::bad_request("The body is not JSON"))?;
                    watches
                        .graph_notifications(&payload)
                        .await
                        .map_err(IntegrationsError::into_app_error)?;
                    Ok(StatusCode::ACCEPTED.into_response())
                }
            };
            routes.push(
                RouteDef::on(Method::POST, "/graph", graph)
                    .public()
                    .name("mail.inbound.graph")
                    .api(api("Receive Microsoft Graph mail notifications")),
            );
        }
        if let Some(token) = self.options.gmail_token.clone() {
            let watches = self.watches.clone();
            let gmail = move |Query(query): Query<HashMap<String, String>>, body: Bytes| {
                let watches = watches.clone();
                let token = token.clone();
                async move {
                    let given = query.get("token").map(String::as_str).unwrap_or_default();
                    if !navree_crypto::constant_time_eq(given.as_bytes(), token.expose().as_bytes())
                    {
                        return Err(AppError::new(
                            StatusCode::UNAUTHORIZED,
                            navree_http::codes::INVALID_SIGNATURE,
                            "Unauthorized",
                        )
                        .with_detail("Invalid webhook signature"));
                    }
                    let payload: Value = serde_json::from_slice(&body)
                        .map_err(|_| AppError::bad_request("The body is not JSON"))?;
                    watches
                        .gmail_push(&payload)
                        .await
                        .map_err(IntegrationsError::into_app_error)?;
                    Ok(StatusCode::NO_CONTENT)
                }
            };
            routes.push(
                RouteDef::on(Method::POST, "/gmail", gmail)
                    .public()
                    .name("mail.inbound.gmail")
                    .api(api("Receive Gmail push notifications (Pub/Sub)")),
            );
        }
        routes
    }
}

// -- command ------------------------------------------------------------------

/// `integrations:mailboxes [--renew] [--sync <id>]`.
pub(crate) fn command() -> CommandSpec {
    CommandSpec::new(
        "integrations:mailboxes",
        "List watched mailboxes [--renew] [--sync <watch id>]",
        mailboxes_command,
    )
}

async fn mailboxes_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let watches = ctx.get::<MailboxWatches>().await?;
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("--renew") => {
            let summary = watches.renew_due().await?;
            if summary.renewed + summary.failed > 0 {
                println!(
                    "Renewed {} mailbox watch(es), {} failed",
                    summary.renewed, summary.failed
                );
            }
        }
        Some("--sync") => {
            let id = args.next().ok_or("--sync needs a watch id")?;
            let id: Uuid = id.parse().map_err(|_| format!("`{id}` is not a UUID"))?;
            let received = watches.sync(id).await?;
            println!("Received {} message(s)", received.len());
        }
        Some(other) => return Err(format!("unknown argument `{other}`").into()),
        None => {
            let all = watches.all().await?;
            if all.is_empty() {
                println!("No watched mailboxes.");
            }
            for watch in all {
                println!(
                    "{}  {}  {}  {}  expires {}{}",
                    watch.id,
                    watch.provider,
                    watch.mailbox,
                    watch.status,
                    watch
                        .expires_at
                        .map_or_else(|| "-".to_owned(), |at| at.to_rfc3339()),
                    watch
                        .last_error
                        .map(|error| format!("  {error}"))
                        .unwrap_or_default()
                );
            }
        }
    }
    Ok(())
}
