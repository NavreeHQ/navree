//! [`Compose`]: a received message built field by field, written as MIME.
//!
//! Tests build inbound mail with it, and the sources that post a message
//! already parsed (Postmark without its raw content, Mailgun routes without
//! `body-mime`) rebuild the raw message with it, so every message reaches
//! the pipeline the same way.

use std::fmt::Write as _;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::mail::{Envelope, Incoming, StoredMail, Verdicts};

/// Headers [`Compose`] writes itself: copies from a provider's header list
/// are left out.
const STRUCTURAL: &[&str] = &[
    "content-type",
    "content-transfer-encoding",
    "mime-version",
    "content-disposition",
    "content-id",
];

/// The fields [`Compose`] writes from its own setters.
const STANDARD_FIELDS: &[&str] = &[
    "date",
    "message-id",
    "from",
    "to",
    "cc",
    "reply-to",
    "in-reply-to",
    "references",
    "subject",
];

/// A received message built field by field: [`Incoming::compose`].
///
/// ```
/// use navree_mail::Incoming;
///
/// let incoming = Incoming::compose()
///     .from("Ada Lovelace <ada@example.com>")
///     .to("reply+k5q2@in.acme.test")
///     .subject("Re: [#42] Printer on fire")
///     .text("Thanks, the fire is out now.")
///     .attach("photo.jpg", "image/jpeg", vec![0xff, 0xd8, 0xff])
///     .build();
///
/// let mail = incoming.parse().unwrap();
/// assert_eq!(mail.subject.as_deref(), Some("Re: [#42] Printer on fire"));
/// assert_eq!(mail.attachments[0].filename.as_deref(), Some("photo.jpg"));
/// ```
#[derive(Clone, Debug, Default)]
pub struct Compose {
    source: Option<String>,
    from: Option<String>,
    to: Vec<String>,
    cc: Vec<String>,
    reply_to: Vec<String>,
    envelope: Vec<String>,
    subject: Option<String>,
    text: Option<String>,
    html: Option<String>,
    message_id: Option<String>,
    in_reply_to: Option<String>,
    references: Vec<String>,
    date: Option<DateTime<Utc>>,
    headers: Vec<(String, String)>,
    raw_headers: Vec<(String, String)>,
    parts: Vec<Part>,
    provider_id: Option<String>,
    verdicts: Verdicts,
    stored: Option<StoredMail>,
}

#[derive(Clone, Debug)]
struct Part {
    filename: Option<String>,
    content_type: String,
    content_id: Option<String>,
    inline: bool,
    content: Bytes,
}

impl Compose {
    /// An empty message.
    pub fn new() -> Self {
        Self::default()
    }

    /// The source the built [`Incoming`] names (default `test`).
    #[must_use]
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// `From` (`ada@example.com` or `Ada <ada@example.com>`).
    #[must_use]
    pub fn from(mut self, address: impl Into<String>) -> Self {
        self.from = Some(address.into());
        self
    }

    /// Adds a `To` address.
    #[must_use]
    pub fn to(mut self, address: impl Into<String>) -> Self {
        self.to.push(address.into());
        self
    }

    /// Adds a `Cc` address.
    #[must_use]
    pub fn cc(mut self, address: impl Into<String>) -> Self {
        self.cc.push(address.into());
        self
    }

    /// Adds a `Reply-To` address.
    #[must_use]
    pub fn reply_to(mut self, address: impl Into<String>) -> Self {
        self.reply_to.push(address.into());
        self
    }

    /// Adds an envelope recipient that the headers don't show (a `Bcc`).
    /// Without one, the envelope recipients are the `To` and `Cc`
    /// addresses.
    #[must_use]
    pub fn envelope_to(mut self, address: impl Into<String>) -> Self {
        self.envelope.push(address.into());
        self
    }

    /// `Subject`.
    #[must_use]
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// The plain-text body.
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// The HTML body.
    #[must_use]
    pub fn html(mut self, html: impl Into<String>) -> Self {
        self.html = Some(html.into());
        self
    }

    /// `Message-ID` (angle brackets optional; default: a generated one).
    #[must_use]
    pub fn message_id(mut self, id: impl Into<String>) -> Self {
        self.message_id = Some(id.into());
        self
    }

