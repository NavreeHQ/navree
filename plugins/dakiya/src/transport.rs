//! [`DakiyaTransport`]: mail delivery over the app's [`HttpClient`].

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

/// Replacement marker written in place of any occurrence of the API key.
const MASK: &str = "***";

/// Delivers mail through the Dakiya HTTP API using the app's shared [`HttpClient`].
///
/// Messages are posted as JSON to the configured `url` with `Authorization: Bearer`
/// authentication, and [`check`](MailTransport::check) sends a read-only `GET` to `/v1/ping`.
/// Non-2xx responses become [`MailError::Provider`] with every plain, quoted, JSON-escaped,
/// percent-encoded and form-encoded occurrence of the key scrubbed before truncation.
pub struct DakiyaTransport {
    client: HttpClient,
    url: Url,
    key: Secret<String>,
    sender_name: Option<String>,
}

impl DakiyaTransport {
    /// Builds a transport from `client` and `settings`.
    ///
    /// Returns [`MailError::Config`] naming the setting key (and never its value) when
    /// `api_key` is empty or `url` is not `https://` (or `http://localhost` / `http://127.0.0.1`).
    pub fn new(client: HttpClient, settings: DakiyaSettings) -> Result<Self, MailError> {
        if settings.api_key.expose().trim().is_empty() {
            return Err(MailError::Config(
                "mail.dakiya.api_key is required".to_owned(),
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
        Ok(Self {
            client,
            url,
            key: settings.api_key,
            sender_name: settings.sender_name.filter(|name| !name.trim().is_empty()),
        })
    }

    fn formatted_sender(&self, from: &str) -> String {
        match &self.sender_name {
            Some(name) if !from.contains('<') => format!("{name} <{from}>"),
            _ => from.to_owned(),
        }
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
        let raw = extract_error_text(response);
        let message = scrub_and_truncate(&raw, self.key.expose());
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
            let id = response
                .json::<Value>()
                .ok()
                .and_then(|body| body.get("id").and_then(Value::as_str).map(str::to_owned))
                .filter(|id| !id.is_empty())
                .map(|id| scrub_key(&id, self.key.expose()));
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

fn extract_error_text(response: &Response) -> String {
    if let Ok(body) = response.json::<Value>()
        && let Some(text) = body
            .get("error")
            .and_then(Value::as_str)
            .or_else(|| {
                body.get("error")
                    .and_then(|err| err.get("message"))
                    .and_then(Value::as_str)
            })
            .or_else(|| body.get("message").and_then(Value::as_str))
    {
        return text.to_owned();
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

fn scrub_and_truncate(raw: &str, key: &str) -> String {
    let scrubbed = scrub_key(raw, key);
    scrubbed.chars().take(MAX_ERROR_CHARS).collect()
}

fn scrub_key(raw: &str, key: &str) -> String {
    if key.is_empty() {
        return raw.to_owned();
    }

    let mut text = raw.to_owned();

    // Scrub JSON-escaped and quoted variants first (longest first, quoted before bare).
    if let Ok(json_quoted) = serde_json::to_string(key) {
        let json_inner = &json_quoted[1..json_quoted.len() - 1];
        let slash_escaped = json_inner.replace('/', "\\/");
        let mut variants = vec![
            json_quoted.clone(),
            format!("'{json_inner}'"),
            format!("`{json_inner}`"),
            format!("\"{slash_escaped}\""),
            format!("'{slash_escaped}'"),
            format!("`{slash_escaped}`"),
            json_inner.to_owned(),
            slash_escaped,
        ];
        if let Ok(double_quoted) = serde_json::to_string(&json_quoted) {
            let double_inner = &double_quoted[1..double_quoted.len() - 1];
            variants.push(double_quoted.clone());
            variants.push(double_inner.to_owned());
        }
        variants.retain(|v| !v.is_empty());
        variants.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        variants.dedup();
        for variant in &variants {
            text = text.replace(variant, MASK);
        }
    }

    // Scrub plain, quoted, percent-encoded (%XX and %xx, full or partial) and URL `+` forms.
    scrub_encoded(&text, key.as_bytes())
}

fn scrub_encoded(text: &str, secret: &[u8]) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if matches!(b, b'"' | b'\'' | b'`')
            && let Some(end) = match_encoded_end(bytes, i + 1, secret)
            && bytes.get(end) == Some(&b)
            && text.is_char_boundary(end + 1)
        {
            out.push_str(MASK);
            i = end + 1;
            continue;
        }
        if text.is_char_boundary(i)
            && let Some(end) = match_encoded_end(bytes, i, secret)
            && text.is_char_boundary(end)
        {
            out.push_str(MASK);
            i = end;
            continue;
        }
        let ch = text[i..].chars().next().expect("valid UTF-8 slice");
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

fn match_encoded_end(bytes: &[u8], start: usize, secret: &[u8]) -> Option<usize> {
    let mut p = start;
    for (idx, &expected) in secret.iter().enumerate() {
        if bytes.get(p) == Some(&b'%')
            && let (Some(&hi), Some(&lo)) = (bytes.get(p + 1), bytes.get(p + 2))
            && let Some(decoded) = hex_pair(hi, lo)
            && decoded == expected
            && !(expected == b'%' && bytes[p..].starts_with(&secret[idx..]))
        {
            p += 3;
            continue;
        }
        if expected == b' ' && bytes.get(p) == Some(&b'+') {
            p += 1;
            continue;
        }
        if bytes.get(p) == Some(&expected) {
            p += 1;
            continue;
        }
        return None;
    }
    Some(p)
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
