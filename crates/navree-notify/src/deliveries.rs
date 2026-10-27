//! The delivery log of the Slack, Teams, SMS, WhatsApp and web push
//! channels: [`NotificationDelivery`] rows and the [`DeliveryLog`] service.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use navree_kernel::WeakAppContext;
use navree_orm::{Model, Record};
use sea_orm::DbErr;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::store::{Exec, on};
use crate::{
    Channel, ChannelCallback, Keyword, NotifyError, OptOutSource, OptOuts, SendNotificationJob,
    SentMessage, StatusUpdate,
};

/// Where a delivery stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    /// The provider accepted it (or, for webhooks and push services, took
    /// it).
    Sent,
    /// The provider reported it delivered to the device.
    Delivered,
    /// The recipient read it (WhatsApp, some SMS routes).
    Read,
    /// Sending failed, or the provider reported it undeliverable.
    Failed,
    /// Not sent: the recipient has opted out (STOP).
    OptedOut,
}

impl DeliveryStatus {
    /// `sent`, `delivered`, `read`, `failed` or `opted_out`.
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryStatus::Sent => "sent",
            DeliveryStatus::Delivered => "delivered",
            DeliveryStatus::Read => "read",
            DeliveryStatus::Failed => "failed",
            DeliveryStatus::OptedOut => "opted_out",
        }
    }

    /// Whether a delivery in this status takes `next` from a later
    /// callback. Callbacks arrive out of order, so a status only moves
    /// forward: `sent` takes any other, `delivered` takes `read`, and
    /// `read`, `failed` and `opted_out` are final.
    pub fn can_become(self, next: DeliveryStatus) -> bool {
        match self {
            DeliveryStatus::Sent => next != DeliveryStatus::Sent,
            DeliveryStatus::Delivered => next == DeliveryStatus::Read,
            DeliveryStatus::Read | DeliveryStatus::Failed | DeliveryStatus::OptedOut => false,
        }
    }
}

impl fmt::Display for DeliveryStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DeliveryStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "sent" => DeliveryStatus::Sent,
            "delivered" => DeliveryStatus::Delivered,
            "read" => DeliveryStatus::Read,
            "failed" => DeliveryStatus::Failed,
            "opted_out" => DeliveryStatus::OptedOut,
            other => return Err(format!("unknown delivery status `{other}`")),
        })
    }
}

/// A row of `notification_deliveries`: one notification over one recorded
/// channel to one recipient. A retried `notify.send` job updates its row.
///
/// `recipient` is the address: an E.164 number (SMS, WhatsApp), a Slack
/// channel id, a Teams conversation id, a push subscription id, or
/// `webhook:<host>` for webhooks (their URLs are secrets).
///
/// ```no_run
/// use navree_notify::NotificationDelivery;
/// use navree_orm::Model as _;
/// # async fn demo(db: navree_db::Db, notification: uuid::Uuid) -> navree_orm::Result<()> {
/// let failed = NotificationDelivery::query()
///     .where_(NotificationDelivery::NOTIFICATION_ID.eq(notification))
///     .where_(NotificationDelivery::STATUS.eq("failed"))
///     .get(&db)
///     .await?;
/// # Ok(()) }
/// ```
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "notification_deliveries")]
pub struct NotificationDelivery {
    /// The id (a UUID v7).
    pub id: Uuid,
    /// The notification ([`Notifier::send`](crate::Notifier::send)'s id).
    pub notification_id: Uuid,
    /// [`Notification::TYPE`](crate::Notification::TYPE).
    pub notification_type: String,
    /// The notifiable's type.
    pub notifiable_type: String,
    /// The notifiable's id.
    pub notifiable_id: String,
    /// `slack`, `teams`, `sms`, `whatsapp` or `web_push`.
    pub channel: String,
    /// The driver that sent it (`twilio`, `meta`, `fake`...).
    pub provider: Option<String>,
    /// Where it went (see above).
    pub recipient: Option<String>,
    /// The provider's message id, which status callbacks name.
    pub provider_message_id: Option<String>,
    /// [`DeliveryStatus`] as text.
    pub status: String,
    /// Why it failed.
    pub error: Option<String>,
    /// When it was first attempted.
    pub created_at: DateTime<Utc>,
    /// When its status last changed.
    pub updated_at: DateTime<Utc>,
}

impl NotificationDelivery {
    /// The status, parsed.
    pub fn delivery_status(&self) -> Option<DeliveryStatus> {
        self.status.parse().ok()
    }
}

/// How a channel attempt ended, for the log.
pub(crate) enum Outcome<'a> {
    Sent(&'a SentMessage),
    Failed {
        provider: Option<String>,
        error: String,
    },
    OptedOut,
}

/// What [`DeliveryLog::ingest`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Ingested {
    /// Deliveries whose status changed.
    pub updated: usize,
    /// Numbers newly opted out.
    pub opted_out: usize,
    /// Numbers opted back in.
    pub opted_in: usize,
}

