//! The authentication request (`AuthnRequest`) and the HTTP-Redirect
//! binding.

use std::io::{Read as _, Write as _};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

use super::{
    ASSERTION_NS, BINDING_POST, IdpMetadata, PROTOCOL_NS, SamlError, ServiceProvider,
    format_instant, new_id,
};
use crate::xml::escape;

/// The largest inflated request [`AuthnRequest::decode_redirect`] reads.
const MAX_INFLATED: u64 = 64 * 1024;

/// A SAML authentication request, sent with the HTTP-Redirect binding: the
/// browser goes to [`url`](AuthnRequest::url), and the identity provider
/// answers at the assertion consumer service with a response whose
/// `InResponseTo` is [`id`](AuthnRequest::id).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthnRequest {
    /// The request id.
    pub id: String,
    /// The identity provider's sign-on URL with `SAMLRequest` (deflated,
    /// base64) and `RelayState`.
    pub url: String,
}

/// Options of an [`AuthnRequest`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RequestOptions {
    /// Ask the identity provider to authenticate the user again even with
    /// a session (`ForceAuthn`).
    pub force_authn: bool,
    /// The name id format to ask for (`NameIDPolicy Format`); the identity
    /// provider's default when `None`.
    pub name_id_format: Option<String>,
}

impl AuthnRequest {
    /// A request from `sp` to `idp`, carrying `relay_state` through the
    /// sign-in. [`SamlError::Metadata`] when the identity provider has no
    /// HTTP-Redirect sign-on URL.
    pub fn new(
        sp: &ServiceProvider,
        idp: &IdpMetadata,
        relay_state: &str,
        now: DateTime<Utc>,
        options: &RequestOptions,
    ) -> Result<AuthnRequest, SamlError> {
        let destination = idp.sso_redirect_url.as_deref().ok_or_else(|| {
            SamlError::Metadata(
                "the identity provider has no HTTP-Redirect single sign-on URL".to_owned(),
            )
        })?;
        let id = new_id();
        let xml = request_xml(&id, sp, destination, now, options);
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(xml.as_bytes())
            .and_then(|()| encoder.finish())
            .map(|deflated| {
                let mut url = url::Url::parse(destination).map_err(|_| {
                    SamlError::Metadata(format!("invalid single sign-on URL `{destination}`"))
                })?;
                url.query_pairs_mut()
                    .append_pair("SAMLRequest", &STANDARD.encode(deflated))
                    .append_pair("RelayState", relay_state);
                Ok(AuthnRequest {
                    id,
                    url: url.to_string(),
                })
            })
            .map_err(|error| SamlError::Invalid(format!("cannot deflate the request: {error}")))?
    }

    /// The request XML and the relay state of an HTTP-Redirect URL (what an
    /// identity provider reads; [`SsoFake`](crate::SsoFake) uses it).
    pub fn decode_redirect(url: &str) -> Result<(String, Option<String>), SamlError> {
        let url = url::Url::parse(url).map_err(|_| SamlError::Invalid("not a URL".to_owned()))?;
        let mut request = None;
        let mut relay_state = None;
        for (name, value) in url.query_pairs() {
            match name.as_ref() {
                "SAMLRequest" => request = Some(value.into_owned()),
                "RelayState" => relay_state = Some(value.into_owned()),
                _ => {}
            }
        }
        let request = request.ok_or_else(|| SamlError::Invalid("no SAMLRequest".to_owned()))?;
        let deflated = STANDARD
            .decode(request.trim())
            .map_err(|_| SamlError::Invalid("SAMLRequest is not base64".to_owned()))?;
        let mut xml = String::new();
        DeflateDecoder::new(deflated.as_slice())
            .take(MAX_INFLATED)
            .read_to_string(&mut xml)
            .map_err(|_| SamlError::Invalid("SAMLRequest is not deflated XML".to_owned()))?;
        Ok((xml, relay_state))
    }
}

