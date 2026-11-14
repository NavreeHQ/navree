//! The names CSRF protection goes by, in one place: the session layer
//! (navree-session) sets and checks the token under them, and pages and
//! forms (navree-ui) carry it under them, so the two cannot drift apart. The
//! same goes for the token's masked form: a page writes the token masked afresh
//! in every response ([`page_token`]), and the check takes the token or a mask
//! of it ([`token_matches`]).

/// The cookie holding the CSRF token for scripts (not `HttpOnly`). A script
/// sends its URL-decoded value back as [`XSRF_HEADER`].
pub const XSRF_COOKIE: &str = "XSRF-TOKEN";

/// The header carrying the URL-encoded [`XSRF_COOKIE`] value, as Axios sends
/// it.
pub const XSRF_HEADER: &str = "x-xsrf-token";

/// The header carrying the CSRF token as it is.
pub const CSRF_HEADER: &str = "x-csrf-token";

/// The field a plain HTML form carries the CSRF token in.
pub const CSRF_FIELD: &str = "_csrf";

/// The length of a session's CSRF token: this many base62 characters
/// (`[A-Za-z0-9]`), as navree-session makes it.
pub const TOKEN_LEN: usize = 40;

/// `token` masked, as a page carries it: `base64url(pad || pad XOR token)`
/// (no padding), where the pad is as many random bytes as the token has, drawn
/// afresh on every call. Two masks of one token share nothing, so a body that
/// holds one says nothing a compressed length could give away, and the
/// session's check takes it back with [`unmask_token`]. The token stays valid
/// as long as the session's token does.
///
/// ```
/// use navree_http::csrf::{mask_token, token_matches, unmask_token};
///
/// let token = "q7zk2mvb4xjt3nrcq7zk2mvb4xjt3nrcq7zk2mvb";
/// let (one, two) = (mask_token(token), mask_token(token));
/// assert_ne!(one, two);
/// assert_eq!(unmask_token(&one).as_deref(), Some(token));
/// assert!(token_matches(&two, token) && token_matches(token, token));
/// ```
pub fn mask_token(token: &str) -> String {
    let token = token.as_bytes();
    let pad = random_bytes(token.len());
    let mut bytes = Vec::with_capacity(token.len() * 2);
    bytes.extend_from_slice(&pad);
    bytes.extend(pad.iter().zip(token).map(|(pad, byte)| pad ^ byte));
    base64url(&bytes)
}

/// The token a [`mask_token`] value holds, or `None` for a value that is not
/// one: not base64url, empty, or of an odd number of bytes (a value cut
/// short).
pub fn unmask_token(masked: &str) -> Option<String> {
    String::from_utf8(unmask(masked)?).ok()
}

/// What a page writes for `value`, the session's token: masked
/// ([`mask_token`]) when it is one ([`TOKEN_LEN`] base62 characters), and any
/// other value as it is (a placeholder in a test, or a value masked already).
pub fn page_token(value: &str) -> String {
    let session_token =
        value.len() == TOKEN_LEN && value.bytes().all(|byte| byte.is_ascii_alphanumeric());
    if session_token {
        mask_token(value)
    } else {
        value.to_owned()
    }
}

/// Whether `given` (trimmed), a token a request sent, stands for the
/// session's `token`: it is the token itself (the `XSRF-TOKEN` cookie's value,
/// as a script sends it back) or a mask of it ([`mask_token`], as a page
/// carries it). Both are compared in time that depends only on the lengths.
/// An empty `token` matches nothing.
pub fn token_matches(given: &str, token: &str) -> bool {
    let given = given.trim();
    if token.is_empty() {
        return false;
    }
    if same_bytes(given.as_bytes(), token.as_bytes()) {
        return true;
    }
    let Some(plain) = unmask(given) else {
        return false;
    };
    same_bytes(&plain, token.as_bytes())
}

/// The bytes a masked value holds (see [`unmask_token`]).
fn unmask(masked: &str) -> Option<Vec<u8>> {
    let bytes = from_base64url(masked)?;
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let (pad, sealed) = bytes.split_at(bytes.len() / 2);
    let plain = pad.iter().zip(sealed).map(|(pad, byte)| pad ^ byte);
    Some(plain.collect())
}

/// Whether `a` and `b` are the same bytes, in time that depends only on their
/// lengths.
fn same_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut differ = 0_u8;
    for (a, b) in a.iter().zip(b) {
        differ |= a ^ b;
    }
    std::hint::black_box(differ) == 0
}

/// `len` random bytes: SHA-256 blocks, each of a fresh version 4 UUID (122 bits
/// from the operating system's generator).
fn random_bytes(len: usize) -> Vec<u8> {
    use sha2::{Digest as _, Sha256};

    let mut out = Vec::with_capacity(len + 32);
    while out.len() < len {
        let block = Sha256::digest(uuid::Uuid::new_v4().as_bytes());
        out.extend_from_slice(block.as_slice());
    }
    out.truncate(len);
    out
}