/// The delivery log: written by the [`Notifier`](crate::Notifier) for each
/// recorded channel (with `notify.deliveries` and `DatabaseModule`), and
/// updated from provider callbacks with [`ingest`](DeliveryLog::ingest).
///
/// Provided by [`NotifyModule`](crate::NotifyModule): inject
/// `Arc<DeliveryLog>`. Inside an unfinished unit of work (a request
/// handler), its writes join the transaction.
pub struct DeliveryLog {
    app: WeakAppContext,
    opt_outs: OptOuts,
}

impl DeliveryLog {
    pub(crate) fn new(app: WeakAppContext, opt_outs: OptOuts) -> Self {
        Self { app, opt_outs }
    }

    /// The opt-outs the log updates.
    pub fn opt_outs(&self) -> &OptOuts {
        &self.opt_outs
    }

    /// Applies provider callbacks: each [`StatusUpdate`] moves its delivery
    /// forward ([`DeliveryStatus::can_become`]; an opt-out error also opts
    /// the recipient out), and each [`InboundMessage`](crate::InboundMessage)
    /// that is an opt-out or opt-in keyword updates the opt-outs of its
    /// channel. Updates for unknown message ids are ignored.
    pub async fn ingest(
        &self,
        callbacks: impl IntoIterator<Item = ChannelCallback>,
    ) -> Result<Ingested, NotifyError> {
        let mut ingested = Ingested::default();
        for callback in callbacks {
            match callback {
                ChannelCallback::Status(update) => {
                    let Some((delivery, changed)) = self.apply_update(&update).await? else {
                        continue;
                    };
                    if changed {
                        ingested.updated += 1;
                    }
                    if update.is_opt_out()
                        && let (Some(recipient), Ok(channel)) =
                            (&delivery.recipient, delivery.channel.parse::<Channel>())
                        && self
                            .opt_outs
                            .opt_out(channel, recipient, OptOutSource::Provider)
                            .await?
                    {
                        ingested.opted_out += 1;
                    }
                }
                ChannelCallback::Inbound(message) => {
                    let (channel, from) = (message.channel, message.from.as_str());
                    match message.keyword() {
                        Some(Keyword::OptOut) => {
                            let added = self
                                .opt_outs
                                .opt_out(channel, from, OptOutSource::Keyword)
                                .await?;
                            ingested.opted_out += usize::from(added);
                        }
                        Some(Keyword::OptIn) => {
                            let removed = self.opt_outs.opt_in(channel, from).await?;
                            ingested.opted_in += usize::from(removed);
                        }
                        None => {}
                    }
                }
            }
        }
        Ok(ingested)
    }

    /// Applies one status update; returns the delivery as it is now, or
    /// `None` when no delivery has that provider and message id.
    pub async fn apply(
        &self,
        update: &StatusUpdate,
    ) -> Result<Option<NotificationDelivery>, NotifyError> {
        Ok(self
            .apply_update(update)
            .await?
            .map(|(delivery, _)| delivery))
    }

    async fn apply_update(
        &self,
        update: &StatusUpdate,
    ) -> Result<Option<(NotificationDelivery, bool)>, NotifyError> {
        let exec = Exec::current(&self.app).await?;
        let found = on!(&exec, e => NotificationDelivery::query()
            .where_(NotificationDelivery::PROVIDER.eq(update.provider.as_str()))
            .where_(NotificationDelivery::PROVIDER_MESSAGE_ID.eq(update.message_id.as_str()))
            .first(e)
            .await)
        .map_err(DbErr::from)?;
        let Some(mut record) = found else {
            return Ok(None);
        };
        let current = record.delivery_status().unwrap_or(DeliveryStatus::Sent);
        if !current.can_become(update.status) {
            return Ok(Some((record.into_inner(), false)));
        }
        record.status = update.status.as_str().to_owned();
        if update.status == DeliveryStatus::Failed {
            record.error = update.error.clone().or_else(|| update.error_code.clone());
        }
        record.updated_at = crate::notifier::now_in(&self.app).await;
        on!(&exec, e => record.save(e).await).map_err(DbErr::from)?;
        exec.wrote();
        Ok(Some((record.into_inner(), true)))
    }

    /// Every delivery of notification `id`, oldest first.
    pub async fn for_notification(
        &self,
        id: Uuid,
    ) -> Result<Vec<NotificationDelivery>, NotifyError> {
        let exec = Exec::current(&self.app).await?;
        let rows = on!(&exec, e => NotificationDelivery::query()
            .where_(NotificationDelivery::NOTIFICATION_ID.eq(id))
            .oldest_by(NotificationDelivery::CREATED_AT)
            .get(e)
            .await)
        .map_err(DbErr::from)?;
        Ok(rows.into_iter().map(Record::into_inner).collect())
    }