fn request_xml(
    id: &str,
    sp: &ServiceProvider,
    destination: &str,
    now: DateTime<Utc>,
    options: &RequestOptions,
) -> String {
    let force = if options.force_authn {
        " ForceAuthn=\"true\""
    } else {
        ""
    };
    let format = options
        .name_id_format
        .as_deref()
        .map(|format| format!(" Format=\"{}\"", escape(format)))
        .unwrap_or_default();
    format!(
        "<samlp:AuthnRequest xmlns:samlp=\"{PROTOCOL_NS}\" xmlns:saml=\"{ASSERTION_NS}\" \
         ID=\"{id}\" Version=\"2.0\" IssueInstant=\"{instant}\" Destination=\"{destination}\" \
         AssertionConsumerServiceURL=\"{acs}\" ProtocolBinding=\"{BINDING_POST}\"{force}>\
         <saml:Issuer>{issuer}</saml:Issuer>\
         <samlp:NameIDPolicy{format} AllowCreate=\"true\"/>\
         </samlp:AuthnRequest>",
        instant = format_instant(now),
        destination = escape(destination),
        acs = escape(&sp.acs_url),
        issuer = escape(&sp.entity_id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::Document;

    fn sp() -> ServiceProvider {
        ServiceProvider {
            entity_id: "https://api.acme.test/api/sso/saml/acme/metadata".to_owned(),
            acs_url: "https://api.acme.test/api/sso/saml/acme/acs".to_owned(),
        }
    }

    fn idp() -> IdpMetadata {
        IdpMetadata::new(
            "https://idp.acme.test",
            "https://idp.acme.test/sso?tenant=1",
            &[include_str!("../fake/idp-cert.pem")],
        )
        .unwrap()
    }

    #[test]
    fn the_redirect_url_carries_the_deflated_request_and_relay_state() {
        let now = DateTime::parse_from_rfc3339("2026-10-27T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let options = RequestOptions {
            force_authn: true,
            name_id_format: Some(
                "urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress".to_owned(),
            ),
        };
        let request = AuthnRequest::new(&sp(), &idp(), "state&1", now, &options).unwrap();
        assert!(
            request
                .url
                .starts_with("https://idp.acme.test/sso?tenant=1&SAMLRequest=")
        );
        let (xml, relay_state) = AuthnRequest::decode_redirect(&request.url).unwrap();
        assert_eq!(relay_state.as_deref(), Some("state&1"));
        let doc = Document::parse(&xml).unwrap();
        let root = doc.root();
        assert!(doc.is(root, PROTOCOL_NS, "AuthnRequest"));
        assert_eq!(doc.attr(root, "ID"), Some(request.id.as_str()));
        assert_eq!(doc.attr(root, "IssueInstant"), Some("2026-10-27T12:00:00Z"));
        assert_eq!(
            doc.attr(root, "Destination"),
            Some("https://idp.acme.test/sso?tenant=1")
        );
        assert_eq!(
            doc.attr(root, "AssertionConsumerServiceURL"),
            Some(sp().acs_url.as_str())
        );
        assert_eq!(doc.attr(root, "ForceAuthn"), Some("true"));
        let issuer = doc.child(root, ASSERTION_NS, "Issuer").unwrap();
        assert_eq!(doc.text(issuer), sp().entity_id);
        let policy = doc.child(root, PROTOCOL_NS, "NameIDPolicy").unwrap();
        assert_eq!(
            doc.attr(policy, "Format"),
            Some("urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress")
        );
    }

    #[test]
    fn an_identity_provider_without_a_redirect_url_is_refused() {
        let mut idp = idp();
        idp.sso_redirect_url = None;
        assert!(matches!(
            AuthnRequest::new(&sp(), &idp, "s", Utc::now(), &RequestOptions::default()),
            Err(SamlError::Metadata(_))
        ));
    }

    #[test]
    fn decoding_refuses_garbage() {
        assert!(AuthnRequest::decode_redirect("https://idp/sso").is_err());
        assert!(AuthnRequest::decode_redirect("https://idp/sso?SAMLRequest=%%%").is_err());
        assert!(AuthnRequest::decode_redirect("https://idp/sso?SAMLRequest=aGVsbG8=").is_err());
    }
}
