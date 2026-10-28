//! [`InboundOptions`]: section `mail.inbound`.

use navree_contracts::Secret;
use serde::Deserialize;

/// 25 MiB.
const DEFAULT_MAX_SIZE: usize = 25 * 1024 * 1024;

/// Inbound mail. Configuration section `mail.inbound` (crate feature
/// `inbound`); a source's route is mounted when its secret is set:
///
/// ```toml
/// [mail.inbound]
/// max_size = 26214400          # bytes (25 MiB); larger messages get a 413
/// disk = "mail"                # keep every raw message as an .eml on this disk
/// path = "inbound"             # under this folder: inbound/2026/10/27/<sha-256>.eml
/// reply_token_ttl_days = 0     # reply tokens never expire
///
/// [mail.inbound.ses]           # POST /api/mail/inbound/ses?token=...
/// token = "..."
/// topic_arns = ["arn:aws:sns:eu-west-1:123456789012:inbound-mail"]
/// disk = "ses-mail"            # the S3 disk the receipt rule writes to
///
/// [mail.inbound.mailgun]       # POST /api/mail/inbound/mailgun (and /mailgun/mime)
/// signing_key = "..."          # default: mail.webhooks.mailgun.signing_key
///
/// [mail.inbound.postmark]      # POST /api/mail/inbound/postmark
/// token = "..."                # the basic-auth password, or ?token=
///
/// [mail.inbound.raw]           # POST /api/mail/inbound/raw
/// token = "..."                # Authorization: Bearer ..., or ?token=
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct InboundOptions {
    /// The largest message accepted, in bytes (default 25 MiB). Payloads
    /// that carry the message encoded may be up to twice as large.
    pub max_size: usize,
    /// A navree-storage disk that keeps every raw message as an `.eml`
    /// file (default: none). Needs `StorageModule`.
    pub disk: Option<String>,
    /// The folder on the disk (default `inbound`): files are
    /// `{path}/{yyyy}/{mm}/{dd}/{sha-256}.eml`, so a message delivered
    /// twice is stored once.
    pub path: String,
    /// Reply tokens older than this many days are refused (default 0:
    /// they never expire).
    pub reply_token_ttl_days: u32,
    /// Amazon SES receipt rules (SNS, or S3 and SNS).
    pub ses: SesInboundOptions,
    /// Mailgun routes.
    pub mailgun: MailgunInboundOptions,
    /// Postmark inbound streams.
    pub postmark: PostmarkInboundOptions,
    /// Raw MIME posted by your own mail server or script.
    pub raw: RawInboundOptions,
}

impl Default for InboundOptions {
    fn default() -> Self {
        Self {
            max_size: DEFAULT_MAX_SIZE,
            disk: None,
            path: "inbound".to_owned(),
            reply_token_ttl_days: 0,
            ses: SesInboundOptions::default(),
            mailgun: MailgunInboundOptions::default(),
            postmark: PostmarkInboundOptions::default(),
            raw: RawInboundOptions::default(),
        }
    }
}

impl InboundOptions {
    /// Checks the settings; the message names the key.
    ///
    /// # Errors
    ///
    /// A message naming the key that is wrong.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_size == 0 {
            return Err("`inbound.max_size` must be greater than 0".to_owned());
        }
        if self
            .disk
            .as_deref()
            .is_some_and(|disk| disk.trim().is_empty())
        {
            return Err("`inbound.disk` must not be empty".to_owned());
        }
        let path = self.path.trim();
        if path.starts_with('/') || path.split('/').any(|segment| segment == "..") {
            return Err("`inbound.path` must be a relative folder without `..`".to_owned());
        }
        if self
            .ses
            .disk
            .as_deref()
            .is_some_and(|disk| disk.trim().is_empty())
        {
            return Err("`inbound.ses.disk` must not be empty".to_owned());
        }
        for (key, secret) in [
            ("ses.token", &self.ses.token),
            ("mailgun.signing_key", &self.mailgun.signing_key),
            ("postmark.token", &self.postmark.token),
            ("raw.token", &self.raw.token),
        ] {
            if secret
                .as_ref()
                .is_some_and(|secret| secret.expose().trim().is_empty())
            {
                return Err(format!("`inbound.{key}` must not be empty when set"));
            }
        }
        Ok(())
    }

    /// The route body limit: room for a message encoded in base64 inside
    /// JSON or a form.
    pub(crate) fn body_limit(&self) -> usize {
        self.max_size.saturating_mul(2).saturating_add(64 * 1024)
    }
}

/// `mail.inbound.ses`: Amazon SES receipt rules that notify an SNS topic
/// subscribed to `POST /api/mail/inbound/ses?token=...`.
///
/// With an SNS action the message is in the notification (up to 150 KB);
/// with an S3 action (any size) the notification names the object, read
/// from `disk`, an S3 disk on the rule's bucket.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SesInboundOptions {
    /// The secret in the subscription URL's `token` query parameter.
    pub token: Option<Secret<String>>,
    /// The SNS topics accepted (every topic when empty).
    pub topic_arns: Vec<String>,
    /// The navree-storage disk on the S3 action's bucket.
    pub disk: Option<String>,
    /// Confirm SNS subscriptions by fetching their `SubscribeURL` (default
    /// `true`; only `https://sns.<region>.amazonaws.com` URLs).
    pub confirm_subscriptions: bool,
}

impl Default for SesInboundOptions {
    fn default() -> Self {
        Self {
            token: None,
            topic_arns: Vec::new(),
            disk: None,
            confirm_subscriptions: true,
        }
    }
}

/// `mail.inbound.mailgun`: a Mailgun route that forwards to
/// `POST /api/mail/inbound/mailgun/mime` (the whole message) or
/// `/api/mail/inbound/mailgun` (the parsed fields).
#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct MailgunInboundOptions {
    /// The HTTP webhook signing key (default:
    /// `mail.webhooks.mailgun.signing_key`).
    pub signing_key: Option<Secret<String>>,
}

/// `mail.inbound.postmark`: a Postmark inbound stream whose webhook URL is
/// `https://postmark:{token}@app.example.com/api/mail/inbound/postmark`.
#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct PostmarkInboundOptions {
    /// The secret: the basic-auth password of the URL, or its `token`
    /// query parameter.
    pub token: Option<Secret<String>>,
}

/// `mail.inbound.raw`: raw MIME posted to `POST /api/mail/inbound/raw`
/// with `Authorization: Bearer {token}`, by a mail server's pipe, a
/// worker or a script.
#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct RawInboundOptions {
    /// The bearer token (or `token` query parameter).
    pub token: Option<Secret<String>>,
}
