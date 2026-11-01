//! Navree's MIME reader for received mail: RFC 5322 headers, RFC 2045 to
//! RFC 2047 MIME (multiparts, base64 and quoted-printable, charsets,
//! encoded words), RFC 2183 dispositions and RFC 2231 parameters.
//!
//! The message is untrusted input from anyone, so the reader is bounded:
//! it reads the bytes once per level of nesting, never recurses deeper than
//! [`Limits::depth`], stops at [`Limits::parts`] parts and
//! [`Limits::headers`] header fields, and every buffer it fills is sized by
//! bytes it has read. Unencoded attachments and forwarded messages share
//! the message's buffer.
//!
//! Which part is the text body, which the HTML body and which are
//! attachments follows the common reading of RFC 2046: in
//! `multipart/alternative` the text and HTML versions are the bodies; in
//! `multipart/mixed` the inline text, HTML and media parts are, until an
//! alternative settles one kind; in `multipart/related` only the first part
//! is, the others are what it refers to.

mod address;
mod charset;
mod date;
mod header;
mod html;
mod params;
mod transfer;
mod words;

use std::ops::Range;

use bytes::Bytes;
use chrono::{DateTime, Utc};

pub(crate) use address::Mailbox;
pub(crate) use header::unfold;
use header::{Field, Fields};
use params::ContentType;

/// How much of a message the reader takes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limits {
    /// Multiparts nested in multiparts, the outermost counting 1.
    pub depth: usize,
    /// Parts in the whole message, multiparts and the message itself
    /// included.
    pub parts: usize,
    /// Fields in the message's own header.
    pub headers: usize,
    /// Addresses in one address field, ids in `References`.
    pub addresses: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            depth: DEFAULT_MAX_DEPTH,
            parts: DEFAULT_MAX_PARTS,
            headers: DEFAULT_MAX_HEADERS,
            addresses: DEFAULT_MAX_ADDRESSES,
        }
    }
}

/// The default of `mail.inbound.max_depth`.
pub(crate) const DEFAULT_MAX_DEPTH: usize = 32;
/// The default of `mail.inbound.max_parts`.
pub(crate) const DEFAULT_MAX_PARTS: usize = 1000;
/// The default of `mail.inbound.max_headers`.
pub(crate) const DEFAULT_MAX_HEADERS: usize = 1000;
/// The default of `mail.inbound.max_addresses`.
pub(crate) const DEFAULT_MAX_ADDRESSES: usize = 1000;

/// Why a message can't be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MimeError {
    /// The bytes hold no header field.
    NoHeaders,
    /// More header fields than [`Limits::headers`].
    TooManyHeaders(usize),
    /// More parts than [`Limits::parts`].
    TooManyParts(usize),
    /// Multiparts nested deeper than [`Limits::depth`].
    TooDeep(usize),
    /// An address field or `References` with more than
    /// [`Limits::addresses`].
    TooManyAddresses(usize),
}

impl std::fmt::Display for MimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MimeError::NoHeaders => f.write_str("no message headers found"),
            MimeError::TooManyHeaders(max) => write!(
                f,
                "the message has more than {max} header fields (`mail.inbound.max_headers`)"
            ),
            MimeError::TooManyParts(max) => write!(
                f,
                "the message has more than {max} parts (`mail.inbound.max_parts`)"
            ),
            MimeError::TooDeep(max) => write!(
                f,
                "the message nests multiparts more than {max} levels deep (`mail.inbound.max_depth`)"
            ),
            MimeError::TooManyAddresses(max) => write!(
                f,
                "a header field lists more than {max} addresses or ids (`mail.inbound.max_addresses`)"
            ),
        }
    }
}

/// A part that is not the text or HTML body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Attachment {
    pub filename: Option<String>,
    pub content_type: String,
    pub content_id: Option<String>,
    pub inline: bool,
    pub content: Bytes,
}

/// A message read by [`parse`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Message {
    /// The message's header fields in order: names as written (bytes that
    /// aren't UTF-8 become U+FFFD), values as written, folds included.
    pub headers: Vec<(String, Bytes)>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub attachments: Vec<Attachment>,
}

