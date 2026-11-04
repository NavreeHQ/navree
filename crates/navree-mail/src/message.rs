//! [`Message`]: a rendered email.

use std::collections::BTreeMap;

use std::path::Path;

use lettre::message::Mailbox;
use serde::{Deserialize, Serialize};

use crate::{Attachment, MailError};

/// A rendered email, ready for a [`MailTransport`](crate::MailTransport).
///
/// Addresses are `"ada@example.com"` or `"Ada Lovelace <ada@example.com>"`.
/// Build one with the chainable setters, which share the field names:
///
/// ```
/// use navree_mail::Message;
///
/// let message = Message::new()
///     .to("Ada <ada@example.com>")
///     .cc("team@example.com")
///     .subject("Your invoice")
///     .html("<p>Thanks!</p>")
///     .text("Thanks!")
///     .header("X-Campaign", "spring")
///     .metadata("invoice", "42")
///     .attach("invoice-42.pdf", "application/pdf", b"%PDF-1.7 ...".to_vec())
///     .locale("en");
/// assert_eq!(message.to, ["Ada <ada@example.com>"]);
/// assert_eq!(message.from, None); // the Mailer fills in its default sender
/// ```
///
/// A message is serializable, so it can travel inside a queued job.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Message {
    /// The sender; `None` lets the [`Mailer`](crate::Mailer) use its default
    /// (`mail.from`).
    pub from: Option<String>,
    /// Primary recipients.
    pub to: Vec<String>,
    /// Carbon-copy recipients.
    pub cc: Vec<String>,
    /// Blind carbon-copy recipients (never shown to the other recipients).
    pub bcc: Vec<String>,
    /// Where replies go, if not to the sender.
    pub reply_to: Option<String>,
    /// The subject line.
    pub subject: String,
    /// The HTML body.
    pub html: Option<String>,
    /// The plain-text body.
    pub text: Option<String>,
    /// Extra headers (`List-Unsubscribe`, `X-Campaign`, ...). The headers a
    /// transport writes itself (`From`, `To`, `Subject`, `Content-Type`,
    /// ...) are refused.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// Key-value pairs for the provider: SES message tags, Mailgun
    /// variables, Postmark metadata, Resend tags. Not sent over SMTP.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    /// The recipient's locale (`en`, `ar-EG`): the HTML body gets `lang`
    /// and, for right-to-left scripts, `dir="rtl"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// Set by [`hold`](Message::hold): the message waits in the outbox for
    /// an admin to release or discard it, with this reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hold: Option<String>,
    /// Files sent with the message ([`attach`](Message::attach)). Every
    /// transport sends them; each refuses a message larger than its
    /// provider takes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    /// Makes a repeated send deliver once where the provider honours keys
    /// (Resend). [`Mailer::queue`](crate::Mailer::queue) gives a queued
    /// message one, so a retried `mail.send` job sends the same key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// Headers a message may not set itself: the transports write them.
const RESERVED_HEADERS: &[&str] = &[
    "bcc",
    "cc",
    "content-transfer-encoding",
    "content-type",
    "dkim-signature",
    "from",
    "mime-version",
    "reply-to",
    "sender",
    "subject",
    "to",
];

impl Message {
    /// An empty message.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the sender.
    pub fn from(mut self, address: impl Into<String>) -> Self {
        self.from = Some(address.into());
        self
    }

    /// Adds a primary recipient.
    pub fn to(mut self, address: impl Into<String>) -> Self {
        self.to.push(address.into());
        self
    }

    /// Adds a carbon-copy recipient.
    pub fn cc(mut self, address: impl Into<String>) -> Self {
        self.cc.push(address.into());
        self
    }

    /// Adds a blind carbon-copy recipient.
    pub fn bcc(mut self, address: impl Into<String>) -> Self {
        self.bcc.push(address.into());
        self
    }

    /// Sets the reply-to address.
    pub fn reply_to(mut self, address: impl Into<String>) -> Self {
        self.reply_to = Some(address.into());
        self
    }

