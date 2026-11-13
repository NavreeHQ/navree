//! [`FcmPush`]: Firebase Cloud Messaging's HTTP v1 API with a service
//! account.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, TimeDelta, Utc};
use navree_contracts::{BoxFuture, Clock, Secret, SystemClock};
use navree_crypto::jose::{Algorithm, SigningKey};
use navree_http_client::{HttpClient, HttpClientOptions, Response};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use url::Url;

use super::{ChannelError, PushDriver, SentMessage, endpoint, unreachable};
use crate::{DeviceTarget, PushMessage, PushOptions, PushPlatform, Urgency};

/// The provider name of Firebase Cloud Messaging.
const PROVIDER: &str = "fcm";

/// The scope of an access token that sends messages.
const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";

/// Google's token endpoint, when the key file names none.
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// The largest message Firebase accepts, in bytes.
pub(crate) const MAX_PAYLOAD: usize = 4096;

/// The longest time to live Firebase accepts: 28 days.
const MAX_TTL_SECS: u32 = 2_419_200;

/// Sends notifications to Android devices through Firebase Cloud
/// Messaging's HTTP v1 API (`POST /v1/projects/{project}/messages:send`),
/// with an OAuth 2 access token for the service account: an RS256 JWT
/// assertion (`iss` the account's email, `scope` firebase.messaging, `aud`
/// the token endpoint, an hour long) exchanged at Google's token endpoint,
/// cached until a minute before it expires and fetched again when Firebase
/// answers `401` (the request is then sent once more).
///
/// An alert is a notification message (`notification` with the title and
/// body; `android.notification` with `sound`, `tag` (the collapse key, so
/// a newer one replaces it) and `notification_count` (the badge)); a
/// [silent](PushMessage::silent) push is a data message. `data` carries
/// `href`, `thread` and the message's data keys; `android` carries
/// `priority` (`HIGH` for [`Urgency::High`], `NORMAL` for the others),
/// `ttl`, `collapse_key` and `restricted_package_name` (the device's app).
/// A message over 4096 bytes is refused before anything is sent.
///
/// Firebase's answers: `UNREGISTERED`, `SENDER_ID_MISMATCH` and an
/// `INVALID_ARGUMENT` about the registration token mean the device is gone
/// ([`ChannelError::Gone`]); every other answer, `QUOTA_EXCEEDED` (`429`)
/// and server errors among them, is a refusal with Firebase's error code,
/// which the queue retries. The message id is the message's `name`.
pub struct FcmPush {
    project_id: String,
    client_email: String,
    key: SigningKey,
    token_url: Url,
    endpoint: Url,
    ttl_secs: u32,
    client: HttpClient,
    clock: Arc<dyn Clock>,
    cached: Mutex<Option<(Secret<String>, DateTime<Utc>)>>,
}

/// The fields of a service account key file the driver reads.
#[derive(Deserialize)]
struct ServiceAccount {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    private_key_id: Option<String>,
    private_key: String,
    client_email: String,
    #[serde(default)]
    token_uri: Option<String>,
}

