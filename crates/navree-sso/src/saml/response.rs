//! Checking a `SAMLResponse` posted to the assertion consumer service.

use std::collections::BTreeMap;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;

use super::{
    ASSERTION_NS, CM_BEARER, IdpMetadata, PROTOCOL_NS, STATUS_SUCCESS, SamlError, ServiceProvider,
    parse_instant,
};
use crate::dsig::{DSIG_NS, SignatureError, check_unique_ids, verify_enveloped};
use crate::xml::{Document, NodeId};

/// What [`validate_response`] checks a response against.
#[derive(Clone, Copy, Debug)]
pub struct ResponseCheck<'a> {
    /// Navree's side of the connection: the audience and the recipient.
    pub sp: &'a ServiceProvider,
    /// The identity provider: the issuer and the signing keys.
    pub idp: &'a IdpMetadata,
    /// The id of the request the response must answer; `None` when no
    /// request was sent (an IdP-initiated sign-in).
    pub request_id: Option<&'a str>,
    /// The current time.
    pub now: DateTime<Utc>,
    /// How far the identity provider's clock may be off.
    pub clock_skew: Duration,
    /// Whether a response that answers no request is accepted.
    pub allow_idp_initiated: bool,
    /// Whether the assertion itself must be signed (a signed response
    /// around an unsigned assertion is refused).
    pub require_signed_assertion: bool,
}

/// A verified SAML assertion: who signed in, and what the identity
/// provider says about them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SamlAssertion {
    /// The assertion id (remembered until it expires, so it works once).
    pub id: String,
    /// The identity provider's entity id.
    pub issuer: String,
    /// The subject's name id.
    pub name_id: String,
    /// The name id's format, if given.
    pub name_id_format: Option<String>,
    /// The identity provider's session index.
    pub session_index: Option<String>,
    /// When the user authenticated at the identity provider.
    pub authn_instant: Option<DateTime<Utc>>,
    /// The request the response answers (`None` for IdP-initiated
    /// sign-in).
    pub in_response_to: Option<String>,
    /// When the assertion stops being acceptable.
    pub not_on_or_after: DateTime<Utc>,
    /// The attributes, by `Name`, each with its values in order.
    pub attributes: BTreeMap<String, Vec<String>>,
}

impl SamlAssertion {
    /// The first value of attribute `name`.
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .get(name)
            .and_then(|values| values.first())
            .map(String::as_str)
    }

    /// Every value of attribute `name` (empty when it is absent).
    pub fn values(&self, name: &str) -> &[String] {
        self.attributes.get(name).map_or(&[], Vec::as_slice)
    }
}

fn invalid(message: impl Into<String>) -> SamlError {
    SamlError::Invalid(message.into())
}

/// Two URLs are the same, ignoring a trailing slash.
fn same_url(a: &str, b: &str) -> bool {
    a.trim().trim_end_matches('/') == b.trim().trim_end_matches('/')
}

/// Checks a base64 `SAMLResponse` against `check` and returns its
/// assertion:
///
/// 1. the status is success ([`SamlError::Status`] otherwise);
/// 2. exactly one assertion, a child of the response, not encrypted;
/// 3. the response, the assertion or both are signed by a key of the
///    identity provider's metadata, each present signature valid (SHA-1
///    refused), signatures nowhere else, ids unique;
/// 4. `Destination`, the response's and the assertion's `Issuer`;
/// 5. `InResponseTo` names the request (or none, when IdP-initiated
///    sign-in is allowed);
/// 6. a bearer subject confirmation for the assertion consumer service,
///    not expired;
/// 7. the conditions' time window and an audience restriction naming this
///    service provider;
/// 8. an authentication statement, whose session has not ended.
///
/// Everything returned is read from the verified assertion element.
pub fn validate_response(
    saml_response: &str,
    check: &ResponseCheck<'_>,
) -> Result<SamlAssertion, SamlError> {
    let compact: String = saml_response
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    let bytes = STANDARD
        .decode(compact)
        .map_err(|_| invalid("SAMLResponse is not base64"))?;
    let xml = String::from_utf8(bytes).map_err(|_| invalid("SAMLResponse is not UTF-8"))?;
    validate_xml(&xml, check)
}

