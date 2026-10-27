//! Identity provider metadata (read) and service provider metadata
//! (written).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use navree_crypto::jose::VerifyingKey;

use super::{BINDING_POST, BINDING_REDIRECT, METADATA_NS, SamlError};
use crate::dsig::DSIG_NS;
use crate::xml::{Document, NodeId, escape};

/// What Navree needs from an identity provider's metadata: who it is,
/// where to send users and which keys sign its responses.
///
/// [`parse`](IdpMetadata::parse) reads an `EntityDescriptor`, or the first
/// identity provider of an `EntitiesDescriptor`. The metadata's own
/// signature is not checked: trust comes from where the document came
/// from (an `https` metadata URL, or XML an administrator pasted).
#[derive(Clone, Debug)]
pub struct IdpMetadata {
    /// The identity provider's entity id (the `Issuer` of its responses).
    pub entity_id: String,
    /// The single sign-on URL of the HTTP-Redirect binding.
    pub sso_redirect_url: Option<String>,
    /// The single sign-on URL of the HTTP-POST binding.
    pub sso_post_url: Option<String>,
    /// The signing certificates (DER), in document order.
    pub certificates: Vec<Vec<u8>>,
    /// The public keys of [`certificates`](IdpMetadata::certificates).
    pub signing_keys: Vec<VerifyingKey>,
    /// Whether the identity provider wants signed requests.
    pub want_authn_requests_signed: bool,
}

impl IdpMetadata {
    /// Metadata from its parts: the entity id, the HTTP-Redirect sign-on
    /// URL and signing certificates as PEM or base64 DER.
    pub fn new(
        entity_id: impl Into<String>,
        sso_redirect_url: impl Into<String>,
        certificates: &[&str],
    ) -> Result<IdpMetadata, SamlError> {
        let mut metadata = IdpMetadata {
            entity_id: entity_id.into(),
            sso_redirect_url: Some(sso_redirect_url.into()),
            sso_post_url: None,
            certificates: Vec::new(),
            signing_keys: Vec::new(),
            want_authn_requests_signed: false,
        };
        for certificate in certificates {
            metadata.add_certificate(&certificate_der(certificate)?)?;
        }
        if metadata.signing_keys.is_empty() {
            return Err(SamlError::Metadata("no signing certificate".to_owned()));
        }
        Ok(metadata)
    }

    /// Reads metadata XML.
    pub fn parse(xml: &str) -> Result<IdpMetadata, SamlError> {
        let doc = Document::parse(xml)?;
        let root = doc.root();
        let entity = if doc.is(root, METADATA_NS, "EntityDescriptor") {
            root
        } else if doc.is(root, METADATA_NS, "EntitiesDescriptor") {
            doc.descendants(root)
                .into_iter()
                .find(|e| {
                    doc.is(*e, METADATA_NS, "EntityDescriptor")
                        && doc.child(*e, METADATA_NS, "IDPSSODescriptor").is_some()
                })
                .ok_or_else(|| {
                    SamlError::Metadata("no identity provider in the metadata".to_owned())
                })?
        } else {
            return Err(SamlError::Metadata(
                "the document is not SAML metadata".to_owned(),
            ));
        };
        let entity_id = doc
            .attr(entity, "entityID")
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| SamlError::Metadata("the entity has no entityID".to_owned()))?
            .trim()
            .to_owned();
        let idp = doc
            .child(entity, METADATA_NS, "IDPSSODescriptor")
            .ok_or_else(|| {
                SamlError::Metadata("the entity is not an identity provider".to_owned())
            })?;
        let mut metadata = IdpMetadata {
            entity_id,
            sso_redirect_url: None,
            sso_post_url: None,
            certificates: Vec::new(),
            signing_keys: Vec::new(),
            want_authn_requests_signed: doc.attr(idp, "WantAuthnRequestsSigned") == Some("true"),
        };
        for service in doc.children_named(idp, METADATA_NS, "SingleSignOnService") {
            let location = doc.attr(service, "Location").map(str::to_owned);
            match doc.attr(service, "Binding") {
                Some(BINDING_REDIRECT) if metadata.sso_redirect_url.is_none() => {
                    metadata.sso_redirect_url = location;
                }
                Some(BINDING_POST) if metadata.sso_post_url.is_none() => {
                    metadata.sso_post_url = location;
                }
                _ => {}
            }
        }
        for descriptor in doc.children_named(idp, METADATA_NS, "KeyDescriptor") {
            if doc.attr(descriptor, "use") == Some("encryption") {
                continue;
            }
            for certificate in certificates_in(&doc, descriptor) {
                metadata.add_certificate(&certificate)?;
            }
        }
        if metadata.signing_keys.is_empty() {
            return Err(SamlError::Metadata(
                "the identity provider has no signing certificate".to_owned(),
            ));
        }
        Ok(metadata)
    }

    fn add_certificate(&mut self, der: &[u8]) -> Result<(), SamlError> {
        if self.certificates.iter().any(|known| known == der) {
            return Ok(());
        }
        let key = VerifyingKey::from_certificate_der(der).map_err(|error| {
            SamlError::Metadata(format!("unusable signing certificate: {error}"))
        })?;
        self.certificates.push(der.to_vec());
        self.signing_keys.push(key);
        Ok(())
    }
}

