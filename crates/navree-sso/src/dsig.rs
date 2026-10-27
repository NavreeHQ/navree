//! Enveloped XML signatures (XML Signature 1.1), as SAML uses them.
//!
//! [`verify_enveloped`] checks the `ds:Signature` child of one element
//! against keys the app trusts (never a key the message carries):
//!
//! - exactly one `Reference`, whose URI is `#` plus the enveloping
//!   element's `ID`, with the transforms `enveloped-signature` then a
//!   canonicalization (no XPath, XSLT or other transforms);
//! - canonicalization: exclusive (with its prefix list) or inclusive
//!   Canonical XML 1.0;
//! - signatures RSA-SHA256/384/512 or ECDSA-SHA256/384 and digests
//!   SHA-256/384/512, through `ring` ([`navree_crypto::jose`]). SHA-1
//!   signatures and digests are refused ([`SignatureError::InsecureAlgorithm`]).
//!
//! What a valid signature covers is the element it encloses, so callers
//! read that element (by its node) and nothing else: the defense against
//! signature wrapping. [`check_unique_ids`] refuses documents in which two
//! elements share an id.
//!
//! [`sign_enveloped`] signs an element (exclusive canonicalization,
//! RSA-SHA256 or ECDSA, SHA-256): the test identity provider of
//! [`SsoFake`](crate::SsoFake) signs with it.

use std::collections::BTreeSet;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use navree_crypto::jose::{Algorithm, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256, Sha384, Sha512};

use crate::xml::c14n::{self, Method};
use crate::xml::{Document, NodeId, XmlError, escape};

/// The XML Signature namespace.
pub(crate) const DSIG_NS: &str = "http://www.w3.org/2000/09/xmldsig#";
/// The exclusive canonicalization namespace (`InclusiveNamespaces`).
const EXC_C14N_NS: &str = "http://www.w3.org/2001/10/xml-exc-c14n#";

const ENVELOPED: &str = "http://www.w3.org/2000/09/xmldsig#enveloped-signature";

const RSA_SHA256: &str = "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256";
const RSA_SHA384: &str = "http://www.w3.org/2001/04/xmldsig-more#rsa-sha384";
const RSA_SHA512: &str = "http://www.w3.org/2001/04/xmldsig-more#rsa-sha512";
const ECDSA_SHA256: &str = "http://www.w3.org/2001/04/xmldsig-more#ecdsa-sha256";
const ECDSA_SHA384: &str = "http://www.w3.org/2001/04/xmldsig-more#ecdsa-sha384";

const SHA256: &str = "http://www.w3.org/2001/04/xmlenc#sha256";
const SHA384: &str = "http://www.w3.org/2001/04/xmldsig-more#sha384";
/// What some signers write for SHA-384 (not in RFC 6931, but unambiguous).
const SHA384_XMLENC: &str = "http://www.w3.org/2001/04/xmlenc#sha384";
const SHA512: &str = "http://www.w3.org/2001/04/xmlenc#sha512";

/// SHA-1 algorithms, refused by name.
const INSECURE: &[&str] = &[
    "http://www.w3.org/2000/09/xmldsig#rsa-sha1",
    "http://www.w3.org/2000/09/xmldsig#dsa-sha1",
    "http://www.w3.org/2001/04/xmldsig-more#ecdsa-sha1",
    "http://www.w3.org/2000/09/xmldsig#hmac-sha1",
    "http://www.w3.org/2000/09/xmldsig#sha1",
    "http://www.w3.org/2001/04/xmldsig-more#rsa-md5",
    "http://www.w3.org/2001/04/xmldsig-more#md5",
];

