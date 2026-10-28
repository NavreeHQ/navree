//! Entry points for the fuzz targets in `fuzz/` (feature `fuzzing`): the
//! XML reader and canonicalizer, and the SAML claims reader without its
//! signature checks, which a fuzzer cannot forge. Not part of the API.

use crate::saml::{ResponseCheck, SamlAssertion, SamlError};
use crate::xml::c14n::{self, Method};
use crate::xml::{Document, NodeId};

/// [`validate_response`](crate::saml::validate_response) for XML text,
/// minus the signature checks.
pub fn validate_unsigned(xml: &str, check: &ResponseCheck<'_>) -> Result<SamlAssertion, SamlError> {
    crate::saml::validate_xml_unsigned(xml, check)
}

/// Parses `text`, reads everything the parser offers, and canonicalizes
/// the root (every method) and a few more elements. Returns the number of
/// elements, `None` when the text is refused.
///
/// # Panics
///
/// When an invariant breaks: the exclusive canonical form of the root must
/// parse and canonicalize to itself.
pub fn xml(text: &str) -> Option<usize> {
    let doc = Document::parse(text).ok()?;
    let elements = doc.elements();
    for id in elements.iter().copied() {
        let _ = doc.text(id);
        let _ = doc.trimmed_text(id);
        let _ = doc.attr(id, "ID");
        let _ = doc.parent(id);
        let _ = doc.child_elements(id).count();
        let _ = doc.is(id, "urn:oasis:names:tc:SAML:2.0:assertion", "Assertion");
    }
    crate::dsig::check_unique_ids(&doc).ok();

    let methods = [
        Method::EXCLUSIVE,
        Method {
            exclusive: true,
            comments: true,
        },
        Method {
            exclusive: false,
            comments: false,
        },
        Method {
            exclusive: false,
            comments: true,
        },
    ];
    let prefixes = ["#default".to_owned(), "ds".to_owned(), "saml".to_owned()];
    let apexes: Vec<NodeId> = elements.iter().copied().take(8).collect();
    for apex in apexes {
        for method in methods {
            let canonical = c14n::canonicalize(&doc, apex, method, &prefixes, None);
            assert!(
                canonical.len() <= 64 * text.len() + 4096,
                "a canonical form of {} bytes from {} bytes",
                canonical.len(),
                text.len()
            );
        }
    }

    let root = doc.root();
    let canonical = c14n::canonicalize(&doc, root, Method::EXCLUSIVE, &[], None);
    if let Ok(again) = Document::parse(&canonical) {
        let twice = c14n::canonicalize(&again, again.root(), Method::EXCLUSIVE, &[], None);
        assert_eq!(twice, canonical, "canonical XML is not idempotent");
    }
    Some(elements.len())
}
