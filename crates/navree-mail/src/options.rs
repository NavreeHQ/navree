//! [`MailOptions`] and [`TransportConfig`].

use std::fmt;
use std::sync::Arc;

use navree_config::ConfigSection;
use navree_contracts::Secret;
use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::message::mailbox;
use crate::{
    ArrayTransport, FailoverTransport, LogTransport, MailError, MailTransport, SmtpTransport,
};

/// Mail settings. Configuration section `mail`:
///
/// ```toml
/// [mail]
/// from = "Acme <hello@acme.test>"   # APP_MAIL__FROM
/// queue = "mail"                    # APP_MAIL__QUEUE (queue for Mailer::queue)
///
/// [mail.transport]                  # or APP_MAIL__TRANSPORT=smtps://user:pass@smtp.acme.test
/// type = "smtp"
/// url = "smtps://user:pass@smtp.acme.test"
/// ```
///
/// See [`TransportConfig`] for every transport and its shorthand.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct MailOptions {
    /// The default sender, `hello@example.com` or `Acme <hello@acme.test>`
    /// (default `hello@example.com`). Empty means no default: every message
    /// must then set its own sender.
    pub from: String,
    /// How messages are delivered (default [`TransportConfig::Log`]).
    pub transport: TransportConfig,
    /// The queue [`Mailer::queue`](crate::Mailer::queue) dispatches to
    /// (default: the job's own queue, `default`).
    pub queue: Option<String>,
}

impl Default for MailOptions {
    fn default() -> Self {
        Self {
            from: "hello@example.com".to_owned(),
            transport: TransportConfig::Log,
            queue: None,
        }
    }
}

impl ConfigSection for MailOptions {
    const KEY: &'static str = "mail";

    /// The sender parses and the transport builds (no connection is made).
    fn validate(&self) -> Result<(), String> {
        if !self.from.is_empty() {
            mailbox("from", &self.from).map_err(|error| error.to_string())?;
        }
        self.transport
            .build()
            .map(drop)
            .map_err(|error| error.to_string())
    }
}

/// Which [`MailTransport`] delivers mail.
///
/// In configuration, a table with a `type`, or a shorthand string (handy in
/// environment variables):
///
/// | Transport | Table | Shorthand |
/// |---|---|---|
/// | [`SmtpTransport`] | `{ type = "smtp", url = "smtps://user:pass@host" }` | `"smtps://user:pass@host"` / `"smtp://..."` |
/// | [`LogTransport`] | `{ type = "log" }` | `"log"` |
/// | [`ArrayTransport`] | `{ type = "array" }` | `"array"` |
/// | [`FailoverTransport`] | `{ type = "failover", transports = ["smtps://...", "log"] }` | — |
///
/// The failover's `transports` may also be one comma-separated string of
/// shorthands (`APP_MAIL__TRANSPORT__TRANSPORTS=smtps://...,log`). The SMTP
/// URL is a [`Secret`]: `Debug` never prints it.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum TransportConfig {
    /// SMTP at `url` (see [`SmtpTransport::from_url`]).
    Smtp {
        /// The connection URL; it usually holds a password.
        url: Secret<String>,
    },
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
        }
    }

    /// Builds the transport. Makes no network connection; an invalid SMTP
    /// URL or an empty failover list is [`MailError::Config`].
    pub fn build(&self) -> Result<Arc<dyn MailTransport>, MailError> {
        Ok(match self {
            TransportConfig::Smtp { url } => Arc::new(SmtpTransport::from_url(url.expose())?),
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

    /// The transport a shorthand string names, if any.
    fn shorthand(s: &str) -> Option<Self> {
        let s = s.trim();
        let scheme = s.split_once("://").map(|(scheme, _)| scheme);
        if s.eq_ignore_ascii_case("log") {
            Some(TransportConfig::Log)
        } else if s.eq_ignore_ascii_case("array") {
            Some(TransportConfig::Array)
        } else if scheme.is_some_and(|scheme| {
            scheme.eq_ignore_ascii_case("smtp") || scheme.eq_ignore_ascii_case("smtps")
        }) {
            Some(TransportConfig::smtp(s))
        } else {
            None
        }
    }
}

/// What a transport must look like (never echoes the value: it may be a
/// mistyped URL with a password).
const EXPECTED: &str = "`log`, `array`, an `smtp://` or `smtps://` URL, or a table with `type`";

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
        TransportConfig::shorthand(value)
            .ok_or_else(|| E::custom(format!("unknown mail transport (expected {EXPECTED})")))
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
    },
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
            Tagged::Smtp { url } => TransportConfig::Smtp { url },
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
