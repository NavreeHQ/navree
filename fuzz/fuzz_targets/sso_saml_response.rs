//! `navree_sso::saml::validate_response`: a `SAMLResponse` from an identity
//! provider (base64 XML) checked against a connection.
//!
//! Byte 0 picks the mode:
//!
//! 0. the rest is the posted form value, with the whole path (base64, XML,
//!    signatures) and the published test certificate as the trusted key;
//! 1. the rest is the XML, read without the signature checks, which a
//!    fuzzer cannot forge, so the claims code gets coverage; byte 1 selects
//!    the connection's settings.
//!
//! Invariant: an accepted assertion has the connection's issuer, an id, a
//! name id, and has not expired by the connection's clock.
#![no_main]

use std::sync::OnceLock;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use libfuzzer_sys::fuzz_target;
use navree_fuzz::sso as fixtures;
use navree_sso::SsoFake;
use navree_sso::saml::{
    IdpMetadata, ResponseCheck, SamlAssertion, ServiceProvider, validate_response,
};

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-27T12:01:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

struct Fixed {
    sp: ServiceProvider,
    idps: Vec<IdpMetadata>,
}

fn fixed() -> &'static Fixed {
    static FIXED: OnceLock<Fixed> = OnceLock::new();
    FIXED.get_or_init(|| Fixed {
        sp: ServiceProvider {
            entity_id: fixtures::ENTITY.to_owned(),
            acs_url: fixtures::ACS.to_owned(),
        },
        idps: [
            fixtures::OKTA_ISSUER,
            fixtures::ENTRA_ISSUER,
            fixtures::GOOGLE_ISSUER,
        ]
        .iter()
        .map(|issuer| {
            IdpMetadata::new(*issuer, "https://idp.test/sso", &[SsoFake::CERTIFICATE_PEM])
                .expect("metadata")
        })
        .collect(),
    })
}

fn accepted(assertion: &SamlAssertion, idp: &IdpMetadata, skew: Duration) {
    assert_eq!(assertion.issuer, idp.entity_id);
    assert!(!assertion.id.is_empty());
    assert!(!assertion.name_id.is_empty());
    assert_eq!(assertion.name_id, assertion.name_id.trim());
    let skew = chrono::TimeDelta::from_std(skew).unwrap();
    assert!(
        assertion.not_on_or_after > now() - skew,
        "an expired assertion"
    );
}

fuzz_target!(|data: &[u8]| {
    let [mode, flags, rest @ ..] = data else {
        return;
    };
    let Ok(text) = std::str::from_utf8(rest) else {
        return;
    };
    let fixed = fixed();
    let idp = &fixed.idps[usize::from(*flags >> 4) % fixed.idps.len()];
    let skew = Duration::from_secs(60);
    let check = ResponseCheck {
        sp: &fixed.sp,
        idp,
        request_id: (flags & 1 == 0).then_some(fixtures::REQUEST_ID),
        now: now(),
        clock_skew: skew,
        allow_idp_initiated: flags & 2 != 0,
        require_signed_assertion: flags & 4 != 0,
    };
    match mode % 2 {
        0 => {
            if let Ok(assertion) = validate_response(text, &check) {
                accepted(&assertion, idp, skew);
            }
            let encoded = STANDARD.encode(text);
            if let Ok(assertion) = validate_response(&encoded, &check) {
                accepted(&assertion, idp, skew);
            }
        }
        _ => {
            if let Ok(assertion) = navree_sso::fuzzing::validate_unsigned(text, &check) {
                accepted(&assertion, idp, skew);
            }
        }
    }
});