/// Why a signature was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SignatureError {
    /// The message is not signed where a signature is required.
    #[error("the message is not signed")]
    Missing,
    /// The signature's structure is not what an enveloped signature is.
    #[error("malformed signature: {0}")]
    Malformed(String),
    /// A SHA-1 (or older) signature or digest.
    #[error("insecure signature algorithm `{0}`")]
    InsecureAlgorithm(String),
    /// An algorithm or transform outside the supported set.
    #[error("unsupported signature algorithm `{0}`")]
    UnsupportedAlgorithm(String),
    /// The reference does not point at the element that encloses the
    /// signature.
    #[error("the signature does not cover the element it is in")]
    WrongReference,
    /// Two elements share an id (a signature wrapping attempt).
    #[error("duplicate id `{0}`")]
    DuplicateId(String),
    /// The signed content was changed.
    #[error("the digest does not match the signed content")]
    DigestMismatch,
    /// No trusted key verifies the signature.
    #[error("the signature does not verify with a trusted key")]
    BadSignature,
}

fn malformed(message: impl Into<String>) -> SignatureError {
    SignatureError::Malformed(message.into())
}

/// The id of an element (`ID`, or `Id`).
pub(crate) fn element_id(doc: &Document, element: NodeId) -> Option<&str> {
    doc.attr(element, "ID").or_else(|| doc.attr(element, "Id"))
}

/// Refuses documents in which two elements carry the same `ID`, `Id` or
/// `AssertionID` value.
pub(crate) fn check_unique_ids(doc: &Document) -> Result<(), SignatureError> {
    let mut seen = BTreeSet::new();
    for element in doc.elements() {
        for name in ["ID", "Id", "AssertionID"] {
            if let Some(id) = doc.attr(element, name)
                && !seen.insert(id.to_owned())
            {
                return Err(SignatureError::DuplicateId(id.to_owned()));
            }
        }
    }
    Ok(())
}

/// The `Algorithm` attribute of `id`, which must be there.
fn algorithm(doc: &Document, id: NodeId) -> Result<&str, SignatureError> {
    doc.attr(id, "Algorithm")
        .ok_or_else(|| malformed("an Algorithm attribute is missing"))
}

fn refuse_insecure(uri: &str) -> Result<(), SignatureError> {
    if INSECURE.contains(&uri) {
        return Err(SignatureError::InsecureAlgorithm(uri.to_owned()));
    }
    Ok(())
}

/// A canonicalization method and its prefix list.
fn c14n_of(doc: &Document, id: NodeId) -> Result<(Method, Vec<String>), SignatureError> {
    let uri = algorithm(doc, id)?;
    let method = Method::from_uri(uri)
        .ok_or_else(|| SignatureError::UnsupportedAlgorithm(uri.to_owned()))?;
    let prefixes = match doc.child(id, EXC_C14N_NS, "InclusiveNamespaces") {
        Some(list) if method.exclusive => doc
            .attr(list, "PrefixList")
            .unwrap_or_default()
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    };
    Ok((method, prefixes))
}

/// Decodes base64 that may be wrapped over lines.
fn base64_of(doc: &Document, id: NodeId) -> Result<Vec<u8>, SignatureError> {
    let text: String = doc
        .text(id)
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    STANDARD
        .decode(text)
        .map_err(|_| malformed("a value is not base64"))
}

/// The one child `{DSIG_NS}local` of `id`.
fn only_child(doc: &Document, id: NodeId, local: &str) -> Result<NodeId, SignatureError> {
    let mut found = doc.children_named(id, DSIG_NS, local);
    let first = found
        .next()
        .ok_or_else(|| malformed(format!("no {local}")))?;
    if found.next().is_some() {
        return Err(malformed(format!("more than one {local}")));
    }
    Ok(first)
}

fn digest(uri: &str, data: &[u8]) -> Result<Vec<u8>, SignatureError> {
    refuse_insecure(uri)?;
    Ok(match uri {
        SHA256 => Sha256::digest(data).to_vec(),
        SHA384 | SHA384_XMLENC => Sha384::digest(data).to_vec(),
        SHA512 => Sha512::digest(data).to_vec(),
        other => return Err(SignatureError::UnsupportedAlgorithm(other.to_owned())),
    })
}