/// Reads a raw message.
pub(crate) fn parse(raw: &Bytes, limits: Limits) -> Result<Message, MimeError> {
    let mut fields = Fields::new(raw);
    let mut headers = Vec::new();
    let mut root = EntityHeaders::default();
    for field in fields.by_ref() {
        if headers.len() == limits.headers {
            return Err(MimeError::TooManyHeaders(limits.headers));
        }
        root.note(field);
        headers.push((
            String::from_utf8_lossy(field.name).into_owned(),
            raw.slice_ref(field.value),
        ));
    }
    if headers.is_empty() {
        return Err(MimeError::NoHeaders);
    }

    let mut walker = Walker {
        raw,
        limits,
        count: 1,
        parts: Vec::new(),
        text_body: Vec::new(),
        html_body: Vec::new(),
        attachments: Vec::new(),
    };
    // Without the blank line, the message is all header.
    if let Some(body) = fields.body_start() {
        let mut container = Container {
            kind: ContainerKind::Message,
            in_alternative: false,
            need_html: true,
            need_text: true,
            parts: 1,
        };
        walker.entity(&root, body..raw.len(), &mut container, 0)?;
    }
    Ok(walker.finish(headers))
}

/// The fields the reader needs from a part's header (the last of each).
#[derive(Default)]
struct EntityHeaders<'a> {
    content_type: Option<&'a [u8]>,
    transfer_encoding: Option<&'a [u8]>,
    disposition: Option<&'a [u8]>,
    content_id: Option<&'a [u8]>,
}

impl<'a> EntityHeaders<'a> {
    fn note(&mut self, field: Field<'a>) {
        let slot = if field.name.eq_ignore_ascii_case(b"content-type") {
            &mut self.content_type
        } else if field
            .name
            .eq_ignore_ascii_case(b"content-transfer-encoding")
        {
            &mut self.transfer_encoding
        } else if field.name.eq_ignore_ascii_case(b"content-disposition") {
            &mut self.disposition
        } else if field.name.eq_ignore_ascii_case(b"content-id") {
            &mut self.content_id
        } else {
            return;
        };
        *slot = Some(field.value);
    }
}

/// What kind of container a part is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContainerKind {
    /// The message itself.
    Message,
    Mixed,
    Alternative,
    Related,
    Digest,
    Other,
}

/// The multipart (or the message) whose parts are being read.
struct Container {
    kind: ContainerKind,
    /// Inside a `multipart/alternative`, at any depth.
    in_alternative: bool,
    /// No HTML body was settled on yet.
    need_html: bool,
    /// No text body was settled on yet.
    need_text: bool,
    /// The parts read so far, the current one included.
    parts: usize,
}

/// What a part holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Multipart(ContainerKind),
    TextPlain,
    TextHtml,
    TextOther,
    /// `image/*`, `audio/*`, `video/*`.
    Media,
    /// `message/rfc822` or `message/global`.
    Message,
    Other,
}

impl Kind {
    fn of(content_type: Option<&ContentType>, container: ContainerKind) -> Kind {
        let Some(content_type) = content_type else {
            return if container == ContainerKind::Digest {
                Kind::Message
            } else {
                Kind::TextPlain
            };
        };
        let subtype = content_type.subtype.as_deref();
        match content_type.kind.as_str() {
            "multipart" => Kind::Multipart(match subtype {
                Some("mixed") => ContainerKind::Mixed,
                Some("alternative") => ContainerKind::Alternative,
                Some("related") => ContainerKind::Related,
                Some("digest") => ContainerKind::Digest,
                _ => ContainerKind::Other,
            }),
            "text" => match subtype {
                Some("plain") => Kind::TextPlain,
                Some("html") => Kind::TextHtml,
                _ => Kind::TextOther,
            },
            "image" | "audio" | "video" => Kind::Media,
            "message" if matches!(subtype, Some("rfc822" | "global")) => Kind::Message,
            _ => Kind::Other,
        }
    }

    /// A kind that can be shown in the body.
    fn is_inline(self) -> bool {
        matches!(self, Kind::TextPlain | Kind::TextHtml | Kind::Media)
    }

