//! [`IncomingRequest`]: what a verifier sees of an incoming webhook;
//! [`Verdict`] and [`Handshake`]: what it decides.

use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use http::{HeaderMap, StatusCode, Uri};
use navree_kernel::AppContext;

use crate::keys::SenderKeys;
use crate::options::SourceOptions;
use crate::verify::{Tolerance, Verified, VerifyError};

/// An incoming webhook as a verifier sees it: the source's options, the
/// headers and raw body, the public URL the sender called, the time it
/// arrived and the [`SenderKeys`] that fetch the keys some senders sign
/// with.
///
/// [`VerifiedWebhook`](crate::VerifiedWebhook) builds one per request. Build
/// one yourself to call a verifier directly, in a test for instance:
///
/// ```
/// use http::HeaderMap;
/// use navree_webhooks::{IncomingRequest, SourceOptions};
///
/// let source = SourceOptions::new("client-state");
/// let headers = HeaderMap::new();
/// let request = IncomingRequest::new(&source, &headers, b"")
///     .with_url("https://api.example.com/api/hooks/graph?validationToken=abc");
/// assert_eq!(request.query("validationToken").as_deref(), Some("abc"));
/// ```
pub struct IncomingRequest<'a> {
    source: &'a SourceOptions,
    headers: &'a HeaderMap,
    body: &'a [u8],
    url: String,
    now: DateTime<Utc>,
    tolerance: Option<Tolerance>,
    keys: Option<Arc<SenderKeys>>,
    app: Option<AppContext>,
}

impl<'a> IncomingRequest<'a> {
    /// A request for `source` with `headers` and `body`, arriving now at
    /// `http://localhost/`, with no tolerance (no age check).
    pub fn new(source: &'a SourceOptions, headers: &'a HeaderMap, body: &'a [u8]) -> Self {
        Self {
            source,
            headers,
            body,
            url: "http://localhost/".to_owned(),
            now: Utc::now(),
            tolerance: None,
            keys: None,
            app: None,
        }
    }

    /// The request arrived at `url`: the full public URL the sender called,
    /// query string included.
    #[must_use]
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    /// The request arrived at `now` (a test clock's time).
    #[must_use]
    pub fn at(mut self, now: DateTime<Utc>) -> Self {
        self.now = now;
        self
    }

    /// Signed timestamps must be within `tolerance`.
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = Some(tolerance);
        self
    }

    /// Keys are fetched (and cached) by `keys`.
    #[must_use]
    pub fn with_keys(mut self, keys: Arc<SenderKeys>) -> Self {
        self.keys = Some(keys);
        self
    }

    /// Keys come from the app's [`SenderKeys`].
    pub(crate) fn with_app(mut self, app: AppContext) -> Self {
        self.app = Some(app);
        self
    }

    /// The source's options (`webhooks.incoming.<source>`).
    pub fn source(&self) -> &SourceOptions {
        self.source
    }

    /// The request headers.
    pub fn headers(&self) -> &HeaderMap {
        self.headers
    }

    /// The value of header `name`, if present and text.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    /// The raw body.
    pub fn body(&self) -> &[u8] {
        self.body
    }

    /// The public URL the sender called, query string included.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The first value of query parameter `name`, decoded.
    pub fn query(&self, name: &str) -> Option<String> {
        let (_, query) = self.url.split_once('?')?;
        let query = query.split('#').next().unwrap_or_default();
        form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    }

    /// When the request arrived.
    pub fn now(&self) -> DateTime<Utc> {
        self.now
    }

    /// The accepted age of signed timestamps (`None`: no age check).
    pub fn tolerance(&self) -> Option<Tolerance> {
        self.tolerance
    }

    /// The [`SenderKeys`] to fetch keys with: the one given to
    /// [`with_keys`](IncomingRequest::with_keys), else the app's (provided
    /// by `WebhooksModule`).
    pub async fn keys(&self) -> Result<Arc<SenderKeys>, VerifyError> {
        if let Some(keys) = &self.keys {
            return Ok(keys.clone());
        }
        match &self.app {
            Some(app) => app.get::<SenderKeys>().await.map_err(|_| {
                VerifyError::Misconfigured("SenderKeys needs WebhooksModule in the app".to_owned())
            }),
            None => Err(VerifyError::Misconfigured(
                "this source fetches keys: give the request SenderKeys".to_owned(),
            )),
        }
    }
}

