//! Token material: opaque tokens with their prefixes, SHA-256 digests,
//! PKCE (S256), device user codes and redirect URI matching.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use navree_crypto::{constant_time_eq, random_token, sha256_hex};
use rand::seq::IndexedRandom as _;
use sha2::{Digest as _, Sha256};

/// The prefix of OAuth access tokens.
pub const ACCESS_TOKEN_PREFIX: &str = "nva_";
/// The prefix of refresh tokens.
pub const REFRESH_TOKEN_PREFIX: &str = "nvr_";
/// The prefix of authorization codes.
pub const CODE_PREFIX: &str = "nvc_";
/// The prefix of device codes.
pub const DEVICE_CODE_PREFIX: &str = "nvd_";
/// The prefix of personal access tokens.
pub const PERSONAL_ACCESS_TOKEN_PREFIX: &str = "nvp_";
/// The prefix of client secrets.
pub const CLIENT_SECRET_PREFIX: &str = "nvcs_";

/// Random base62 characters after a token's prefix (about 238 bits).
pub const TOKEN_LEN: usize = 40;

/// The characters of device user codes: consonants only, so codes are
/// easy to read and type and never spell words (RFC 8628 §6.1).
const USER_CODE_ALPHABET: &[u8] = b"BCDFGHJKLMNPQRSTVWXZ";

/// The length of a user code, without its dash (`WDJB-MJHT`).
pub const USER_CODE_LEN: usize = 8;

/// A new opaque token: `prefix` plus [`TOKEN_LEN`] random characters.
pub(crate) fn new_token(prefix: &str) -> String {
    format!("{prefix}{}", random_token(TOKEN_LEN))
}

/// How a token is stored: its SHA-256 digest in hex.
pub fn digest(token: &str) -> String {
    sha256_hex(token.as_bytes())
}

