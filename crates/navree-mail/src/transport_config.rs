//! [`TransportConfig`]: which transport delivers mail.

use std::fmt;
use std::sync::Arc;

use navree_contracts::Secret;
use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value, json};
use url::Url;

use crate::api::decoded;
use crate::{
    ArrayTransport, DkimConfig, DkimSigner, FailoverTransport, LogTransport, MailError,
    MailTransport, MailgunConfig, MailgunRegion, MailgunTransport, PostmarkConfig,
    PostmarkTransport, ResendConfig, ResendTransport, SesConfig, SesTransport, SmtpTransport,
};

/// Which [`MailTransport`] delivers mail.
///
/// In configuration, a table with a `type`, or a shorthand string (handy in
/// environment variables):
///
/// | Transport | Table | Shorthand |
/// |---|---|---|
/// | [`SmtpTransport`] | `{ type = "smtp", url = "smtps://user:pass@host", dkim = {..}, ca = "-----BEGIN CERTIFICATE-----..." }` | `"smtps://user:pass@host"` / `"smtp://..."` |
/// | [`SesTransport`] | `{ type = "ses", region, access_key_id, secret_access_key, configuration_set }` | `"ses://KEY_ID:SECRET@eu-west-1?configuration_set=events"` |
/// | [`MailgunTransport`] | `{ type = "mailgun", domain, api_key, region }` | `"mailgun://API_KEY@mg.acme.test?region=eu"` |
/// | [`PostmarkTransport`] | `{ type = "postmark", server_token, message_stream }` | `"postmark://TOKEN@api.postmarkapp.com?stream=outbound"` |
/// | [`ResendTransport`] | `{ type = "resend", api_key }` | `"resend://API_KEY@api.resend.com"` |
/// | [`LogTransport`] | `{ type = "log" }` | `"log"` |
/// | [`ArrayTransport`] | `{ type = "array" }` | `"array"` |
/// | [`FailoverTransport`] | `{ type = "failover", transports = ["ses://...", "smtps://..."] }` | — |
///
/// Every API transport takes an `endpoint` (`?endpoint=` in a shorthand)
/// to reach another base URL. Percent-encode reserved characters in
/// shorthand credentials (an SES secret's `/` is `%2F`). The failover's
/// `transports` may also be one comma-separated string of shorthands
/// (`APP_MAIL__TRANSPORT__TRANSPORTS=ses://...,log`). URLs, keys and tokens
/// are [`Secret`]s: `Debug` never prints them.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum TransportConfig {
    /// SMTP at `url` (see [`SmtpTransport::from_url`]), DKIM-signed when
    /// `dkim` is set, and trusting the certificate in `ca` besides the
    /// public authorities (see [`SmtpTransport::with_root_certificate`]).
    Smtp {
        /// The connection URL; it usually holds a password.
        url: Secret<String>,
        /// DKIM signing.
        dkim: Option<DkimConfig>,
        /// One more certificate or certificate authority to trust, as PEM
        /// text.
        ca: Option<String>,
    },
    /// Amazon SES.
    Ses(SesConfig),
    /// Mailgun.
    Mailgun(MailgunConfig),
    /// Postmark.
    Postmark(PostmarkConfig),
    /// Resend.
    Resend(ResendConfig),
    /// Write messages to the log (development).
    #[default]
    Log,
    /// Record messages in memory (tests; see
    /// [`Mailer::array_transport`](crate::Mailer::array_transport)).
    Array,
    /// Try each transport in order until one delivers.
    Failover(Vec<TransportConfig>),
}

impl TransportConfig {
    /// SMTP at `url`.
    pub fn smtp(url: impl Into<String>) -> Self {
        TransportConfig::Smtp {
            url: Secret::new(url.into()),
            dkim: None,
            ca: None,
        }
    }

    /// This SMTP transport, also trusting the certificate in `pem` (see
    /// [`SmtpTransport::with_root_certificate`]). Any other transport is
    /// returned as it is.
    #[must_use]
    pub fn with_ca(mut self, pem: impl Into<String>) -> Self {
        if let TransportConfig::Smtp { ca, .. } = &mut self {
            *ca = Some(pem.into());
        }
        self
    }

