//! SAML 2.0 Web Browser SSO, the service provider's side.
//!
//! - [`IdpMetadata`] reads an identity provider's metadata: its entity id,
//!   sign-on URLs and signing certificates.
//! - [`ServiceProvider`] is Navree's side of one connection (entity id and
//!   assertion consumer service URL) and writes its metadata.
//! - [`AuthnRequest`] is the sign-in request, sent with the HTTP-Redirect
//!   binding.
//! - [`validate_response`] checks a `SAMLResponse` posted to the assertion
//!   consumer service and returns the [`SamlAssertion`].
//!
//! [`SsoModule`](crate::SsoModule) wires these into routes; they are public
//! so an app can test an identity provider's responses directly.

mod metadata;
mod request;
mod response;

use navree_datetime::{DateTime, Timestamp};

pub(crate) use metadata::certificate_der;
pub use metadata::{IdpMetadata, ServiceProvider};
pub use request::{AuthnRequest, RequestOptions};
#[cfg(feature = "fuzzing")]
pub(crate) use response::validate_xml_unsigned;
pub use response::{ResponseCheck, SamlAssertion, validate_response};

use crate::dsig::SignatureError;
use crate::xml::XmlError;

/// The SAML 2.0 assertion namespace.
pub const ASSERTION_NS: &str = "urn:oasis:names:tc:SAML:2.0:assertion";
/// The SAML 2.0 protocol namespace.
pub const PROTOCOL_NS: &str = "urn:oasis:names:tc:SAML:2.0:protocol";
/// The SAML 2.0 metadata namespace.
pub const METADATA_NS: &str = "urn:oasis:names:tc:SAML:2.0:metadata";
/// The HTTP-Redirect binding.
pub const BINDING_REDIRECT: &str = "urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect";
/// The HTTP-POST binding.
pub const BINDING_POST: &str = "urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST";
/// The status of a successful response.
pub const STATUS_SUCCESS: &str = "urn:oasis:names:tc:SAML:2.0:status:Success";
/// The bearer subject confirmation method.
pub const CM_BEARER: &str = "urn:oasis:names:tc:SAML:2.0:cm:bearer";

/// Why a SAML message or metadata document was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SamlError {
    /// The message is not valid XML, or not XML Navree accepts.
    #[error(transparent)]
    Xml(#[from] XmlError),
    /// The signature is missing or does not verify.
    #[error(transparent)]
    Signature(#[from] SignatureError),
    /// The message is not base64, not a SAML response, or not shaped as
    /// one.
    #[error("invalid SAML message: {0}")]
    Invalid(String),
    /// The identity provider answered with an error status.
    #[error("the identity provider refused the sign-in ({code}{})", message.as_deref().map(|m| format!(": {m}")).unwrap_or_default())]
    Status {
        /// The status code (and the second-level code after a `/`).
        code: String,
        /// The status message, if any.
        message: Option<String>,
    },
    /// The assertion is encrypted; Navree reads plain assertions over TLS.
    #[error(
        "encrypted assertions are not supported; turn assertion encryption off at the identity provider"
    )]
    EncryptedAssertion,
    /// The issuer is not the connection's identity provider.
    #[error("the response comes from `{0}`, not the connection's identity provider")]
    WrongIssuer(String),
    /// `Destination` or the bearer `Recipient` is another URL.
    #[error("the response is addressed to `{0}`, not this connection's assertion consumer service")]
    WrongRecipient(String),
    /// The audience restriction does not name this service provider.
    #[error("the assertion is meant for another service provider")]
    WrongAudience,
    /// The assertion or its subject confirmation has expired.
    #[error("the assertion has expired")]
    Expired,
    /// The assertion is not valid yet.
    #[error("the assertion is not valid yet")]
    NotYetValid,
    /// `InResponseTo` names another request.
    #[error("the response answers another request")]
    InResponseToMismatch,
    /// A response nobody asked for, and IdP-initiated sign-in is off.
    #[error("unsolicited responses (IdP-initiated sign-in) are not allowed")]
    Unsolicited,
    /// The metadata cannot be used.
    #[error("invalid identity provider metadata: {0}")]
    Metadata(String),
}

/// Parses an `xs:dateTime` in UTC (`YYYY-MM-DDTHH:MM:SS`, optional fractional
/// digits, optional trailing `Z`; a missing zone is read as UTC).
pub(crate) fn parse_instant(value: &str) -> Result<Timestamp, SamlError> {
    if !has_saml_instant_shape(value) {
        return Err(SamlError::Invalid(format!("invalid time `{value}`")));
    }
    if value.ends_with('Z') {
        return Timestamp::parse_rfc3339(value)
            .map_err(|_| SamlError::Invalid(format!("invalid time `{value}`")));
    }
    DateTime::parse("%Y-%m-%dT%H:%M:%S%.f", value)
        .map(DateTime::in_utc)
        .map_err(|_| SamlError::Invalid(format!("invalid time `{value}`")))
}