    /// `In-Reply-To` (angle brackets optional).
    #[must_use]
    pub fn in_reply_to(mut self, id: impl Into<String>) -> Self {
        self.in_reply_to = Some(id.into());
        self
    }

    /// Adds a `References` id (angle brackets optional).
    #[must_use]
    pub fn reference(mut self, id: impl Into<String>) -> Self {
        self.references.push(id.into());
        self
    }

    /// `Date` (default: now).
    #[must_use]
    pub fn date(mut self, date: DateTime<Utc>) -> Self {
        self.date = Some(date);
        self
    }

    /// Adds a header. Line breaks in the value are replaced by spaces;
    /// `Content-Type` and the other MIME structure headers are ignored.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Adds a header as the provider reported it, written first and as
    /// is (on one line), `From`, `To`, `Subject` and `Date` included: the
    /// fields set with the other methods are only written when no such
    /// header was given. `Content-Type` and the other MIME structure
    /// headers are ignored.
    #[must_use]
    pub fn raw_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.raw_headers.push((name.into(), value.into()));
        self
    }

    /// Adds an attachment.
    #[must_use]
    pub fn attach(
        mut self,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        content: impl Into<Bytes>,
    ) -> Self {
        self.parts.push(Part {
            filename: Some(filename.into()),
            content_type: content_type.into(),
            content_id: None,
            inline: false,
            content: content.into(),
        });
        self
    }

    /// Adds an inline part, referenced from the HTML as `cid:{content_id}`.
    #[must_use]
    pub fn inline(
        mut self,
        content_id: impl Into<String>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        content: impl Into<Bytes>,
    ) -> Self {
        self.parts.push(Part {
            filename: Some(filename.into()),
            content_type: content_type.into(),
            content_id: Some(content_id.into()),
            inline: true,
            content: content.into(),
        });
        self
    }

    /// Attaches a whole message (`message/rfc822`), as a user forwarding a
    /// suspicious message does.
    #[must_use]
    pub fn forward(mut self, filename: impl Into<String>, raw: impl Into<Bytes>) -> Self {
        self.parts.push(Part {
            filename: Some(filename.into()),
            content_type: "message/rfc822".to_owned(),
            content_id: None,
            inline: false,
            content: raw.into(),
        });
        self
    }

    /// The provider id of the built [`Incoming`].
    #[must_use]
    pub fn provider_id(mut self, id: impl Into<String>) -> Self {
        self.provider_id = Some(id.into());
        self
    }

    /// The verdicts of the built [`Incoming`].
    #[must_use]
    pub fn verdicts(mut self, verdicts: Verdicts) -> Self {
        self.verdicts = verdicts;
        self
    }

    /// Where the built [`Incoming`] says the raw message is stored.
    #[must_use]
    pub fn stored(mut self, stored: StoredMail) -> Self {
        self.stored = Some(stored);
        self
    }

    /// The message as MIME bytes.
    pub fn to_bytes(&self) -> Bytes {
        let mut out = String::new();
        let mut written: Vec<String> = Vec::new();
        for (name, value) in &self.raw_headers {
            let name = name.trim();
            let lower = name.to_ascii_lowercase();
            if STRUCTURAL.contains(&lower.as_str()) || !valid_name(name) {
                continue;
            }
            header(&mut out, name, &single_line(value));
            written.push(lower);
        }
        let field = |out: &mut String, name: &str, value: Option<String>| {
            if let Some(value) = value
                && !written.iter().any(|w| w.eq_ignore_ascii_case(name))
            {
                header(out, name, &value);
            }
        };
        let date = self.date.unwrap_or_else(Utc::now);
        field(&mut out, "Date", Some(date.to_rfc2822()));
        let message_id = self
            .message_id
            .clone()
            .unwrap_or_else(|| format!("{}@inbound.navree", Uuid::now_v7().simple()));
        field(&mut out, "Message-ID", Some(angled(&message_id)));
        field(&mut out, "From", self.from.as_deref().map(encode_address));
        field(&mut out, "To", address_list(&self.to));
        field(&mut out, "Cc", address_list(&self.cc));
        field(&mut out, "Reply-To", address_list(&self.reply_to));
        field(
            &mut out,
            "In-Reply-To",
            self.in_reply_to.as_deref().map(angled),
        );
        field(
            &mut out,
            "References",
            (!self.references.is_empty()).then(|| {
                self.references
                    .iter()
                    .map(|id| angled(id))
                    .collect::<Vec<_>>()
                    .join(" ")
            }),
        );
        field(
            &mut out,
            "Subject",
            self.subject
                .as_deref()
                .map(|subject| encode_word(&single_line(subject))),
        );
        for (name, value) in &self.headers {
            let lower = name.trim().to_ascii_lowercase();
            if STRUCTURAL.contains(&lower.as_str())
                || STANDARD_FIELDS.contains(&lower.as_str())
                || !valid_name(name.trim())
            {
                continue;
            }
            header(&mut out, name.trim(), &encode_word(&single_line(value)));
        }
        header(&mut out, "MIME-Version", "1.0");

        let mut bytes = out.into_bytes();
        bytes.extend_from_slice(&self.body_entity());
        Bytes::from(bytes)
    }

    /// Builds the [`Incoming`] (source `test` unless set).
    pub fn build(self) -> Incoming {
        let raw = self.to_bytes();
        let recipients = if self.envelope.is_empty() {
            self.to
                .iter()
                .chain(&self.cc)
                .map(|address| bare(address))
                .collect()
        } else {
            self.envelope.iter().map(|address| bare(address)).collect()
        };
        Incoming {
            raw,
            source: self.source.unwrap_or_else(|| "test".to_owned()),
            provider_id: self.provider_id,
            envelope: Envelope {
                sender: self.from.as_deref().map(bare),
                recipients,
            },
            verdicts: self.verdicts,
            stored: self.stored,
        }
    }

    /// The body entity: its `Content-*` headers, a blank line and the body.
    fn body_entity(&self) -> Vec<u8> {
        let alternative = match (&self.text, &self.html) {
            (Some(text), Some(html)) => {
                let boundary = boundary("alt");
                let mut entity =
                    format!("Content-Type: multipart/alternative; boundary=\"{boundary}\"\r\n\r\n");
                for (subtype, body) in [("plain", text), ("html", html)] {
                    let _ = write!(entity, "--{boundary}\r\n");
                    entity.push_str(&text_entity(subtype, body));
                    entity.push_str("\r\n");
                }
                let _ = write!(entity, "--{boundary}--\r\n");
                entity
            }
            (None, Some(html)) => text_entity("html", html),
            (Some(text), None) => text_entity("plain", text),
            (None, None) => text_entity("plain", ""),
        };
        if self.parts.is_empty() {
            return alternative.into_bytes();
        }
        let boundary = boundary("mixed");
        let mut entity = format!(
            "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\r\n--{boundary}\r\n{alternative}\r\n"
        )
        .into_bytes();
        for part in &self.parts {
            entity.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            entity.extend_from_slice(&part_entity(part));
            entity.extend_from_slice(b"\r\n");
        }
        entity.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        entity
    }
}

