//! [`DakiyaTransport`]: mail delivery over the app's [`HttpClient`].

use std::collections::HashSet;
use std::fmt;

use navree::BoxFuture;
use navree::contracts::Secret;
use navree::http_client::{HttpClient, HttpClientError, Response};
use navree::mail::{MailError, MailTransport, Message, Sent};
use serde_json::{Value, json};
use url::Url;

use crate::DakiyaSettings;

/// Maximum length, in Unicode scalar values, of a scrubbed provider error message.
const MAX_ERROR_CHARS: usize = 200;

/// Replacement marker written in place of any occurrence of a secret value.
const MASK: &str = "***";

/// Quote wrappers stripped around a matched secret, longest first.
const WRAPPERS: &[(&[u8], &[u8])] = &[
    (b"\"\\\"", b"\\\"\""),
    (b"\\\"", b"\\\""),
    (b"\"", b"\""),
    (b"'", b"'"),
    (b"`", b"`"),
];

/// Single-byte JSON escape sequences (double-escaped forms before single-escaped forms).
const JSON_ESCAPES: &[(&[u8], u8)] = &[
    (b"\\\\\\\"", b'"'),
    (b"\\\\\\\\", b'\\'),
    (b"\\\\\\/", b'/'),
    (b"\\\\/", b'/'),
    (b"\\\\b", 0x08),
    (b"\\\\f", 0x0C),
    (b"\\\\n", b'\n'),
    (b"\\\\r", b'\r'),
    (b"\\\\t", b'\t'),
    (b"\\\"", b'"'),
    (b"\\\\", b'\\'),
    (b"\\/", b'/'),
    (b"\\b", 0x08),
    (b"\\f", 0x0C),
    (b"\\n", b'\n'),
    (b"\\r", b'\r'),
    (b"\\t", b'\t'),
];

/// Delivers mail through the Dakiya HTTP API using the app's shared [`HttpClient`].
///
/// Messages are posted as JSON to the configured `url` with `Authorization: Bearer`
/// authentication, and [`check`](MailTransport::check) sends a read-only `GET` to `/v1/ping`.
/// Non-2xx responses become [`MailError::Provider`] with every plain, quoted, JSON-escaped,
/// Unicode-escaped, percent-encoded and form-encoded occurrence of the API key and URL
/// credentials scrubbed before truncation.
pub struct DakiyaTransport {
    client: HttpClient,
    url: Url,
    key: Secret<String>,
    url_secrets: Vec<Secret<String>>,
    sender_name: Option<String>,
}

impl DakiyaTransport {
    /// Builds a transport from `client` and `settings`.
    ///
    /// Trims leading and trailing whitespace from `api_key`, and returns [`MailError::Config`]
    /// naming the setting key (and never its value) when `api_key` is empty or contains control
    /// characters, when `sender_name` contains control characters, or when `url` is not
    /// `https://` (or `http://localhost` / `http://127.0.0.1`).
    pub fn new(client: HttpClient, settings: DakiyaSettings) -> Result<Self, MailError> {
        let trimmed_key = settings.api_key.expose().trim();
        if trimmed_key.is_empty() {
            return Err(MailError::Config(
                "mail.dakiya.api_key is required".to_owned(),
            ));
        }
        if trimmed_key.chars().any(char::is_control) {
            return Err(MailError::Config(
                "mail.dakiya.api_key must not contain control characters".to_owned(),
            ));
        }
        let url = parse_endpoint(&settings.url)?;
        if settings
            .sender_name
            .as_deref()
            .is_some_and(|name| name.chars().any(char::is_control))
        {
            return Err(MailError::Config(
                "mail.dakiya.sender_name must not contain control characters".to_owned(),
            ));
        }
        let url_secrets = collect_url_secrets(&url);
        Ok(Self {
            client,
            url,
            key: Secret::new(trimmed_key.to_owned()),
            url_secrets,
            sender_name: settings
                .sender_name
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty()),
        })
    }

    fn formatted_sender(&self, from: &str) -> String {
        match &self.sender_name {
            Some(name) if !from.contains('<') => {
                let display = format_display_name(name);
                format!("{display} <{from}>")
            }
            _ => from.to_owned(),
        }
    }

    fn ordered_secrets(&self) -> Vec<&str> {
        let mut secrets: Vec<&str> = Vec::with_capacity(1 + self.url_secrets.len());
        secrets.push(self.key.expose());
        for secret in &self.url_secrets {
            secrets.push(secret.expose());
        }
        secrets.retain(|s| !s.is_empty());
        secrets.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        secrets.dedup();
        secrets
    }

    fn payload(&self, message: &Message) -> Result<Value, MailError> {
        let from = message
            .from
            .as_deref()
            .filter(|sender| !sender.is_empty())
            .ok_or(MailError::MissingSender)?;
        if message.to.is_empty() && message.cc.is_empty() && message.bcc.is_empty() {
            return Err(MailError::NoRecipients);
        }
        Ok(json!({
            "from": self.formatted_sender(from),
            "to": message.to,
            "cc": message.cc,
            "bcc": message.bcc,
            "subject": message.subject,
            "html": message.html,
            "text": message.text,
        }))
    }

    fn ping_url(&self) -> Url {
        let mut ping = self.url.clone();
        ping.set_path("/v1/ping");
        ping.set_query(None);
        ping.set_fragment(None);
        ping
    }

    fn transport_error(&self, error: HttpClientError) -> MailError {
        MailError::Transport {
            transport: "dakiya",
            source: Box::new(error),
        }
    }

    fn provider_error(&self, response: &Response) -> MailError {
        let secrets = self.ordered_secrets();
        let raw = extract_error_text(response, &secrets);
        let message = scrub_and_truncate(&raw, &secrets);
        MailError::Provider {
            transport: "dakiya",
            status: response.status().as_u16(),
            message,
        }
    }
}

