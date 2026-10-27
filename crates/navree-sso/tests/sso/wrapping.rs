//! Signature wrapping and other forgeries built from a validly signed
//! response: every one is refused, and what is read always comes from the
//! element the signature covers.

use navree_sso::SignatureError;
use navree_sso::saml::SamlError;

use crate::samples::{OKTA, OKTA_ISSUER, check};

/// The signed assertion of the Okta sample, its signature, and the parts
/// around them.
struct Parts {
    before: String,
    assertion: String,
    after: String,
}

fn split() -> Parts {
    let start = OKTA.find("<saml2:Assertion").unwrap();
    let end = OKTA.find("</saml2:Assertion>").unwrap() + "</saml2:Assertion>".len();
    Parts {
        before: OKTA[..start].to_owned(),
        assertion: OKTA[start..end].to_owned(),
        after: OKTA[end..].to_owned(),
    }
}

fn without_signature(assertion: &str) -> String {
    let start = assertion.find("<ds:Signature").unwrap();
    let end = assertion.find("</ds:Signature>").unwrap() + "</ds:Signature>".len();
    format!("{}{}", &assertion[..start], &assertion[end..])
}

/// An unsigned copy claiming another user, with id `id`.
fn evil(assertion: &str, id: &str) -> String {
    without_signature(assertion)
        .replace("alice@acme.test", "mallory@acme.test")
        .replace("ID=\"id17616282001234567890456\"", &format!("ID=\"{id}\""))
}

fn invalid(result: Result<navree_sso::saml::SamlAssertion, SamlError>) -> bool {
    matches!(result, Err(SamlError::Invalid(_)))
}

#[test]
fn an_evil_assertion_next_to_the_signed_one() {
    let p = split();
    for xml in [
        // Before it, and after it.
        format!(
            "{}{}{}{}",
            p.before,
            evil(&p.assertion, "_evil"),
            p.assertion,
            p.after
        ),
        format!(
            "{}{}{}{}",
            p.before,
            p.assertion,
            evil(&p.assertion, "_evil"),
            p.after
        ),
    ] {
        assert!(invalid(check(&xml, OKTA_ISSUER)), "{xml}");
    }
}

#[test]
fn the_signed_assertion_hidden_in_extensions() {
    let p = split();
    let xml = format!(
        "{}<saml2p:Extensions>{}</saml2p:Extensions>{}{}",
        p.before,
        p.assertion,
        evil(&p.assertion, "_evil"),
        p.after
    );
    assert!(invalid(check(&xml, OKTA_ISSUER)));
}

#[test]
fn the_signed_assertion_inside_the_evil_one_with_its_id() {
    let p = split();
    // The evil assertion takes the signed one's id and carries the original
    // in its Advice, where a lookup by id might find it.
    let evil = evil(&p.assertion, "id17616282001234567890456").replace(
        "</saml2:Assertion>",
        &format!(
            "<saml2:Advice>{}</saml2:Advice></saml2:Assertion>",
            p.assertion
        ),
    );
    let xml = format!("{}{evil}{}", p.before, p.after);
    let result = check(&xml, OKTA_ISSUER);
    assert!(
        matches!(
            result,
            Err(SamlError::Signature(SignatureError::DuplicateId(_))) | Err(SamlError::Invalid(_))
        ),
        "{result:?}"
    );
}

#[test]
fn the_signature_moved_to_the_response() {
    let p = split();
    let start = p.assertion.find("<ds:Signature").unwrap();
    let end = p.assertion.find("</ds:Signature>").unwrap() + "</ds:Signature>".len();
    let signature = &p.assertion[start..end];
    let unsigned = without_signature(&p.assertion).replace("alice@acme.test", "mallory@acme.test");
    // The response's signature now references the assertion.
    let before = p
        .before
        .replacen("</saml2:Issuer>", &format!("</saml2:Issuer>{signature}"), 1);
    let xml = format!("{before}{unsigned}{}", p.after);
    assert_eq!(
        check(&xml, OKTA_ISSUER),
        Err(SamlError::Signature(SignatureError::WrongReference))
    );
}

#[test]
fn the_whole_response_wrapped_in_another_element() {
    let body =
        OKTA.trim_start_matches("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>");
    let xml = format!("<Envelope>{body}</Envelope>");
    assert!(invalid(check(&xml, OKTA_ISSUER)));
}

/// A comment inside the name id leaves the canonical form (and so the
/// signature) intact; the whole text is read, never the first text node.
#[test]
fn a_comment_in_the_name_id_does_not_shorten_it() {
    let xml = OKTA.replace(
        ">alice@acme.test</saml2:NameID>",
        ">alice@acme<!---->.test</saml2:NameID>",
    );
    assert_eq!(check(&xml, OKTA_ISSUER).unwrap().name_id, "alice@acme.test");
}

#[test]
fn entities_and_document_types_are_refused() {
    let xml = OKTA.replacen(
        "<saml2p:Response",
        "<!DOCTYPE r [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><saml2p:Response",
        1,
    );
    assert!(matches!(
        check(&xml, OKTA_ISSUER),
        Err(SamlError::Xml(navree_sso::XmlError::Doctype))
    ));
}

/// Real wrapping attacks from other SAML toolkits' test suites
/// (`fixtures/samples/NOTICE.md`).
#[test]
fn published_wrapping_attacks_are_refused() {
    for xml in [
        include_str!("../fixtures/samples/wrapped-response.xml"),
        include_str!("../fixtures/samples/assertion-wrapped.xml"),
    ] {
        let result = check(xml, OKTA_ISSUER);
        assert!(invalid(result.clone()), "{result:?}");
    }
}