impl FcmPush {
    /// A driver for `notify.push` (its `fcm` table and `ttl_secs`). The
    /// errors name the setting, never its value.
    pub fn new(options: &PushOptions) -> Result<Self, String> {
        let fcm = &options.fcm;
        let account: ServiceAccount = serde_json::from_str(fcm.service_account.expose())
            .map_err(|_| {
                "notify.push.fcm.service_account is not a service account key file (JSON with private_key and client_email)".to_owned()
            })?;
        let key = SigningKey::from_pem(&account.private_key).map_err(|error| {
            format!("the private_key of notify.push.fcm.service_account is not usable: {error}")
        })?;
        let key = key.with_algorithm(Algorithm::RS256).map_err(|_| {
            "the private_key of notify.push.fcm.service_account must be an RSA key".to_owned()
        })?;
        let key = match &account.private_key_id {
            Some(id) if !id.is_empty() => key.with_kid(id.clone()),
            _ => key,
        };
        let project_id = fcm
            .project_id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .or(account.project_id)
            .filter(|id| !id.trim().is_empty())
            .ok_or("notify.push.fcm needs project_id (the key file names none)")?;
        let token_url = endpoint(
            fcm.token_url.as_deref().or(account.token_uri.as_deref()),
            TOKEN_URL,
            "notify.push.fcm.token_url",
        )?;
        let api = endpoint(
            fcm.endpoint.as_deref(),
            "https://fcm.googleapis.com",
            "notify.push.fcm.endpoint",
        )?;
        let mut client = HttpClientOptions {
            retries: 0,
            ..HttpClientOptions::default()
        };
        for (configured, url) in [
            (fcm.endpoint.is_some(), &api),
            (fcm.token_url.is_some(), &token_url),
        ] {
            if configured && let Some(host) = url.host_str() {
                client.ssrf.allow_hosts.push(host.to_owned());
            }
        }
        let client = HttpClient::new(client)
            .map_err(|error| format!("cannot build the HTTP client: {error}"))?;
        Ok(Self {
            project_id: project_id.trim().to_owned(),
            client_email: account.client_email,
            key,
            token_url,
            endpoint: api,
            ttl_secs: options.ttl_secs,
            client,
            clock: Arc::new(SystemClock),
            cached: Mutex::new(None),
        })
    }

    /// This driver, sending with `client`.
    #[must_use]
    pub fn with_client(mut self, client: HttpClient) -> Self {
        self.client = client;
        self
    }

    /// This driver, reading the time (assertions, token lifetimes) from
    /// `clock`.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// An access token: the cached one until a minute before it expires,
    /// unless `renew`.
    async fn access_token(&self, renew: bool) -> Result<Secret<String>, ChannelError> {
        let now = self.clock.now();
        if !renew
            && let Some((token, until)) =
                &*self.cached.lock().unwrap_or_else(PoisonError::into_inner)
            && now < *until
        {
            return Ok(token.clone());
        }
        let claims = json!({
            "iss": self.client_email,
            "scope": SCOPE,
            "aud": self.token_url.as_str(),
            "iat": now.timestamp(),
            "exp": now.timestamp() + 3600,
        });
        let assertion = self.key.sign_jwt(&claims).map_err(|error| {
            ChannelError::Config(format!(
                "signing the service account assertion failed: {error}"
            ))
        })?;
        let form = [
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", assertion.as_str()),
        ];
        let response = self
            .client
            .post(self.token_url.as_str())
            .form(&form)
            .send()
            .await
            .map_err(|error| unreachable(PROVIDER, error))?;
        let answer: Value = response.json().unwrap_or(Value::Null);
        if !response.is_success() {
            return Err(ChannelError::Refused {
                provider: PROVIDER.to_owned(),
                status: response.status().as_u16(),
                code: answer["error"].as_str().map(str::to_owned),
                message: answer["error_description"]
                    .as_str()
                    .unwrap_or("the token endpoint refused the service account")
                    .chars()
                    .take(200)
                    .collect(),
            });
        }
        let token = answer["access_token"]
            .as_str()
            .filter(|token| !token.is_empty())
            .ok_or_else(|| ChannelError::Refused {
                provider: PROVIDER.to_owned(),
                status: response.status().as_u16(),
                code: None,
                message: "the token response has no access_token".to_owned(),
            })?;
        let lifetime = answer["expires_in"].as_i64().unwrap_or(3600);
        let token = Secret::new(token.to_owned());
        let until = now + TimeDelta::seconds(lifetime.saturating_sub(60));
        *self.cached.lock().unwrap_or_else(PoisonError::into_inner) = Some((token.clone(), until));
        Ok(token)
    }