    fn is_text(self) -> bool {
        matches!(self, Kind::TextPlain | Kind::TextHtml | Kind::TextOther)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TransferEncoding {
    Identity,
    Base64,
    QuotedPrintable,
}

enum Body {
    Text(String),
    Html(String),
    Binary(Bytes),
    Message(Bytes),
}

struct Part {
    content_type: Option<ContentType>,
    disposition: Option<ContentType>,
    content_id: Option<String>,
    body: Body,
}

struct Walker<'a> {
    raw: &'a Bytes,
    limits: Limits,
    /// Parts counted so far.
    count: usize,
    parts: Vec<Part>,
    text_body: Vec<usize>,
    html_body: Vec<usize>,
    attachments: Vec<usize>,
}

impl Walker<'_> {
    /// Reads one part: its header was read into `headers`, its body is
    /// `body`, and `depth` multiparts hold it.
    fn entity(
        &mut self,
        headers: &EntityHeaders<'_>,
        body: Range<usize>,
        container: &mut Container,
        depth: usize,
    ) -> Result<(), MimeError> {
        let content_type = headers.content_type.and_then(params::parse);
        let disposition = headers.disposition.and_then(params::parse);
        let content_id = headers
            .content_id
            .and_then(|value| ids(&header::unfold(value)).last().map(str::to_owned));
        let mut kind = Kind::of(content_type.as_ref(), container.kind);

        if let Kind::Multipart(multipart) = kind {
            match content_type.as_ref().and_then(|ct| ct.param("boundary")) {
                Some(boundary) if !boundary.is_empty() => {
                    match self.split(body.clone(), boundary.as_bytes())? {
                        Some(children) => {
                            return self.multipart(multipart, children, container, depth);
                        }
                        // No delimiter: the body is text.
                        None => kind = Kind::TextOther,
                    }
                }
                Some(_) => kind = Kind::TextOther,
                // No boundary: a part to keep as it is.
                None => kind = Kind::Other,
            }
        }

        let encoding = match headers.transfer_encoding.map(words::unstructured) {
            Some(value) if value.eq_ignore_ascii_case("base64") => TransferEncoding::Base64,
            Some(value) if value.eq_ignore_ascii_case("quoted-printable") => {
                TransferEncoding::QuotedPrintable
            }
            _ => TransferEncoding::Identity,
        };
        let slice = self.raw.get(body).unwrap_or_default();
        let content = match encoding {
            // The part shares the message's buffer.
            TransferEncoding::Identity => self.raw.slice_ref(slice),
            TransferEncoding::Base64 => Bytes::from(transfer::base64(slice)),
            TransferEncoding::QuotedPrintable => Bytes::from(transfer::quoted_printable(slice)),
        };

        let index = self.parts.len();
        if kind == Kind::Message {
            self.attachments.push(index);
            self.parts.push(Part {
                content_type,
                disposition,
                content_id,
                body: Body::Message(content),
            });
            return Ok(());
        }

        let is_inline = kind.is_inline()
            && disposition.as_ref().is_none_or(|d| d.kind != "attachment")
            && (container.parts == 1
                || (container.kind != ContainerKind::Related
                    && (kind == Kind::Media
                        || content_type.as_ref().is_none_or(|ct| !ct.has_param("name")))));
        let (to_html, to_text) = if container.kind == ContainerKind::Alternative {
            (kind == Kind::TextHtml, kind == Kind::TextPlain)
        } else if is_inline {
            if container.in_alternative && (container.need_text || container.need_html) {
                match kind {
                    Kind::TextHtml => container.need_text = false,
                    Kind::TextPlain => container.need_html = false,
                    _ => {}
                }
            }
            (container.need_html, container.need_text)
        } else {
            (false, false)
        };
        if to_html {
            self.html_body.push(index);
        }
        if to_text {
            self.text_body.push(index);
        }

        let body = if kind.is_text() {
            let charset = content_type.as_ref().and_then(|ct| ct.param("charset"));
            let text = charset::decode(&content, charset);
            let is_html = kind == Kind::TextHtml;
            if (is_html && !to_html) || (!is_html && !to_text) {
                self.attachments.push(index);
            }
            if is_html {
                Body::Html(text)
            } else {
                Body::Text(text)
            }
        } else {
            self.attachments.push(index);
            Body::Binary(content)
        };
        self.parts.push(Part {
            content_type,
            disposition,
            content_id,
            body,
        });
        Ok(())
    }

