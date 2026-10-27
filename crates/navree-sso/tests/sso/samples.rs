//! Responses shaped like Okta's, Entra ID's and Google Workspace's, signed
//! by the JDK's XML Signature implementation (an independent signer) with
//! the `SsoFake` identity provider's key, checked by `validate_response`.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use navree_sso::saml::{
    IdpMetadata, ResponseCheck, SamlAssertion, SamlError, ServiceProvider, validate_response,
};
use navree_sso::{SignatureError, SsoFake};

pub(crate) const REQUEST_ID: &str = "_a1b2c3d4e5f60718293a4b5c6d7e8f90";
pub(crate) const ACS: &str = "https://api.acme.test/api/sso/saml/acme/acs";
pub(crate) const ENTITY: &str = "https://api.acme.test/api/sso/saml/acme/metadata";

pub(crate) const OKTA: &str = include_str!("../fixtures/idp/okta.xml");
pub(crate) const OKTA_BOTH: &str = include_str!("../fixtures/idp/okta-both.xml");
pub(crate) const ENTRA: &str = include_str!("../fixtures/idp/entra.xml");
pub(crate) const GOOGLE: &str = include_str!("../fixtures/idp/google.xml");

pub(crate) const OKTA_ISSUER: &str = "http://www.okta.com/exk8acmeTEST4x7";
pub(crate) const ENTRA_ISSUER: &str = "https://sts.windows.net/9e3c5a0f-1b2d-4c6e-8f0a-2b4c6d8e0f1a/";
pub(crate) const GOOGLE_ISSUER: &str = "https://accounts.google.com/o/saml2?idpid=C03acme01";

pub(crate) fn at(instant: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(instant)
        .unwrap()
        .with_timezone(&Utc)
}

pub(crate) fn sp() -> ServiceProvider {
    ServiceProvider {
        entity_id: ENTITY.to_owned(),
        acs_url: ACS.to_owned(),
    }
}

pub(crate) fn idp(issuer: &str) -> IdpMetadata {
    IdpMetadata::new(issuer, "https://idp.test/sso", &[SsoFake::CERTIFICATE_PEM]).unwrap()
}

/// Checks `xml` as the response to [`REQUEST_ID`] at 12:01.
pub(crate) fn check(xml: &str, issuer: &str) -> Result<SamlAssertion, SamlError> {
    check_at(xml, issuer, at("2026-10-27T12:01:00Z"))
}

pub(crate) fn check_at(
    xml: &str,
    issuer: &str,
    now: DateTime<Utc>,
) -> Result<SamlAssertion, SamlError> {
    let (sp, idp) = (sp(), idp(issuer));
    validate_response(
        &STANDARD.encode(xml),
        &ResponseCheck {
            sp: &sp,
            idp: &idp,
            request_id: Some(REQUEST_ID),
            now,
            clock_skew: Duration::from_secs(60),
            allow_idp_initiated: false,
            require_signed_assertion: false,
        },
    )
}

#[test]
fn okta_signed_assertion() {
    let assertion = check(OKTA, OKTA_ISSUER).unwrap();
    assert_eq!(assertion.name_id, "alice@acme.test");
    assert_eq!(assertion.issuer, OKTA_ISSUER);
    assert_eq!(assertion.attribute("email"), Some("alice@acme.test"));
    assert_eq!(assertion.attribute("firstName"), Some("Alice"));
    assert_eq!(assertion.values("groups"), ["Engineering", "Acme Admins"]);
    assert_eq!(assertion.in_response_to.as_deref(), Some(REQUEST_ID));
    assert_eq!(assertion.not_on_or_after, at("2026-10-27T12:05:00.123Z"));
}

#[test]
fn okta_signed_response_and_assertion() {
    let assertion = check(OKTA_BOTH, OKTA_ISSUER).unwrap();
    assert_eq!(assertion.name_id, "alice@acme.test");
}

#[test]
fn entra_id_default_namespaces_and_claim_uris() {
    let assertion = check(ENTRA, ENTRA_ISSUER).unwrap();
    assert_eq!(assertion.name_id, "bob@acme.test");
    assert_eq!(
        assertion.attribute("http://schemas.xmlsoap.org/ws/2005/05/identity/claims/emailaddress"),
        Some("bob@acme.test")
    );
    assert_eq!(
        assertion
            .values("http://schemas.microsoft.com/ws/2008/06/identity/claims/groups")
            .len(),
        2
    );
    // Its confirmation lasts an hour.
    assert!(check_at(ENTRA, ENTRA_ISSUER, at("2026-10-27T12:30:00Z")).is_ok());
}

#[test]
fn google_signed_response_around_an_unsigned_assertion() {
    let assertion = check(GOOGLE, GOOGLE_ISSUER).unwrap();
    assert_eq!(assertion.name_id, "carol@acme.test");
    assert_eq!(assertion.attribute("first_name"), Some("Carol"));
    assert_eq!(assertion.values("groups"), ["admins@acme.test"]);
    // A connection that wants signed assertions refuses it.
    let (sp, idp) = (sp(), idp(GOOGLE_ISSUER));
    let strict = ResponseCheck {
        sp: &sp,
        idp: &idp,
        request_id: Some(REQUEST_ID),
        now: at("2026-10-27T12:01:00Z"),
        clock_skew: Duration::from_secs(60),
        allow_idp_initiated: false,
        require_signed_assertion: true,
    };
    assert_eq!(
        validate_response(&STANDARD.encode(GOOGLE), &strict),
        Err(SamlError::Signature(SignatureError::Missing))
    );
}

#[test]
fn each_sample_is_refused_once_changed() {
    for (xml, issuer, from, to) in [
        (OKTA, OKTA_ISSUER, "alice@acme.test</saml2:NameID>", "admin@acme.test</saml2:NameID>"),
        (OKTA_BOTH, OKTA_ISSUER, ">Engineering<", ">Executives<"),
        (ENTRA, ENTRA_ISSUER, "<AttributeValue>Bob</AttributeValue>", "<AttributeValue>Rob</AttributeValue>"),
        (GOOGLE, GOOGLE_ISSUER, "admins@acme.test", "owners@acme.test"),
    ] {
        assert!(xml.contains(from), "{from}");
        let changed = xml.replace(from, to);
        assert_eq!(
            check(&changed, issuer),
            Err(SamlError::Signature(SignatureError::DigestMismatch)),
            "{issuer}"
        );
    }
}

#[test]
fn samples_expire_and_need_their_issuer_and_key() {
    assert_eq!(
        check_at(OKTA, OKTA_ISSUER, at("2026-10-27T12:07:00Z")),
        Err(SamlError::Expired)
    );
    assert_eq!(
        check(OKTA, ENTRA_ISSUER),
        Err(SamlError::WrongIssuer(OKTA_ISSUER.to_owned()))
    );
    // The right issuer with another identity provider's key.
    let (sp, idp) = (
        sp(),
        IdpMetadata::new(
            OKTA_ISSUER,
            "https://idp.test/sso",
            &[include_str!("../fixtures/other-cert.pem")],
        )
        .unwrap(),
    );
    let result = validate_response(
        &STANDARD.encode(OKTA),
        &ResponseCheck {
            sp: &sp,
            idp: &idp,
            request_id: Some(REQUEST_ID),
            now: at("2026-10-27T12:01:00Z"),
            clock_skew: Duration::from_secs(60),
            allow_idp_initiated: false,
            require_signed_assertion: false,
        },
    );
    assert_eq!(
        result,
        Err(SamlError::Signature(SignatureError::BadSignature))
    );
}
