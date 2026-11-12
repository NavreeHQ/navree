//! [`ApnsPush`]: Apple's push service, with token-based authentication.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, TimeDelta, Utc};
use navree_contracts::{BoxFuture, Clock, SystemClock};
use navree_crypto::jose::SigningKey;
use navree_http_client::{HttpClient, HttpClientOptions, Response};
use serde_json::{Map, Value, json};
use url::Url;

use super::{ChannelError, PushDriver, SentMessage, endpoint, unreachable};
use crate::{DeviceTarget, PushEnvironment, PushMessage, PushOptions, PushPlatform, Urgency};

/// The provider name of Apple's push service.
const PROVIDER: &str = "apns";

/// The largest payload Apple accepts for a notification, in bytes.
pub(crate) const MAX_PAYLOAD: usize = 4096;

/// How long a provider token is used before a new one is signed. Apple
/// refuses tokens older than an hour and new tokens more often than every
/// 20 minutes.
const TOKEN_MINUTES: i64 = 45;

/// Sends notifications to iOS devices through Apple's push service
/// (`POST /3/device/{token}` over HTTP/2, negotiated by TLS ALPN), with a
/// provider token: an ES256 JWT (`kid` the key id, `iss` the team id,
/// `iat`) signed with the team's `.p8` key, used for 45 minutes and
/// signed again sooner when Apple answers `ExpiredProviderToken` (the
/// request is then sent once more). Devices of a development build go to
/// the sandbox, with the sandbox key when one is configured.
///
/// Each request names the device's app as `apns-topic`, `apns-push-type`
/// `alert` (or `background` for a [silent](PushMessage::silent) push),
/// `apns-priority` (10 at once, 5 when power allows: a silent push and
/// [`Urgency::Normal`] or lower; 1 for [`Urgency::VeryLow`]),
/// `apns-expiration` (now plus the time to live; 0 asks for now or never)
/// and, with a collapse key, `apns-collapse-id`. A payload over 4096 bytes
/// is refused before anything is sent.
///
/// Apple's answers: `410` (`Unregistered`, `ExpiredToken`), and
/// `BadDeviceToken` or `DeviceTokenNotForTopic`, mean the device is gone
/// ([`ChannelError::Gone`]); every other answer, throttling (`429`) and
/// server errors (`5xx`) among them, is a refusal with Apple's reason as
/// the code, which the queue retries. The message id is `apns-id`.
pub struct ApnsPush {
    team_id: String,
    production: Option<Signer>,
    sandbox: Option<Signer>,
    endpoint: Url,
    sandbox_endpoint: Url,
    ttl_secs: u32,
    client: HttpClient,
    clock: Arc<dyn Clock>,
}

/// One `.p8` key and the provider token it signed last.
struct Signer {
    key: SigningKey,
    cached: Mutex<Option<(String, DateTime<Utc>)>>,
}

impl Signer {
    fn new(pem: &str, key_id: &str, which: &str) -> Result<Self, String> {
        let key = SigningKey::from_pem(pem).map_err(|error| {
            format!("notify.push.apns.{which} is not a usable .p8 key: {error}")
        })?;
        if key.algorithm() != navree_crypto::jose::Algorithm::ES256 {
            return Err(format!(
                "notify.push.apns.{which} must be a P-256 key (Apple's .p8 file)"
            ));
        }
        Ok(Self {
            key: key.with_kid(key_id),
            cached: Mutex::new(None),
        })
    }

    /// The provider token: the cached one while it is younger than 45
    /// minutes and is not `stale`, else a new one issued `now`.
    fn token(
        &self,
        team_id: &str,
        now: DateTime<Utc>,
        stale: Option<&str>,
    ) -> Result<String, ChannelError> {
        let mut cached = self.cached.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((token, issued)) = &*cached {
            let age = now - *issued;
            let fresh = age >= TimeDelta::zero() && age < TimeDelta::minutes(TOKEN_MINUTES);
            if fresh && stale != Some(token.as_str()) {
                return Ok(token.clone());
            }
        }
        let claims = json!({ "iss": team_id, "iat": now.timestamp() });
        let token = self
            .key
            .sign_json(&self.key.header(), &claims)
            .map_err(|error| {
                ChannelError::Config(format!("signing the provider token failed: {error}"))
            })?;
        *cached = Some((token.clone(), now));
        Ok(token)
    }
}