    async fn post(&self, token: &Secret<String>, body: &Value) -> Result<Response, ChannelError> {
        let url = format!(
            "{}/v1/projects/{}/messages:send",
            self.endpoint.as_str().trim_end_matches('/'),
            self.project_id
        );
        self.client
            .post(url)
            .bearer(token.expose())
            .json(body)
            .send()
            .await
            .map_err(|error| unreachable(PROVIDER, error))
    }
}

/// The `message` sent for `device`, without its `token`.
pub(crate) fn message_json(message: &PushMessage, app_id: &str, default_ttl: u32) -> Value {
    let mut fields = Map::new();
    let silent = message.is_silent();
    if !silent && (message.title.is_some() || message.body.is_some()) {
        let mut notification = Map::new();
        if let Some(title) = &message.title {
            notification.insert("title".to_owned(), json!(title));
        }
        if let Some(body) = &message.body {
            notification.insert("body".to_owned(), json!(body));
        }
        fields.insert("notification".to_owned(), Value::Object(notification));
    }
    let mut data = Map::new();
    if let Some(href) = &message.href {
        data.insert("href".to_owned(), json!(href));
    }
    if let Some(thread) = &message.thread {
        data.insert("thread".to_owned(), json!(thread));
    }
    for (key, value) in &message.data {
        data.insert(key.clone(), json!(value));
    }
    if !data.is_empty() {
        fields.insert("data".to_owned(), Value::Object(data));
    }
    let ttl = message.ttl_secs.unwrap_or(default_ttl).min(MAX_TTL_SECS);
    let mut android = Map::new();
    android.insert("ttl".to_owned(), json!(format!("{ttl}s")));
    android.insert("restricted_package_name".to_owned(), json!(app_id));
    if let Some(urgency) = message.urgency {
        let priority = if urgency == Urgency::High {
            "HIGH"
        } else {
            "NORMAL"
        };
        android.insert("priority".to_owned(), json!(priority));
    }
    if let Some(collapse) = &message.collapse {
        android.insert("collapse_key".to_owned(), json!(collapse));
    }
    if !silent {
        let mut notification = Map::new();
        if let Some(sound) = &message.sound {
            notification.insert("sound".to_owned(), json!(sound));
        }
        if let Some(collapse) = &message.collapse {
            notification.insert("tag".to_owned(), json!(collapse));
        }
        if let Some(badge) = message.badge {
            notification.insert("notification_count".to_owned(), json!(badge));
        }
        if !notification.is_empty() {
            android.insert("notification".to_owned(), Value::Object(notification));
        }
    }
    fields.insert("android".to_owned(), Value::Object(android));
    Value::Object(fields)
}

/// Firebase's error code (from the `FcmError` detail, else the status)
/// and message.
fn error_of(response: &Response) -> (Option<String>, String) {
    let body: Value = response.json().unwrap_or(Value::Null);
    let error = &body["error"];
    let detail = error["details"].as_array().and_then(|details| {
        details
            .iter()
            .find_map(|detail| detail["errorCode"].as_str().map(str::to_owned))
    });
    let code = detail.or_else(|| error["status"].as_str().map(str::to_owned));
    let message = error["message"]
        .as_str()
        .map(|m| m.chars().take(200).collect())
        .unwrap_or_else(|| {
            response
                .status()
                .canonical_reason()
                .unwrap_or("error")
                .to_owned()
        });
    (code, message)
}

/// Whether an `INVALID_ARGUMENT` answer is about the registration token
/// (the payload is checked before it is sent, so then the token is bad).
fn bad_token(response: &Response, message: &str) -> bool {
    let body: Value = response.json().unwrap_or(Value::Null);
    let names_token = body["error"]["details"].as_array().is_some_and(|details| {
        details.iter().any(|detail| {
            detail["fieldViolations"]
                .as_array()
                .is_some_and(|violations| {
                    violations
                        .iter()
                        .any(|violation| violation["field"] == "message.token")
                })
        })
    });
    names_token || message.to_ascii_lowercase().contains("registration token")
}

