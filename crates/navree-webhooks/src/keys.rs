//! [`SenderKeys`]: the public keys senders sign with, fetched and cached.

use std::fmt;
use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, TimeDelta, Utc};
use navree_crypto::jose::{JwkSet, VerifyingKey};
use navree_crypto::sns::CertificateCache;
use navree_http_client::HttpClient;

use crate::verify::VerifyError;

/// How long Google's keys are kept when the response has no `max-age`.
const DEFAULT_KEYS_TTL: TimeDelta = TimeDelta::hours(1);
/// The shortest time between two fetches of Google's keys for a key id
/// the cached set lacks.
const KEYS_REFRESH_INTERVAL: TimeDelta = TimeDelta::minutes(1);

/// Google's OAuth 2.0 public keys, which sign the OIDC tokens of Pub/Sub
/// push requests.
pub(crate) const GOOGLE_KEYS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";

/// Fetches and caches the public keys that senders without a shared secret
/// sign with: AWS SNS signing certificates (kept a day, by URL, in the
/// same `navree_crypto::sns::CertificateCache` navree-mail uses) and
/// Google's OAuth 2.0 key set (kept for the `max-age` Google sends, an hour
/// when it sends none, and fetched again, at most once a minute, when a
/// token names a key the cached set lacks).
///
/// `WebhooksModule` provides one per app, on the app's `HttpClient` (a
/// default one when the app has no `HttpClientModule`), so a test's fake
/// client serves the keys. Build one with [`SenderKeys::new`] to call
/// [`AwsSns`](crate::AwsSns) or [`GooglePubSub`](crate::GooglePubSub)
/// directly.
pub struct SenderKeys {
    client: HttpClient,
    certificates: CertificateCache,
    google: Mutex<Option<Cached<JwkSet>>>,
}

/// A fetched value and when it was fetched and expires.
#[derive(Clone)]
struct Cached<T> {
    value: T,
    fetched_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl SenderKeys {
    /// Keys fetched with `client`.
    pub fn new(client: HttpClient) -> Self {
        Self {
            client,
            certificates: CertificateCache::new(),
            google: Mutex::new(None),
        }
    }

    /// The client keys are fetched with.
    pub fn client(&self) -> &HttpClient {
        &self.client
    }

    /// The public key of the PEM certificate at `url`, from the cache or
    /// fetched. The caller checks that `url` belongs to the sender.
    pub async fn certificate(
        &self,
        url: &str,
        now: DateTime<Utc>,
    ) -> Result<VerifyingKey, VerifyError> {
        if let Some(key) = self.certificates.get(url, now) {
            return Ok(key);
        }
        let response = self
            .client
            .get(url)
            .send()
            .await
            .ok()
            .filter(|response| response.is_success())
            .ok_or_else(|| VerifyError::Unavailable("the signing certificate".to_owned()))?;
        let key = VerifyingKey::from_pem(&response.text()).map_err(|_| {
            VerifyError::Rejected("the signing certificate is not valid".to_owned())
        })?;
        self.certificates.insert(url, key.clone(), now);
        Ok(key)
    }

    /// Google's OAuth 2.0 key set, from the cache or fetched. With `kid`, a
    /// cached set that lacks it is fetched again (at most once a minute).
    pub async fn google_keys(
        &self,
        kid: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<JwkSet, VerifyError> {
        if let Some(cached) = lock(&self.google).as_ref() {
            let fresh = cached.expires_at > now;
            let has_key = kid.is_none_or(|kid| cached.value.has_kid(kid));
            let recently = now - cached.fetched_at < KEYS_REFRESH_INTERVAL;
            if fresh && (has_key || recently) {
                return Ok(cached.value.clone());
            }
        }
        let unavailable = || VerifyError::Unavailable("Google's signing keys".to_owned());
        let response = self
            .client
            .get(GOOGLE_KEYS_URL)
            .send()
            .await
            .ok()
            .filter(|response| response.is_success())
            .ok_or_else(unavailable)?;
        let keys: JwkSet = response.json().map_err(|_| unavailable())?;
        let ttl = response
            .header("cache-control")
            .and_then(max_age)
            .unwrap_or(DEFAULT_KEYS_TTL);
        *lock(&self.google) = Some(Cached {
            value: keys.clone(),
            fetched_at: now,
            expires_at: now + ttl,
        });
        Ok(keys)
    }
}

impl fmt::Debug for SenderKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SenderKeys")
            .field("certificates", &self.certificates.len())
            .field("google", &lock(&self.google).is_some())
            .finish_non_exhaustive()
    }
}

/// Locks `mutex`, recovering it from a panic elsewhere (the cache stays
/// usable).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The `max-age` of a `Cache-Control` value.
fn max_age(cache_control: &str) -> Option<TimeDelta> {
    cache_control
        .split(',')
        .filter_map(|directive| directive.trim().split_once('='))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("max-age"))
        .and_then(|(_, secs)| secs.trim().parse::<i64>().ok())
        .filter(|secs| *secs > 0)
        .map(TimeDelta::seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_age_is_read_from_cache_control() {
        assert_eq!(
            max_age("public, max-age=19193, must-revalidate, no-transform"),
            Some(TimeDelta::seconds(19_193))
        );
        assert_eq!(max_age("no-cache"), None);
        assert_eq!(max_age("max-age=0"), None);
        assert_eq!(max_age("max-age=soon"), None);
    }
}