/// The URL-safe base64 alphabet.
const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// `bytes` in URL-safe base64, without padding.
fn base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut group = 0_u32;
        for (at, byte) in chunk.iter().enumerate() {
            group |= u32::from(*byte) << (16 - 8 * at);
        }
        for at in 0..=chunk.len() {
            let index = (group >> (18 - 6 * at)) & 63;
            out.push(char::from(BASE64URL[index as usize]));
        }
    }
    out
}

/// The bytes of URL-safe base64 `text` without padding, or `None` when it has
/// another character, a length no bytes encode to, or bits set past its last
/// byte.
fn from_base64url(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3 + 2);
    let mut bits = 0_u32;
    let mut held = 0_u32;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        bits = ((bits << 6) | u32::from(value)) & 0xffff;
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push(((bits >> held) & 0xff) as u8);
        }
    }
    if held >= 6 || (bits & ((1 << held) - 1)) != 0 {
        return None;
    }
    Some(out)
}

/// Whether a browser's `Origin` names the host the request was sent to: the request's
/// `Host`, else its URI's authority (HTTP/2 and HTTP/3 send `:authority` and may send no
/// `Host`). The hosts are compared without case and the ports as numbers, a scheme's
/// default port (80 for `http`, 443 for `https`) written or not. An `Origin` that is
/// `null`, has another scheme, a path, a user or a port that isn't one, never matches.
///
/// The scheme is not compared: an HTTP/1.1 request doesn't carry it, and behind a proxy
/// that ends TLS the app sees `http` for a page the browser loaded over `https`. So
/// `http://app.example` matches a request to `app.example`. A browser that sends
/// `Sec-Fetch-Site` (every current one) calls such a page `same-site`, not
/// `same-origin`, and the checks that read it refuse it before this comparison.
///
/// ```
/// use http::{HeaderMap, Uri};
/// use navree_http::csrf::origin_is_host;
///
/// let mut headers = HeaderMap::new();
/// headers.insert("host", "App.Example:443".parse().unwrap());
/// let uri = Uri::from_static("/x");
/// assert!(origin_is_host("https://app.example", &headers, &uri));
/// assert!(!origin_is_host("https://app.example.evil.example", &headers, &uri));
/// // HTTP/2: no Host, the URI's authority.
/// let h2 = Uri::from_static("https://app.example/x");
/// assert!(origin_is_host("https://app.example", &HeaderMap::new(), &h2));
/// ```
pub fn origin_is_host(origin: &str, headers: &http::HeaderMap, uri: &http::Uri) -> bool {
    let Some((scheme, authority)) = origin.trim().split_once("://") else {
        return false;
    };
    let default = match scheme.to_ascii_lowercase().as_str() {
        "http" => 80,
        "https" => 443,
        _ => return false,
    };
    if authority.is_empty() || authority.contains(['/', '?', '#', '@']) {
        return false;
    }
    let host = headers
        .get(http::header::HOST)
        .and_then(|host| host.to_str().ok())
        .map(str::to_owned)
        .or_else(|| {
            uri.authority()
                .map(|authority| authority.as_str().to_owned())
        });
    let (Some(origin), Some(target)) = (
        host_and_port(authority, default),
        host.as_deref()
            .and_then(|host| host_and_port(host.trim(), default)),
    ) else {
        return false;
    };
    origin.0.eq_ignore_ascii_case(target.0) && origin.1 == target.1
}

/// `authority` as its host and its port, `default` when none is written; `None` for one
/// that doesn't read as a host and a port.
fn host_and_port(authority: &str, default: u16) -> Option<(&str, u16)> {
    if authority.contains('@') || authority.is_empty() {
        return None;
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = &authority[..end + 2];
        match &rest[end + 1..] {
            "" => (host, None),
            port => (host, Some(port.strip_prefix(':')?)),
        }
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    if host.is_empty() {
        return None;
    }
    let port = match port {
        None => default,
        Some(port) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            port.parse().ok()?
        }
        Some(_) => return None,
    };
    Some((host, port))
}

/// A path whose posts the session layer checks by their origin instead of the token: a
/// route that changes nothing on the server and nothing of the user's, only a preference
/// the browser keeps for itself (a theme cookie), so that a page cached for everyone,
/// which carries no token, can post to it. A module contributes one with
/// `m.contribute(SameOriginOnly("/_navree/ui/appearance"))`; navree-ui's appearance route
/// is one.
///
/// A request with a state-changing method (any of them, not only `POST`) to the path
/// passes when the browser says it came from this origin (`Sec-Fetch-Site: same-origin`),
/// or, from a browser that doesn't say, when its `Origin` is this host
/// ([`origin_is_host`]); one from another site, a sibling subdomain included, is refused
/// with a 403 as a token mismatch is. A request that sends neither header passes only
/// without a session cookie (a script, which has no session to ride on); with one it is
/// refused. The path is matched exactly, and the route should refuse requests from other
/// sites itself as well.
///
/// The check is weaker than the token: it trusts the browser's headers, and so any page
/// of this origin (and any script that can make the browser post from it). Use it only
/// for a route whose worst outcome is a changed preference; the session layer logs every
/// such path on the `navree::security` target when it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SameOriginOnly(pub &'static str);