fn signature_algorithm(uri: &str) -> Result<Algorithm, SignatureError> {
    refuse_insecure(uri)?;
    Ok(match uri {
        RSA_SHA256 => Algorithm::RS256,
        RSA_SHA384 => Algorithm::RS384,
        RSA_SHA512 => Algorithm::RS512,
        ECDSA_SHA256 => Algorithm::ES256,
        ECDSA_SHA384 => Algorithm::ES384,
        other => return Err(SignatureError::UnsupportedAlgorithm(other.to_owned())),
    })
}

/// The `ds:Signature` children of `element`.
pub(crate) fn signature_of(
    doc: &Document,
    element: NodeId,
) -> Result<Option<NodeId>, SignatureError> {
    let mut signatures = doc.children_named(element, DSIG_NS, "Signature");
    let first = signatures.next();
    if signatures.next().is_some() {
        return Err(malformed("more than one signature on one element"));
    }
    Ok(first)
}

/// Verifies the enveloped signature of `element` with `keys`. `Ok(None)`
/// when `element` has no signature, `Ok(Some(signature))` when it has one
/// that a key verifies, an error otherwise (a present but invalid signature
/// is never ignored).
pub(crate) fn verify_enveloped(
    doc: &Document,
    element: NodeId,
    keys: &[VerifyingKey],
) -> Result<Option<NodeId>, SignatureError> {
    let Some(signature) = signature_of(doc, element)? else {
        return Ok(None);
    };
    let signed_info = only_child(doc, signature, "SignedInfo")?;
    if doc.child_elements(signature).next() != Some(signed_info) {
        return Err(malformed("SignedInfo must come first"));
    }
    let signature_value = only_child(doc, signature, "SignatureValue")?;

    let (method, prefixes) = c14n_of(doc, only_child(doc, signed_info, "CanonicalizationMethod")?)?;
    let alg = signature_algorithm(algorithm(
        doc,
        only_child(doc, signed_info, "SignatureMethod")?,
    )?)?;
    let reference = only_child(doc, signed_info, "Reference")?;

    // The reference names the enveloping element, by its id.
    let id = element_id(doc, element).ok_or(SignatureError::WrongReference)?;
    match doc.attr(reference, "URI") {
        Some(uri) if uri.strip_prefix('#') == Some(id) => {}
        _ => return Err(SignatureError::WrongReference),
    }

    // Transforms: enveloped-signature, then at most one canonicalization.
    let mut transforms: Vec<NodeId> = Vec::new();
    if let Some(list) = doc.child(reference, DSIG_NS, "Transforms") {
        transforms.extend(doc.children_named(list, DSIG_NS, "Transform"));
        if doc.child_elements(list).count() != transforms.len() {
            return Err(malformed("unexpected element in Transforms"));
        }
    }
    let mut transforms = transforms.into_iter();
    match transforms.next() {
        Some(first) if algorithm(doc, first)? == ENVELOPED => {}
        Some(first) => {
            return Err(SignatureError::UnsupportedAlgorithm(
                algorithm(doc, first)?.to_owned(),
            ));
        }
        None => return Err(malformed("the enveloped-signature transform is missing")),
    }
    // Without a canonicalization transform, the node set is serialized
    // with Canonical XML 1.0 (XML Signature §4.4.3.2).
    let (reference_method, reference_prefixes) = match transforms.next() {
        Some(transform) => c14n_of(doc, transform)?,
        None => (
            Method {
                exclusive: false,
                comments: false,
            },
            Vec::new(),
        ),
    };
    if let Some(extra) = transforms.next() {
        return Err(SignatureError::UnsupportedAlgorithm(
            algorithm(doc, extra)?.to_owned(),
        ));
    }
    let digest_method = algorithm(doc, only_child(doc, reference, "DigestMethod")?)?;
    refuse_insecure(digest_method)?;
    let expected_digest = base64_of(doc, only_child(doc, reference, "DigestValue")?)?;

    // First the signature over SignedInfo, with a trusted key...
    let canonical_info = c14n::canonicalize(doc, signed_info, method, &prefixes, None);
    let signature_bytes = base64_of(doc, signature_value)?;
    let verified = keys.iter().any(|key| {
        key.accepts(alg)
            && key
                .verify(alg, canonical_info.as_bytes(), &signature_bytes)
                .is_ok()
    });
    if !verified {
        return Err(SignatureError::BadSignature);
    }
    // ...then the digest of the element without its signature.
    let canonical = c14n::canonicalize(
        doc,
        element,
        reference_method,
        &reference_prefixes,
        Some(signature),
    );
    let actual = digest(digest_method, canonical.as_bytes())?;
    if !navree_crypto::constant_time_eq(&actual, &expected_digest) {
        return Err(SignatureError::DigestMismatch);
    }
    Ok(Some(signature))
}