    /// The transport's type: `smtp`, `ses`, `mailgun`, `postmark`,
    /// `resend`, `log`, `array` or `failover`.
    pub fn kind(&self) -> &'static str {
        match self {
            TransportConfig::Smtp { .. } => "smtp",
            TransportConfig::Ses(_) => "ses",
            TransportConfig::Mailgun(_) => "mailgun",
            TransportConfig::Postmark(_) => "postmark",
            TransportConfig::Resend(_) => "resend",
            TransportConfig::Log => "log",
            TransportConfig::Array => "array",
            TransportConfig::Failover(_) => "failover",
        }
    }

    /// Builds the transport. Makes no network connection; invalid settings
    /// (an SMTP URL, missing credentials, an empty failover list) are
    /// [`MailError::Config`].
    pub fn build(&self) -> Result<Arc<dyn MailTransport>, MailError> {
        Ok(match self {
            TransportConfig::Smtp { url, dkim, ca } => {
                let mut transport = SmtpTransport::from_url(url.expose())?;
                if let Some(ca) = ca {
                    transport = transport.with_root_certificate(ca)?;
                }
                if let Some(dkim) = dkim {
                    transport = transport.with_dkim(DkimSigner::new(dkim)?);
                }
                Arc::new(transport)
            }
            TransportConfig::Ses(config) => Arc::new(SesTransport::new(config.clone())?),
            TransportConfig::Mailgun(config) => Arc::new(MailgunTransport::new(config.clone())?),
            TransportConfig::Postmark(config) => Arc::new(PostmarkTransport::new(config.clone())?),
            TransportConfig::Resend(config) => Arc::new(ResendTransport::new(config.clone())?),
            TransportConfig::Log => Arc::new(LogTransport),
            TransportConfig::Array => Arc::new(ArrayTransport::new()),
            TransportConfig::Failover(transports) => {
                if transports.is_empty() {
                    return Err(MailError::Config(
                        "a failover transport needs at least one transport".to_owned(),
                    ));
                }
                let built = transports
                    .iter()
                    .map(TransportConfig::build)
                    .collect::<Result<Vec<_>, _>>()?;
                Arc::new(FailoverTransport::new(built))
            }
        })
    }

    /// The table form as JSON, credentials included: what
    /// [`TransportResolver`](crate::TransportResolver) encrypts into
    /// `mail_transports.config`. It reads back with `serde_json::from_value`.
    pub fn to_json(&self) -> Value {
        fn secret(value: &Secret<String>) -> Value {
            json!(value.expose())
        }
        fn optional(map: &mut Map<String, Value>, key: &str, value: Option<Value>) {
            if let Some(value) = value {
                map.insert(key.to_owned(), value);
            }
        }
        let mut map = Map::new();
        map.insert("type".to_owned(), json!(self.kind()));
        match self {
            TransportConfig::Smtp { url, dkim, ca } => {
                map.insert("url".to_owned(), secret(url));
                optional(&mut map, "ca", ca.as_ref().map(|pem| json!(pem)));
                optional(
                    &mut map,
                    "dkim",
                    dkim.as_ref().map(|dkim| {
                        json!({
                            "domain": dkim.domain,
                            "selector": dkim.selector,
                            "private_key": dkim.private_key.expose(),
                            "headers": dkim.headers,
                        })
                    }),
                );
            }
            TransportConfig::Ses(config) => {
                map.insert("region".to_owned(), json!(config.region));
                map.insert("access_key_id".to_owned(), json!(config.access_key_id));
                map.insert(
                    "secret_access_key".to_owned(),
                    secret(&config.secret_access_key),
                );
                optional(
                    &mut map,
                    "session_token",
                    config.session_token.as_ref().map(secret),
                );
                optional(
                    &mut map,
                    "configuration_set",
                    config.configuration_set.as_ref().map(|s| json!(s)),
                );
                optional(
                    &mut map,
                    "endpoint",
                    config.endpoint.as_ref().map(|s| json!(s)),
                );
            }
            TransportConfig::Mailgun(config) => {
                map.insert("domain".to_owned(), json!(config.domain));
                map.insert("api_key".to_owned(), secret(&config.api_key));
                map.insert(
                    "region".to_owned(),
                    json!(match config.region {
                        MailgunRegion::Us => "us",
                        MailgunRegion::Eu => "eu",
                    }),
                );
                optional(
                    &mut map,
                    "endpoint",
                    config.endpoint.as_ref().map(|s| json!(s)),
                );
            }
            TransportConfig::Postmark(config) => {
                map.insert("server_token".to_owned(), secret(&config.server_token));
                optional(
                    &mut map,
                    "message_stream",
                    config.message_stream.as_ref().map(|s| json!(s)),
                );
                optional(
                    &mut map,
                    "endpoint",
                    config.endpoint.as_ref().map(|s| json!(s)),
                );
            }
            TransportConfig::Resend(config) => {
                map.insert("api_key".to_owned(), secret(&config.api_key));
                optional(
                    &mut map,
                    "endpoint",
                    config.endpoint.as_ref().map(|s| json!(s)),
                );
            }
            TransportConfig::Log | TransportConfig::Array => {}
            TransportConfig::Failover(transports) => {
                map.insert(
                    "transports".to_owned(),
                    transports.iter().map(TransportConfig::to_json).collect(),
                );
            }
        }
        Value::Object(map)
    }

    /// Whether any `endpoint` is set (here or in a failover's transports).
    pub(crate) fn has_endpoint(&self) -> bool {
        match self {
            TransportConfig::Ses(c) => c.endpoint.is_some(),
            TransportConfig::Mailgun(c) => c.endpoint.is_some(),
            TransportConfig::Postmark(c) => c.endpoint.is_some(),
            TransportConfig::Resend(c) => c.endpoint.is_some(),
            TransportConfig::Failover(list) => list.iter().any(TransportConfig::has_endpoint),
            _ => false,
        }
    }

    /// The SMTP hosts this transport connects to (for the SSRF check of
    /// tenant transports).
    pub(crate) fn smtp_hosts(&self) -> Vec<String> {
        match self {
            TransportConfig::Smtp { url, .. } => Url::parse(url.expose())
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned))
                .into_iter()
                .collect(),
            TransportConfig::Failover(list) => {
                list.iter().flat_map(TransportConfig::smtp_hosts).collect()
            }
            _ => Vec::new(),
        }
    }

    /// The transport a shorthand string names: `Ok(None)` when it is no
    /// shorthand at all.
    fn shorthand(s: &str) -> Result<Option<Self>, String> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("log") {
            return Ok(Some(TransportConfig::Log));
        }
        if s.eq_ignore_ascii_case("array") {
            return Ok(Some(TransportConfig::Array));
        }
        let Some((scheme, _)) = s.split_once("://") else {
            return Ok(None);
        };
        let scheme = scheme.to_ascii_lowercase();
        if scheme == "smtp" || scheme == "smtps" {
            return Ok(Some(TransportConfig::smtp(s)));
        }
        if !matches!(scheme.as_str(), "ses" | "mailgun" | "postmark" | "resend") {
            return Ok(None);
        }
        let url = Url::parse(s).map_err(|_| format!("invalid `{scheme}://` URL"))?;
        let query = |key: &str| {
            url.query_pairs()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.into_owned())
                .filter(|v| !v.is_empty())
        };
        let user = decoded(url.username());
        let password = url.password().map(decoded).unwrap_or_default();
        let host = url.host_str().unwrap_or_default().to_owned();
        Ok(Some(match scheme.as_str() {
            "ses" => {
                if host.is_empty() {
                    return Err("an `ses://` URL names the region as its host".to_owned());
                }
                TransportConfig::Ses(SesConfig {
                    region: host,
                    access_key_id: user,
                    secret_access_key: Secret::new(password),
                    session_token: query("session_token").map(Secret::new),
                    configuration_set: query("configuration_set"),
                    endpoint: query("endpoint"),
                })
            }
            "mailgun" => {
                let region = match query("region").as_deref() {
                    None | Some("us") => MailgunRegion::Us,
                    Some("eu") => MailgunRegion::Eu,
                    Some(_) => return Err("a Mailgun `region` is `us` or `eu`".to_owned()),
                };
                TransportConfig::Mailgun(MailgunConfig {
                    domain: host,
                    api_key: Secret::new(user),
                    region,
                    endpoint: query("endpoint"),
                })
            }
            "postmark" => TransportConfig::Postmark(PostmarkConfig {
                server_token: Secret::new(user),
                message_stream: query("stream"),
                endpoint: query("endpoint"),
            }),
            _ => TransportConfig::Resend(ResendConfig {
                api_key: Secret::new(user),
                endpoint: query("endpoint"),
            }),
        }))
    }
}

