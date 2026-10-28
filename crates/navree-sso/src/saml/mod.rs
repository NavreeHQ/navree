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

use chrono::{DateTime, NaiveDateTime, Utc};

pub(crate) use metadata::certificate_der;
pub use metadata::{IdpMetadata, ServiceProvider};
pub use request::{AuthnRequest, RequestOptions};
pub use response::{ResponseCheck, SamlAssertion, validate_response};
#[cfg(feature = "fuzzing")]
pub(crate) use response::validate_xml_unsigned;

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

/// Parses an `xs:dateTime` (UTC; a missing zone is read as UTC).
pub(crate) fn parse_instant(value: &str) -> Result<DateTime<Utc>, SamlError> {
    let value = value.trim();
    if let Ok(instant) = DateTime::parse_from_rfc3339(value) {
        return Ok(instant.with_timezone(&Utc));
    }
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
        .map(|naive| naive.and_utc())
        .map_err(|_| SamlError::Invalid(format!("invalid time `{value}`")))
}

/// Formats an instant as SAML writes it (`2026-10-27T12:00:00Z`).
pub(crate) fn format_instant(instant: DateTime<Utc>) -> String {
    instant.format("%Y-%m-%dT%H:%M:%SZ").to_string()
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
        let expected = DateTime::parse_from_rfc3339("2026-10-27T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(parse_instant("2026-10-27T12:00:00Z").unwrap(), expected);
        assert_eq!(parse_instant("2026-10-27T12:00:00.000Z").unwrap(), expected);
        assert_eq!(parse_instant("2026-10-27T12:00:00").unwrap(), expected);
        assert_eq!(
            parse_instant("2026-10-27T14:00:00+02:00").unwrap(),
            expected
        );
        assert!(parse_instant("yesterday").is_err());
        assert_eq!(format_instant(expected), "2026-10-27T12:00:00Z");
    }

    #[test]
    fn ids_are_ncnames() {
        let id = new_id();
        assert!(id.starts_with('_'));
        assert_eq!(id.len(), 33);
        assert_ne!(id, new_id());
    }
}