/// A `text/{subtype}` entity in UTF-8: 7bit when the text is short-lined
/// ASCII, base64 otherwise.
fn text_entity(subtype: &str, body: &str) -> String {
    let normalized = body.replace("\r\n", "\n").replace('\n', "\r\n");
    if normalized.is_ascii() && normalized.lines().all(|line| line.len() <= 998) {
        format!(
            "Content-Type: text/{subtype}; charset=utf-8\r\nContent-Transfer-Encoding: 7bit\r\n\r\n{normalized}\r\n"
        )
    } else {
        format!(
            "Content-Type: text/{subtype}; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}",
            wrapped_base64(normalized.as_bytes())
        )
    }
}

/// An attachment entity. A forwarded message is written as it is (its
/// bytes need not be UTF-8); anything else in base64.
fn part_entity(part: &Part) -> Vec<u8> {
    let mut entity = String::new();
    let filename = part.filename.as_deref().map(single_line);
    let content_type = single_line(&part.content_type);
    match &filename {
        Some(name) => {
            let _ = write!(
                entity,
                "Content-Type: {content_type}; name=\"{}\"\r\n",
                quoted(name)
            );
        }
        None => {
            let _ = write!(entity, "Content-Type: {content_type}\r\n");
        }
    }
    if let Some(id) = &part.content_id {
        let _ = write!(entity, "Content-ID: {}\r\n", angled(&single_line(id)));
    }
    let disposition = if part.inline { "inline" } else { "attachment" };
    match &filename {
        Some(name) => {
            let _ = write!(
                entity,
                "Content-Disposition: {disposition}; filename=\"{}\"\r\n",
                quoted(name)
            );
        }
        None => {
            let _ = write!(entity, "Content-Disposition: {disposition}\r\n");
        }
    }
    if content_type.eq_ignore_ascii_case("message/rfc822") {
        entity.push_str("\r\n");
        let mut bytes = entity.into_bytes();
        bytes.extend_from_slice(&part.content);
        bytes.extend_from_slice(b"\r\n");
        bytes
    } else {
        entity.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
        entity.push_str(&wrapped_base64(&part.content));
        entity.into_bytes()
    }
}

