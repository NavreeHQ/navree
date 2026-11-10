//! [`InboundMail`]: a received message read by Navree's MIME reader, and
//! [`Incoming`], what a source hands to the pipeline.

use std::fmt;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::compose::Compose;
use super::error::InboundError;
use super::mime::{self, Limits, Mailbox};

/// A mailbox address of a received message: `Anu Lakhotia <anu@example.com>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    /// The display name, decoded (`Anu Lakhotia`), when the header has one.
    pub name: Option<String>,
    /// The address (`anu@example.com`), as written.
    pub email: String,
}

impl Address {
    /// An address without a display name.
    pub fn new(email: impl Into<String>) -> Self {
        Self {
            name: None,
            email: email.into(),
        }
    }

    /// The address in lower case, as mailbox patterns compare it.
    pub fn normalized(&self) -> String {
        self.email.trim().to_ascii_lowercase()
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name.as_deref().filter(|name| !name.is_empty()) {
            Some(name) => write!(f, "{name} <{}>", self.email),
            None => f.write_str(&self.email),
        }
    }
}

/// A part of a received message that is not its text or HTML body: a file,
/// an inline image, or a forwarded message (`message/rfc822`).
#[derive(Clone, PartialEq, Eq)]
pub struct Attachment {
    /// The file name, from `Content-Disposition` or `Content-Type`.
    pub filename: Option<String>,
    /// The media type in lower case (`application/pdf`, `message/rfc822`).
    pub content_type: String,
    /// The `Content-ID`, without angle brackets (an inline image's `cid:`).
    pub content_id: Option<String>,
    /// Whether the part is shown inline (`Content-Disposition: inline`).
    pub inline: bool,
    /// The decoded content. A forwarded message is its raw MIME.
    pub content: Bytes,
}

impl Attachment {
    /// The size of the decoded content, in bytes.
    pub fn size(&self) -> usize {
        self.content.len()
    }

    /// Whether the part is a whole message (`message/rfc822`): a message a
    /// user forwarded as an attachment.
    pub fn is_message(&self) -> bool {
        self.content_type == "message/rfc822" || self.content_type == "message/global"
    }

    /// The forwarded message, parsed.
    ///
    /// # Errors
    ///
    /// [`InboundError::Unparseable`] when the part is not a message.
    pub fn as_mail(&self) -> Result<InboundMail, InboundError> {
        if !self.is_message() {
            return Err(InboundError::Unparseable(format!(
                "the attachment is {}, not a message",
                self.content_type
            )));
        }
        InboundMail::parse(self.content.clone())
    }
}

impl fmt::Debug for Attachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Attachment")
            .field("filename", &self.filename)
            .field("content_type", &self.content_type)
            .field("content_id", &self.content_id)
            .field("inline", &self.inline)
            .field("size", &self.content.len())
            .finish()
    }
}

/// Runs `parse` on Tokio's blocking pool: a message can make the parser
/// work for a long time, and an async worker thread is not for that.
pub(crate) async fn blocking<T, F>(parse: F) -> Result<T, InboundError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, InboundError> + Send + 'static,
{
    match tokio::task::spawn_blocking(parse).await {
        Ok(result) => result,
        // A parser that panics on this message: the message is the problem.
        Err(error) if error.is_panic() => {
            tracing::warn!(target: "navree::mail", %error, "parsing an inbound message panicked");
            Err(InboundError::Unparseable(
                "the parser failed on the message".to_owned(),
            ))
        }
        Err(_) => Err(InboundError::Config("the app is shutting down".to_owned())),
    }
}

/// The SMTP envelope as the provider reports it: who the mail server
/// accepted the message from and for. Recipients here include `Bcc` and
/// forwarding addresses that the headers never show.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    /// The envelope sender (`MAIL FROM`), when reported.
    pub sender: Option<String>,
    /// The envelope recipients (`RCPT TO`) the message was accepted for.
    pub recipients: Vec<String>,
}

/// A verdict of the receiving provider's checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Verdict {
    /// The check passed (not spam, no virus, SPF, DKIM or DMARC aligned).
    Pass,
    /// The check failed (spam, a virus, a failed SPF, DKIM or DMARC check).
    Fail,
    /// The check could not decide.
    Gray,
    /// The provider could not run the check.
    ProcessingFailed,
    /// The check is turned off for the domain.
    Disabled,
}

