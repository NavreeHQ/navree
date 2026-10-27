//! Browser push subscriptions: [`PushSubscription`] rows and the
//! [`PushSubscriptions`] service.

use std::fmt;

use chrono::{DateTime, Utc};
use navree_crypto::webpush;
use navree_kernel::WeakAppContext;
use navree_orm::{Model, Record};
use sea_orm::DbErr;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::store::{Exec, on};
use crate::{Notifiable, NotifyError, PushTarget};

/// The longest endpoint accepted.
const MAX_ENDPOINT: usize = 2048;

/// A row of `push_subscriptions`: one browser (or device) that receives
/// the web push notifications of a notifiable. The subscription's keys
/// (`p256dh`, `auth`) are never serialized.
#[derive(Model, Clone, Default, PartialEq, Eq, Serialize)]
#[model(table = "push_subscriptions")]
pub struct PushSubscription {
    /// The id (a UUID v7).
    pub id: Uuid,
    /// The notifiable's type (`user`).
    pub notifiable_type: String,
    /// The notifiable's id.
    pub notifiable_id: String,
    /// The push service URL.
    pub endpoint: String,
    /// The SHA-256 of `endpoint` (hex): one row per endpoint.
    #[serde(skip_serializing)]
    pub endpoint_hash: String,
    /// The subscription's P-256 public key (base64url).
    #[serde(skip_serializing)]
    #[model(hidden)]
    pub p256dh: String,
    /// The subscription's auth secret (base64url).
    #[serde(skip_serializing)]
    #[model(hidden)]
    pub auth: String,
    /// The browser that subscribed (`User-Agent`).
    pub user_agent: Option<String>,
    /// When it subscribed.
    pub created_at: DateTime<Utc>,
    /// When a notification was last pushed to it.
    pub last_used_at: Option<DateTime<Utc>>,
}

impl PushSubscription {
    /// Where the driver sends.
    pub fn target(&self) -> PushTarget {
        PushTarget {
            endpoint: self.endpoint.clone(),
            p256dh: self.p256dh.clone(),
            auth: self.auth.clone(),
        }
    }
}

/// The keys are redacted.
impl fmt::Debug for PushSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PushSubscription")
            .field("id", &self.id)
            .field("notifiable_type", &self.notifiable_type)
            .field("notifiable_id", &self.notifiable_id)
            .field("endpoint", &self.endpoint)
            .field("user_agent", &self.user_agent)
            .field("created_at", &self.created_at)
            .field("last_used_at", &self.last_used_at)
            .finish_non_exhaustive()
    }
}

/// A subscription as the browser gives it: `subscription.toJSON()`.
///
/// ```
/// use navree_notify::NewPushSubscription;
///
/// let subscription: NewPushSubscription = serde_json::from_str(r#"{
///     "endpoint": "https://fcm.googleapis.com/fcm/send/abc:def",
///     "expirationTime": null,
///     "keys": {
///         "p256dh": "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
///         "auth": "BTBZMqHH6r4Tts7J_aSIgg"
///     }
/// }"#).unwrap();
/// assert!(subscription.check().is_ok());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewPushSubscription {
    /// The push service URL.
    pub endpoint: String,
    /// When the browser expires it (Unix milliseconds), if it says.
    #[serde(rename = "expirationTime", default)]
    pub expiration_time: Option<i64>,
    /// Its keys.
    pub keys: PushKeys,
}

/// The keys of a [`NewPushSubscription`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushKeys {
    /// The P-256 public key (base64url, 65 bytes).
    pub p256dh: String,
    /// The auth secret (base64url, 16 bytes).
    pub auth: String,
}