impl ApnsPush {
    /// A driver for `notify.push` (its `apns` table and `ttl_secs`).
    pub fn new(options: &PushOptions) -> Result<Self, String> {
        let apns = &options.apns;
        let production = if apns.key.expose().trim().is_empty() {
            None
        } else {
            Some(Signer::new(apns.key.expose(), apns.key_id.trim(), "key")?)
        };
        let sandbox = match (
            apns.sandbox_key.expose().trim().is_empty(),
            &apns.sandbox_key_id,
        ) {
            (false, Some(id)) => Some(Signer::new(
                apns.sandbox_key.expose(),
                id.trim(),
                "sandbox_key",
            )?),
            (false, None) => return Err("notify.push.apns.sandbox_key needs sandbox_key_id".into()),
            (true, _) => None,
        };
        if production.is_none() && sandbox.is_none() {
            return Err("Apple's push service needs notify.push.apns.key".into());
        }
        let endpoint = endpoint(
            apns.endpoint.as_deref(),
            "https://api.push.apple.com",
            "notify.push.apns.endpoint",
        )?;
        let sandbox_endpoint = super::endpoint(
            apns.sandbox_endpoint.as_deref(),
            "https://api.sandbox.push.apple.com",
            "notify.push.apns.sandbox_endpoint",
        )?;
        let mut client = HttpClientOptions {
            retries: 0,
            ..HttpClientOptions::default()
        };
        for (configured, url) in [
            (&apns.endpoint, &endpoint),
            (&apns.sandbox_endpoint, &sandbox_endpoint),
        ] {
            if configured.is_some()
                && let Some(host) = url.host_str()
            {
                client.ssrf.allow_hosts.push(host.to_owned());
            }
        }
        let client = HttpClient::new(client)
            .map_err(|error| format!("cannot build the HTTP client: {error}"))?;
        Ok(Self {
            team_id: apns.team_id.trim().to_owned(),
            production,
            sandbox,
            endpoint,
            sandbox_endpoint,
            ttl_secs: options.ttl_secs,
            client,
            clock: Arc::new(SystemClock),
        })
    }

    /// This driver, sending with `client`.
    #[must_use]
    pub fn with_client(mut self, client: HttpClient) -> Self {
        self.client = client;
        self
    }

    /// This driver, reading the time (provider tokens, expiry) from
    /// `clock`.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// The key and service for `environment`.
    fn route(&self, environment: PushEnvironment) -> Result<(&Signer, &Url), ChannelError> {
        let none = || {
            ChannelError::Config(format!(
                "no key for Apple's {} environment in notify.push.apns",
                environment.as_str()
            ))
        };
        match environment {
            PushEnvironment::Sandbox => Ok((
                self.sandbox
                    .as_ref()
                    .or(self.production.as_ref())
                    .ok_or_else(none)?,
                &self.sandbox_endpoint,
            )),
            _ => Ok((self.production.as_ref().ok_or_else(none)?, &self.endpoint)),
        }
    }

    async fn post(
        &self,
        url: &str,
        token: &str,
        device: &DeviceTarget,
        message: &PushMessage,
        body: &[u8],
    ) -> Result<Response, ChannelError> {
        let silent = message.is_silent();
        let ttl = message.ttl_secs.unwrap_or(self.ttl_secs);
        let expiration = if ttl == 0 {
            0
        } else {
            (self.clock.now() + TimeDelta::seconds(i64::from(ttl))).timestamp()
        };
        let mut request = self
            .client
            .post(url)
            .secret_url()
            .bearer(token)
            .header("apns-topic", device.app_id.as_str())
            .header(
                "apns-push-type",
                if silent { "background" } else { "alert" },
            )
            .header("apns-priority", priority(message).to_string())
            .header("apns-expiration", expiration.to_string())
            .header("content-type", "application/json");
        if let Some(collapse) = &message.collapse {
            request = request.header("apns-collapse-id", collapse.as_str());
        }
        request
            .body(body.to_vec())
            .send()
            .await
            .map_err(|error| unreachable(PROVIDER, error))
    }
}

/// `apns-priority`: 10 sends at once, 5 when the device has power to
/// spare (always for a silent push), 1 prioritizes the device's power.
pub(crate) fn priority(message: &PushMessage) -> u8 {
    match (message.is_silent(), message.urgency) {
        (true, _) => 5,
        (false, None | Some(Urgency::High)) => 10,
        (false, Some(Urgency::Normal | Urgency::Low)) => 5,
        (false, Some(Urgency::VeryLow)) => 1,
    }
}

/// The JSON Apple delivers to the app: `aps` (the alert's `title` and
/// `body`, `badge`, `sound`, `thread-id`, `mutable-content`, or
/// `content-available` for a silent push), and beside it `href` and each
/// data key.
pub(crate) fn payload(message: &PushMessage) -> Value {
    let mut aps = Map::new();
    if message.is_silent() {
        aps.insert("content-available".to_owned(), json!(1));
    } else {
        let mut alert = Map::new();
        if let Some(title) = &message.title {
            alert.insert("title".to_owned(), json!(title));
        }
        if let Some(body) = &message.body {
            alert.insert("body".to_owned(), json!(body));
        }
        if !alert.is_empty() {
            aps.insert("alert".to_owned(), Value::Object(alert));
        }
        if let Some(badge) = message.badge {
            aps.insert("badge".to_owned(), json!(badge));
        }
        if let Some(sound) = &message.sound {
            aps.insert("sound".to_owned(), json!(sound));
        }
        if message.mutable_content {
            aps.insert("mutable-content".to_owned(), json!(1));
        }
    }
    if let Some(thread) = &message.thread {
        aps.insert("thread-id".to_owned(), json!(thread));
    }
    let mut root = Map::new();
    root.insert("aps".to_owned(), Value::Object(aps));
    if let Some(href) = &message.href {
        root.insert("href".to_owned(), json!(href));
    }
    for (key, value) in &message.data {
        root.insert(key.clone(), json!(value));
    }
    Value::Object(root)
}