impl Verdict {
    /// Reads a provider's status (`PASS`, `FAIL`, `GRAY`,
    /// `PROCESSING_FAILED`, `DISABLED`).
    pub fn parse(status: &str) -> Option<Self> {
        match status
            .trim()
            .to_ascii_lowercase()
            .replace('-', "_")
            .as_str()
        {
            "pass" | "passed" => Some(Verdict::Pass),
            "fail" | "failed" | "hardfail" | "softfail" => Some(Verdict::Fail),
            "gray" | "grey" | "neutral" | "none" => Some(Verdict::Gray),
            "processing_failed" | "temperror" | "permerror" => Some(Verdict::ProcessingFailed),
            "disabled" => Some(Verdict::Disabled),
            _ => None,
        }
    }
}

/// What the receiving provider concluded about the message, when it says
/// (Amazon SES reports all five; the other sources report none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdicts {
    /// Spam: [`Verdict::Fail`] means the provider thinks it is spam.
    pub spam: Option<Verdict>,
    /// Viruses: [`Verdict::Fail`] means a virus was found.
    pub virus: Option<Verdict>,
    /// SPF of the envelope sender.
    pub spf: Option<Verdict>,
    /// DKIM signatures.
    pub dkim: Option<Verdict>,
    /// DMARC alignment.
    pub dmarc: Option<Verdict>,
}

impl Verdicts {
    /// Whether the provider flagged the message as spam or found a virus.
    pub fn is_flagged(&self) -> bool {
        self.spam == Some(Verdict::Fail) || self.virus == Some(Verdict::Fail)
    }
}

/// Where the raw message (`.eml`) is kept on a navree-storage disk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredMail {
    /// The disk name.
    pub disk: String,
    /// The path on the disk.
    pub path: String,
}

/// One header of a received message: its name as written and its value
/// unfolded (continuation lines joined), not decoded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    /// The name as written (`X-Mailer`).
    pub name: String,
    /// The value, unfolded and trimmed.
    pub value: String,
}

/// What a source hands to [`Inbound::receive`](super::Inbound::receive):
/// the raw MIME message and what the provider said about it.
///
/// Sources build one from their payloads ([`Incoming::from_postmark`],
/// the Mailgun route, the SES notification); tests build one with
/// [`Incoming::compose`].
#[derive(Clone, PartialEq, Eq)]
pub struct Incoming {
    /// The raw message (RFC 5322 with MIME).
    pub raw: Bytes,
    /// The source: `ses`, `mailgun`, `postmark`, `raw`, `gmail`, `graph`,
    /// `test`, or your own.
    pub source: String,
    /// The provider's id for the message (SES `messageId`, Postmark
    /// `MessageID`, the Gmail or Graph message id).
    pub provider_id: Option<String>,
    /// The SMTP envelope, when the provider reports it.
    pub envelope: Envelope,
    /// The provider's verdicts.
    pub verdicts: Verdicts,
    /// Where the raw message already is (SES's S3 action): the pipeline
    /// does not store it again.
    pub stored: Option<StoredMail>,
}

impl Incoming {
    /// A raw message from `source`.
    pub fn new(source: impl Into<String>, raw: impl Into<Bytes>) -> Self {
        Self {
            raw: raw.into(),
            source: source.into(),
            provider_id: None,
            envelope: Envelope::default(),
            verdicts: Verdicts::default(),
            stored: None,
        }
    }

    /// A message built field by field (for tests and for providers that
    /// post the message parsed): see [`Compose`].
    pub fn compose() -> Compose {
        Compose::new()
    }

    /// Adds an envelope recipient.
    #[must_use]
    pub fn recipient(mut self, address: impl Into<String>) -> Self {
        self.envelope.recipients.push(address.into());
        self
    }

    /// Sets the envelope sender.
    #[must_use]
    pub fn sender(mut self, address: impl Into<String>) -> Self {
        self.envelope.sender = Some(address.into());
        self
    }

    /// Sets the provider's message id.
    #[must_use]
    pub fn provider_id(mut self, id: impl Into<String>) -> Self {
        self.provider_id = Some(id.into());
        self
    }

