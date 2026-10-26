//! [`WebhooksOptions`]: the `webhooks` configuration section.

use std::collections::BTreeMap;
use std::fmt;

use navree_config::ConfigSection;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};

use crate::template::{SignatureFormat, SignedTemplate};

/// Options of the webhooks module (configuration section `webhooks`).
///
/// ```toml
/// [webhooks]
/// queue = "webhooks"          # the queue deliveries run on
/// timeout_secs = 15           # per attempt
/// require_https = true        # endpoint URLs must use https
/// disable_after_days = 5      # disable an endpoint that has failed for this long
/// max_payload_bytes = 262144  # largest outgoing payload
/// retain_days = 30            # webhooks:prune keeps deliveries this long
/// routes = true               # the /api/webhooks routes
///
/// [webhooks.incoming]
/// tolerance_secs = 300        # accepted age of an incoming signature
/// max_body_bytes = 1048576    # largest incoming body
/// receipt_days = 30           # how long mark_processed receipts are kept
///
/// [webhooks.incoming.stripe]
/// secret = ""                 # APP_WEBHOOKS__INCOMING__STRIPE__SECRET
/// ```
///
/// Every key also comes from the environment: `APP_WEBHOOKS__REQUIRE_HTTPS=false`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct WebhooksOptions {
    /// The queue of the fan-out and delivery jobs (default `default`).
    pub queue: String,
    /// Time allowed per delivery attempt, in seconds (default 15, at least 1).
    pub timeout_secs: u64,
    /// Refuse `http://` endpoint URLs (default true).
    pub require_https: bool,
    /// Disable an endpoint whose deliveries have failed, with no success in
    /// between, for this many days (default 5; `0` never disables).
    pub disable_after_days: u64,
    /// The largest outgoing payload in bytes (default 256 KB); a bigger one
    /// fails the dispatch.
    pub max_payload_bytes: usize,
    /// `webhooks:prune` deletes deliveries older than this many days
    /// (default 30).
    pub retain_days: u64,
    /// Mount the `/api/webhooks` routes (default true; permission
    /// `webhooks.manage`).
    pub routes: bool,
    /// Incoming webhooks.
    pub incoming: IncomingOptions,
}

impl Default for WebhooksOptions {
    fn default() -> Self {
        Self {
            queue: "default".to_owned(),
            timeout_secs: 15,
            require_https: true,
            disable_after_days: 5,
            max_payload_bytes: 256 * 1024,
            retain_days: 30,
            routes: true,
            incoming: IncomingOptions::default(),
        }
    }
}

impl ConfigSection for WebhooksOptions {
    const KEY: &'static str = "webhooks";

    fn validate(&self) -> Result<(), String> {
        if self.queue.trim().is_empty() {
            return Err("`queue` must not be empty".to_owned());
        }
        if self.timeout_secs == 0 {
            return Err("`timeout_secs` must be at least 1".to_owned());
        }
        if self.max_payload_bytes == 0 {
            return Err("`max_payload_bytes` must be at least 1".to_owned());
        }
        self.incoming.validate()
    }
}

/// Incoming webhooks (`webhooks.incoming`): the shared limits, and one
/// table per source (`webhooks.incoming.<source>`) with its secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingOptions {
    /// The accepted age of a signed timestamp, in seconds, in either
    /// direction (default 300).
    pub tolerance_secs: u64,
    /// The largest incoming body in bytes (default 1 MiB); a bigger one is
    /// answered with 413.
    pub max_body_bytes: usize,
    /// How long [`mark_processed`](crate::VerifiedWebhook::mark_processed)
    /// receipts are kept, in days (default 30).
    pub receipt_days: u64,
    /// The sources, by name (`stripe`, `github`, `mail_events`).
    pub sources: BTreeMap<String, SourceOptions>,
}

impl Default for IncomingOptions {
    fn default() -> Self {
        Self {
            tolerance_secs: 300,
            max_body_bytes: 1024 * 1024,
            receipt_days: 30,
            sources: BTreeMap::new(),
        }
    }
}

impl IncomingOptions {
    /// The options of source `name`, if configured.
    pub fn source(&self, name: &str) -> Option<&SourceOptions> {
        self.sources.get(name)
    }

    /// The same options with source `name` set to `source`.
    pub fn with_source(mut self, name: impl Into<String>, source: SourceOptions) -> Self {
        self.sources.insert(name.into(), source);
        self
    }