pub(crate) fn validate_xml(
    xml: &str,
    check: &ResponseCheck<'_>,
) -> Result<SamlAssertion, SamlError> {
    let doc = Document::parse(xml)?;
    let response = doc.root();
    if !doc.is(response, PROTOCOL_NS, "Response") {
        return Err(invalid("the message is not a SAML response"));
    }
    if doc.attr(response, "Version") != Some("2.0") {
        return Err(invalid("the response is not SAML 2.0"));
    }
    // An error status grants nothing, so it is read before the signature
    // checks (identity providers often send it unsigned).
    check_status(&doc, response)?;
    check_unique_ids(&doc)?;

    let elements = doc.elements();
    if elements
        .iter()
        .any(|e| doc.is(*e, ASSERTION_NS, "EncryptedAssertion"))
    {
        return Err(SamlError::EncryptedAssertion);
    }
    let assertions: Vec<NodeId> = elements
        .iter()
        .copied()
        .filter(|e| doc.is(*e, ASSERTION_NS, "Assertion"))
        .collect();
    let [assertion] = assertions.as_slice() else {
        return Err(invalid(format!(
            "expected one assertion, found {}",
            assertions.len()
        )));
    };
    let assertion = *assertion;
    if doc.parent(assertion) != Some(response) {
        return Err(invalid("the assertion is not a child of the response"));
    }
    for signature in elements
        .iter()
        .filter(|e| doc.is(**e, DSIG_NS, "Signature"))
    {
        let parent = doc.parent(*signature);
        if parent != Some(response) && parent != Some(assertion) {
            return Err(invalid("a signature in an unexpected place"));
        }
    }

    let keys = &check.idp.signing_keys;
    let response_signed = verify_enveloped(&doc, response, keys)?.is_some();
    let assertion_signed = verify_enveloped(&doc, assertion, keys)?.is_some();
    if !(response_signed || assertion_signed)
        || (check.require_signed_assertion && !assertion_signed)
    {
        return Err(SamlError::Signature(SignatureError::Missing));
    }

    if let Some(destination) = doc.attr(response, "Destination")
        && !same_url(destination, &check.sp.acs_url)
    {
        return Err(SamlError::WrongRecipient(destination.to_owned()));
    }
    if let Some(issuer) = doc.child(response, ASSERTION_NS, "Issuer") {
        check_issuer(&doc, issuer, check)?;
    }
    let response_in_response_to = doc.attr(response, "InResponseTo");
    match response_in_response_to {
        Some(answered) if Some(answered) != check.request_id => {
            return Err(SamlError::InResponseToMismatch);
        }
        Some(_) => {}
        None if !check.allow_idp_initiated => return Err(SamlError::Unsolicited),
        None => {}
    }

    read_assertion(&doc, assertion, response_in_response_to, check)
}

fn check_status(doc: &Document, response: NodeId) -> Result<(), SamlError> {
    let status = doc
        .child(response, PROTOCOL_NS, "Status")
        .ok_or_else(|| invalid("the response has no status"))?;
    let code = doc
        .child(status, PROTOCOL_NS, "StatusCode")
        .ok_or_else(|| invalid("the response has no status code"))?;
    let value = doc.attr(code, "Value").unwrap_or_default();
    if value == STATUS_SUCCESS {
        return Ok(());
    }
    let mut code_text = value.to_owned();
    if let Some(second) = doc
        .child(code, PROTOCOL_NS, "StatusCode")
        .and_then(|second| doc.attr(second, "Value"))
    {
        code_text = format!("{code_text}/{second}");
    }
    Err(SamlError::Status {
        code: code_text,
        message: doc
            .child(status, PROTOCOL_NS, "StatusMessage")
            .and_then(|m| doc.trimmed_text(m)),
    })
}

fn check_issuer(
    doc: &Document,
    issuer: NodeId,
    check: &ResponseCheck<'_>,
) -> Result<(), SamlError> {
    let value = doc.trimmed_text(issuer).unwrap_or_default();
    if value != check.idp.entity_id {
        return Err(SamlError::WrongIssuer(value));
    }
    Ok(())
}