/// The `X509Certificate`s under `node`, decoded.
fn certificates_in(doc: &Document, node: NodeId) -> Vec<Vec<u8>> {
    doc.descendants(node)
        .into_iter()
        .filter(|e| doc.is(*e, DSIG_NS, "X509Certificate"))
        .filter_map(|e| {
            let text: String = doc
                .text(e)
                .chars()
                .filter(|c| !c.is_ascii_whitespace())
                .collect();
            STANDARD.decode(text).ok()
        })
        .collect()
}

/// A certificate given as PEM or as base64 DER (what metadata and admin
/// screens carry).
pub(crate) fn certificate_der(text: &str) -> Result<Vec<u8>, SamlError> {
    let body: String = text
        .replace("\\n", "\n")
        .lines()
        .filter(|line| !line.trim_start().starts_with("-----"))
        .flat_map(|line| line.chars())
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    STANDARD
        .decode(body)
        .map_err(|_| SamlError::Metadata("the certificate is not PEM or base64".to_owned()))
}

/// Navree's side of a SAML connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceProvider {
    /// The service provider's entity id (the audience of assertions):
    /// by default the connection's metadata URL.
    pub entity_id: String,
    /// The assertion consumer service URL (HTTP-POST).
    pub acs_url: String,
}

impl ServiceProvider {
    /// The service provider metadata to give the identity provider: the
    /// entity id, the assertion consumer service (HTTP-POST) and a request
    /// for signed assertions.
    pub fn metadata_xml(&self) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <md:EntityDescriptor xmlns:md=\"{METADATA_NS}\" entityID=\"{entity}\">\
             <md:SPSSODescriptor AuthnRequestsSigned=\"false\" WantAssertionsSigned=\"true\" \
             protocolSupportEnumeration=\"urn:oasis:names:tc:SAML:2.0:protocol\">\
             <md:NameIDFormat>urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress</md:NameIDFormat>\
             <md:NameIDFormat>urn:oasis:names:tc:SAML:2.0:nameid-format:persistent</md:NameIDFormat>\
             <md:AssertionConsumerService Binding=\"{BINDING_POST}\" Location=\"{acs}\" index=\"0\" isDefault=\"true\"/>\
             </md:SPSSODescriptor>\
             </md:EntityDescriptor>",
            entity = escape(&self.entity_id),
            acs = escape(&self.acs_url),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CERT: &str = include_str!("../fake/idp-cert.pem");
    const EC_CERT: &str = include_str!("../../tests/fixtures/ec-cert.pem");
    const WEAK_CERT: &str = include_str!("../../tests/fixtures/weak-cert.pem");

    fn base64_cert(pem: &str) -> String {
        pem.lines().filter(|l| !l.starts_with("-----")).collect()
    }

    fn metadata(body: &str) -> String {
        format!(
            r#"<md:EntityDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata" xmlns:ds="http://www.w3.org/2000/09/xmldsig#" entityID="https://idp.acme.test/saml"><md:IDPSSODescriptor WantAuthnRequestsSigned="false" protocolSupportEnumeration="urn:oasis:names:tc:SAML:2.0:protocol">{body}</md:IDPSSODescriptor></md:EntityDescriptor>"#
        )
    }