/// The S256 code challenge of `verifier`: `BASE64URL(SHA256(verifier))`.
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Whether `verifier` is a valid PKCE code verifier (RFC 7636 §4.1: 43 to
/// 128 characters of `[A-Za-z0-9-._~]`) whose S256 challenge is
/// `challenge`.
pub(crate) fn pkce_matches(verifier: &str, challenge: &str) -> bool {
    is_pkce_value(verifier)
        && constant_time_eq(pkce_challenge(verifier).as_bytes(), challenge.as_bytes())
}

/// Whether `value` has the PKCE verifier (and S256 challenge) syntax.
pub(crate) fn is_pkce_value(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
}

/// A new device user code, normalized (8 letters, no dash).
pub(crate) fn new_user_code() -> String {
    let mut rng = rand::rng();
    (0..USER_CODE_LEN)
        .map(|_| char::from(*USER_CODE_ALPHABET.choose(&mut rng).expect("not empty")))
        .collect()
}

/// A user code as people type it, normalized: upper case, without dashes
/// or spaces.
pub fn normalize_user_code(code: &str) -> String {
    code.chars()
        .filter(|c| !matches!(c, '-' | ' '))
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// A normalized user code as it is shown: `WDJB-MJHT`.
pub fn display_user_code(code: &str) -> String {
    if code.len() == USER_CODE_LEN && code.is_ascii() {
        format!("{}-{}", &code[..4], &code[4..])
    } else {
        code.to_owned()
    }
}

/// Checks a redirect URI a client registers: an absolute URI without a
/// fragment; `https`, `http` only for loopback hosts (`127.0.0.1`,
/// `[::1]`, `localhost`), or a private-use scheme of a native app
/// (`com.example.app:/callback`).
pub(crate) fn check_redirect_uri(uri: &str) -> Result<(), &'static str> {
    let parsed = url::Url::parse(uri).map_err(|_| "must be an absolute URI")?;
    if parsed.fragment().is_some() {
        return Err("must not contain a fragment");
    }
    match parsed.scheme() {
        "https" => {
            if parsed.host().is_none() {
                return Err("must name a host");
            }
        }
        "http" => {
            if !is_loopback(&parsed) {
                return Err("must use https (http is allowed for 127.0.0.1, [::1] and localhost)");
            }
        }
        "javascript" | "data" | "file" | "vbscript" => return Err("uses a forbidden scheme"),
        scheme if !scheme.contains('.') => {
            return Err(
                "must use https, a loopback http URI or a reverse-domain scheme (com.example.app:/callback)",
            );
        }
        _ => {}
    }
    Ok(())
}

/// Whether the presented `redirect_uri` matches the registered one:
/// exactly, except that the port of a loopback `http` URI may differ
/// (native apps listen on an ephemeral port, RFC 8252 §7.3).
pub(crate) fn redirect_uri_matches(registered: &str, presented: &str) -> bool {
    if registered == presented {
        return true;
    }
    let (Ok(mut registered), Ok(mut presented)) =
        (url::Url::parse(registered), url::Url::parse(presented))
    else {
        return false;
    };
    let loopback_ip = |url: &url::Url| {
        url.scheme() == "http"
            && matches!(
                url.host(),
                Some(url::Host::Ipv4(ip)) if ip.is_loopback()
            )
            || url.scheme() == "http"
                && matches!(url.host(), Some(url::Host::Ipv6(ip)) if ip.is_loopback())
    };
    if !loopback_ip(&registered) || !loopback_ip(&presented) {
        return false;
    }
    let _ = registered.set_port(None);
    let _ = presented.set_port(None);
    registered == presented
}

fn is_loopback(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

/// `uri` with the query parameters `params` appended (`?` or `&`).
pub(crate) fn with_query(uri: &str, params: &[(&str, &str)]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (name, value) in params {
        serializer.append_pair(name, value);
    }
    let query = serializer.finish();
    if query.is_empty() {
        return uri.to_owned();
    }
    let (base, fragment) = match uri.split_once('#') {
        Some((base, fragment)) => (base, Some(fragment)),
        None => (uri, None),
    };
    let separator = if base.contains('?') {
        if base.ends_with('?') || base.ends_with('&') {
            ""
        } else {
            "&"
        }
    } else {
        "?"
    };
    match fragment {
        Some(fragment) => format!("{base}{separator}{query}#{fragment}"),
        None => format!("{base}{separator}{query}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_have_prefixes_and_are_stored_as_digests() {
        let token = new_token(ACCESS_TOKEN_PREFIX);
        assert!(token.starts_with("nva_"));
        assert_eq!(token.len(), 4 + TOKEN_LEN);
        assert_ne!(token, new_token(ACCESS_TOKEN_PREFIX));
        assert_eq!(digest(&token).len(), 64);
        assert_eq!(digest(&token), digest(&token));
    }

    #[test]
    fn pkce_uses_s256() {
        // RFC 7636 Appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert_eq!(pkce_challenge(verifier), challenge);
        assert!(pkce_matches(verifier, challenge));
        assert!(!pkce_matches(
            verifier,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cX"
        ));
        assert!(!pkce_matches("short", &pkce_challenge("short")));
        let long = "a".repeat(129);
        assert!(!pkce_matches(&long, &pkce_challenge(&long)));
        assert!(!is_pkce_value(&"a b".repeat(20)));
    }

    #[test]
    fn user_codes_are_easy_to_type() {
        let code = new_user_code();
        assert_eq!(code.len(), USER_CODE_LEN);
        assert!(code.bytes().all(|b| USER_CODE_ALPHABET.contains(&b)));
        assert_eq!(normalize_user_code("wdjb-mjht"), "WDJBMJHT");
        assert_eq!(normalize_user_code(" WDJB MJHT "), "WDJBMJHT");
        assert_eq!(display_user_code("WDJBMJHT"), "WDJB-MJHT");
    }

    #[test]
    fn redirect_uris_are_checked() {
        for good in [
            "https://zapier.com/dashboard/auth/oauth/return/AcmeCRMCLIAPI/",
            "http://127.0.0.1/callback",
            "http://[::1]:8080/cb",
            "http://localhost:3000/cb",
            "com.acme.crm:/oauth/callback",
        ] {
            assert!(check_redirect_uri(good).is_ok(), "{good}");
        }
        for bad in [
            "/callback",
            "http://example.com/cb",
            "https://example.com/cb#frag",
            "javascript:alert(1)",
            "myapp:/cb",
        ] {
            assert!(check_redirect_uri(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn redirect_uris_match_exactly_except_loopback_ports() {
        let registered = "https://app.example.com/cb";
        assert!(redirect_uri_matches(registered, registered));
        assert!(!redirect_uri_matches(
            registered,
            "https://app.example.com/cb/"
        ));
        assert!(!redirect_uri_matches(
            registered,
            "https://app.example.com/cb?x=1"
        ));
        assert!(!redirect_uri_matches(
            registered,
            "https://APP.example.com:443/cb2"
        ));
        assert!(redirect_uri_matches(
            "http://127.0.0.1/callback",
            "http://127.0.0.1:51234/callback"
        ));
        assert!(redirect_uri_matches(
            "http://[::1]:8080/callback",
            "http://[::1]:9999/callback"
        ));
        assert!(!redirect_uri_matches(
            "http://127.0.0.1/callback",
            "http://127.0.0.1:51234/other"
        ));
        // `localhost` is a name, not a loopback literal: no port exception.
        assert!(!redirect_uri_matches(
            "http://localhost/callback",
            "http://localhost:51234/callback"
        ));
    }

    #[test]
    fn query_parameters_are_appended() {
        assert_eq!(
            with_query("https://x.test/cb", &[("code", "a b"), ("state", "s")]),
            "https://x.test/cb?code=a+b&state=s"
        );
        assert_eq!(
            with_query("https://x.test/cb?x=1", &[("code", "c")]),
            "https://x.test/cb?x=1&code=c"
        );
        assert_eq!(with_query("https://x.test/cb", &[]), "https://x.test/cb");
    }
}
