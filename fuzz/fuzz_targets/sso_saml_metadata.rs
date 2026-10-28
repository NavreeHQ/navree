//! `navree_sso::saml`: identity provider metadata XML, certificates given as
//! PEM or base64, and HTTP-Redirect requests (`SAMLRequest` is deflated
//! base64 inside a URL).
//!
//! Invariant: metadata that parses names an entity and has as many signing
//! keys as certificates; the service provider's own metadata is well-formed
//! XML whatever its names hold.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_sso::saml::{AuthnRequest, IdpMetadata, ServiceProvider};

/// Whether XML 1.0 allows `c` in a document.
fn legal(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}'
    )
}

fuzz_target!(|data: &[u8]| {
    let [mode, rest @ ..] = data else { return };
    let Ok(text) = std::str::from_utf8(rest) else {
        return;
    };
    match mode % 4 {
        0 => {
            if let Ok(metadata) = IdpMetadata::parse(text) {
                assert!(!metadata.entity_id.trim().is_empty());
                assert_eq!(metadata.certificates.len(), metadata.signing_keys.len());
                assert!(!metadata.signing_keys.is_empty());
                let _ = format!("{metadata:?}");
            }
        }
        1 => {
            let _ = IdpMetadata::new("https://idp.test", "https://idp.test/sso", &[text]);
        }
        2 => {
            let _ = AuthnRequest::decode_redirect(text);
            let _ =
                AuthnRequest::decode_redirect(&format!("https://idp.test/sso?SAMLRequest={text}"));
        }
        _ => {
            let (entity, acs) = text.split_once('\n').unwrap_or((text, text));
            let sp = ServiceProvider {
                entity_id: entity.to_owned(),
                acs_url: acs.to_owned(),
            };
            let xml = sp.metadata_xml();
            if entity.chars().all(legal) && acs.chars().all(legal) {
                assert!(
                    navree_sso::fuzzing::xml(&xml).is_some(),
                    "service provider metadata is not well-formed XML: {xml:?}"
                );
            }
        }
    }
});