impl fmt::Debug for IncomingRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IncomingRequest")
            .field("url", &self.url)
            .field("now", &self.now)
            .field("tolerance", &self.tolerance)
            .field("body_bytes", &self.body.len())
            .finish_non_exhaustive()
    }
}

/// What a verifier decided about an authentic request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// An authentic delivery: the handler runs.
    Verified(Verified),
    /// A handshake the sender expects a fixed answer to (Microsoft Graph's
    /// `validationToken`, Slack's `url_verification` challenge, an AWS SNS
    /// subscription confirmation): it is answered without running the
    /// handler.
    Handshake(Handshake),
}

/// The answer to a handshake: a status and a `text/plain` body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handshake {
    /// The response status.
    pub status: StatusCode,
    /// The response body (`text/plain`; empty for none).
    pub body: String,
}

impl Handshake {
    /// `200 OK` with `body` as `text/plain`.
    pub fn text(body: impl Into<String>) -> Self {
        Self {
            status: StatusCode::OK,
            body: body.into(),
        }
    }

    /// `200 OK` with no body.
    pub fn ok() -> Self {
        Self::text(String::new())
    }
}

/// The public URL of a request: `url` (a source's configured URL) followed
/// by the request's query string, else rebuilt from `X-Forwarded-Proto`,
/// `X-Forwarded-Host` and `Host`, and the request target.
pub(crate) fn public_url(configured: Option<&str>, headers: &HeaderMap, uri: &Uri) -> String {
    let query = uri.query().map(|query| format!("?{query}")).unwrap_or_default();
    if let Some(base) = configured.map(str::trim).filter(|base| !base.is_empty()) {
        return format!("{base}{query}");
    }
    let first = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let scheme = first("x-forwarded-proto")
        .or(uri.scheme_str())
        .unwrap_or("http");
    let host = first("x-forwarded-host")
        .or_else(|| first("host"))
        .or(uri.authority().map(|authority| authority.as_str()))
        .unwrap_or("localhost");
    format!("{scheme}://{host}{}{query}", uri.path())
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;

    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(*name, HeaderValue::from_str(value).unwrap());
        }
        map
    }

    #[test]
    fn the_public_url_prefers_the_configured_one_and_keeps_the_query() {
        let uri: Uri = "/api/hooks/twilio?tenant=7".parse().unwrap();
        assert_eq!(
            public_url(
                Some("https://api.example.com/hooks/twilio"),
                &headers(&[("host", "internal:3000")]),
                &uri
            ),
            "https://api.example.com/hooks/twilio?tenant=7"
        );
        assert_eq!(
            public_url(None, &headers(&[("host", "api.example.com")]), &uri),
            "http://api.example.com/api/hooks/twilio?tenant=7"
        );
        assert_eq!(
            public_url(
                None,
                &headers(&[
                    ("host", "internal:3000"),
                    ("x-forwarded-proto", "https, http"),
                    ("x-forwarded-host", "api.example.com"),
                ]),
                &"/api/hooks/twilio".parse().unwrap()
            ),
            "https://api.example.com/api/hooks/twilio"
        );
        assert_eq!(
            public_url(None, &HeaderMap::new(), &"/x".parse().unwrap()),
            "http://localhost/x"
        );
    }

    #[test]
    fn query_parameters_are_decoded() {
        let source = SourceOptions::new("s");
        let headers = HeaderMap::new();
        let request = IncomingRequest::new(&source, &headers, b"")
            .with_url("https://a.example/hook?validationToken=Validation%3A+Testing&x=1#frag");
        assert_eq!(
            request.query("validationToken").as_deref(),
            Some("Validation: Testing")
        );
        assert_eq!(request.query("x").as_deref(), Some("1"));
        assert_eq!(request.query("missing"), None);
        assert_eq!(
            IncomingRequest::new(&source, &headers, b"").query("x"),
            None
        );
    }

    #[test]
    fn handshakes_answer_text() {
        assert_eq!(Handshake::text("abc").body, "abc");
        assert_eq!(Handshake::ok().status, StatusCode::OK);
        assert!(Handshake::ok().body.is_empty());
    }
}