impl MailTransport for DakiyaTransport {
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<(), MailError>> {
        Box::pin(async move { self.deliver(message).await.map(drop) })
    }

    fn deliver<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Result<Sent, MailError>> {
        Box::pin(async move {
            let payload = self.payload(message)?;
            let mut request = self
                .client
                .post(self.url.as_str())
                .secret_url()
                .bearer(self.key.expose());
            if let Some(idempotency_key) = &message.idempotency_key {
                request = request.idempotency_key(idempotency_key);
            }
            let response = request
                .json(&payload)
                .send()
                .await
                .map_err(|error| self.transport_error(error))?;
            if !response.is_success() {
                return Err(self.provider_error(&response));
            }
            let secrets = self.ordered_secrets();
            let id = response
                .json::<Value>()
                .ok()
                .and_then(|body| body.get("id").and_then(Value::as_str).map(str::to_owned))
                .filter(|id| !id.is_empty())
                .map(|id| scrub_secrets(&id, &secrets));
            let sent = Sent::new("dakiya");
            Ok(match id {
                Some(id) => sent.with_provider_id(id),
                None => sent,
            })
        })
    }

    fn name(&self) -> &'static str {
        "dakiya"
    }

    fn check(&self) -> BoxFuture<'_, Result<(), MailError>> {
        Box::pin(async move {
            let ping = self.ping_url();
            let response = self
                .client
                .get(ping.as_str())
                .secret_url()
                .bearer(self.key.expose())
                .send()
                .await
                .map_err(|error| self.transport_error(error))?;
            if response.is_success() {
                Ok(())
            } else {
                Err(self.provider_error(&response))
            }
        })
    }
}

impl fmt::Debug for DakiyaTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DakiyaTransport")
            .field("host", &self.url.host_str().unwrap_or_default())
            .finish_non_exhaustive()
    }
}

fn format_display_name(name: &str) -> String {
    if name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == ' ')
    {
        name.to_owned()
    } else {
        let mut quoted = String::with_capacity(name.len() + 2);
        quoted.push('"');
        for ch in name.chars() {
            if matches!(ch, '\\' | '"') {
                quoted.push('\\');
            }
            quoted.push(ch);
        }
        quoted.push('"');
        quoted
    }
}