/// What a transport must look like (never echoes the value: it may be a
/// mistyped URL with a password).
const EXPECTED: &str = "`log`, `array`, an `smtp://`, `smtps://`, `ses://`, `mailgun://`, `postmark://` or `resend://` URL, or a table with `type`";

impl<'de> Deserialize<'de> for TransportConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(TransportVisitor)
    }
}

/// Reads a shorthand string or a tagged table.
struct TransportVisitor;

impl<'de> Visitor<'de> for TransportVisitor {
    type Value = TransportConfig;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a mail transport: {EXPECTED}")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<TransportConfig, E> {
        match TransportConfig::shorthand(value) {
            Ok(Some(config)) => Ok(config),
            Ok(None) => Err(E::custom(format!(
                "unknown mail transport (expected {EXPECTED})"
            ))),
            Err(reason) => Err(E::custom(format!("invalid mail transport: {reason}"))),
        }
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<TransportConfig, A::Error> {
        Tagged::deserialize(de::value::MapAccessDeserializer::new(map)).map(Into::into)
    }
}

/// The table form, tagged by `type`.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Tagged {
    Smtp {
        url: Secret<String>,
        #[serde(default)]
        dkim: Option<DkimConfig>,
        #[serde(default)]
        ca: Option<String>,
    },
    Ses(SesConfig),
    Mailgun(MailgunConfig),
    Postmark(PostmarkConfig),
    Resend(ResendConfig),
    Log,
    Array,
    Failover {
        #[serde(deserialize_with = "transport_list")]
        transports: Vec<TransportConfig>,
    },
}