impl NewPushSubscription {
    /// Checks the subscription: an `https` endpoint (or `http` on
    /// `localhost`), a P-256 `p256dh` and an `auth` of at least 16 bytes.
    /// The error names the field.
    pub fn check(&self) -> Result<(), (&'static str, String)> {
        let endpoint = self.endpoint.trim();
        let url = Url::parse(endpoint)
            .ok()
            .filter(|_| endpoint.len() <= MAX_ENDPOINT)
            .ok_or(("endpoint", "endpoint must be a URL".to_owned()))?;
        let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if !(url.scheme() == "https" || (url.scheme() == "http" && local)) {
            return Err(("endpoint", "endpoint must be an https URL".to_owned()));
        }
        let p256dh = webpush::decode_key(&self.keys.p256dh);
        if !p256dh.is_some_and(|key| key.len() == 65 && key[0] == 4) {
            return Err((
                "keys.p256dh",
                "keys.p256dh must be a base64url P-256 public key".to_owned(),
            ));
        }
        let auth = webpush::decode_key(&self.keys.auth);
        if !auth.is_some_and(|key| (16..=64).contains(&key.len())) {
            return Err((
                "keys.auth",
                "keys.auth must be a base64url secret of at least 16 bytes".to_owned(),
            ));
        }
        Ok(())
    }
}

/// The push subscriptions of notifiables: the web push channel sends to
/// each subscription of the recipient, and deletes the ones the push
/// service reports gone.
///
/// The signed-in user manages their own with `POST` and `DELETE
/// /api/notifications/push-subscriptions`. Provided by
/// [`NotifyModule`](crate::NotifyModule): inject `Arc<PushSubscriptions>`.
/// Needs `DatabaseModule`; inside an unfinished unit of work its writes join
/// the transaction.
pub struct PushSubscriptions {
    app: WeakAppContext,
}

impl PushSubscriptions {
    pub(crate) fn new(app: WeakAppContext) -> Self {
        Self { app }
    }

    /// Saves `subscription` for `notifiable` ([`check`](NewPushSubscription::check)ed
    /// first). An endpoint that is already saved moves to `notifiable` with
    /// the new keys: a browser profile belongs to whoever signed in last.
    pub async fn subscribe(
        &self,
        notifiable: &dyn Notifiable,
        subscription: NewPushSubscription,
        user_agent: Option<&str>,
    ) -> Result<PushSubscription, NotifyError> {
        subscription
            .check()
            .map_err(|(_, message)| NotifyError::InvalidRecipient(message))?;
        let endpoint = subscription.endpoint.trim().to_owned();
        let hash = navree_crypto::sha256_hex(endpoint.as_bytes());
        let now = crate::notifier::now_in(&self.app).await;
        let exec = Exec::current(&self.app).await?;
        let existing = on!(&exec, e => PushSubscription::query()
            .where_(PushSubscription::ENDPOINT_HASH.eq(hash.as_str()))
            .first(e)
            .await)
        .map_err(DbErr::from)?;
        let mut record = existing.unwrap_or_else(|| {
            Record::new(PushSubscription {
                id: Uuid::now_v7(),
                endpoint: endpoint.clone(),
                endpoint_hash: hash.clone(),
                created_at: now,
                ..PushSubscription::default()
            })
        });
        record.notifiable_type = notifiable.notifiable_type().to_owned();
        record.notifiable_id = notifiable.notifiable_id();
        record.p256dh = subscription.keys.p256dh.trim().to_owned();
        record.auth = subscription.keys.auth.trim().to_owned();
        record.user_agent = user_agent.map(|ua| ua.chars().take(255).collect());
        on!(&exec, e => record.save(e).await).map_err(DbErr::from)?;
        exec.wrote();
        Ok(record.into_inner())
    }

    /// Deletes `notifiable`'s subscription at `endpoint`; `false` when it
    /// has none there.
    pub async fn unsubscribe(
        &self,
        notifiable: &dyn Notifiable,
        endpoint: &str,
    ) -> Result<bool, NotifyError> {
        let hash = navree_crypto::sha256_hex(endpoint.trim().as_bytes());
        let exec = Exec::current(&self.app).await?;
        let deleted = on!(&exec, e => PushSubscription::query()
            .where_(PushSubscription::ENDPOINT_HASH.eq(hash.as_str()))
            .where_(PushSubscription::NOTIFIABLE_TYPE.eq(notifiable.notifiable_type()))
            .where_(PushSubscription::NOTIFIABLE_ID.eq(notifiable.notifiable_id()))
            .delete(e)
            .await)
        .map_err(DbErr::from)?;
        exec.wrote();
        Ok(deleted > 0)
    }