    /// The recipients notification `id` already reached over `channel`
    /// (status other than `failed` and `opted_out`), on the pool.
    pub(crate) async fn sent_recipients(
        &self,
        id: Uuid,
        channel: Channel,
    ) -> Result<Vec<String>, NotifyError> {
        let exec = Exec::pool(&self.app).await?;
        let rows = on!(&exec, e => NotificationDelivery::query()
            .where_(NotificationDelivery::NOTIFICATION_ID.eq(id))
            .where_(NotificationDelivery::CHANNEL.eq(channel.as_str()))
            .where_in(
                NotificationDelivery::STATUS,
                [
                    DeliveryStatus::Sent.as_str(),
                    DeliveryStatus::Delivered.as_str(),
                    DeliveryStatus::Read.as_str(),
                ],
            )
            .get(e)
            .await)
        .map_err(DbErr::from)?;
        Ok(rows
            .into_iter()
            .filter_map(|row| row.into_inner().recipient)
            .collect())
    }

    /// The delivery a provider's message id names.
    pub async fn find_by_message(
        &self,
        provider: &str,
        message_id: &str,
    ) -> Result<Option<NotificationDelivery>, NotifyError> {
        let exec = Exec::current(&self.app).await?;
        let row = on!(&exec, e => NotificationDelivery::query()
            .where_(NotificationDelivery::PROVIDER.eq(provider))
            .where_(NotificationDelivery::PROVIDER_MESSAGE_ID.eq(message_id))
            .first(e)
            .await)
        .map_err(DbErr::from)?;
        Ok(row.map(Record::into_inner))
    }

    /// Records one attempt of `job` over `channel` to `recipient`, updating
    /// the row of an earlier attempt. Failures are logged: the log never
    /// fails a channel.
    pub(crate) async fn record(
        &self,
        job: &SendNotificationJob,
        channel: Channel,
        recipient: Option<&str>,
        outcome: Outcome<'_>,
    ) {
        if let Err(error) = self.try_record(job, channel, recipient, outcome).await {
            tracing::warn!(
                target: "navree::notify",
                notification = %job.id,
                %channel,
                %error,
                "recording a notification delivery failed"
            );
        }
    }

    async fn try_record(
        &self,
        job: &SendNotificationJob,
        channel: Channel,
        recipient: Option<&str>,
        outcome: Outcome<'_>,
    ) -> Result<(), NotifyError> {
        let exec = Exec::pool(&self.app).await?;
        let now = crate::notifier::now_in(&self.app).await;
        let mut query = NotificationDelivery::query()
            .where_(NotificationDelivery::NOTIFICATION_ID.eq(job.id))
            .where_(NotificationDelivery::CHANNEL.eq(channel.as_str()));
        query = match recipient {
            Some(recipient) => query.where_(NotificationDelivery::RECIPIENT.eq(recipient)),
            None => query.where_null(NotificationDelivery::RECIPIENT),
        };
        let existing = on!(&exec, e => query.first(e).await).map_err(DbErr::from)?;
        let mut record = existing.unwrap_or_else(|| {
            Record::new(NotificationDelivery {
                id: Uuid::now_v7(),
                notification_id: job.id,
                notification_type: job.notification_type.clone(),
                notifiable_type: job.notifiable_type.clone(),
                notifiable_id: job.notifiable_id.clone(),
                channel: channel.as_str().to_owned(),
                recipient: recipient.map(str::to_owned),
                created_at: now,
                ..NotificationDelivery::default()
            })
        });
        match outcome {
            Outcome::Sent(sent) => {
                record.provider = Some(sent.provider.clone());
                record.provider_message_id = sent.message_id.clone();
                record.status = DeliveryStatus::Sent.as_str().to_owned();
                record.error = None;
            }
            Outcome::Failed { provider, error } => {
                if provider.is_some() {
                    record.provider = provider;
                }
                record.status = DeliveryStatus::Failed.as_str().to_owned();
                record.error = Some(error);
            }
            Outcome::OptedOut => {
                record.status = DeliveryStatus::OptedOut.as_str().to_owned();
                record.error = None;
            }
        }
        record.updated_at = now;
        on!(&exec, e => record.save(e).await).map_err(DbErr::from)?;
        Ok(())
    }
}

impl fmt::Debug for DeliveryLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeliveryLog").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_only_move_forward() {
        use DeliveryStatus::*;
        assert!(Sent.can_become(Delivered));
        assert!(Sent.can_become(Read));
        assert!(Sent.can_become(Failed));
        assert!(!Sent.can_become(Sent));
        assert!(Delivered.can_become(Read));
        assert!(!Delivered.can_become(Sent));
        assert!(!Delivered.can_become(Failed));
        assert!(!Read.can_become(Delivered));
        assert!(!Failed.can_become(Delivered));
        assert!(!OptedOut.can_become(Sent));
    }

    #[test]
    fn statuses_round_trip_through_text() {
        for status in [
            DeliveryStatus::Sent,
            DeliveryStatus::Delivered,
            DeliveryStatus::Read,
            DeliveryStatus::Failed,
            DeliveryStatus::OptedOut,
        ] {
            assert_eq!(status.as_str().parse::<DeliveryStatus>(), Ok(status));
            assert_eq!(serde_json::to_value(status).unwrap(), status.as_str());
        }
        assert!("lost".parse::<DeliveryStatus>().is_err());
    }
}