/// Apple's `reason`, from an error answer.
fn reason(response: &Response) -> Option<String> {
    response
        .json::<Value>()
        .ok()
        .and_then(|body| body["reason"].as_str().map(str::to_owned))
}

impl PushDriver for ApnsPush {
    fn send<'a>(
        &'a self,
        device: &'a DeviceTarget,
        message: &'a PushMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        Box::pin(async move {
            if device.platform != PushPlatform::Ios {
                return Err(ChannelError::InvalidRecipient(
                    "Apple's push service sends only to iOS devices".into(),
                ));
            }
            if device.token.is_empty() || !device.token.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(ChannelError::InvalidRecipient(
                    "the device token is not hexadecimal".into(),
                ));
            }
            message.check().map_err(|(field, error)| {
                ChannelError::InvalidMessage(format!("{field}: {error}"))
            })?;
            let body = serde_json::to_vec(&payload(message)).map_err(|error| {
                ChannelError::InvalidMessage(format!("the payload is not JSON: {error}"))
            })?;
            if body.len() > MAX_PAYLOAD {
                return Err(ChannelError::InvalidMessage(format!(
                    "the payload is {} bytes; Apple takes at most {MAX_PAYLOAD}",
                    body.len()
                )));
            }
            let (signer, base) = self.route(device.environment)?;
            let url = format!(
                "{}/3/device/{}",
                base.as_str().trim_end_matches('/'),
                device.token
            );
            let mut token = signer.token(&self.team_id, self.clock.now(), None)?;
            let mut renewed = false;
            loop {
                let response = self.post(&url, &token, device, message, &body).await?;
                let status = response.status().as_u16();
                if (200..300).contains(&status) {
                    let sent = SentMessage::new(PROVIDER);
                    return Ok(match response.header("apns-id") {
                        Some(id) => sent.id(id),
                        None => sent,
                    });
                }
                let reason = reason(&response);
                match (status, reason.as_deref()) {
                    (403, Some("ExpiredProviderToken")) if !renewed => {
                        renewed = true;
                        token = signer.token(&self.team_id, self.clock.now(), Some(&token))?;
                    }
                    (410, _) | (400, Some("BadDeviceToken" | "DeviceTokenNotForTopic")) => {
                        return Err(ChannelError::Gone);
                    }
                    _ => {
                        return Err(ChannelError::Refused {
                            provider: PROVIDER.to_owned(),
                            status,
                            message: reason.clone().unwrap_or_else(|| {
                                response
                                    .status()
                                    .canonical_reason()
                                    .unwrap_or("error")
                                    .to_owned()
                            }),
                            code: reason,
                        });
                    }
                }
            }
        })
    }

    fn serves(&self, platform: PushPlatform) -> bool {
        platform == PushPlatform::Ios
    }
}

impl fmt::Debug for ApnsPush {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApnsPush")
            .field("team_id", &self.team_id)
            .field("endpoint", &self.endpoint.as_str())
            .field("sandbox_endpoint", &self.sandbox_endpoint.as_str())
            .field("ttl_secs", &self.ttl_secs)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apple_payloads_carry_what_is_set() {
        let push = PushMessage::new("Deal won")
            .body("Agra renewal")
            .href("/deals/42")
            .badge(3)
            .sound("default")
            .thread("deals")
            .mutable_content()
            .data("deal_id", "42");
        assert_eq!(
            payload(&push),
            json!({
                "aps": {
                    "alert": { "title": "Deal won", "body": "Agra renewal" },
                    "badge": 3,
                    "sound": "default",
                    "thread-id": "deals",
                    "mutable-content": 1,
                },
                "href": "/deals/42",
                "deal_id": "42",
            })
        );
        assert_eq!(
            payload(&PushMessage::new("Hi")),
            json!({ "aps": { "alert": { "title": "Hi" } } })
        );
        assert_eq!(priority(&push), 10);
        assert_eq!(priority(&push.clone().urgency(Urgency::Normal)), 5);
        assert_eq!(priority(&push.urgency(Urgency::VeryLow)), 1);
    }

    #[test]
    fn a_silent_push_is_a_background_push() {
        let push = PushMessage::silent()
            .data("refresh", "inbox")
            .urgency(Urgency::High);
        assert_eq!(
            payload(&push),
            json!({ "aps": { "content-available": 1 }, "refresh": "inbox" })
        );
        assert_eq!(priority(&push), 5, "Apple wants 5 for background pushes");
    }
}