fn collect_url_secrets(url: &Url) -> Vec<Secret<String>> {
    let mut raw = Vec::new();
    if let Some(password) = url.password()
        && !password.is_empty()
    {
        raw.push(password.to_owned());
        if let Some(decoded) = percent_decode_utf8(password)
            && !decoded.is_empty()
        {
            raw.push(decoded);
        }
    }
    let username = url.username();
    if !username.is_empty() {
        raw.push(username.to_owned());
        if let Some(decoded) = percent_decode_utf8(username)
            && !decoded.is_empty()
        {
            raw.push(decoded);
        }
    }
    for (_, value) in url.query_pairs() {
        if !value.is_empty() {
            raw.push(value.into_owned());
        }
    }
    if let Some(query) = url.query() {
        for pair in query.split('&') {
            if let Some((_, value)) = pair.split_once('=')
                && !value.is_empty()
            {
                raw.push(value.to_owned());
            }
        }
    }
    raw.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    raw.dedup();
    raw.into_iter().map(Secret::new).collect()
}

fn percent_decode_utf8(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(&hi), Some(&lo)) = (bytes.get(i + 1), bytes.get(i + 2))
            && let Some(decoded) = hex_pair(hi, lo)
        {
            out.push(decoded);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn parse_endpoint(raw: &str) -> Result<Url, MailError> {
    let bad = || MailError::Config("mail.dakiya.url must start with https://".to_owned());
    let parsed = Url::parse(raw).map_err(|_| bad())?;
    let allowed = match parsed.scheme() {
        "https" => parsed.host_str().is_some(),
        "http" => matches!(parsed.host_str(), Some("localhost" | "127.0.0.1")),
        _ => false,
    };
    if allowed { Ok(parsed) } else { Err(bad()) }
}

fn extract_error_text(response: &Response, secrets: &[&str]) -> String {
    if let Ok(mut body) = response.json::<Value>() {
        if let Some(text) = body
            .get("error")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                body.get("error")
                    .and_then(|err| err.get("message"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| {
                body.get("message")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
            })
        {
            return scrub_secrets(text.trim(), secrets);
        }
        scrub_json_value(&mut body, secrets);
        if let Value::String(text) = body {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return trimmed.to_owned();
            }
        } else if !body.is_null()
            && let Ok(serialised) = serde_json::to_string(&body)
        {
            return serialised;
        }
    }
    let text = response.text();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        response
            .status()
            .canonical_reason()
            .unwrap_or("request refused")
            .to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn scrub_json_value(value: &mut Value, secrets: &[&str]) {
    match value {
        Value::String(text) => {
            *text = scrub_secrets(text, secrets);
        }
        Value::Array(items) => {
            for item in items {
                scrub_json_value(item, secrets);
            }
        }
        Value::Object(map) => {
            let entries: Vec<(String, Value)> = std::mem::take(map)
                .into_iter()
                .map(|(key, mut item)| {
                    scrub_json_value(&mut item, secrets);
                    (scrub_secrets(&key, secrets), item)
                })
                .collect();
            for (key, item) in entries {
                map.insert(key, item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn scrub_and_truncate(raw: &str, secrets: &[&str]) -> String {
    let scrubbed = scrub_secrets(raw, secrets);
    scrubbed.chars().take(MAX_ERROR_CHARS).collect()
}

fn scrub_secrets(raw: &str, secrets: &[&str]) -> String {
    let mut text = raw.to_owned();
    for secret in secrets {
        text = scrub_key(&text, secret);
    }
    text
}

fn scrub_key(raw: &str, key: &str) -> String {
    if key.is_empty() {
        return raw.to_owned();
    }
    scrub_encoded(raw, key.as_bytes())
}

fn scrub_encoded(text: &str, secret: &[u8]) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    'scan: while i < bytes.len() {
        if text.is_char_boundary(i) {
            for &(open, close) in WRAPPERS {
                if bytes[i..].starts_with(open)
                    && let Some(end) = match_encoded_end(bytes, i + open.len(), secret)
                    && bytes[end..].starts_with(close)
                    && text.is_char_boundary(end + close.len())
                {
                    out.push_str(MASK);
                    i = end + close.len();
                    continue 'scan;
                }
            }
            if let Some(end) = match_encoded_end(bytes, i, secret)
                && text.is_char_boundary(end)
            {
                out.push_str(MASK);
                i = end;
                continue 'scan;
            }
        }
        let ch = text[i..].chars().next().expect("valid UTF-8 slice");
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

fn match_encoded_end(bytes: &[u8], start: usize, secret: &[u8]) -> Option<usize> {
    let mut failed = HashSet::new();
    match_encoded_inner(bytes, start, secret, &mut failed)
}

fn match_encoded_inner(
    bytes: &[u8],
    p: usize,
    secret: &[u8],
    failed: &mut HashSet<(usize, usize)>,
) -> Option<usize> {
    if secret.is_empty() {
        return Some(p);
    }
    if p >= bytes.len() || failed.contains(&(p, secret.len())) {
        return None;
    }

    if bytes[p] == b'\\' {
        for slashes in [2, 1] {
            if let Some((first, after_first)) = parse_unicode_unit(bytes, p, slashes) {
                if (0xD800..=0xDBFF).contains(&first) {
                    if let Some((second, after_second)) =
                        parse_unicode_unit(bytes, after_first, slashes)
                        && (0xDC00..=0xDFFF).contains(&second)
                    {
                        let scalar = 0x1_0000
                            + (((u32::from(first) - 0xD800) << 10) | (u32::from(second) - 0xDC00));
                        if let Some(ch) = char::from_u32(scalar) {
                            let mut buf = [0u8; 4];
                            let ch_bytes = ch.encode_utf8(&mut buf).as_bytes();
                            if secret.starts_with(ch_bytes)
                                && let Some(end) = match_encoded_inner(
                                    bytes,
                                    after_second,
                                    &secret[ch_bytes.len()..],
                                    failed,
                                )
                            {
                                return Some(end);
                            }
                        }
                    }
                } else if let Some(ch) = char::from_u32(u32::from(first)) {
                    let mut buf = [0u8; 4];
                    let ch_bytes = ch.encode_utf8(&mut buf).as_bytes();
                    if secret.starts_with(ch_bytes)
                        && let Some(end) = match_encoded_inner(
                            bytes,
                            after_first,
                            &secret[ch_bytes.len()..],
                            failed,
                        )
                    {
                        return Some(end);
                    }
                    if u8::try_from(first) == Ok(secret[0])
                        && let Some(end) =
                            match_encoded_inner(bytes, after_first, &secret[1..], failed)
                    {
                        return Some(end);
                    }
                }
            }
        }

        for &(escaped, decoded) in JSON_ESCAPES {
            if decoded == secret[0]
                && bytes[p..].starts_with(escaped)
                && let Some(end) =
                    match_encoded_inner(bytes, p + escaped.len(), &secret[1..], failed)
            {
                return Some(end);
            }
        }
    }

    if bytes[p] == b'%'
        && let (Some(&hi), Some(&lo)) = (bytes.get(p + 1), bytes.get(p + 2))
        && let Some(decoded) = hex_pair(hi, lo)
        && decoded == secret[0]
        && let Some(end) = match_encoded_inner(bytes, p + 3, &secret[1..], failed)
    {
        return Some(end);
    }

    if secret[0] == b' '
        && bytes[p] == b'+'
        && let Some(end) = match_encoded_inner(bytes, p + 1, &secret[1..], failed)
    {
        return Some(end);
    }

    if bytes[p] == secret[0]
        && let Some(end) = match_encoded_inner(bytes, p + 1, &secret[1..], failed)
    {
        return Some(end);
    }

    failed.insert((p, secret.len()));
    None
}

fn parse_unicode_unit(bytes: &[u8], start: usize, slashes: usize) -> Option<(u16, usize)> {
    if !bytes
        .get(start..start + slashes)?
        .iter()
        .all(|&b| b == b'\\')
    {
        return None;
    }
    if bytes.get(start + slashes) != Some(&b'u') {
        return None;
    }
    let hex_start = start + slashes + 1;
    let unit = hex_u16(bytes.get(hex_start..hex_start + 4)?)?;
    Some((unit, hex_start + 4))
}

fn hex_u16(digits: &[u8]) -> Option<u16> {
    if digits.len() != 4 {
        return None;
    }
    let hi = u16::from(hex_pair(digits[0], digits[1])?);
    let lo = u16::from(hex_pair(digits[2], digits[3])?);
    Some((hi << 8) | lo)
}

fn hex_pair(hi: u8, lo: u8) -> Option<u8> {
    Some((hex_nibble(hi)? << 4) | hex_nibble(lo)?)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