impl PushDriver for FcmPush {
    fn send<'a>(
        &'a self,
        device: &'a DeviceTarget,
        message: &'a PushMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        Box::pin(async move {
            if device.platform != PushPlatform::Android {
                return Err(ChannelError::InvalidRecipient(
                    "Firebase Cloud Messaging sends only to Android devices here".into(),
                ));
            }
            message.check().map_err(|(field, error)| {
                ChannelError::InvalidMessage(format!("{field}: {error}"))
            })?;
            let mut fields = message_json(message, &device.app_id, self.ttl_secs);
            let size = serde_json::to_vec(&fields)
                .map(|bytes| bytes.len())
                .unwrap_or(usize::MAX);
            if size > MAX_PAYLOAD {
                return Err(ChannelError::InvalidMessage(format!(
                    "the message is {size} bytes; Firebase takes at most {MAX_PAYLOAD}"
                )));
            }
            if let Value::Object(map) = &mut fields {
                map.insert("token".to_owned(), json!(device.token));
            }
            let body = json!({ "message": fields });
            let mut token = self.access_token(false).await?;
            let mut renewed = false;
            loop {
                let response = self.post(&token, &body).await?;
                let status = response.status().as_u16();
                if (200..300).contains(&status) {
                    let answer: Value = response.json().unwrap_or(Value::Null);
                    let sent = SentMessage::new(PROVIDER);
                    return Ok(match answer["name"].as_str() {
                        Some(name) => sent.id(name),
                        None => sent,
                    });
                }
                let (code, text) = error_of(&response);
                match (status, code.as_deref()) {
                    (401, code) if !renewed && code != Some("THIRD_PARTY_AUTH_ERROR") => {
                        renewed = true;
                        token = self.access_token(true).await?;
                    }
                    (_, Some("UNREGISTERED" | "SENDER_ID_MISMATCH")) => {
                        return Err(ChannelError::Gone);
                    }
                    (400, Some("INVALID_ARGUMENT")) if bad_token(&response, &text) => {
                        return Err(ChannelError::Gone);
                    }
                    _ => {
                        return Err(ChannelError::Refused {
                            provider: PROVIDER.to_owned(),
                            status,
                            code,
                            message: text,
                        });
                    }
                }
            }
        })
    }

    fn serves(&self, platform: PushPlatform) -> bool {
        platform == PushPlatform::Android
    }
}

impl fmt::Debug for FcmPush {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FcmPush")
            .field("project_id", &self.project_id)
            .field("client_email", &self.client_email)
            .field("endpoint", &self.endpoint.as_str())
            .field("ttl_secs", &self.ttl_secs)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_messages_carry_the_notification_fields() {
        let push = PushMessage::new("Deal won")
            .body("Agra renewal")
            .href("/deals/42")
            .badge(3)
            .sound("default")
            .thread("deals")
            .collapse("deal-42")
            .ttl_secs(600)
            .urgency(Urgency::High)
            .data("deal_id", "42");
        assert_eq!(
            message_json(&push, "com.acme.crm", 86_400),
            json!({
                "notification": { "title": "Deal won", "body": "Agra renewal" },
                "data": { "href": "/deals/42", "thread": "deals", "deal_id": "42" },
                "android": {
                    "ttl": "600s",
                    "restricted_package_name": "com.acme.crm",
                    "priority": "HIGH",
                    "collapse_key": "deal-42",
                    "notification": {
                        "sound": "default",
                        "tag": "deal-42",
                        "notification_count": 3,
                    },
                },
            })
        );
    }

    #[test]
    fn a_silent_push_is_a_data_message() {
        let push = PushMessage::silent()
            .data("refresh", "inbox")
            .ttl_secs(u32::MAX);
        assert_eq!(
            message_json(&push, "com.acme.crm", 86_400),
            json!({
                "data": { "refresh": "inbox" },
                "android": { "ttl": "2419200s", "restricted_package_name": "com.acme.crm" },
            })
        );
    }
}