#[cfg(test)]
mod tests {
    use super::*;

    /// Header names are lower case: `HeaderName::from_static` takes no other.
    #[test]
    fn the_header_names_are_valid_static_header_names() {
        for name in [XSRF_HEADER, CSRF_HEADER] {
            assert_eq!(http::HeaderName::from_static(name).as_str(), name);
        }
    }

    /// A token as navree-session makes one.
    const TOKEN: &str = "Agra4Pune7Kota2Surat9Patna1Ranchi8Mysore";

    #[test]
    fn a_masked_token_unmasks_to_the_token() {
        assert_eq!(TOKEN.len(), TOKEN_LEN);
        for token in [TOKEN, "a", "ab", "abc", "चाय"] {
            let masked = mask_token(token);
            assert_eq!(unmask_token(&masked).as_deref(), Some(token), "{masked}");
            assert!(
                masked
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
                "{masked}"
            );
        }
        // Twice the token's bytes, in base64url: 80 bytes are 107 characters.
        assert_eq!(mask_token(TOKEN).len(), 107);
    }

    #[test]
    fn two_masks_of_one_token_differ() {
        let masks: std::collections::BTreeSet<String> =
            (0..64).map(|_| mask_token(TOKEN)).collect();
        assert_eq!(masks.len(), 64);
        for masked in &masks {
            assert!(!masked.contains(TOKEN), "{masked}");
            assert!(token_matches(masked, TOKEN), "{masked}");
        }
    }

    #[test]
    fn a_cut_short_or_odd_value_unmasks_to_nothing() {
        let masked = mask_token(TOKEN);
        // A character or two less is not a whole mask.
        assert_eq!(unmask_token(&masked[..masked.len() - 1]), None);
        assert_eq!(unmask_token(&masked[..masked.len() - 2]), None);
        // Three bytes in base64url: an odd number.
        assert_eq!(unmask_token("YWJj"), None);
        for value in ["", "=", "YWJj=", "YW Jj", "YW+j", "YW/j", "Y"] {
            assert_eq!(unmask_token(value), None, "{value:?}");
        }
        // Bits set past the last byte: not a value this side wrote.
        assert_eq!(unmask_token("YWJjZB"), None);
        assert_eq!(unmask_token("YWJjZA").as_deref().map(str::len), Some(2));
        // No cut of a mask matches the token.
        for end in 0..masked.len() {
            assert!(!token_matches(&masked[..end], TOKEN), "{end}");
        }
    }

    #[test]
    fn the_token_or_a_mask_of_it_matches_and_nothing_else() {
        assert!(token_matches(TOKEN, TOKEN));
        assert!(token_matches(&format!(" {TOKEN}\n"), TOKEN));
        assert!(token_matches(&mask_token(TOKEN), TOKEN));
        assert!(token_matches(&format!(" {}\t", mask_token(TOKEN)), TOKEN));
        let other = "Delhi4Agra7Kota2Surat9Patna1Ranchi8Mysore";
        assert!(!token_matches(other, TOKEN));
        assert!(!token_matches(&mask_token(other), TOKEN));
        assert!(!token_matches(&TOKEN[..39], TOKEN));
        assert!(!token_matches(&mask_token(&TOKEN[..39]), TOKEN));
        assert!(!token_matches("", TOKEN));
        // A session without a token has nothing to match.
        assert!(!token_matches("", ""));
        assert!(!token_matches(&mask_token(""), ""));
    }

    #[test]
    fn a_page_masks_a_session_token_and_writes_any_other_value_as_it_is() {
        let written = page_token(TOKEN);
        assert_ne!(written, TOKEN);
        assert_eq!(unmask_token(&written).as_deref(), Some(TOKEN));
        assert_ne!(page_token(TOKEN), written, "a fresh mask every time");
        for value in ["tok", "session-token", &TOKEN[..39], written.as_str()] {
            assert_eq!(page_token(value), value);
        }
    }

    #[test]
    fn base64url_reads_back_what_it_writes() {
        for (bytes, text) in [
            (&b""[..], ""),
            (&b"f"[..], "Zg"),
            (&b"fo"[..], "Zm8"),
            (&b"foo"[..], "Zm9v"),
            (&b"foob"[..], "Zm9vYg"),
            (&b"\xfb\xff\xbf"[..], "-_-_"),
        ] {
            assert_eq!(base64url(bytes), text);
            assert_eq!(from_base64url(text).as_deref(), Some(bytes));
        }
    }
}
