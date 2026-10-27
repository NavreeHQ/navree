//! Opt-outs of the SMS and WhatsApp channels: [`NotificationOptOut`] rows
//! and the [`OptOuts`] service.

use std::fmt;

use chrono::{DateTime, Utc};
use navree_kernel::WeakAppContext;
use navree_orm::{Model, Record};
use navree_validation::{Country, Phone};
use sea_orm::DbErr;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::store::{Exec, on};
use crate::{Channel, NotifyError};

/// Why a number opted out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptOutSource {
    /// The recipient replied with an opt-out keyword (STOP).
    Keyword,
    /// The provider refused to send because the recipient opted out with
    /// it (Twilio's `21610`).
    Provider,
    /// The app opted the number out ([`OptOuts::opt_out`] from your code,
    /// a user's settings).
    Manual,
}

impl OptOutSource {
    /// `keyword`, `provider` or `manual`.
    pub fn as_str(self) -> &'static str {
        match self {
            OptOutSource::Keyword => "keyword",
            OptOutSource::Provider => "provider",
            OptOutSource::Manual => "manual",
        }
    }
}

/// A row of `notification_opt_outs`: a number (E.164) that receives no
/// more messages over a channel (`sms` or `whatsapp`). Unique per channel
/// and number.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "notification_opt_outs")]
pub struct NotificationOptOut {
    /// The id (a UUID v7).
    pub id: Uuid,
    /// `sms` or `whatsapp`.
    pub channel: String,
    /// The number, in E.164.
    pub address: String,
    /// [`OptOutSource`] as text.
    pub source: String,
    /// When it opted out.
    pub created_at: DateTime<Utc>,
}

/// The opt-out list of the SMS and WhatsApp channels. The
/// [`Notifier`](crate::Notifier) skips opted-out numbers (their delivery
/// is recorded as `opted_out`); replies with an opt-out keyword, and
/// provider refusals for opted-out recipients, add numbers through
/// [`DeliveryLog::ingest`](crate::DeliveryLog::ingest).
///
/// Numbers are read as E.164 in `notify.sms.default_region`, so
/// `(415) 555-0100` and `+14155550100` are one entry. Provided by
/// [`NotifyModule`](crate::NotifyModule): inject `Arc<OptOuts>`. Needs
/// `DatabaseModule`.
pub struct OptOuts {
    app: WeakAppContext,
    region: Option<Country>,
}

impl OptOuts {
    pub(crate) fn new(app: WeakAppContext, region: Option<Country>) -> Self {
        Self { app, region }
    }

    /// `address` in E.164, read in `notify.sms.default_region`.
    pub fn normalize(&self, address: &str) -> Result<String, NotifyError> {
        normalize(address, self.region)
    }

    /// Opts `address` out of `channel`; `false` when it already was.
    pub async fn opt_out(
        &self,
        channel: Channel,
        address: &str,
        source: OptOutSource,
    ) -> Result<bool, NotifyError> {
        let address = self.normalize(address)?;
        let exec = Exec::current(&self.app).await?;
        let row = NotificationOptOut {
            id: Uuid::now_v7(),
            channel: channel.as_str().to_owned(),
            address,
            source: source.as_str().to_owned(),
            created_at: crate::notifier::now_in(&self.app).await,
        };
        let inserted = on!(&exec, e => NotificationOptOut::insert_or_ignore(e, [row]).await)
            .map_err(DbErr::from)?;
        exec.wrote();
        Ok(inserted > 0)
    }

    /// Opts `address` back into `channel`; `false` when it was not opted
    /// out.
    pub async fn opt_in(&self, channel: Channel, address: &str) -> Result<bool, NotifyError> {
        let address = self.normalize(address)?;
        let exec = Exec::current(&self.app).await?;
        let deleted = on!(&exec, e => NotificationOptOut::query()
            .where_(NotificationOptOut::CHANNEL.eq(channel.as_str()))
            .where_(NotificationOptOut::ADDRESS.eq(address.as_str()))
            .delete(e)
            .await)
        .map_err(DbErr::from)?;
        exec.wrote();
        Ok(deleted > 0)
    }

    /// Whether `address` has opted out of `channel`.
    pub async fn is_opted_out(&self, channel: Channel, address: &str) -> Result<bool, NotifyError> {
        let address = self.normalize(address)?;
        let exec = Exec::current(&self.app).await?;
        Ok(on!(&exec, e => NotificationOptOut::query()
            .where_(NotificationOptOut::CHANNEL.eq(channel.as_str()))
            .where_(NotificationOptOut::ADDRESS.eq(address.as_str()))
            .exists(e)
            .await)
        .map_err(DbErr::from)?)
    }

    /// Every opt-out of `channel`, newest first.
    pub async fn list(&self, channel: Channel) -> Result<Vec<NotificationOptOut>, NotifyError> {
        let exec = Exec::current(&self.app).await?;
        let rows = on!(&exec, e => NotificationOptOut::query()
            .where_(NotificationOptOut::CHANNEL.eq(channel.as_str()))
            .latest_by(NotificationOptOut::CREATED_AT)
            .get(e)
            .await)
        .map_err(DbErr::from)?;
        Ok(rows.into_iter().map(Record::into_inner).collect())
    }
}

impl fmt::Debug for OptOuts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OptOuts")
            .field("region", &self.region)
            .finish_non_exhaustive()
    }
}

/// `address` in E.164, read in `region`.
pub(crate) fn normalize(address: &str, region: Option<Country>) -> Result<String, NotifyError> {
    let address = address.trim().trim_start_matches("whatsapp:");
    Phone::parse(address, region)
        .map(Phone::into_string)
        .map_err(|_| NotifyError::InvalidRecipient(format!("`{address}` is not a phone number")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_normalized_to_e164_in_the_region() {
        let us = Country::parse("US").ok();
        assert_eq!(normalize("(415) 555-0100", us).unwrap(), "+14155550100");
        assert_eq!(normalize("+44 7400 123456", us).unwrap(), "+447400123456");
        assert_eq!(normalize("whatsapp:+447400123456", None).unwrap(), "+447400123456");
        assert!(normalize("415 555 0100", None).is_err());
        assert!(normalize("not a number", us).is_err());
    }
}
