//! The relay: processes without the WebSocket server (`queue:work`, the
//! scheduler, commands) hand their broadcasts to the process that serves
//! the sockets, over `POST /apps/{app_id}/events`.
//!
//! Messages are JSON ([`RelayMessage`]) signed with HMAC-SHA256 under the
//! relay key ([`relay_key`]): the header `X-Navis-Relay-Signature:
//! t=<unix seconds>,v1=<hex HMAC of "<t>.<body>">`. A signature older or
//! newer than five minutes is refused, so a captured request cannot be
//! replayed later.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use navis_contracts::Secret;
use navis_crypto::{Encrypter, constant_time_eq, hmac_sha256};
use navis_kernel::BoxError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// The header carrying the signature.
pub(crate) const SIGNATURE_HEADER: &str = "x-navis-relay-signature";

/// How far a signature's timestamp may be from the receiver's clock.
const MAX_SKEW_SECS: i64 = 300;

/// How long the sender waits for the serving process.
const TIMEOUT: Duration = Duration::from_secs(5);

/// The purpose the relay key is derived for (from the realtime secret or
/// the app key).
const KEY_PURPOSE: &str = "navis-realtime.relay";

/// What one process asks the serving process to do.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum RelayMessage {
    /// Send `event` with `data` to `channels`.
    Broadcast {
        channels: Vec<String>,
        event: String,
        data: Value,
    },
    /// End every subscription of a user (`Broadcaster::end_user_subscriptions`).
    EndUserSubscriptions { user_id: Uuid },
}

/// The key relayed messages are signed with: derived from
/// `realtime.secret` when it is set, else from the app key; `None` when
/// there is neither (a random per-process secret cannot be shared).
pub(crate) fn relay_key(secret: &str, encrypter: Option<&Encrypter>) -> Option<Secret<Vec<u8>>> {
    if !secret.is_empty() {
        return Some(Secret::new(
            hmac_sha256(secret.as_bytes(), KEY_PURPOSE.as_bytes()).to_vec(),
        ));
    }
    let key = encrypter?.derive_key(KEY_PURPOSE).ok()?;
    Some(Secret::new(key.to_vec()))
}

/// The signature header value for `body` at unix time `timestamp`.
pub(crate) fn sign(key: &[u8], timestamp: i64, body: &[u8]) -> String {
    format!(
        "t={timestamp},v1={}",
        hex::encode(mac(key, timestamp, body))
    )
}

/// Whether `header` is a valid signature of `body` made within five
/// minutes of `now` (unix seconds).
pub(crate) fn verify(key: &[u8], header: &str, body: &[u8], now: i64) -> bool {
    let mut timestamp = None;
    let mut signature = None;
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", value)) => timestamp = value.parse::<i64>().ok(),
            Some(("v1", value)) => signature = hex::decode(value).ok(),
            _ => {}
        }
    }
    let (Some(timestamp), Some(signature)) = (timestamp, signature) else {
        return false;
    };
    if (now - timestamp).abs() > MAX_SKEW_SECS {
        return false;
    }
    constant_time_eq(&mac(key, timestamp, body), &signature)
}

fn mac(key: &[u8], timestamp: i64, body: &[u8]) -> [u8; 32] {
    let mut message = format!("{timestamp}.").into_bytes();
    message.extend_from_slice(body);
    hmac_sha256(key, &message)
}

/// The current unix time in seconds.
pub(crate) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

/// The base URL `realtime.relay_url = "auto"` stands for: the address
/// `serve` binds (`http.addr`), a wildcard address as loopback.
pub(crate) fn auto_url(http_addr: &str) -> String {
    match http_addr.parse::<SocketAddr>() {
        Ok(mut addr) => {
            if addr.ip().is_unspecified() {
                addr.set_ip(match addr.ip() {
                    IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
                    IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
                });
            }
            format!("http://{addr}")
        }
        Err(_) => format!("http://{http_addr}"),
    }
}