    /// Sets the provider's verdicts.
    #[must_use]
    pub fn verdicts(mut self, verdicts: Verdicts) -> Self {
        self.verdicts = verdicts;
        self
    }

    /// Says where the raw message is already stored.
    #[must_use]
    pub fn stored(mut self, stored: StoredMail) -> Self {
        self.stored = Some(stored);
        self
    }

    /// Parses the message, received now.
    ///
    /// # Errors
    ///
    /// [`InboundError::Unparseable`] when the bytes are not a message, or
    /// the message is over the default reading limits (see
    /// [`InboundMail::parse`]).
    pub fn parse(self) -> Result<InboundMail, InboundError> {
        self.parse_at(Utc::now(), Limits::default())
    }

    /// Parses the message within `limits`, received at `received_at`.
    pub(crate) fn parse_at(
        self,
        received_at: DateTime<Utc>,
        limits: Limits,
    ) -> Result<InboundMail, InboundError> {
        let mut mail = InboundMail::parse_with(self.raw, limits)?;
        mail.source = self.source;
        mail.provider_id = self.provider_id.filter(|id| !id.is_empty());
        mail.envelope = self.envelope;
        mail.verdicts = self.verdicts;
        mail.stored = self.stored;
        mail.received_at = received_at;
        Ok(mail)
    }
}

impl fmt::Debug for Incoming {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Incoming")
            .field("source", &self.source)
            .field("provider_id", &self.provider_id)
            .field("envelope", &self.envelope)
            .field("verdicts", &self.verdicts)
            .field("stored", &self.stored)
            .field("size", &self.raw.len())
            .finish()
    }
}

/// A received message, parsed: headers, addresses, the text and HTML
/// bodies and the attachments, every charset decoded to UTF-8. The raw
/// bytes stay available ([`raw`](InboundMail::raw)) and are stored as an
/// `.eml` file when `mail.inbound.disk` is set.
///
/// Mailboxes ([`ModuleBuilderMailExt::mailbox`](super::ModuleBuilderMailExt::mailbox))
/// receive one per message.
#[derive(Clone)]
pub struct InboundMail {
    /// This receipt's id (a UUID v7).
    pub id: Uuid,
    /// The source that received it (`ses`, `mailgun`, `postmark`, `raw`,
    /// `gmail`, `graph`, `test`).
    pub source: String,
    /// The provider's id for the message.
    pub provider_id: Option<String>,
    /// The SMTP envelope, when the provider reports it.
    pub envelope: Envelope,
    /// The provider's spam, virus, SPF, DKIM and DMARC verdicts.
    pub verdicts: Verdicts,
    /// `Message-ID`, without angle brackets.
    pub message_id: Option<String>,
    /// `In-Reply-To`: the message this one answers, without angle brackets.
    pub in_reply_to: Option<String>,
    /// `References`, oldest first, without angle brackets.
    pub references: Vec<String>,
    /// `From`: what the sender claims, which only [`verdicts`](Self::verdicts)
    /// vouch for. `None` when another reader could take a different `From`
    /// from the head: two such fields, more than one address, an address that
    /// is not only an address, or lines that end in different ways.
    pub from: Option<Address>,
    /// `Reply-To`.
    pub reply_to: Vec<Address>,
    /// `To`.
    pub to: Vec<Address>,
    /// `Cc`.
    pub cc: Vec<Address>,
    /// `Subject`, decoded, on one line: a control character is a space.
    pub subject: Option<String>,
    /// `Date`, in UTC.
    pub date: Option<DateTime<Utc>>,
    /// The plain-text body (from the HTML when the message has no text
    /// part).
    pub text: Option<String>,
    /// The HTML body, when the message has one.
    pub html: Option<String>,
    /// Every header, in order.
    pub headers: Vec<Header>,
    /// The attachments, inline parts and forwarded messages, in order.
    pub attachments: Vec<Attachment>,
    /// The size of the raw message, in bytes.
    pub size: usize,
    /// The SHA-256 of the raw message, in hex: the same bytes always give
    /// the same fingerprint, so a provider's retry can be recognized.
    pub fingerprint: String,
    /// Where the raw message is stored, once it is.
    pub stored: Option<StoredMail>,
    /// When it was received.
    pub received_at: DateTime<Utc>,
    raw: Bytes,
}