fn instant_attr(
    doc: &Document,
    id: NodeId,
    name: &str,
) -> Result<Option<DateTime<Utc>>, SamlError> {
    doc.attr(id, name).map(parse_instant).transpose()
}

fn read_assertion(
    doc: &Document,
    assertion: NodeId,
    response_in_response_to: Option<&str>,
    check: &ResponseCheck<'_>,
) -> Result<SamlAssertion, SamlError> {
    let skew = TimeDelta::from_std(check.clock_skew).unwrap_or(TimeDelta::zero());
    let (earliest, latest) = (check.now - skew, check.now + skew);

    if doc.attr(assertion, "Version") != Some("2.0") {
        return Err(invalid("the assertion is not SAML 2.0"));
    }
    let id = crate::dsig::element_id(doc, assertion)
        .ok_or_else(|| invalid("the assertion has no ID"))?
        .to_owned();
    let issuer = doc
        .child(assertion, ASSERTION_NS, "Issuer")
        .ok_or_else(|| invalid("the assertion has no issuer"))?;
    check_issuer(doc, issuer, check)?;

    // Subject.
    let subject = doc
        .child(assertion, ASSERTION_NS, "Subject")
        .ok_or_else(|| invalid("the assertion has no subject"))?;
    if doc.child(subject, ASSERTION_NS, "EncryptedID").is_some() {
        return Err(SamlError::EncryptedAssertion);
    }
    let name_id_node = doc
        .child(subject, ASSERTION_NS, "NameID")
        .ok_or_else(|| invalid("the subject has no NameID"))?;
    let name_id = doc
        .trimmed_text(name_id_node)
        .ok_or_else(|| invalid("the NameID is empty"))?;

    let mut confirmed: Option<(DateTime<Utc>, Option<String>)> = None;
    let mut refusal = invalid("the subject has no bearer confirmation");
    for confirmation in doc.children_named(subject, ASSERTION_NS, "SubjectConfirmation") {
        if doc.attr(confirmation, "Method") != Some(CM_BEARER) {
            continue;
        }
        let Some(data) = doc.child(confirmation, ASSERTION_NS, "SubjectConfirmationData") else {
            continue;
        };
        match check_confirmation(doc, data, response_in_response_to, check, earliest, latest) {
            Ok(result) => {
                confirmed = Some(result);
                break;
            }
            Err(error) => refusal = error,
        }
    }
    let (mut not_on_or_after, in_response_to) = confirmed.ok_or(refusal)?;

    // Conditions.
    let conditions = doc
        .child(assertion, ASSERTION_NS, "Conditions")
        .ok_or_else(|| invalid("the assertion has no conditions"))?;
    if let Some(not_before) = instant_attr(doc, conditions, "NotBefore")?
        && latest < not_before
    {
        return Err(SamlError::NotYetValid);
    }
    if let Some(until) = instant_attr(doc, conditions, "NotOnOrAfter")? {
        if earliest >= until {
            return Err(SamlError::Expired);
        }
        not_on_or_after = not_on_or_after.min(until);
    }
    let restrictions: Vec<NodeId> = doc
        .children_named(conditions, ASSERTION_NS, "AudienceRestriction")
        .collect();
    if restrictions.is_empty() {
        return Err(SamlError::WrongAudience);
    }
    for restriction in restrictions {
        let names_us = doc
            .children_named(restriction, ASSERTION_NS, "Audience")
            .any(|audience| {
                doc.trimmed_text(audience)
                    .is_some_and(|value| same_url(&value, &check.sp.entity_id))
            });
        if !names_us {
            return Err(SamlError::WrongAudience);
        }
    }

    // Authentication statement.
    let statement = doc
        .child(assertion, ASSERTION_NS, "AuthnStatement")
        .ok_or_else(|| invalid("the assertion has no authentication statement"))?;
    if let Some(session_end) = instant_attr(doc, statement, "SessionNotOnOrAfter")?
        && earliest >= session_end
    {
        return Err(SamlError::Expired);
    }

    // Attributes.
    let mut attributes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for statement in doc.children_named(assertion, ASSERTION_NS, "AttributeStatement") {
        for attribute in doc.children_named(statement, ASSERTION_NS, "Attribute") {
            let Some(name) = doc.attr(attribute, "Name") else {
                continue;
            };
            let values = attributes.entry(name.to_owned()).or_default();
            for value in doc.children_named(attribute, ASSERTION_NS, "AttributeValue") {
                values.push(doc.text(value).trim().to_owned());
            }
        }
    }

    Ok(SamlAssertion {
        id,
        issuer: check.idp.entity_id.clone(),
        name_id,
        name_id_format: doc.attr(name_id_node, "Format").map(str::to_owned),
        session_index: doc.attr(statement, "SessionIndex").map(str::to_owned),
        authn_instant: instant_attr(doc, statement, "AuthnInstant")?,
        in_response_to,
        not_on_or_after,
        attributes,
    })
}