/// The placeholder [`sign_enveloped`] replaces with the signature: put it
/// where the schema wants the signature (after a SAML `Issuer`).
pub(crate) const SIGNATURE_PLACEHOLDER: &str = "<!--navree:signature-->";

/// Signs the element with id `element_id` of `xml`, which must contain
/// [`SIGNATURE_PLACEHOLDER`] once, inside that element: exclusive
/// canonicalization, SHA-256 digest, RSA-SHA256 or ECDSA-SHA256/384
/// depending on the key, and `certificate` (DER) in `KeyInfo` when given.
pub(crate) fn sign_enveloped(
    xml: &str,
    element_id: &str,
    key: &SigningKey,
    certificate: Option<&[u8]>,
) -> Result<String, SignError> {
    if xml.matches(SIGNATURE_PLACEHOLDER).count() != 1 {
        return Err(SignError::Placeholder);
    }
    let doc = Document::parse(xml)?;
    let element = doc
        .elements()
        .into_iter()
        .find(|e| self::element_id(&doc, *e) == Some(element_id))
        .ok_or(SignError::NoElement)?;
    // Comments are not part of the exclusive canonical form, so the
    // placeholder does not change the digest.
    let canonical = c14n::canonicalize(&doc, element, Method::EXCLUSIVE, &[], None);
    let digest_value = STANDARD.encode(Sha256::digest(canonical.as_bytes()));
    let signature_method = match key.algorithm() {
        Algorithm::RS256 => RSA_SHA256,
        Algorithm::ES256 => ECDSA_SHA256,
        Algorithm::ES384 => ECDSA_SHA384,
        _ => return Err(SignError::Key),
    };
    let signed_info = format!(
        "<ds:SignedInfo xmlns:ds=\"{DSIG_NS}\">\
         <ds:CanonicalizationMethod Algorithm=\"{exc}\"/>\
         <ds:SignatureMethod Algorithm=\"{signature_method}\"/>\
         <ds:Reference URI=\"#{id}\">\
         <ds:Transforms>\
         <ds:Transform Algorithm=\"{ENVELOPED}\"/>\
         <ds:Transform Algorithm=\"{exc}\"/>\
         </ds:Transforms>\
         <ds:DigestMethod Algorithm=\"{SHA256}\"/>\
         <ds:DigestValue>{digest_value}</ds:DigestValue>\
         </ds:Reference>\
         </ds:SignedInfo>",
        exc = c14n::EXCLUSIVE,
        id = escape(element_id),
    );
    // Standalone, the exclusive canonical form of SignedInfo is the one it
    // has inside the signature: it uses only the `ds` prefix.
    let info_doc = Document::parse(&signed_info)?;
    let canonical_info =
        c14n::canonicalize(&info_doc, info_doc.root(), Method::EXCLUSIVE, &[], None);
    let signature_value = STANDARD.encode(
        key.sign(canonical_info.as_bytes())
            .map_err(|_| SignError::Key)?,
    );
    let inner_info = signed_info.replacen(
        &format!("<ds:SignedInfo xmlns:ds=\"{DSIG_NS}\">"),
        "<ds:SignedInfo>",
        1,
    );
    let key_info = certificate.map_or_else(String::new, |der| {
        format!(
            "<ds:KeyInfo><ds:X509Data><ds:X509Certificate>{}</ds:X509Certificate></ds:X509Data></ds:KeyInfo>",
            STANDARD.encode(der)
        )
    });
    let signature = format!(
        "<ds:Signature xmlns:ds=\"{DSIG_NS}\">{inner_info}<ds:SignatureValue>{signature_value}</ds:SignatureValue>{key_info}</ds:Signature>"
    );
    Ok(xml.replacen(SIGNATURE_PLACEHOLDER, &signature, 1))
}