impl InboundMail {
    /// Parses a raw message (RFC 5322 with MIME) received now, from source
    /// `raw`.
    ///
    /// # Errors
    ///
    /// [`InboundError::Unparseable`] when the bytes hold no message
    /// headers, or the message is over the default reading limits
    /// (`mail.inbound.max_parts`, `max_depth`, `max_headers` and
    /// `max_addresses`; [`Inbound::receive`](super::Inbound::receive)
    /// applies the configured ones).
    pub fn parse(raw: impl Into<Bytes>) -> Result<Self, InboundError> {
        Self::parse_with(raw.into(), Limits::default())
    }

    /// Parses a raw message within `limits` (`mail.inbound.max_depth`,
    /// `max_parts`, `max_headers` and `max_addresses`).
    pub(crate) fn parse_with(raw: Bytes, limits: Limits) -> Result<Self, InboundError> {
        let unparseable = |error: mime::MimeError| InboundError::Unparseable(error.to_string());
        let parsed = mime::parse(&raw, limits).map_err(unparseable)?;
        // A field the message repeats counts once: its last occurrence.
        let last = |name: &str| {
            parsed
                .headers
                .iter()
                .rev()
                .find(|(field, _)| field.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_ref())
        };
        // Ids are ASCII: read them from the value as text, unfolded.
        let text = |name: &str| last(name).map(|value| without_controls(&mime::unfold(value)));
        let addresses = |name: &str| -> Result<Vec<Address>, InboundError> {
            let Some(value) = last(name) else {
                return Ok(Vec::new());
            };
            let list = mime::addresses(value, limits.addresses).map_err(unparseable)?;
            Ok(list.into_iter().map(address).collect())
        };
        let references = match text("References") {
            Some(value) => {
                let references: Vec<String> = ids(&value).take(limits.addresses + 1).collect();
                if references.len() > limits.addresses {
                    return Err(unparseable(mime::MimeError::TooManyAddresses(
                        limits.addresses,
                    )));
                }
                references
            }
            None => Vec::new(),
        };

        let mail = InboundMail {
            id: Uuid::now_v7(),
            source: "raw".to_owned(),
            provider_id: None,
            envelope: Envelope::default(),
            verdicts: Verdicts::default(),
            message_id: text("Message-ID")
                .and_then(|value| {
                    mime::ids(&value)
                        .last()
                        .map(|id| strip_angles(id).to_owned())
                })
                .filter(|id| !id.is_empty()),
            in_reply_to: text("In-Reply-To").and_then(|value| ids(&value).next()),
            references,
            from: addresses("From")?
                .into_iter()
                .next()
                .filter(|_| from_is_clear(&raw, &parsed.headers)),
            reply_to: addresses("Reply-To")?,
            to: addresses("To")?,
            cc: addresses("Cc")?,
            subject: last("Subject")
                .and_then(mime::text)
                .map(|subject| on_one_line(&subject)),
            date: last("Date").and_then(mime::date),
            text: parsed.text,
            html: parsed.html,
            headers: parsed
                .headers
                .into_iter()
                .map(|(name, value)| Header {
                    name,
                    value: unfold(&String::from_utf8_lossy(&value)),
                })
                .collect(),
            attachments: parsed
                .attachments
                .into_iter()
                .map(|part| Attachment {
                    filename: part.filename.map(|name| without_controls(&name)),
                    content_type: without_controls(&part.content_type),
                    content_id: part
                        .content_id
                        .map(|id| without_controls(strip_angles(&id))),
                    inline: part.inline,
                    content: part.content,
                })
                .collect(),
            size: raw.len(),
            fingerprint: hex::encode(Sha256::digest(&raw)),
            stored: None,
            received_at: Utc::now(),
            raw: Bytes::new(),
        };
        Ok(InboundMail { raw, ..mail })
    }

    /// The raw message, as received.
    pub fn raw(&self) -> &Bytes {
        &self.raw
    }