/// One bearer `SubjectConfirmationData`: its expiry and `InResponseTo`.
fn check_confirmation(
    doc: &Document,
    data: NodeId,
    response_in_response_to: Option<&str>,
    check: &ResponseCheck<'_>,
    earliest: DateTime<Utc>,
    latest: DateTime<Utc>,
) -> Result<(DateTime<Utc>, Option<String>), SamlError> {
    let recipient = doc
        .attr(data, "Recipient")
        .ok_or_else(|| invalid("the bearer confirmation has no Recipient"))?;
    if !same_url(recipient, &check.sp.acs_url) {
        return Err(SamlError::WrongRecipient(recipient.to_owned()));
    }
    let not_on_or_after = instant_attr(doc, data, "NotOnOrAfter")?
        .ok_or_else(|| invalid("the bearer confirmation has no NotOnOrAfter"))?;
    if earliest >= not_on_or_after {
        return Err(SamlError::Expired);
    }
    if let Some(not_before) = instant_attr(doc, data, "NotBefore")?
        && latest < not_before
    {
        return Err(SamlError::NotYetValid);
    }
    let in_response_to = doc.attr(data, "InResponseTo");
    match (in_response_to, response_in_response_to) {
        (Some(answered), _) if Some(answered) != check.request_id => {
            Err(SamlError::InResponseToMismatch)
        }
        // A solicited response confirms the subject for that request.
        (None, Some(_)) => Err(SamlError::InResponseToMismatch),
        _ => Ok((not_on_or_after, in_response_to.map(str::to_owned))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsig::{SIGNATURE_PLACEHOLDER, sign_enveloped};
    use navree_crypto::jose::SigningKey;

    const KEY: &str = include_str!("../fake/idp-key.pem");
    const CERT: &str = include_str!("../fake/idp-cert.pem");
    const REQUEST: &str = "_req1";
    const ACS: &str = "https://api.acme.test/api/sso/saml/acme/acs";
    const AUDIENCE: &str = "https://api.acme.test/api/sso/saml/acme/metadata";
    const IDP: &str = "https://idp.acme.test";

    fn now() -> DateTime<Utc> {
        parse_instant("2026-10-27T12:00:30Z").unwrap()
    }

    fn sp() -> ServiceProvider {
        ServiceProvider {
            entity_id: AUDIENCE.to_owned(),
            acs_url: ACS.to_owned(),
        }
    }

    fn idp() -> IdpMetadata {
        IdpMetadata::new(IDP, "https://idp.acme.test/sso", &[CERT]).unwrap()
    }

    /// A response template; `{sig}` marks where the assertion's signature
    /// goes.
    fn template() -> String {
        format!(
            r#"<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion" ID="_resp1" Version="2.0" IssueInstant="2026-10-27T12:00:00Z" Destination="{ACS}" InResponseTo="{REQUEST}"><saml:Issuer>{IDP}</saml:Issuer><samlp:Status><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:Success"/></samlp:Status><saml:Assertion ID="_a1" Version="2.0" IssueInstant="2026-10-27T12:00:00Z"><saml:Issuer>{IDP}</saml:Issuer>{SIGNATURE_PLACEHOLDER}<saml:Subject><saml:NameID Format="urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress">alice@acme.test</saml:NameID><saml:SubjectConfirmation Method="urn:oasis:names:tc:SAML:2.0:cm:bearer"><saml:SubjectConfirmationData InResponseTo="{REQUEST}" NotOnOrAfter="2026-10-27T12:05:00Z" Recipient="{ACS}"/></saml:SubjectConfirmation></saml:Subject><saml:Conditions NotBefore="2026-10-27T11:59:00Z" NotOnOrAfter="2026-10-27T12:10:00Z"><saml:AudienceRestriction><saml:Audience>{AUDIENCE}</saml:Audience></saml:AudienceRestriction></saml:Conditions><saml:AuthnStatement AuthnInstant="2026-10-27T11:59:59Z" SessionIndex="_s1"><saml:AuthnContext><saml:AuthnContextClassRef>urn:oasis:names:tc:SAML:2.0:ac:classes:Password</saml:AuthnContextClassRef></saml:AuthnContext></saml:AuthnStatement><saml:AttributeStatement><saml:Attribute Name="groups"><saml:AttributeValue>Admins</saml:AttributeValue><saml:AttributeValue>Sales</saml:AttributeValue></saml:Attribute></saml:AttributeStatement></saml:Assertion></samlp:Response>"#
        )
    }

    fn sign(xml: &str) -> String {
        sign_enveloped(xml, "_a1", &SigningKey::from_pem(KEY).unwrap(), None).unwrap()
    }

    fn validate_with(
        xml: &str,
        adjust: impl FnOnce(&mut ResponseCheck<'_>),
    ) -> Result<SamlAssertion, SamlError> {
        let (sp, idp) = (sp(), idp());
        let mut check = ResponseCheck {
            sp: &sp,
            idp: &idp,
            request_id: Some(REQUEST),
            now: now(),
            clock_skew: Duration::from_secs(60),
            allow_idp_initiated: false,
            require_signed_assertion: false,
        };
        adjust(&mut check);
        validate_response(&STANDARD.encode(xml), &check)
    }

    fn validate(xml: &str) -> Result<SamlAssertion, SamlError> {
        validate_with(xml, |_| {})
    }

    #[test]
    fn a_signed_assertion_is_accepted_and_read() {
        let assertion = validate(&sign(&template())).unwrap();
        assert_eq!(assertion.id, "_a1");
        assert_eq!(assertion.issuer, IDP);
        assert_eq!(assertion.name_id, "alice@acme.test");
        assert_eq!(
            assertion.name_id_format.as_deref(),
            Some("urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress")
        );
        assert_eq!(assertion.session_index.as_deref(), Some("_s1"));
        assert_eq!(assertion.in_response_to.as_deref(), Some(REQUEST));
        assert_eq!(
            assertion.not_on_or_after,
            parse_instant("2026-10-27T12:05:00Z").unwrap()
        );
        assert_eq!(assertion.values("groups"), ["Admins", "Sales"]);
        assert_eq!(assertion.attribute("groups"), Some("Admins"));
        assert_eq!(assertion.attribute("missing"), None);
    }

    #[test]
    fn an_unsigned_response_is_refused() {
        let xml = template().replace(SIGNATURE_PLACEHOLDER, "");
        assert_eq!(
            validate(&xml),
            Err(SamlError::Signature(SignatureError::Missing))
        );
    }

    #[test]
    fn an_error_status_is_reported() {
        let xml = template().replace(
            r#"<samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:Success"/>"#,
            r#"<samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:Responder"><samlp:StatusCode Value="urn:oasis:names:tc:SAML:2.0:status:AuthnFailed"/></samlp:StatusCode><samlp:StatusMessage>User is not assigned</samlp:StatusMessage>"#,
        );
        assert_eq!(
            validate(&xml),
            Err(SamlError::Status {
                code: "urn:oasis:names:tc:SAML:2.0:status:Responder/urn:oasis:names:tc:SAML:2.0:status:AuthnFailed".to_owned(),
                message: Some("User is not assigned".to_owned()),
            })
        );
    }

    #[test]
    fn time_audience_recipient_and_issuer_are_checked() {
        let signed = sign(&template());
        // 12:05 is the bearer confirmation's end, with a minute of skew.
        let at = |t: &str| parse_instant(t).unwrap();
        assert!(validate_with(&signed, |c| c.now = at("2026-10-27T12:05:59Z")).is_ok());
        assert_eq!(
            validate_with(&signed, |c| c.now = at("2026-10-27T12:06:00Z")),
            Err(SamlError::Expired)
        );
        assert_eq!(
            validate_with(&signed, |c| c.now = at("2026-10-27T11:57:59Z")),
            Err(SamlError::NotYetValid)
        );
        assert_eq!(
            validate_with(&signed, |c| c.clock_skew = Duration::ZERO),
            Ok(validate(&signed).unwrap())
        );
        let sp = ServiceProvider {
            entity_id: "https://other.test/metadata".to_owned(),
            acs_url: ACS.to_owned(),
        };
        assert_eq!(
            validate_with(&signed, |c| c.sp = Box::leak(Box::new(sp))),
            Err(SamlError::WrongAudience)
        );
        let sp = ServiceProvider {
            entity_id: AUDIENCE.to_owned(),
            acs_url: "https://other.test/acs".to_owned(),
        };
        assert_eq!(
            validate_with(&signed, |c| c.sp = Box::leak(Box::new(sp))),
            Err(SamlError::WrongRecipient(ACS.to_owned()))
        );
        let other_idp =
            IdpMetadata::new("https://evil.test", "https://evil.test/sso", &[CERT]).unwrap();
        assert_eq!(
            validate_with(&signed, |c| c.idp = Box::leak(Box::new(other_idp))),
            Err(SamlError::WrongIssuer(IDP.to_owned()))
        );
    }

    #[test]
    fn in_response_to_must_name_the_request() {
        let signed = sign(&template());
        assert_eq!(
            validate_with(&signed, |c| c.request_id = Some("_other")),
            Err(SamlError::InResponseToMismatch)
        );
        assert_eq!(
            validate_with(&signed, |c| c.request_id = None),
            Err(SamlError::InResponseToMismatch)
        );
    }

    #[test]
    fn idp_initiated_responses_need_the_switch() {
        let xml = sign(&template().replace(&format!(" InResponseTo=\"{REQUEST}\""), ""));
        assert_eq!(
            validate_with(&xml, |c| c.request_id = None),
            Err(SamlError::Unsolicited)
        );
        let assertion = validate_with(&xml, |c| {
            c.request_id = None;
            c.allow_idp_initiated = true;
        })
        .unwrap();
        assert_eq!(assertion.in_response_to, None);
    }

    #[test]
    fn required_assertion_signatures() {
        // The response signed around an unsigned assertion.
        let xml = template().replace(SIGNATURE_PLACEHOLDER, "").replace(
            &format!("<saml:Issuer>{IDP}</saml:Issuer><samlp:Status>"),
            &format!("<saml:Issuer>{IDP}</saml:Issuer>{SIGNATURE_PLACEHOLDER}<samlp:Status>"),
        );
        let signed =
            sign_enveloped(&xml, "_resp1", &SigningKey::from_pem(KEY).unwrap(), None).unwrap();
        assert!(validate(&signed).is_ok());
        assert_eq!(
            validate_with(&signed, |c| c.require_signed_assertion = true),
            Err(SamlError::Signature(SignatureError::Missing))
        );
    }

    #[test]
    fn encrypted_and_extra_assertions_are_refused() {
        let encrypted = template().replace(
            "</samlp:Response>",
            "<saml:EncryptedAssertion/></samlp:Response>",
        );
        assert_eq!(
            validate(&sign(&encrypted)),
            Err(SamlError::EncryptedAssertion)
        );
        // A second, unsigned assertion next to the signed one.
        let signed = sign(&template());
        let start = signed.find("<saml:Assertion").unwrap();
        let end = signed.find("</saml:Assertion>").unwrap() + "</saml:Assertion>".len();
        let forged = signed[start..end]
            .replace("alice@acme.test", "admin@acme.test")
            .replace("ID=\"_a1\"", "ID=\"_a2\"");
        let forged = forged.split("<ds:Signature").next().unwrap().to_owned()
            + &forged[forged.find("</ds:Signature>").unwrap() + "</ds:Signature>".len()..];
        let wrapped = signed.replacen("<saml:Assertion", &format!("{forged}<saml:Assertion"), 1);
        assert!(matches!(validate(&wrapped), Err(SamlError::Invalid(_))));
    }
}