    /// Checks the limits and every source: a source needs a secret, and a
    /// `format` or `signed` template must be well-formed.
    fn validate(&self) -> Result<(), String> {
        if self.max_body_bytes == 0 {
            return Err("`incoming.max_body_bytes` must be at least 1".to_owned());
        }
        for (name, source) in &self.sources {
            if source.secret.trim().is_empty() {
                return Err(format!(
                    "`incoming.{name}.secret` is empty: set webhooks.incoming.{name}.secret (APP_WEBHOOKS__INCOMING__{}__SECRET)",
                    name.to_ascii_uppercase()
                ));
            }
            if let Some(format) = &source.format {
                SignatureFormat::parse(format)
                    .map_err(|error| format!("`incoming.{name}.format`: {error}"))?;
            }
            if let Some(signed) = &source.signed {
                SignedTemplate::parse(signed)
                    .map_err(|error| format!("`incoming.{name}.signed`: {error}"))?;
            }
        }
        Ok(())
    }
}

/// `tolerance_secs`, `max_body_bytes` and `receipt_days` are read as
/// numbers (or numeric strings, from the environment); every other key is a
/// source table.
impl<'de> Deserialize<'de> for IncomingOptions {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct IncomingVisitor;

        impl<'de> Visitor<'de> for IncomingVisitor {
            type Value = IncomingOptions;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("the `webhooks.incoming` table")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut options = IncomingOptions::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "tolerance_secs" => options.tolerance_secs = map.next_value()?,
                        "max_body_bytes" => options.max_body_bytes = map.next_value()?,
                        "receipt_days" => options.receipt_days = map.next_value()?,
                        _ => {
                            let source: SourceOptions = map.next_value().map_err(|error| {
                                de::Error::custom(format!("incoming.{key}: {error}"))
                            })?;
                            options.sources.insert(key, source);
                        }
                    }
                }
                Ok(options)
            }
        }

        deserializer.deserialize_map(IncomingVisitor)
    }
}

/// One incoming source (`webhooks.incoming.<source>`): its secret, and for
/// the generic [`Hmac`](crate::Hmac) verifier how the signature is sent.
///
/// ```toml
/// [webhooks.incoming.calendly]
/// secret = ""                       # APP_WEBHOOKS__INCOMING__CALENDLY__SECRET
/// header = "Calendly-Webhook-Signature"
/// algorithm = "sha256"              # sha1 | sha256 | sha512
/// encoding = "hex"                  # hex | base64
/// format = "t={timestamp},v1={signature}"
/// signed = "{timestamp}.{body}"
/// ```
///
/// `Debug` never shows the secret.
#[derive(Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct SourceOptions {
    /// The signing secret. Required.
    pub secret: String,
    /// The accepted age of this source's signatures, in seconds, instead of
    /// `webhooks.incoming.tolerance_secs`.
    pub tolerance_secs: Option<u64>,
    /// [`Hmac`](crate::Hmac): the header carrying the signature.
    pub header: Option<String>,
    /// [`Hmac`](crate::Hmac): the HMAC hash (default `sha256`).
    pub algorithm: HmacAlgorithm,
    /// [`Hmac`](crate::Hmac): how the signature is written (default `hex`).
    pub encoding: SignatureEncoding,
    /// [`Hmac`](crate::Hmac): the shape of the header value, with
    /// `{signature}` and optionally `{timestamp}` (default `{signature}`).
    pub format: Option<String>,
    /// [`Hmac`](crate::Hmac): what is signed, from `{timestamp}`, `{id}`
    /// and `{body}` (default `{body}`).
    pub signed: Option<String>,
    /// [`Hmac`](crate::Hmac): a header carrying the signed timestamp (Unix
    /// seconds), when it is not part of `format`.
    pub timestamp_header: Option<String>,
    /// [`Hmac`](crate::Hmac): the header carrying the sender's delivery id.
    pub id_header: Option<String>,
    /// [`Hmac`](crate::Hmac): the header carrying the sender's event type.
    pub event_header: Option<String>,
}

impl SourceOptions {
    /// A source with `secret` and default settings.
    pub fn new(secret: impl Into<String>) -> Self {
        Self {
            secret: secret.into(),
            ..Self::default()
        }
    }
}