/// Why [`sign_enveloped`] failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum SignError {
    #[error("the document needs the signature placeholder exactly once")]
    Placeholder,
    #[error("no element has the id to sign")]
    NoElement,
    #[error("the key cannot sign XML signatures")]
    Key,
    #[error(transparent)]
    Xml(#[from] XmlError),
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = include_str!("fake/idp-key.pem");
    const CERT: &str = include_str!("fake/idp-cert.pem");
    const OTHER_KEY: &str = include_str!("../tests/fixtures/other-key.pem");
    const EC_KEY: &str = include_str!("../tests/fixtures/ec-key.pem");
    const EC_CERT: &str = include_str!("../tests/fixtures/ec-cert.pem");

    const DOC: &str = r#"<r:Root xmlns:r="urn:root" xmlns:x="urn:unused"><a:Signed xmlns:a="urn:a" ID="_s1"><a:Issuer>idp</a:Issuer><!--navree:signature--><a:Body v="1">text &amp; more</a:Body></a:Signed></r:Root>"#;

    fn key() -> SigningKey {
        SigningKey::from_pem(KEY).unwrap()
    }

    fn trusted() -> Vec<VerifyingKey> {
        vec![VerifyingKey::from_pem(CERT).unwrap()]
    }

    fn signed() -> String {
        sign_enveloped(DOC, "_s1", &key(), None).unwrap()
    }

    fn signed_element(doc: &Document) -> NodeId {
        doc.child_elements(doc.root()).next().unwrap()
    }

    fn verify(xml: &str) -> Result<Option<NodeId>, SignatureError> {
        let doc = Document::parse(xml).unwrap();
        let element = signed_element(&doc);
        check_unique_ids(&doc)?;
        verify_enveloped(&doc, element, &trusted())
    }

    #[test]
    fn a_signed_element_verifies_with_the_trusted_key_only() {
        let xml = signed();
        assert!(verify(&xml).unwrap().is_some());
        let doc = Document::parse(&xml).unwrap();
        let other = SigningKey::from_pem(OTHER_KEY).unwrap().verifying_key();
        assert_eq!(
            verify_enveloped(&doc, signed_element(&doc), &[other]),
            Err(SignatureError::BadSignature)
        );
        assert_eq!(
            verify_enveloped(&doc, signed_element(&doc), &[]),
            Err(SignatureError::BadSignature)
        );
    }

    #[test]
    fn an_unsigned_element_has_no_signature() {
        let xml = DOC.replace(SIGNATURE_PLACEHOLDER, "");
        assert_eq!(verify(&xml), Ok(None));
    }

    #[test]
    fn ecdsa_signatures_verify() {
        let key = SigningKey::from_pem(EC_KEY).unwrap();
        let xml = sign_enveloped(DOC, "_s1", &key, None).unwrap();
        let doc = Document::parse(&xml).unwrap();
        let trusted = VerifyingKey::from_pem(EC_CERT).unwrap();
        assert!(
            verify_enveloped(&doc, signed_element(&doc), &[trusted])
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn changed_content_breaks_the_digest() {
        let xml = signed().replace("text &amp; more", "text &amp; less");
        assert_eq!(verify(&xml), Err(SignatureError::DigestMismatch));
        let xml = signed().replace("v=\"1\"", "v=\"2\"");
        assert_eq!(verify(&xml), Err(SignatureError::DigestMismatch));
        // Comments are outside the canonical form (no comment trick can
        // change what the signature says, and none is needed to break it).
        let xml = signed().replace("<a:Body", "<!--x--><a:Body");
        assert!(verify(&xml).unwrap().is_some());
        // A changed SignedInfo breaks the signature.
        let xml = signed().replace("ds:DigestValue>", "ds:DigestValue> ");
        assert_eq!(verify(&xml), Err(SignatureError::BadSignature));
    }

    #[test]
    fn layout_of_the_unsigned_parts_does_not_matter() {
        // Namespace declarations moved to an ancestor, unused ones added:
        // the exclusive canonical form stays the same.
        let xml = signed()
            .replace(
                r#"<a:Signed xmlns:a="urn:a" ID="_s1">"#,
                r#"<a:Signed ID="_s1">"#,
            )
            .replace(
                r#"<r:Root xmlns:r="urn:root""#,
                r#"<r:Root xmlns:r="urn:root" xmlns:a="urn:a""#,
            );
        assert!(verify(&xml).unwrap().is_some());
    }

    #[test]
    fn the_reference_must_name_the_enveloping_element() {
        let xml = signed().replace("URI=\"#_s1\"", "URI=\"#_other\"");
        assert_eq!(verify(&xml), Err(SignatureError::WrongReference));
        let xml = signed().replace("URI=\"#_s1\"", "URI=\"\"");
        assert_eq!(verify(&xml), Err(SignatureError::WrongReference));
        let xml = signed().replace(" ID=\"_s1\"", "");
        assert_eq!(verify(&xml), Err(SignatureError::WrongReference));
    }

    #[test]
    fn duplicate_ids_are_refused() {
        let xml = signed().replace("<a:Body v=\"1\">", "<a:Body v=\"1\"><a:Copy ID=\"_s1\"/>");
        assert_eq!(
            verify(&xml),
            Err(SignatureError::DuplicateId("_s1".to_owned()))
        );
    }

    #[test]
    fn sha1_and_unknown_algorithms_are_refused() {
        let xml = signed().replace(RSA_SHA256, "http://www.w3.org/2000/09/xmldsig#rsa-sha1");
        assert_eq!(
            verify(&xml),
            Err(SignatureError::InsecureAlgorithm(
                "http://www.w3.org/2000/09/xmldsig#rsa-sha1".to_owned()
            ))
        );
        let xml = signed().replace(SHA256, "http://www.w3.org/2000/09/xmldsig#sha1");
        assert!(matches!(
            verify(&xml),
            Err(SignatureError::InsecureAlgorithm(_))
        ));
        let xml = signed().replace(RSA_SHA256, "urn:made-up");
        assert!(matches!(
            verify(&xml),
            Err(SignatureError::UnsupportedAlgorithm(_))
        ));
    }

    #[test]
    fn only_the_enveloped_and_canonicalization_transforms_are_allowed() {
        let xpath = "<ds:Transform Algorithm=\"http://www.w3.org/TR/1999/REC-xpath-19991116\"/>";
        let xml = signed().replace("</ds:Transforms>", &format!("{xpath}</ds:Transforms>"));
        assert!(matches!(
            verify(&xml),
            Err(SignatureError::UnsupportedAlgorithm(_))
        ));
        let xml = signed().replace(&format!("<ds:Transform Algorithm=\"{ENVELOPED}\"/>"), "");
        assert!(verify(&xml).is_err());
    }

    #[test]
    fn two_signatures_on_one_element_are_refused() {
        let xml = signed();
        let start = xml.find("<ds:Signature").unwrap();
        let end = xml.find("</ds:Signature>").unwrap() + "</ds:Signature>".len();
        let signature = &xml[start..end];
        let doubled = xml.replacen(signature, &format!("{signature}{signature}"), 1);
        assert!(matches!(
            verify(&doubled),
            Err(SignatureError::Malformed(_))
        ));
    }

    /// A real sample's signatures: `(signed element, signature)`.
    fn sample_signatures(doc: &Document) -> Vec<(NodeId, NodeId)> {
        doc.elements()
            .into_iter()
            .filter(|e| doc.is(*e, DSIG_NS, "Signature"))
            .map(|signature| (doc.parent(signature).unwrap(), signature))
            .collect()
    }

    /// The PKCS#1 `RSAPublicKey` of the certificate in a sample's
    /// `KeyInfo` (some samples carry a PEM document base64-encoded once
    /// more). A test helper: it finds the rsaEncryption OID and reads the
    /// BIT STRING after it.
    fn sample_rsa_key(doc: &Document, signature: NodeId) -> Vec<u8> {
        let cert = doc
            .descendants(signature)
            .into_iter()
            .find(|e| doc.is(*e, DSIG_NS, "X509Certificate"))
            .unwrap();
        let mut der = base64_of(doc, cert).unwrap();
        if der.starts_with(b"-----") {
            let text = String::from_utf8(der).unwrap();
            let body: String = text.lines().filter(|l| !l.starts_with("-----")).collect();
            der = STANDARD.decode(body).unwrap();
        }
        let oid = [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
        let at = der.windows(oid.len()).position(|w| w == oid).unwrap() + oid.len();
        // NULL parameters, then the BIT STRING (no unused bits).
        let rest = &der[at + 2..];
        assert_eq!(rest[0], 0x03);
        let (len, header) = match rest[1] {
            l if l < 0x80 => (usize::from(l), 2),
            0x81 => (usize::from(rest[2]), 3),
            0x82 => ((usize::from(rest[2]) << 8) | usize::from(rest[3]), 4),
            _ => panic!("unexpected length"),
        };
        rest[header + 1..header + len].to_vec()
    }

    /// Other signers' signatures (AD FS, SimpleSAMLphp, OneLogin, with
    /// exclusive and inclusive canonicalization and a prefix list) check
    /// out against Navree's canonical forms. They use SHA-1 or 1024-bit
    /// keys, so this uses `ring`'s legacy verifiers, and the verifier
    /// itself refuses every one of them.
    #[test]
    fn canonical_forms_match_real_signers() {
        use sha1::Sha1;
        let samples = [
            include_str!("../tests/fixtures/samples/signed-response-sha1.xml"),
            include_str!("../tests/fixtures/samples/signed-assertion-sha1.xml"),
            include_str!("../tests/fixtures/samples/double-signed-sha1.xml"),
            include_str!("../tests/fixtures/samples/inclusive-namespaces.xml"),
            include_str!("../tests/fixtures/samples/inclusive-c14n.xml"),
            include_str!("../tests/fixtures/samples/ds-namespace-at-root.xml"),
            include_str!("../tests/fixtures/samples/adfs-sha256.xml"),
            include_str!("../tests/fixtures/samples/adfs-sha512.xml"),
        ];
        let mut checked = 0;
        for xml in samples {
            let doc = Document::parse(xml).unwrap();
            for (element, signature) in sample_signatures(&doc) {
                let signed_info = only_child(&doc, signature, "SignedInfo").unwrap();
                let (method, prefixes) = c14n_of(
                    &doc,
                    only_child(&doc, signed_info, "CanonicalizationMethod").unwrap(),
                )
                .unwrap();
                let reference = only_child(&doc, signed_info, "Reference").unwrap();
                let transforms: Vec<NodeId> = doc
                    .children_named(
                        doc.child(reference, DSIG_NS, "Transforms").unwrap(),
                        DSIG_NS,
                        "Transform",
                    )
                    .collect();
                // Without a canonicalization transform: Canonical XML 1.0.
                let (reference_method, reference_prefixes) = match transforms.get(1) {
                    Some(transform) => c14n_of(&doc, *transform).unwrap(),
                    None => (
                        Method {
                            exclusive: false,
                            comments: false,
                        },
                        Vec::new(),
                    ),
                };
                let canonical = c14n::canonicalize(
                    &doc,
                    element,
                    reference_method,
                    &reference_prefixes,
                    Some(signature),
                );
                let digest_method =
                    algorithm(&doc, only_child(&doc, reference, "DigestMethod").unwrap()).unwrap();
                let actual = if digest_method.ends_with("#sha1") {
                    Sha1::digest(canonical.as_bytes()).to_vec()
                } else {
                    digest(digest_method, canonical.as_bytes()).unwrap()
                };
                let expected =
                    base64_of(&doc, only_child(&doc, reference, "DigestValue").unwrap()).unwrap();
                assert_eq!(
                    actual,
                    expected,
                    "digest of {}",
                    doc.element(element).unwrap().local
                );

                let info = c14n::canonicalize(&doc, signed_info, method, &prefixes, None);
                let signature_method = algorithm(
                    &doc,
                    only_child(&doc, signed_info, "SignatureMethod").unwrap(),
                )
                .unwrap();
                let legacy: &dyn ring::signature::VerificationAlgorithm =
                    if signature_method.ends_with("sha1") {
                        &ring::signature::RSA_PKCS1_1024_8192_SHA1_FOR_LEGACY_USE_ONLY
                    } else if signature_method.ends_with("sha512") {
                        &ring::signature::RSA_PKCS1_1024_8192_SHA512_FOR_LEGACY_USE_ONLY
                    } else {
                        &ring::signature::RSA_PKCS1_1024_8192_SHA256_FOR_LEGACY_USE_ONLY
                    };
                let value = base64_of(&doc, only_child(&doc, signature, "SignatureValue").unwrap())
                    .unwrap();
                ring::signature::UnparsedPublicKey::new(legacy, sample_rsa_key(&doc, signature))
                    .verify(info.as_bytes(), &value)
                    .unwrap();

                // Navree's verifier refuses them: SHA-1, or (with a key it
                // never trusted) a bad signature.
                let refused = verify_enveloped(&doc, element, &trusted()).unwrap_err();
                assert!(
                    matches!(
                        refused,
                        SignatureError::InsecureAlgorithm(_) | SignatureError::BadSignature
                    ),
                    "{refused:?}"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 9);
    }

    /// A text node changed after signing: the digest no longer matches.
    #[test]
    fn a_real_text_node_attack_breaks_the_digest() {
        let doc = Document::parse(include_str!(
            "../tests/fixtures/samples/node-text-attack.xml"
        ))
        .unwrap();
        let (element, signature) = sample_signatures(&doc)[0];
        let reference = only_child(
            &doc,
            only_child(&doc, signature, "SignedInfo").unwrap(),
            "Reference",
        )
        .unwrap();
        let canonical = c14n::canonicalize(&doc, element, Method::EXCLUSIVE, &[], Some(signature));
        let expected =
            base64_of(&doc, only_child(&doc, reference, "DigestValue").unwrap()).unwrap();
        assert_ne!(sha1::Sha1::digest(canonical.as_bytes()).to_vec(), expected);
    }
    #[test]
    fn signing_needs_one_placeholder_and_the_element() {
        assert!(matches!(
            sign_enveloped("<a ID=\"x\"/>", "x", &key(), None),
            Err(SignError::Placeholder)
        ));
        assert!(matches!(
            sign_enveloped(DOC, "_nope", &key(), None),
            Err(SignError::NoElement)
        ));
        let with_cert = sign_enveloped(DOC, "_s1", &key(), Some(b"cert")).unwrap();
        assert!(with_cert.contains("<ds:X509Certificate>Y2VydA==</ds:X509Certificate>"));
        assert!(verify(&with_cert).unwrap().is_some());
    }
}