    /// The first header named `name` (any case), unfolded.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .map(|header| header.value.as_str())
    }

    /// Every header named `name` (any case), in order.
    pub fn header_all(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|header| header.name.eq_ignore_ascii_case(name))
            .map(|header| header.value.as_str())
            .collect()
    }

    /// The addresses the message was delivered for, in lower case and
    /// without duplicates, in the order mailboxes try them: the envelope
    /// recipients when the source reports any, and only those, because every
    /// header is what the sender wrote. Without an envelope, `Delivered-To`,
    /// `X-Original-To`, `To` and `Cc`.
    pub fn recipients(&self) -> Vec<String> {
        let mut all: Vec<String> = Vec::new();
        let mut push = |address: &str| {
            let address = address.trim().trim_matches(['<', '>']).to_ascii_lowercase();
            if !address.is_empty() && !all.contains(&address) {
                all.push(address);
            }
        };
        for recipient in &self.envelope.recipients {
            push(recipient);
        }
        if !self.envelope.recipients.is_empty() {
            return all;
        }
        for name in ["Delivered-To", "X-Original-To"] {
            for value in self.header_all(name) {
                push(value);
            }
        }
        for address in self.to.iter().chain(&self.cc) {
            push(&address.email);
        }
        all
    }

    /// Whether the message was sent by a machine in answer to another one:
    /// an out-of-office reply, a delivery report, or bulk mail
    /// (`Auto-Submitted` other than `no`, `X-Autoreply`,
    /// `X-Autorespond`, `Precedence: auto_reply`, `bulk`, `junk` or `list`,
    /// a `multipart/report`). Answering such a message risks a mail loop.
    pub fn is_auto_reply(&self) -> bool {
        if self
            .header("Auto-Submitted")
            .is_some_and(|value| !value.trim().eq_ignore_ascii_case("no"))
        {
            return true;
        }
        if self.header("X-Autoreply").is_some()
            || self.header("X-Autorespond").is_some()
            || self
                .header("X-Auto-Response-Suppress")
                .is_some_and(|value| {
                    value.split(',').any(|item| {
                        matches!(item.trim().to_ascii_lowercase().as_str(), "all" | "oof")
                    })
                })
        {
            return true;
        }
        if self.header("Precedence").is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "auto_reply" | "bulk" | "junk" | "list"
            )
        }) {
            return true;
        }
        self.header("Content-Type").is_some_and(|value| {
            value
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("multipart/report")
        })
    }

    /// The new text of a reply: the plain-text body up to the quoted
    /// message (`On ... wrote:`, `-----Original Message-----`, a line of
    /// `>` quotes or a `From:` block), trimmed. The whole text when nothing
    /// is quoted.
    pub fn reply_text(&self) -> Option<String> {
        let text = self.text.as_deref()?;
        let mut kept: Vec<&str> = Vec::new();
        let lines: Vec<&str> = text.lines().collect();
        let mut index = 0;
        while index < lines.len() {
            let line = lines[index];
            let trimmed = line.trim();
            if trimmed.starts_with('>')
                || is_original_marker(trimmed)
                || is_wrote_line(trimmed, lines.get(index + 1).copied())
            {
                break;
            }
            kept.push(line);
            index += 1;
        }
        let reply = kept.join("\n").trim().to_owned();
        Some(reply)
    }
}

impl fmt::Debug for InboundMail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InboundMail")
            .field("id", &self.id)
            .field("source", &self.source)
            .field("provider_id", &self.provider_id)
            .field("message_id", &self.message_id)
            .field("from", &self.from.as_ref().map(|a| a.email.as_str()))
            .field("recipients", &self.recipients())
            .field("subject", &self.subject)
            .field("attachments", &self.attachments.len())
            .field("size", &self.size)
            .field("stored", &self.stored)
            .finish_non_exhaustive()
    }
}

/// `-----Original Message-----` and the like.
fn is_original_marker(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    (lower.starts_with("-----") && lower.contains("original message"))
        || lower.starts_with("________________________________")
        || (lower.starts_with("from:") && lower.contains('@') && !lower.starts_with("from: http"))
}

/// `On Mon, 26 Oct 2026 at 18:02, Anu <anu@example.com> wrote:`, also
/// wrapped onto the next line.
fn is_wrote_line(line: &str, next: Option<&str>) -> bool {
    let lower = line.to_ascii_lowercase();
    if !lower.starts_with("on ") {
        return false;
    }
    if lower.ends_with("wrote:") {
        return true;
    }
    next.is_some_and(|next| next.trim().to_ascii_lowercase().ends_with("wrote:"))
}