    /// Reads a multipart's parts.
    fn multipart(
        &mut self,
        kind: ContainerKind,
        children: Vec<Range<usize>>,
        container: &Container,
        depth: usize,
    ) -> Result<(), MimeError> {
        let depth = depth + 1;
        if depth > self.limits.depth {
            return Err(MimeError::TooDeep(self.limits.depth));
        }
        let mut inner = Container {
            kind,
            in_alternative: container.in_alternative || kind == ContainerKind::Alternative,
            need_html: container.need_html,
            need_text: container.need_text,
            parts: 0,
        };
        let html_before = self.html_body.len();
        let text_before = self.text_body.len();
        for child in children {
            inner.parts += 1;
            let bytes = self.raw.get(child.clone()).unwrap_or_default();
            let mut fields = Fields::new(bytes);
            let mut headers = EntityHeaders::default();
            for field in fields.by_ref() {
                headers.note(field);
            }
            let body_start = fields.body_start().map_or(child.end, |at| child.start + at);
            self.entity(&headers, body_start..child.end, &mut inner, depth)?;
        }
        // An alternative with one kind of body only: it serves as both.
        if kind == ContainerKind::Alternative && inner.need_html && inner.need_text {
            if self.text_body.len() == text_before && self.html_body.len() != html_before {
                let html = self
                    .html_body
                    .get(html_before..)
                    .unwrap_or_default()
                    .to_vec();
                self.text_body.extend(html);
            }
            if self.html_body.len() == html_before && self.text_body.len() != text_before {
                let text = self
                    .text_body
                    .get(text_before..)
                    .unwrap_or_default()
                    .to_vec();
                self.html_body.extend(text);
            }
        }
        Ok(())
    }

    /// The parts of a multipart body: between the delimiter lines
    /// (`--boundary`, a whole line but for blanks), each without the line
    /// break before the next delimiter. `None` when no delimiter is found.
    /// A body that ends before its closing delimiter (`--boundary--`) ends
    /// its last part.
    fn split(
        &mut self,
        body: Range<usize>,
        boundary: &[u8],
    ) -> Result<Option<Vec<Range<usize>>>, MimeError> {
        let raw: &[u8] = self.raw;
        let mut parts = Vec::new();
        let mut current: Option<usize> = None;
        let mut found = false;
        let mut line_start = body.start;
        while line_start < body.end {
            let rest = raw.get(line_start..body.end).unwrap_or_default();
            let line_end = memchr::memchr(b'\n', rest).map_or(body.end, |at| line_start + at);
            let next = (line_end + 1).min(body.end);
            let line = raw.get(line_start..line_end).unwrap_or_default();
            if let Some(after) = line
                .strip_prefix(b"--")
                .and_then(|line| line.strip_prefix(boundary))
            {
                let (closing, after) = match after.strip_prefix(b"--") {
                    Some(after) => (true, after),
                    None => (false, after),
                };
                if after
                    .iter()
                    .all(|&byte| matches!(byte, b' ' | b'\t' | b'\r'))
                {
                    found = true;
                    if let Some(start) = current.take() {
                        // The line break before the delimiter is the delimiter's.
                        let mut end = line_start.saturating_sub(1).max(start);
                        if end > start && raw.get(end - 1) == Some(&b'\r') {
                            end -= 1;
                        }
                        self.count_part()?;
                        parts.push(start..end);
                    }
                    if closing {
                        return Ok(Some(parts));
                    }
                    current = Some(next);
                }
            }
            line_start = line_end + 1;
        }
        if let Some(start) = current {
            self.count_part()?;
            parts.push(start.min(body.end)..body.end);
        }
        Ok(found.then_some(parts))
    }

    fn count_part(&mut self) -> Result<(), MimeError> {
        if self.count >= self.limits.parts {
            return Err(MimeError::TooManyParts(self.limits.parts));
        }
        self.count += 1;
        Ok(())
    }