/// Sends [`RelayMessage`]s to the serving process.
pub(crate) struct RelayClient {
    endpoint: String,
    key: Secret<Vec<u8>>,
    client: reqwest::Client,
}

impl RelayClient {
    /// A client for the process at `base_url` (`http://127.0.0.1:8080`)
    /// serving app `app_id`.
    pub(crate) fn new(
        base_url: &str,
        app_id: &str,
        key: Secret<Vec<u8>>,
    ) -> Result<RelayClient, BoxError> {
        // reqwest is built without a default TLS provider: install ring's
        // (an `Err` means one is installed already).
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder().timeout(TIMEOUT).build()?;
        Ok(RelayClient {
            endpoint: format!("{}/apps/{app_id}/events", base_url.trim_end_matches('/')),
            key,
            client,
        })
    }

    /// Posts `message`; an error unless the serving process accepted it.
    pub(crate) async fn send(&self, message: &RelayMessage) -> Result<(), BoxError> {
        let body = serde_json::to_vec(message)?;
        let signature = sign(self.key.expose(), unix_now(), &body);
        let response = self
            .client
            .post(&self.endpoint)
            .header("content-type", "application/json")
            .header(SIGNATURE_HEADER, signature)
            .body(body)
            .send()
            .await
            .map_err(|error| format!("the realtime relay at {} failed: {error}", self.endpoint))?;
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else {
            Err(format!("the realtime relay at {} answered {status}", self.endpoint).into())
        }
    }
}

impl fmt::Debug for RelayClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelayClient")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_round_trip_and_expire() {
        let key = b"relay key";
        let body = br#"{"type":"broadcast"}"#;
        let header = sign(key, 1_000, body);
        assert!(header.starts_with("t=1000,v1="), "{header}");
        assert!(verify(key, &header, body, 1_000));
        assert!(verify(key, &header, body, 1_300));
        assert!(!verify(key, &header, body, 1_301), "too old");
        assert!(!verify(key, &header, body, 699), "from the future");
        assert!(!verify(key, &header, b"{}", 1_000), "another body");
        assert!(!verify(b"other key", &header, body, 1_000), "another key");
        assert!(!verify(key, "t=1000", body, 1_000));
        assert!(!verify(key, "garbage", body, 1_000));
        // The timestamp is signed too.
        let forged = header.replace("t=1000", "t=1100");
        assert!(!verify(key, &forged, body, 1_100));
    }

    #[test]
    fn the_key_comes_from_the_secret_or_the_app_key() {
        assert!(relay_key("", None).is_none());
        let from_secret = relay_key("s3cret", None).expect("a key");
        assert_eq!(from_secret.expose().len(), 32);
        assert_ne!(from_secret.expose().as_slice(), b"s3cret".as_slice());
        let key = navis_crypto::AppKey::parse(&navis_crypto::generate_key()).expect("a key");
        let encrypter = Encrypter::new(key);
        let from_app_key = relay_key("", Some(&encrypter)).expect("a key");
        assert_eq!(
            from_app_key.expose(),
            relay_key("", Some(&encrypter)).unwrap().expose(),
            "stable"
        );
    }

    #[test]
    fn auto_urls_use_loopback_for_wildcard_addresses() {
        assert_eq!(auto_url("127.0.0.1:8080"), "http://127.0.0.1:8080");
        assert_eq!(auto_url("0.0.0.0:8080"), "http://127.0.0.1:8080");
        assert_eq!(auto_url("[::]:8080"), "http://[::1]:8080");
        assert_eq!(auto_url("api.internal:8080"), "http://api.internal:8080");
    }

    #[test]
    fn messages_are_tagged_json() {
        let message = RelayMessage::EndUserSubscriptions {
            user_id: Uuid::nil(),
        };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["type"], "end_user_subscriptions");
        assert_eq!(
            serde_json::from_value::<RelayMessage>(json).unwrap(),
            message
        );
    }
}
