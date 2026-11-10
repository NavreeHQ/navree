//! Reading a WebSocket handshake: RFC 6455 §4.2.1 over HTTP/1.1, RFC 8441
//! over HTTP/2. Nothing here touches the connection.

use http::header::{self, HeaderMap, HeaderName, HeaderValue};
use http::request::Parts;
use http::{Method, Version};
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;

use super::WebSocketRejection;

/// What a well-formed handshake says.
#[derive(Clone, Debug)]
pub struct Handshake {
    // `None` over HTTP/2, which has no key.
    key: Option<HeaderValue>,
    protocols: Vec<HeaderValue>,
    origin: Option<HeaderValue>,
    host: Option<String>,
}

impl Handshake {
    /// Reads and checks the handshake in a request head.
    pub fn read(parts: &Parts) -> Result<Self, WebSocketRejection> {
        let key = match parts.version {
            Version::HTTP_09 | Version::HTTP_10 => return Err(WebSocketRejection::HttpVersion),
            Version::HTTP_11 => Some(read_http1(parts)?),
            _ => {
                read_extended_connect(parts)?;
                None
            }
        };
        if !one_value_is(&parts.headers, &header::SEC_WEBSOCKET_VERSION, b"13") {
            return Err(WebSocketRejection::Version);
        }
        let host = parts
            .headers
            .get(header::HOST)
            .and_then(|host| host.to_str().ok())
            .or_else(|| parts.uri.authority().map(|authority| authority.as_str()));
        Ok(Self {
            key,
            protocols: offered_protocols(&parts.headers),
            origin: parts.headers.get(header::ORIGIN).cloned(),
            host: host.map(str::to_owned),
        })
    }

    /// The `Sec-WebSocket-Accept` that answers this handshake; `None` over
    /// HTTP/2, which has none.
    pub fn accept_key(&self) -> Option<HeaderValue> {
        let key = self.key.as_ref()?;
        HeaderValue::try_from(derive_accept_key(key.as_bytes())).ok()
    }

    /// The subprotocols offered, in the client's order, without the empty
    /// items of its list.
    pub fn requested_protocols(&self) -> &[HeaderValue] {
        &self.protocols
    }

    /// The `Origin` header, when it is text.
    pub fn origin(&self) -> Option<&str> {
        self.origin.as_ref()?.to_str().ok()
    }

    /// The host the client connected to: `Host`, or the authority of an
    /// HTTP/2 request.
    pub fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }
}

/// Checks an HTTP/1.1 handshake and returns its key.
fn read_http1(parts: &Parts) -> Result<HeaderValue, WebSocketRejection> {
    if parts.method != Method::GET {
        return Err(WebSocketRejection::MethodNotGet);
    }
    if !has_token(&parts.headers, &header::CONNECTION, "upgrade") {
        return Err(WebSocketRejection::Connection);
    }
    if !has_token(&parts.headers, &header::UPGRADE, "websocket") {
        return Err(WebSocketRejection::Upgrade);
    }
    let mut keys = parts.headers.get_all(header::SEC_WEBSOCKET_KEY).iter();
    match (keys.next(), keys.next()) {
        (None, _) => Err(WebSocketRejection::KeyMissing),
        (Some(key), None) if valid_key(key.as_bytes()) => Ok(key.clone()),
        _ => Err(WebSocketRejection::Key),
    }
}

/// Checks an HTTP/2 handshake: `CONNECT` with `:protocol` `websocket`.
fn read_extended_connect(parts: &Parts) -> Result<(), WebSocketRejection> {
    if parts.method != Method::CONNECT {
        return Err(WebSocketRejection::MethodNotConnect);
    }
    let protocol = parts.extensions.get::<hyper::ext::Protocol>();
    if protocol.is_none_or(|protocol| protocol.as_str() != "websocket") {
        return Err(WebSocketRejection::Protocol);
    }
    Ok(())
}

/// Whether `key` is the base64 of 16 bytes: 22 characters whose last holds
/// only two bits of the value, then `==`.
fn valid_key(key: &[u8]) -> bool {
    let [value @ .., last, b'=', b'='] = key else {
        return false;
    };
    value.len() == 21
        && value
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
        && matches!(last, b'A' | b'Q' | b'g' | b'w')
}

/// Whether the header appears once, with exactly `value`.
fn one_value_is(headers: &HeaderMap, name: &HeaderName, value: &[u8]) -> bool {
    let mut values = headers.get_all(name).iter();
    matches!(
        (values.next(), values.next()),
        (Some(first), None) if first.as_bytes() == value
    )
}