    fn finish(self, headers: Vec<(String, Bytes)>) -> Message {
        let mut parts: Vec<Option<Part>> = self.parts.into_iter().map(Some).collect();
        let body = |index: &usize| {
            parts
                .get(*index)
                .and_then(Option::as_ref)
                .map(|part| &part.body)
        };
        let mut text = self.text_body.iter().find_map(|index| match body(index) {
            Some(Body::Text(text)) => Some(text.clone()),
            _ => None,
        });
        let html = self.html_body.iter().find_map(|index| match body(index) {
            Some(Body::Html(html)) => Some(html.clone()),
            _ => None,
        });
        if text.is_none() {
            text = match self.text_body.first().and_then(body) {
                Some(Body::Html(html)) => Some(html::to_text(html)),
                _ => None,
            };
        }
        // The attachments are moved out of the parts, not copied.
        let attachments = self
            .attachments
            .iter()
            .filter_map(|&index| parts.get_mut(index).and_then(Option::take))
            .map(|part| {
                let content_type = match &part.content_type {
                    Some(content_type) => content_type.media_type(),
                    None if matches!(part.body, Body::Message(_)) => "message/rfc822".to_owned(),
                    None => "application/octet-stream".to_owned(),
                };
                Attachment {
                    filename: part
                        .disposition
                        .as_ref()
                        .and_then(|d| d.param("filename"))
                        .or_else(|| part.content_type.as_ref().and_then(|ct| ct.param("name")))
                        .map(str::to_owned),
                    content_type,
                    content_id: part.content_id,
                    inline: part
                        .disposition
                        .as_ref()
                        .is_some_and(|d| d.kind == "inline"),
                    content: match part.body {
                        Body::Text(text) | Body::Html(text) => Bytes::from(text),
                        Body::Binary(bytes) | Body::Message(bytes) => bytes,
                    },
                }
            })
            .collect();
        Message {
            headers,
            text,
            html,
            attachments,
        }
    }
}

// An unstructured header value (`Subject`), decoded. `None` when empty.
pub(crate) fn text(value: &[u8]) -> Option<String> {
    Some(words::unstructured(value)).filter(|text| !text.is_empty())
}

/// The mailboxes of an address header.
///
/// # Errors
///
/// [`MimeError::TooManyAddresses`] when it lists more than `max`.
pub(crate) fn addresses(value: &[u8], max: usize) -> Result<Vec<Mailbox>, MimeError> {
    address::list(value, max).ok_or(MimeError::TooManyAddresses(max))
}

/// Whether an address header's value is one mailbox every reader takes for the
/// same address.
pub(crate) fn is_one_address(value: &[u8]) -> bool {
    address::is_one_address(value)
}

/// Whether every line of the message's head ends the same way.
pub(crate) fn head_lines_agree(raw: &[u8]) -> bool {
    let mut fields = Fields::new(raw);
    fields.by_ref().for_each(drop);
    let head = raw.get(..fields.body_start().unwrap_or(raw.len()));
    header::line_ends_agree(head.unwrap_or_default())
}

/// A `Date` value in UTC.
pub(crate) fn date(value: &[u8]) -> Option<DateTime<Utc>> {
    date::parse(&header::unfold(value))
}

/// The ids of a `Message-ID`, `In-Reply-To`, `References` or `Content-ID`
/// value, in order: what each pair of angle brackets holds (blanks at its
/// ends trimmed), or, without any, the text before the first `<`, trimmed.
pub(crate) fn ids(value: &str) -> Ids<'_> {
    Ids {
        value,
        rest: value,
        found: false,
    }
}

/// See [`ids`].
pub(crate) struct Ids<'a> {
    value: &'a str,
    rest: &'a str,
    found: bool,
}

impl<'a> Iterator for Ids<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        while let Some(open) = self.rest.find('<') {
            let after = &self.rest[open + 1..];
            let Some(close) = after.find('>') else {
                break;
            };
            self.rest = &after[close + 1..];
            let id = after[..close].trim_matches(|c: char| matches!(c, ' ' | '\t' | '\r' | '\n'));
            if !id.is_empty() {
                self.found = true;
                return Some(id);
            }
        }
        self.rest = "";
        if self.found {
            return None;
        }
        self.found = true;
        Some(self.value.split('<').next().unwrap_or(self.value).trim()).filter(|id| !id.is_empty())
    }
}

#[cfg(test)]
mod tests;