impl From<Tagged> for TransportConfig {
    fn from(tagged: Tagged) -> Self {
        match tagged {
            Tagged::Smtp { url, dkim, ca } => TransportConfig::Smtp { url, dkim, ca },
            Tagged::Ses(config) => TransportConfig::Ses(config),
            Tagged::Mailgun(config) => TransportConfig::Mailgun(config),
            Tagged::Postmark(config) => TransportConfig::Postmark(config),
            Tagged::Resend(config) => TransportConfig::Resend(config),
            Tagged::Log => TransportConfig::Log,
            Tagged::Array => TransportConfig::Array,
            Tagged::Failover { transports } => TransportConfig::Failover(transports),
        }
    }
}

/// A list of transports, or one comma-separated string of shorthands.
fn transport_list<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<TransportConfig>, D::Error> {
    struct ListVisitor;

    impl<'de> Visitor<'de> for ListVisitor {
        type Value = Vec<TransportConfig>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a list of mail transports or a comma-separated string of shorthands")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut transports = Vec::new();
            while let Some(transport) = seq.next_element::<TransportConfig>()? {
                transports.push(transport);
            }
            Ok(transports)
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            value
                .split(',')
                .filter(|part| !part.trim().is_empty())
                .map(|part| TransportVisitor.visit_str(part))
                .collect()
        }
    }

    deserializer.deserialize_any(ListVisitor)
}
