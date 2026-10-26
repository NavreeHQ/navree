//! Signed URLs: [`UrlSigner`] and the [`ValidSignature`] guard.

use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use http::Uri;
use http::request::Parts;
use navree_contracts::Clock;
use navree_kernel::{AppContext, BoxFuture};
use sha2::Sha256;

use crate::{AppError, Guard};

/// Signs URLs with HMAC-SHA256 so links (email verification, downloads) can
/// be trusted without a session. The signature covers the path, the query
/// and an `expires` timestamp.
///
/// ```
/// use chrono::{Duration, Utc};
/// use navree_http::UrlSigner;
///
/// let signer = UrlSigner::new(b"a long random key");
/// let now = Utc::now();
/// let url = signer.sign("/verify-email/42?hash=abc", now + Duration::minutes(60));
/// assert!(signer.verify(&url.parse().unwrap(), now));
/// assert!(!signer.verify(&url.parse().unwrap(), now + Duration::minutes(61)));
/// ```
///
/// The app's signer is a provider of `HttpModule`, keyed by
/// `http.signing_key`, or else by a key derived from the app key (`app.key`
/// or `APP_KEY`), or else by a random per-process key (links then stop
/// working after a restart).
#[derive(Clone)]
pub struct UrlSigner {
    key: Vec<u8>,
}

impl UrlSigner {
    /// A signer using `key`.
    pub fn new(key: &[u8]) -> Self {
        Self { key: key.to_vec() }
    }

    /// `path_and_query` plus `expires=<unix seconds>&signature=<hex>`.
    pub fn sign(&self, path_and_query: &str, expires_at: DateTime<Utc>) -> String {
        let separator = if path_and_query.contains('?') {
            '&'
        } else {
            '?'
        };
        let unsigned = format!(
            "{path_and_query}{separator}expires={}",
            expires_at.timestamp()
        );
        let signature = hex::encode(self.mac(&unsigned).finalize().into_bytes());
        format!("{unsigned}&signature={signature}")
    }

    /// Whether `uri` carries a valid signature that has not expired at `now`.
    pub fn verify(&self, uri: &Uri, now: DateTime<Utc>) -> bool {
        let Some(query) = uri.query() else {
            return false;
        };
        let mut signature = None;
        let mut expires = None;
        let mut rest = Vec::new();
        for pair in query.split('&') {
            match pair.split_once('=') {
                Some(("signature", value)) => {
                    if signature.replace(value).is_some() {
                        return false;
                    }
                }
                Some(("expires", value)) => {
                    expires = value.parse::<i64>().ok();
                    rest.push(pair);
                }
                _ => rest.push(pair),
            }
        }
        let (Some(signature), Some(expires)) = (signature, expires) else {
            return false;
        };
        if now.timestamp() >= expires {
            return false;
        }
        let Ok(signature) = hex::decode(signature) else {
            return false;
        };
        let unsigned = format!("{}?{}", uri.path(), rest.join("&"));
        self.mac(&unsigned).verify_slice(&signature).is_ok()
    }

    /// A signer keyed by a subkey of the app key (`APP_KEY`): an
    /// HMAC-SHA256 of a fixed purpose string, so the raw app key never signs
    /// anything.
    pub(crate) fn derived_from_app_key(app_key: &str) -> Self {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(app_key.as_bytes())
            .expect("HMAC accepts keys of any length");
        mac.update(b"navree-http/url-signing/v1");
        Self::new(&mac.finalize().into_bytes())
    }

    fn mac(&self, message: &str) -> Hmac<Sha256> {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&self.key)
            .expect("HMAC accepts keys of any length");
        mac.update(message.as_bytes());
        mac
    }
}

impl fmt::Debug for UrlSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UrlSigner")
            .field("key", &"[redacted]")
            .finish()
    }
}

/// A [`Guard`] that rejects requests without a valid, unexpired URL
/// signature (401). Uses the app's [`UrlSigner`] and `dyn Clock` when one is
/// registered.
#[derive(Clone, Copy, Debug, Default)]
pub struct ValidSignature;

impl ValidSignature {
    /// The guard as a route guard (`route.guard(ValidSignature::guard())`).
    pub fn guard() -> Arc<dyn Guard> {
        Arc::new(Self)
    }
}

impl Guard for ValidSignature {
    fn check<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let ctx = parts
                .extensions
                .get::<AppContext>()
                .cloned()
                .ok_or_else(|| {
                    AppError::internal("ValidSignature runs outside the navree pipeline")
                })?;
            let signer = ctx.get::<UrlSigner>().await?;
            let now = match ctx.try_get::<dyn Clock>().await {
                Some(clock) => clock.now(),
                None => Utc::now(),
            };
            if signer.verify(&parts.uri, now) {
                Ok(())
            } else {
                Err(AppError::new(
                    http::StatusCode::UNAUTHORIZED,
                    "invalid_signature",
                    "Invalid signature.",
                ))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn signed_url_roundtrip_and_expiry() {
        let signer = UrlSigner::new(b"key-1");
        let now = Utc::now();
        let url = signer.sign("/verify/1?hash=abc", now + Duration::seconds(60));
        assert!(url.starts_with("/verify/1?hash=abc&expires="));
        assert!(signer.verify(&uri(&url), now));
        assert!(signer.verify(&uri(&url), now + Duration::seconds(59)));
        assert!(
            !signer.verify(&uri(&url), now + Duration::seconds(60)),
            "expired"
        );

        let no_query = signer.sign("/download", now + Duration::seconds(60));
        assert!(no_query.starts_with("/download?expires="));
        assert!(signer.verify(&uri(&no_query), now));
    }

    #[test]
    fn tampering_or_another_key_fails() {
        let signer = UrlSigner::new(b"key-1");
        let now = Utc::now();
        let url = signer.sign("/verify/1?hash=abc", now + Duration::seconds(60));

        assert!(!signer.verify(&uri(&url.replace("/verify/1", "/verify/2")), now));
        assert!(!signer.verify(&uri(&url.replace("hash=abc", "hash=abd")), now));
        let later = (now + Duration::days(1)).timestamp().to_string();
        let expires = url
            .split("expires=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap();
        assert!(!signer.verify(&uri(&url.replace(expires, &later)), now));
        assert!(!UrlSigner::new(b"key-2").verify(&uri(&url), now));
        assert!(!signer.verify(&uri("/verify/1?hash=abc"), now), "unsigned");
        assert!(
            !signer.verify(&uri(&format!("{url}&signature=00")), now),
            "two signatures"
        );
        assert!(!signer.verify(&uri(&url.replace("signature=", "signature=zz")), now));
    }

    #[test]
    fn debug_hides_the_key() {
        assert!(!format!("{:?}", UrlSigner::new(b"hunter2")).contains("hunter2"));
    }
}