fn has_saml_instant_shape(value: &str) -> bool {
    let b = value.as_bytes();
    if b.len() < 19 {
        return false;
    }
    let fixed_ok = b[0..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit)
        && b[10] == b'T'
        && b[11..13].iter().all(u8::is_ascii_digit)
        && b[13] == b':'
        && b[14..16].iter().all(u8::is_ascii_digit)
        && b[16] == b':'
        && b[17..19].iter().all(u8::is_ascii_digit)
        && &b[17..19] != b"60";
    if !fixed_ok {
        return false;
    }
    let rest = &b[19..];
    let rest = rest.strip_suffix(b"Z").unwrap_or(rest);
    match rest.strip_prefix(b".") {
        Some(frac) => !frac.is_empty() && frac.iter().all(u8::is_ascii_digit),
        None => rest.is_empty(),
    }
}

/// Formats an instant as SAML writes it (`2026-10-27T12:00:00Z`).
pub(crate) fn format_instant(instant: Timestamp) -> String {
    instant.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Cache lifetime for a consumed assertion id: covers `not_on_or_after + skew`
/// plus two whole seconds so subsecond truncation never drops the entry early.
pub(crate) fn assertion_replay_ttl(
    not_on_or_after: Timestamp,
    now: Timestamp,
    skew: std::time::Duration,
) -> std::time::Duration {
    let remaining = not_on_or_after
        .duration_since(now)
        .checked_add(navree_datetime::Span::try_from(skew).unwrap_or(navree_datetime::Span::ZERO))
        .unwrap_or(navree_datetime::Span::MAX);
    std::time::Duration::from_secs(
        u64::try_from(remaining.as_seconds())
            .unwrap_or(0)
            .saturating_add(2),
    )
}

/// A fresh message id: `_` and 32 random letters and digits (an XML
/// NCName).
pub(crate) fn new_id() -> String {
    format!("_{}", navree_crypto::random_token(32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instants_parse_with_or_without_fractions_and_zone() {
        let expected = Timestamp::parse_rfc3339("2026-10-27T12:00:00Z").unwrap();
        assert_eq!(parse_instant("2026-10-27T12:00:00Z").unwrap(), expected);
        assert_eq!(parse_instant("2026-10-27T12:00:00.000Z").unwrap(), expected);
        assert_eq!(parse_instant("2026-10-27T12:00:00").unwrap(), expected);
        assert_eq!(
            parse_instant("2026-10-27T12:00:00.456").unwrap(),
            Timestamp::parse_rfc3339("2026-10-27T12:00:00.456Z").unwrap()
        );
        for invalid in [
            "yesterday",
            "2026-10-27T14:00:00+02:00",
            "2026-10-27T12:00:00+00:00",
            "2026-10-27T12:00:00-00:00",
            " 2026-10-27T12:00:00Z",
            "2026-10-27T12:00:00Z ",
            "2026-10-27 12:00:00Z",
            "2026-10-27t12:00:00Z",
            "2026-10-27T12:00:00z",
            "+02026-10-27T12:00:00Z",
            "-2026-10-27T12:00:00Z",
            "2026-10-27T12:00:00.",
            "2026-10-27T12:00:00.Z",
            "2026-10-27T23:59:60Z",
        ] {
            assert!(parse_instant(invalid).is_err(), "{invalid}");
        }
        assert_eq!(format_instant(expected), "2026-10-27T12:00:00Z");
    }

    #[test]
    fn assertion_replay_ttl_covers_subsecond_remainder() {
        let until = parse_instant("2026-10-27T12:05:00.900Z").unwrap();
        let now = parse_instant("2026-10-27T12:05:00.100Z").unwrap();
        assert_eq!(
            assertion_replay_ttl(until, now, std::time::Duration::ZERO),
            std::time::Duration::from_secs(2)
        );
        let past_until = parse_instant("2026-10-27T12:04:00.900Z").unwrap();
        assert_eq!(
            assertion_replay_ttl(past_until, now, std::time::Duration::from_secs(60)),
            std::time::Duration::from_secs(2)
        );
    }

    #[test]
    fn ids_are_ncnames() {
        let id = new_id();
        assert!(id.starts_with('_'));
        assert_eq!(id.len(), 33);
        assert_ne!(id, new_id());
    }
}