/// Base64 in 76-character lines, each ending in CRLF.
fn wrapped_base64(bytes: &[u8]) -> String {
    let encoded = STANDARD.encode(bytes);
    let mut out = String::with_capacity(encoded.len() + encoded.len() / 76 * 2 + 2);
    for chunk in encoded.as_bytes().chunks(76) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push_str("\r\n");
    }
    out
}

/// `Name: value\r\n`.
fn header(out: &mut String, name: &str, value: &str) {
    let _ = write!(out, "{name}: {value}\r\n");
}

/// An address list value, when there are addresses.
fn address_list(addresses: &[String]) -> Option<String> {
    if addresses.is_empty() {
        return None;
    }
    let encoded: Vec<String> = addresses.iter().map(|a| encode_address(a)).collect();
    Some(encoded.join(", "))
}

/// `Ada <ada@example.com>` with a non-ASCII name as an encoded word.
fn encode_address(address: &str) -> String {
    let address = single_line(address);
    match address.rsplit_once('<') {
        Some((name, rest)) if rest.trim_end().ends_with('>') => {
            let name = name.trim().trim_matches('"').trim();
            let email = rest.trim_end().trim_end_matches('>').trim();
            if name.is_empty() {
                format!("<{email}>")
            } else if name.is_ascii() {
                format!("\"{}\" <{email}>", quoted(name))
            } else {
                format!("{} <{email}>", encode_word(name))
            }
        }
        _ => address.trim().to_owned(),
    }
}

/// `ada@example.com` from `Ada <ada@example.com>`.
fn bare(address: &str) -> String {
    match address.rsplit_once('<') {
        Some((_, rest)) => rest.trim_end().trim_end_matches('>').trim().to_owned(),
        None => address.trim().to_owned(),
    }
}

/// An RFC 2047 encoded word when `value` is not ASCII.
fn encode_word(value: &str) -> String {
    if value.is_ascii() {
        value.to_owned()
    } else {
        format!("=?UTF-8?B?{}?=", STANDARD.encode(value.as_bytes()))
    }
}

/// `<id>`.
fn angled(id: &str) -> String {
    let id = id.trim().trim_start_matches('<').trim_end_matches('>');
    format!("<{id}>")
}

/// Line breaks replaced by spaces.
fn single_line(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

/// Quotes and backslashes escaped for a quoted string.
fn quoted(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// A header name: printable ASCII without a colon.
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_graphic() && b != b':')
}

/// A boundary that no body contains.
fn boundary(kind: &str) -> String {
    format!("navree-{kind}-{}", Uuid::now_v7().simple())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_with_non_ascii_names_are_encoded() {
        assert_eq!(
            encode_address("Jürgen <j@example.de>"),
            format!(
                "=?UTF-8?B?{}?= <j@example.de>",
                STANDARD.encode("Jürgen".as_bytes())
            )
        );
        assert_eq!(
            encode_address("Ada <ada@example.com>"),
            "\"Ada\" <ada@example.com>"
        );
        assert_eq!(encode_address("ada@example.com"), "ada@example.com");
        assert_eq!(bare("Ada <ada@example.com>"), "ada@example.com");
    }

    #[test]
    fn header_values_stay_on_one_line() {
        let raw = Compose::new()
            .from("a@example.com")
            .subject("one\r\nBcc: evil@example.com")
            .header("X-Evil", "a\nb")
            .header("Content-Type", "text/evil")
            .to_bytes();
        let text = String::from_utf8(raw.to_vec()).unwrap();
        assert!(!text.contains("\r\nBcc:"), "{text}");
        assert!(text.contains("X-Evil: a b\r\n"), "{text}");
        assert!(!text.contains("text/evil"), "{text}");
    }
}