    /// Sets the subject line.
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = subject.into();
        self
    }

    /// Sets the HTML body.
    pub fn html(mut self, body: impl Into<String>) -> Self {
        self.html = Some(body.into());
        self
    }

    /// Sets the plain-text body.
    pub fn text(mut self, body: impl Into<String>) -> Self {
        self.text = Some(body.into());
        self
    }

    /// Adds (or replaces) a header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Adds (or replaces) a provider metadata pair.
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Sets the recipient's locale.
    pub fn locale(mut self, locale: impl Into<String>) -> Self {
        self.locale = Some(locale.into());
        self
    }

    /// Holds the message for approval: instead of sending it, the
    /// [`Mailer`](crate::Mailer) stores it in the outbox, where an admin
    /// releases or discards it (needs `mail.delivery.enabled`).
    pub fn hold(mut self, reason: impl Into<String>) -> Self {
        self.hold = Some(reason.into());
        self
    }

    /// Attaches a file named `filename` of MIME type `content_type`
    /// (`application/pdf`, `text/calendar; method=REQUEST`).
    pub fn attach(
        mut self,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        content: impl Into<Vec<u8>>,
    ) -> Self {
        self.attachments
            .push(Attachment::new(filename, content_type, content));
        self
    }

    /// Attaches `attachment`.
    pub fn attachment(mut self, attachment: Attachment) -> Self {
        self.attachments.push(attachment);
        self
    }

    /// Attaches the file at `path`, named after it, its type guessed from
    /// its extension. [`MailError::Attachment`] when it cannot be read.
    pub fn attach_from_disk(mut self, path: impl AsRef<Path>) -> Result<Self, MailError> {
        self.attachments.push(Attachment::from_path(path)?);
        Ok(self)
    }

    /// Sets the idempotency key the transports that honour one send.
    pub fn idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    /// Every recipient: `to`, then `cc`, then `bcc`.
    pub fn recipients(&self) -> impl Iterator<Item = &str> {
        self.to
            .iter()
            .chain(&self.cc)
            .chain(&self.bcc)
            .map(String::as_str)
    }

    /// The domain of the sender's address, in lower case (`acme.test` for
    /// `Acme <hello@acme.test>`), if the sender parses.
    pub fn sender_domain(&self) -> Option<String> {
        let mailbox = self.from.as_deref()?.parse::<Mailbox>().ok()?;
        Some(mailbox.email.domain().to_ascii_lowercase())
    }

    /// Checks that the message can be delivered: it has a sender and at
    /// least one recipient, every address parses, the subject and header
    /// values hold no line break, header names are valid and none is a
    /// header the transports write, and every attachment passes
    /// [`Attachment::check`]. The [`Mailer`](crate::Mailer) runs
    /// this (after filling in the default sender) before handing the
    /// message to a transport.
    pub fn validate(&self) -> Result<(), MailError> {
        match &self.from {
            Some(from) => drop(mailbox("from", from)?),
            None => return Err(MailError::MissingSender),
        }
        if self.to.is_empty() && self.cc.is_empty() && self.bcc.is_empty() {
            return Err(MailError::NoRecipients);
        }
        for (field, addresses) in [("to", &self.to), ("cc", &self.cc), ("bcc", &self.bcc)] {
            for address in addresses {
                mailbox(field, address)?;
            }
        }
        if let Some(reply_to) = &self.reply_to {
            mailbox("reply_to", reply_to)?;
        }
        if self.subject.contains(['\r', '\n']) {
            return Err(MailError::InvalidHeader { field: "subject" });
        }
        for (name, value) in &self.headers {
            check_header(name, value)?;
        }
        for attachment in &self.attachments {
            attachment.check()?;
        }
        if let Some(key) = &self.idempotency_key
            && (key.is_empty() || key.len() > 255 || !key.bytes().all(|b| b.is_ascii_graphic()))
        {
            return Err(MailError::InvalidHeader {
                field: "idempotency_key",
            });
        }
        Ok(())
    }
}

/// Checks one extra header.
fn check_header(name: &str, value: &str) -> Result<(), MailError> {
    let invalid = |reason: &'static str| MailError::Header {
        name: name.to_owned(),
        reason,
    };
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_graphic() && b != b':') {
        return Err(invalid(
            "a header name is printable ASCII without `:` or spaces",
        ));
    }
    if RESERVED_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
        return Err(invalid("the transport writes this header"));
    }
    if value.contains(['\r', '\n']) {
        return Err(invalid("a header value must not contain line breaks"));
    }
    Ok(())
}

/// Parses `address`, the value of field `field`, as a mailbox.
pub(crate) fn mailbox(field: &'static str, address: &str) -> Result<Mailbox, MailError> {
    address
        .parse::<Mailbox>()
        .map_err(|error| MailError::InvalidAddress {
            field,
            address: address.to_owned(),
            reason: error.to_string(),
        })
}

/// The bare address of `address` (`ada@example.com` for `Ada
/// <ada@example.com>`), lower-cased; `address` itself, trimmed and
/// lower-cased, when it does not parse.
pub(crate) fn bare_address(address: &str) -> String {
    match address.parse::<Mailbox>() {
        Ok(mailbox) => mailbox.email.to_string().to_ascii_lowercase(),
        Err(_) => address.trim().to_ascii_lowercase(),
    }
}