    fn key_descriptor(usage: Option<&str>, pem: &str) -> String {
        let usage = usage.map(|u| format!(" use=\"{u}\"")).unwrap_or_default();
        format!(
            "<md:KeyDescriptor{usage}><ds:KeyInfo><ds:X509Data><ds:X509Certificate>\n{}\n</ds:X509Certificate></ds:X509Data></ds:KeyInfo></md:KeyDescriptor>",
            base64_cert(pem)
        )
    }

    const SSO: &str = r#"<md:SingleSignOnService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST" Location="https://idp.acme.test/sso/post"/><md:SingleSignOnService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect" Location="https://idp.acme.test/sso/redirect"/>"#;

    #[test]
    fn metadata_gives_the_entity_endpoints_and_signing_keys() {
        let xml = metadata(&format!(
            "{}{}{}{SSO}",
            key_descriptor(Some("signing"), CERT),
            key_descriptor(Some("encryption"), WEAK_CERT),
            key_descriptor(None, EC_CERT),
        ));
        let idp = IdpMetadata::parse(&xml).unwrap();
        assert_eq!(idp.entity_id, "https://idp.acme.test/saml");
        assert_eq!(
            idp.sso_redirect_url.as_deref(),
            Some("https://idp.acme.test/sso/redirect")
        );
        assert_eq!(
            idp.sso_post_url.as_deref(),
            Some("https://idp.acme.test/sso/post")
        );
        // The encryption certificate (a weak key) is skipped.
        assert_eq!(idp.signing_keys.len(), 2);
        assert!(!idp.want_authn_requests_signed);
    }

    #[test]
    fn an_entities_descriptor_gives_its_identity_provider() {
        let inner = metadata(&format!("{}{SSO}", key_descriptor(None, CERT)));
        let xml = format!(
            r#"<md:EntitiesDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata"><md:EntityDescriptor entityID="https://sp.other.test"><md:SPSSODescriptor protocolSupportEnumeration="x"/></md:EntityDescriptor>{inner}</md:EntitiesDescriptor>"#
        );
        assert_eq!(
            IdpMetadata::parse(&xml).unwrap().entity_id,
            "https://idp.acme.test/saml"
        );
    }

    #[test]
    fn unusable_metadata_is_refused_with_a_reason() {
        for (xml, reason) in [
            (metadata(SSO), "no signing certificate"),
            (
                metadata(&format!("{}{SSO}", key_descriptor(None, WEAK_CERT))),
                "unusable signing certificate",
            ),
            ("<a/>".to_owned(), "not SAML metadata"),
            (
                r#"<md:EntityDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata" entityID="x"/>"#
                    .to_owned(),
                "not an identity provider",
            ),
        ] {
            let error = IdpMetadata::parse(&xml).unwrap_err().to_string();
            assert!(error.contains(reason), "{error}");
        }
    }

    #[test]
    fn metadata_from_parts_accepts_pem_and_base64() {
        let idp = IdpMetadata::new(
            "https://idp.acme.test",
            "https://idp.acme.test/sso",
            &[CERT, &base64_cert(EC_CERT)],
        )
        .unwrap();
        assert_eq!(idp.signing_keys.len(), 2);
        assert!(IdpMetadata::new("x", "y", &[]).is_err());
        assert!(IdpMetadata::new("x", "y", &["not base64!"]).is_err());
    }

    #[test]
    fn service_provider_metadata_names_the_acs() {
        let sp = ServiceProvider {
            entity_id: "https://api.acme.test/api/sso/saml/acme/metadata".to_owned(),
            acs_url: "https://api.acme.test/api/sso/saml/acme/acs?a=1&b=2".to_owned(),
        };
        let xml = sp.metadata_xml();
        let doc = Document::parse(&xml).unwrap();
        assert_eq!(
            doc.attr(doc.root(), "entityID"),
            Some(sp.entity_id.as_str())
        );
        let acs = doc
            .elements()
            .into_iter()
            .find(|e| doc.is(*e, METADATA_NS, "AssertionConsumerService"))
            .unwrap();
        assert_eq!(doc.attr(acs, "Location"), Some(sp.acs_url.as_str()));
        assert_eq!(doc.attr(acs, "Binding"), Some(BINDING_POST));
    }
}