    /// Every subscription of the notifiable `notifiable_type` /
    /// `notifiable_id`, oldest first.
    pub async fn of(
        &self,
        notifiable_type: &str,
        notifiable_id: &str,
    ) -> Result<Vec<PushSubscription>, NotifyError> {
        let exec = Exec::current(&self.app).await?;
        let rows = on!(&exec, e => PushSubscription::query()
            .where_(PushSubscription::NOTIFIABLE_TYPE.eq(notifiable_type))
            .where_(PushSubscription::NOTIFIABLE_ID.eq(notifiable_id))
            .oldest_by(PushSubscription::CREATED_AT)
            .get(e)
            .await)
        .map_err(DbErr::from)?;
        Ok(rows.into_iter().map(Record::into_inner).collect())
    }

    /// Deletes subscription `id`; `false` when it does not exist.
    pub async fn delete(&self, id: Uuid) -> Result<bool, NotifyError> {
        let exec = Exec::current(&self.app).await?;
        let deleted = on!(&exec, e => PushSubscription::query()
            .where_(PushSubscription::ID.eq(id))
            .delete(e)
            .await)
        .map_err(DbErr::from)?;
        exec.wrote();
        Ok(deleted > 0)
    }

    /// Notes a push to subscription `id` at `at` (pool, outside any unit
    /// of work).
    pub(crate) async fn touch(&self, id: Uuid, at: DateTime<Utc>) -> Result<(), NotifyError> {
        let exec = Exec::pool(&self.app).await?;
        on!(&exec, e => PushSubscription::query()
            .where_(PushSubscription::ID.eq(id))
            .update(e, [PushSubscription::LAST_USED_AT.set(Some(at))])
            .await)
        .map_err(DbErr::from)?;
        Ok(())
    }

    /// [`delete`](PushSubscriptions::delete) on the pool, outside any unit
    /// of work (the channel runs after commit).
    pub(crate) async fn forget(&self, id: Uuid) -> Result<(), NotifyError> {
        let exec = Exec::pool(&self.app).await?;
        on!(&exec, e => PushSubscription::query()
            .where_(PushSubscription::ID.eq(id))
            .delete(e)
            .await)
        .map_err(DbErr::from)?;
        Ok(())
    }
}

impl fmt::Debug for PushSubscriptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PushSubscriptions").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P256DH: &str =
        "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";

    fn subscription(endpoint: &str, p256dh: &str, auth: &str) -> NewPushSubscription {
        NewPushSubscription {
            endpoint: endpoint.to_owned(),
            expiration_time: None,
            keys: PushKeys {
                p256dh: p256dh.to_owned(),
                auth: auth.to_owned(),
            },
        }
    }

    #[test]
    fn subscriptions_need_https_and_valid_keys() {
        let auth = "BTBZMqHH6r4Tts7J_aSIgg";
        assert!(subscription("https://fcm.googleapis.com/fcm/send/x", P256DH, auth).check().is_ok());
        assert!(subscription("http://localhost:8080/push", P256DH, auth).check().is_ok());
        assert_eq!(
            subscription("http://push.example.com/x", P256DH, auth).check().unwrap_err().0,
            "endpoint"
        );
        assert_eq!(
            subscription("not a url", P256DH, auth).check().unwrap_err().0,
            "endpoint"
        );
        assert_eq!(
            subscription("https://x.test/1", "BAAA", auth).check().unwrap_err().0,
            "keys.p256dh"
        );
        assert_eq!(
            subscription("https://x.test/1", P256DH, "c2hvcnQ").check().unwrap_err().0,
            "keys.auth"
        );
    }
}