/// `<id@host>` -> `id@host`.
fn strip_angles(id: &str) -> &str {
    id.trim().trim_start_matches('<').trim_end_matches('>')
}

/// Message ids from an `In-Reply-To` or `References` value, in order. A
/// single id (or text without angle brackets) is split at blanks.
fn ids(value: &str) -> impl Iterator<Item = String> + '_ {
    let mut found = mime::ids(value);
    let first = found.next();
    let second = found.next();
    let ids: Box<dyn Iterator<Item = &str>> = match (first, second) {
        (Some(one), None) => Box::new(one.split_whitespace()),
        (first, second) => Box::new(first.into_iter().chain(second).chain(found)),
    };
    ids.map(|id| strip_angles(id).to_owned())
        .filter(|id| !id.is_empty())
}

fn address(mailbox: Mailbox) -> Address {
    Address {
        name: mailbox.name.map(|name| on_one_line(&name)),
        email: without_controls(&mailbox.email),
    }
}

/// `text` with each control character a space: a decoded subject or name goes
/// into logs and headers, where a line break would start a new line.
fn on_one_line(text: &str) -> String {
    text.chars()
        .map(|c| if is_control(c) { ' ' } else { c })
        .collect()
}

/// `text` without control characters: a name, an address or a file name, where
/// a NUL or a line break is never meant and a space would be a new character.
fn without_controls(text: &str) -> String {
    text.chars().filter(|c| !is_control(*c)).collect()
}

/// C0 and C1 controls, and the line and paragraph separators.
fn is_control(c: char) -> bool {
    c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')
}

/// Whether the head has a single `From`, an address or a name and an address
/// in angle brackets and nothing else, and line ends no other reader could
/// split differently. A provider's verdict
/// vouches for the `From` it read: when it could have read another, this one
/// must not be taken for it.
fn from_is_clear(raw: &[u8], headers: &[(String, Bytes)]) -> bool {
    let mut fields = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("From"))
        .map(|(_, value)| value.as_ref());
    let (Some(value), None) = (fields.next(), fields.next()) else {
        return false;
    };
    mime::is_clear_mailbox(value) && mime::head_lines_agree(raw)
}

/// Joins continuation lines and trims.
pub(crate) fn unfold(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut pending_space = false;
    for line in value.split('\n') {
        let line = line.trim_end_matches('\r');
        let piece = if out.is_empty() {
            line.trim()
        } else {
            line.trim_start()
        };
        if piece.is_empty() {
            continue;
        }
        if pending_space {
            out.push(' ');
        }
        out.push_str(piece.trim_end());
        pending_space = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfold_joins_continuation_lines() {
        assert_eq!(unfold(" a\r\n\tb\r\n  c \r\n"), "a b c");
        assert_eq!(unfold("plain"), "plain");
    }

    #[test]
    fn verdicts_read_provider_statuses() {
        assert_eq!(Verdict::parse("PASS"), Some(Verdict::Pass));
        assert_eq!(
            Verdict::parse("PROCESSING_FAILED"),
            Some(Verdict::ProcessingFailed)
        );
        assert_eq!(Verdict::parse("softfail"), Some(Verdict::Fail));
        assert_eq!(Verdict::parse("maybe"), None);
        assert!(
            Verdicts {
                virus: Some(Verdict::Fail),
                ..Verdicts::default()
            }
            .is_flagged()
        );
        assert!(!Verdicts::default().is_flagged());
    }

    #[test]
    fn addresses_display_with_their_name() {
        let ada = Address {
            name: Some("Anu".into()),
            email: "Anu@Example.com".into(),
        };
        assert_eq!(ada.to_string(), "Anu <Anu@Example.com>");
        assert_eq!(ada.normalized(), "anu@example.com");
        assert_eq!(Address::new("x@y.test").to_string(), "x@y.test");
    }

    #[tokio::test]
    async fn a_parser_that_panics_leaves_an_unparseable_message() {
        let result = blocking(|| -> Result<(), InboundError> { panic!("the parser failed") }).await;
        assert!(matches!(result, Err(InboundError::Unparseable(_))));
        let parsed = blocking(|| Ok(7)).await;
        assert_eq!(parsed.unwrap(), 7);
    }
}