impl fmt::Debug for SourceOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceOptions")
            .field("secret", &"[redacted]")
            .field("tolerance_secs", &self.tolerance_secs)
            .field("header", &self.header)
            .field("algorithm", &self.algorithm)
            .field("encoding", &self.encoding)
            .field("format", &self.format)
            .field("signed", &self.signed)
            .field("timestamp_header", &self.timestamp_header)
            .field("id_header", &self.id_header)
            .field("event_header", &self.event_header)
            .finish()
    }
}

/// The hash of an HMAC signature.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HmacAlgorithm {
    /// HMAC-SHA1 (only for senders that still use it).
    Sha1,
    /// HMAC-SHA256.
    #[default]
    Sha256,
    /// HMAC-SHA512.
    Sha512,
}

/// How a signature is written in a header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignatureEncoding {
    /// Hexadecimal (either case).
    #[default]
    Hex,
    /// Base64 (standard or URL-safe alphabet, padded or not).
    Base64,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse(value: serde_json::Value) -> WebhooksOptions {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn defaults_match_the_documented_ones() {
        let options = parse(json!({}));
        assert_eq!(options, WebhooksOptions::default());
        assert_eq!(options.queue, "default");
        assert_eq!(options.timeout_secs, 15);
        assert!(options.require_https);
        assert_eq!(options.disable_after_days, 5);
        assert_eq!(options.max_payload_bytes, 262_144);
        assert_eq!(options.retain_days, 30);
        assert!(options.routes);
        assert_eq!(options.incoming.tolerance_secs, 300);
        assert_eq!(options.incoming.max_body_bytes, 1_048_576);
        assert_eq!(options.incoming.receipt_days, 30);
        assert!(options.incoming.sources.is_empty());
        assert_eq!(options.validate(), Ok(()));
    }

    #[test]
    fn incoming_tables_are_sources_next_to_the_limits() {
        let options = parse(json!({
            "incoming": {
                "tolerance_secs": 60,
                "stripe": { "secret": "whsec_abc" },
                "calendly": {
                    "secret": "cal",
                    "header": "Calendly-Webhook-Signature",
                    "algorithm": "sha1",
                    "encoding": "base64",
                    "format": "t={timestamp},v1={signature}",
                    "signed": "{timestamp}.{body}",
                    "tolerance_secs": 30
                }
            }
        }));
        assert_eq!(options.incoming.tolerance_secs, 60);
        assert_eq!(
            options.incoming.source("stripe").unwrap().secret,
            "whsec_abc"
        );
        let calendly = options.incoming.source("calendly").unwrap();
        assert_eq!(calendly.algorithm, HmacAlgorithm::Sha1);
        assert_eq!(calendly.encoding, SignatureEncoding::Base64);
        assert_eq!(calendly.tolerance_secs, Some(30));
        assert_eq!(options.validate(), Ok(()));
    }

    #[test]
    fn a_source_without_a_secret_names_the_key() {
        let options = parse(json!({ "incoming": { "github": { "secret": "" } } }));
        let error = options.validate().unwrap_err();
        assert!(error.contains("incoming.github.secret"), "{error}");
        assert!(
            error.contains("APP_WEBHOOKS__INCOMING__GITHUB__SECRET"),
            "{error}"
        );
    }

    #[test]
    fn malformed_templates_and_limits_are_refused() {
        let bad_format = parse(json!({
            "incoming": { "x": { "secret": "s", "format": "t={timestamp}" } }
        }));
        assert!(
            bad_format
                .validate()
                .unwrap_err()
                .contains("incoming.x.format")
        );
        let bad_signed = parse(json!({
            "incoming": { "x": { "secret": "s", "signed": "{nonce}.{body}" } }
        }));
        assert!(
            bad_signed
                .validate()
                .unwrap_err()
                .contains("incoming.x.signed")
        );
        let no_timeout = parse(json!({ "timeout_secs": 0 }));
        assert!(no_timeout.validate().unwrap_err().contains("timeout_secs"));
        let no_queue = parse(json!({ "queue": " " }));
        assert!(no_queue.validate().unwrap_err().contains("queue"));
    }

    #[test]
    fn debug_never_shows_a_secret() {
        let shown = format!("{:?}", SourceOptions::new("whsec_top_secret"));
        assert!(!shown.contains("top_secret"), "{shown}");
        assert!(shown.contains("[redacted]"));
    }
}
