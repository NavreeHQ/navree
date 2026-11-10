//! Transports that send through an email provider's HTTP API: SES,
//! Mailgun, Postmark and Resend.

pub(crate) mod mailgun;
pub(crate) mod postmark;
pub(crate) mod resend;
pub(crate) mod ses;

use navree_http_client::{HttpClient, HttpClientOptions, Response};
use serde_json::Value;
use url::Url;

use crate::MailError;
use crate::message::mailbox;

/// An HTTP client for a provider: the default SSRF policy, with the host
/// of an explicitly configured `endpoint` allowed (a local mock server, a
/// private relay).
pub(crate) fn client_for(endpoint: Option<&Url>) -> Result<HttpClient, MailError> {
    let mut options = HttpClientOptions {
        retries: 0,
        ..HttpClientOptions::default()
    };
    if let Some(host) = endpoint.and_then(Url::host_str) {
        options.ssrf.allow_hosts.push(host.to_owned());
    }
    HttpClient::new(options)
        .map_err(|error| MailError::Config(format!("cannot build the HTTP client: {error}")))
}

/// Parses an `endpoint` setting (`https://api.example.com`), or `default`.
pub(crate) fn endpoint(
    configured: Option<&str>,
    default: &str,
    transport: &str,
) -> Result<Url, MailError> {
    let raw = configured.filter(|e| !e.is_empty()).unwrap_or(default);
    let url = Url::parse(raw.trim_end_matches('/')).map_err(|error| {
        MailError::Config(format!(
            "invalid `endpoint` for the {transport} transport: {error}"
        ))
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(MailError::Config(format!(
            "the {transport} transport's `endpoint` must be an http(s) URL"
        )));
    }
    Ok(url)
}

/// `base` joined with `path` (`/v3/acme.test/messages`).
pub(crate) fn join(base: &Url, path: &str) -> Result<Url, MailError> {
    let joined = format!("{}{path}", base.as_str().trim_end_matches('/'));
    Url::parse(&joined).map_err(|error| MailError::Config(format!("invalid API URL: {error}")))
}

/// `address` normalized for an API: `Name <user@host>` with the name quoted
/// when needed.
pub(crate) fn address(field: &'static str, address: &str) -> Result<String, MailError> {
    Ok(mailbox(field, address)?.to_string())
}

/// Every address of `addresses`, normalized.
pub(crate) fn addresses(field: &'static str, list: &[String]) -> Result<Vec<String>, MailError> {
    list.iter().map(|a| address(field, a)).collect()
}

/// The transport error for a request that did not get an answer.
pub(crate) fn unreachable(
    transport: &'static str,
    error: navree_http_client::HttpClientError,
) -> MailError {
    MailError::Transport {
        transport,
        source: Box::new(error),
    }
}

/// The provider's refusal: its status and its own message (`message`,
/// `Message` or `error`), never the request.
pub(crate) fn refusal(transport: &'static str, response: &Response) -> MailError {
    let message = response
        .json::<Value>()
        .ok()
        .and_then(|body| {
            ["message", "Message", "error", "name"]
                .iter()
                .find_map(|key| body.get(*key).and_then(Value::as_str).map(str::to_owned))
        })
        .unwrap_or_else(|| {
            let text = response.text();
            let text = text.trim();
            if text.is_empty() {
                response
                    .status()
                    .canonical_reason()
                    .unwrap_or("no message")
                    .to_owned()
            } else {
                text.chars().take(300).collect()
            }
        });
    MailError::Provider {
        transport,
        status: response.status().as_u16(),
        message,
    }
}

/// The `usable` error for a provider that refused a read-only call.
pub(crate) fn unusable(transport: &str, response: &Response) -> MailError {
    let status = response.status().as_u16();
    let reason = match status {
        401 | 403 => "the provider refused the credentials".to_owned(),
        404 => "the provider does not know this domain or server".to_owned(),
        _ => format!("the provider answered {status}"),
    };
    MailError::Unusable(format!("{transport}: {reason}"))
}

/// A `Basic` authorization header value.
pub(crate) fn basic(user: &str, password: &str) -> String {
    use base64::Engine as _;
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
    )
}

/// A percent-decoded part of a shorthand URL (user name, password).
pub(crate) fn decoded(text: &str) -> String {
    percent_encoding::percent_decode_str(text)
        .decode_utf8_lossy()
        .into_owned()
}