/// Whether a comma-separated header, over all its lines, has the token
/// (any case).
fn has_token(headers: &HeaderMap, name: &HeaderName, token: &str) -> bool {
    headers.get_all(name).iter().any(|value| {
        value
            .as_bytes()
            .split(|byte| *byte == b',')
            .any(|item| item.trim_ascii().eq_ignore_ascii_case(token.as_bytes()))
    })
}

/// The items of every `Sec-WebSocket-Protocol` line.
fn offered_protocols(headers: &HeaderMap) -> Vec<HeaderValue> {
    headers
        .get_all(header::SEC_WEBSOCKET_PROTOCOL)
        .iter()
        .flat_map(|value| value.as_bytes().split(|byte| *byte == b','))
        .map(<[u8]>::trim_ascii)
        .filter(|item| !item.is_empty())
        .filter_map(|item| HeaderValue::from_bytes(item).ok())
        .collect()
}

/// Whether a page from `origin` may open a socket to `host`: with no
/// `Origin` (not a browser), an origin in `allowed` (or a `*` there), or the
/// server's own origin. A trailing `/` and the case of an origin do not
/// matter, and the server's own origin is compared by authority, so a port
/// has to match as the browser wrote it.
pub fn origin_allowed<S: AsRef<str>>(
    origin: Option<&str>,
    host: Option<&str>,
    allowed: &[S],
) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let origin = origin.trim().trim_end_matches('/');
    if allowed.iter().any(|allowed| {
        let allowed = allowed.as_ref();
        allowed == "*"
            || allowed
                .trim()
                .trim_end_matches('/')
                .eq_ignore_ascii_case(origin)
    }) {
        return true;
    }
    match (origin.split_once("://"), host) {
        (Some((_, authority)), Some(host)) => authority.eq_ignore_ascii_case(host.trim()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use http::Request;
    use http::StatusCode;

    use super::*;

    const KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";

    fn http1(headers: &[(&'static str, &str)]) -> Parts {
        let mut request = Request::get("/ws").version(Version::HTTP_11);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        request.body(()).unwrap().into_parts().0
    }

    fn valid() -> Vec<(&'static str, &'static str)> {
        vec![
            ("connection", "Upgrade"),
            ("upgrade", "websocket"),
            ("sec-websocket-version", "13"),
            ("sec-websocket-key", KEY),
        ]
    }

    fn http2(method: Method, protocol: Option<&'static str>) -> Parts {
        let mut request = Request::builder()
            .method(method)
            .uri("https://app.example.com/ws")
            .version(Version::HTTP_2)
            .header("sec-websocket-version", "13")
            .body(())
            .unwrap();
        if let Some(protocol) = protocol {
            request
                .extensions_mut()
                .insert(hyper::ext::Protocol::from_static(protocol));
        }
        request.into_parts().0
    }

    #[test]
    fn a_good_handshake_reads() {
        let handshake = Handshake::read(&http1(&valid())).unwrap();
        // RFC 6455, section 1.3.
        assert_eq!(
            handshake.accept_key().unwrap(),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
        assert!(handshake.requested_protocols().is_empty());
        assert_eq!(handshake.origin(), None);
    }

    #[test]
    fn keys_are_the_base64_of_16_bytes() {
        assert!(valid_key(KEY.as_bytes()));
        assert!(valid_key(b"x3JJHMbDL1EzLkh9GBhXDw=="));
        assert!(valid_key(b"AAAAAAAAAAAAAAAAAAAAAA=="));
        assert!(valid_key(b"+/+/+/+/+/+/+/+/+/+/+w=="));
        for bad in [
            &b""[..],
            b"==",
            b"dGhlIHNhbXBsZSBub25jZQ",
            b"dGhlIHNhbXBsZSBub25jZQ=",
            b"dGhlIHNhbXBsZSBub25jZQ===",
            b"dGhlIHNhbXBsZSBub25jZR==",
            b"dGhlIHNhbXBsZSBub25j-Q==",
            b"dGhlIHNhbXBsZSBub25j_Q==",
            b"dGhlIHNhbXBsZSBub25jZQ=A",
            b"dGhlIHNhbXBsZSBub25jZSBib2R5IGFkZGVk",
        ] {
            assert!(!valid_key(bad), "{}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn each_rejection_has_its_status_and_message() {
        let with = |name: &'static str, value: &'static str| {
            let mut headers = valid();
            headers.retain(|(other, _)| *other != name);
            if !value.is_empty() {
                headers.push((name, value));
            }
            Handshake::read(&http1(&headers)).unwrap_err()
        };
        let table = [
            (
                with("connection", ""),
                WebSocketRejection::Connection,
                StatusCode::BAD_REQUEST,
                "Connection header did not include 'upgrade'",
            ),
            (
                with("upgrade", "h2c"),
                WebSocketRejection::Upgrade,
                StatusCode::BAD_REQUEST,
                "`Upgrade` header did not include 'websocket'",
            ),
            (
                with("sec-websocket-version", "8"),
                WebSocketRejection::Version,
                StatusCode::BAD_REQUEST,
                "`Sec-WebSocket-Version` header did not include '13'",
            ),
            (
                with("sec-websocket-key", ""),
                WebSocketRejection::KeyMissing,
                StatusCode::BAD_REQUEST,
                "`Sec-WebSocket-Key` header missing",
            ),
            (
                with("sec-websocket-key", "short"),
                WebSocketRejection::Key,
                StatusCode::BAD_REQUEST,
                "`Sec-WebSocket-Key` must be the base64 of 16 bytes",
            ),
        ];
        for (rejection, expected, status, message) in table {
            assert_eq!(rejection, expected);
            assert_eq!(rejection.status(), status);
            assert_eq!(rejection.body_text(), message);
            assert_eq!(rejection.to_string(), message);
        }
    }

    #[test]
    fn the_version_rejection_names_the_version_this_server_speaks() {
        let error = WebSocketRejection::Version.to_app_error();
        assert_eq!(error.headers[header::SEC_WEBSOCKET_VERSION], "13");
        let error = WebSocketRejection::Key.to_app_error();
        assert!(error.headers.is_empty());
    }

    #[test]
    fn an_http1_upgrade_must_be_a_get_over_http11() {
        let mut parts = http1(&valid());
        parts.method = Method::POST;
        assert_eq!(
            Handshake::read(&parts).unwrap_err(),
            WebSocketRejection::MethodNotGet
        );
        assert_eq!(
            WebSocketRejection::MethodNotGet.status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        parts.method = Method::CONNECT;
        assert_eq!(
            Handshake::read(&parts).unwrap_err(),
            WebSocketRejection::MethodNotGet
        );
        let mut parts = http1(&valid());
        parts.version = Version::HTTP_10;
        assert_eq!(
            Handshake::read(&parts).unwrap_err(),
            WebSocketRejection::HttpVersion
        );
    }

    #[test]
    fn http2_uses_extended_connect() {
        let handshake = Handshake::read(&http2(Method::CONNECT, Some("websocket"))).unwrap();
        assert_eq!(handshake.accept_key(), None);
        assert_eq!(handshake.host(), Some("app.example.com"));
        assert_eq!(
            Handshake::read(&http2(Method::GET, Some("websocket"))).unwrap_err(),
            WebSocketRejection::MethodNotConnect
        );
        assert_eq!(
            Handshake::read(&http2(Method::CONNECT, None)).unwrap_err(),
            WebSocketRejection::Protocol
        );
        assert_eq!(
            Handshake::read(&http2(Method::CONNECT, Some("webtransport"))).unwrap_err(),
            WebSocketRejection::Protocol
        );
    }

    #[test]
    fn subprotocols_are_read_from_every_line() {
        let mut headers = valid();
        headers.push(("sec-websocket-protocol", "a, b ,, "));
        headers.push(("sec-websocket-protocol", "c"));
        let handshake = Handshake::read(&http1(&headers)).unwrap();
        assert_eq!(handshake.requested_protocols(), ["a", "b", "c"]);
    }

    #[test]
    fn origins() {
        let allowed = ["https://app.example.com".to_owned()];
        let check = |origin, host| origin_allowed(origin, host, &allowed);
        // Not a browser.
        assert!(check(None, None));
        assert!(check(Some("https://app.example.com"), None));
        assert!(check(Some("https://APP.example.com/"), None));
        // The server's own pages.
        assert!(check(
            Some("https://own.example.com"),
            Some("own.example.com")
        ));
        assert!(check(Some("http://localhost:3000"), Some("localhost:3000")));
        // Everyone else, however close.
        for origin in [
            "https://evil.example",
            "null",
            "",
            "https://app.example.com.evil.example",
            "https://evil.example/app.example.com",
            "https://app.example.com@evil.example",
            "http://app.example.com",
        ] {
            assert!(!check(Some(origin), Some("own.example.com")), "{origin}");
        }
        assert!(!check(Some("https://localhost:3000"), Some("localhost")));
        assert!(origin_allowed(Some("https://x.example"), None, &["*"]));
    }

    #[test]
    fn the_upgrade_keeps_the_origin_and_host_of_its_request() {
        let mut headers = valid();
        headers.push(("origin", "https://evil.example"));
        headers.push(("host", "own.example.com"));
        let handshake = Handshake::read(&http1(&headers)).unwrap();
        assert_eq!(handshake.origin(), Some("https://evil.example"));
        assert_eq!(handshake.host(), Some("own.example.com"));
    }
}
