//! [`SsoFake`]: a SAML identity provider for tests.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, TimeDelta, Utc};
use navree_crypto::jose::SigningKey;

use crate::dsig::{SIGNATURE_PLACEHOLDER, sign_enveloped};
use crate::saml::{
    ASSERTION_NS, AuthnRequest, CM_BEARER, IdpMetadata, PROTOCOL_NS, STATUS_SUCCESS, SamlError,
    ServiceProvider, format_instant, new_id, parse_instant,
};
use crate::xml::{Document, escape};

/// A SAML identity provider for tests: its metadata, and responses signed
/// like a real identity provider's (RSA-SHA256 over the exclusive
/// canonical form, the certificate in `KeyInfo`), which pass the same
/// checks as any other.
///
/// Give a connection [`metadata_xml`](SsoFake::metadata_xml), send the
/// browser to the connection's login route, then answer the request the
/// login route redirected to:
///
/// ```
/// use navree_sso::SsoFake;
/// use navree_sso::saml::{AuthnRequest, RequestOptions, ServiceProvider};
///
/// let idp = SsoFake::new();
/// let sp = ServiceProvider {
///     entity_id: "https://api.acme.test/api/sso/saml/acme/metadata".into(),
///     acs_url: "https://api.acme.test/api/sso/saml/acme/acs".into(),
/// };
/// // What `GET /api/sso/acme/login` redirects to:
/// let request = AuthnRequest::new(
///     &sp, &idp.metadata(), "relay", chrono::Utc::now(), &RequestOptions::default(),
/// )?;
/// let response = idp
///     .respond_to(&request.url)?
///     .name_id("alice@acme.test")
///     .attribute("groups", ["Admins"]);
/// // POST these to the assertion consumer service.
/// let form = response.form();
/// assert_eq!(form[0].0, "SAMLResponse");
/// assert_eq!(form[1], ("RelayState".to_owned(), "relay".to_owned()));
/// # Ok::<(), navree_sso::saml::SamlError>(())
/// ```
///
/// Its key and certificate are fixed and published with Navree: never
/// trust [`CERTIFICATE_PEM`](SsoFake::CERTIFICATE_PEM) outside tests.
#[derive(Clone, Debug)]
pub struct SsoFake {
    entity_id: String,
    sso_url: String,
    key: SigningKey,
}

impl Default for SsoFake {
    fn default() -> Self {
        SsoFake::new()
    }
}

impl SsoFake {
    /// The fake's signing certificate (RSA 2048, self-signed).
    pub const CERTIFICATE_PEM: &'static str = include_str!("fake/idp-cert.pem");
    /// The fake's default entity id.
    pub const ENTITY_ID: &'static str = "https://idp.sso-fake.test";

    /// The identity provider `https://idp.sso-fake.test`.
    pub fn new() -> SsoFake {
        SsoFake::with_entity_id(Self::ENTITY_ID)
    }

    /// An identity provider with another entity id (its sign-on URL is
    /// `{entity_id}/sso`).
    pub fn with_entity_id(entity_id: impl Into<String>) -> SsoFake {
        let entity_id = entity_id.into();
        SsoFake {
            sso_url: format!("{}/sso", entity_id.trim_end_matches('/')),
            entity_id,
            key: SigningKey::from_pem(include_str!("fake/idp-key.pem"))
                .expect("the fake identity provider's key loads"),
        }
    }

    /// The entity id (the `Issuer` of its responses).
    pub fn entity_id(&self) -> &str {
        &self.entity_id
    }

    /// The single sign-on URL (HTTP-Redirect).
    pub fn sso_url(&self) -> &str {
        &self.sso_url
    }

    /// The metadata XML to configure a connection with.
    pub fn metadata_xml(&self) -> String {
        let certificate: String = Self::CERTIFICATE_PEM
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <md:EntityDescriptor xmlns:md=\"urn:oasis:names:tc:SAML:2.0:metadata\" \
             xmlns:ds=\"http://www.w3.org/2000/09/xmldsig#\" entityID=\"{entity}\">\
             <md:IDPSSODescriptor WantAuthnRequestsSigned=\"false\" \
             protocolSupportEnumeration=\"urn:oasis:names:tc:SAML:2.0:protocol\">\
             <md:KeyDescriptor use=\"signing\"><ds:KeyInfo><ds:X509Data>\
             <ds:X509Certificate>{certificate}</ds:X509Certificate>\
             </ds:X509Data></ds:KeyInfo></md:KeyDescriptor>\
             <md:NameIDFormat>urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress</md:NameIDFormat>\
             <md:SingleSignOnService Binding=\"urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect\" Location=\"{sso}\"/>\
             <md:SingleSignOnService Binding=\"urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST\" Location=\"{sso}\"/>\
             </md:IDPSSODescriptor></md:EntityDescriptor>",
            entity = escape(&self.entity_id),
            sso = escape(&self.sso_url),
        )
    }

    /// The metadata, parsed.
    pub fn metadata(&self) -> IdpMetadata {
        IdpMetadata::parse(&self.metadata_xml()).expect("the fake's metadata parses")
    }

    /// A response to the request in `login_url` (where the login route
    /// redirected the browser): for its `ID`, assertion consumer service,
    /// issuer (the audience), relay state and issue instant.
    pub fn respond_to(&self, login_url: &str) -> Result<FakeResponse, SamlError> {
        let (xml, relay_state) = AuthnRequest::decode_redirect(login_url)?;
        let doc = Document::parse(&xml)?;
        let root = doc.root();
        if !doc.is(root, PROTOCOL_NS, "AuthnRequest") {
            return Err(SamlError::Invalid("not an AuthnRequest".to_owned()));
        }
        let acs = doc
            .attr(root, "AssertionConsumerServiceURL")
            .ok_or_else(|| SamlError::Invalid("the request names no ACS URL".to_owned()))?;
        let audience = doc
            .child(root, ASSERTION_NS, "Issuer")
            .and_then(|issuer| doc.trimmed_text(issuer))
            .ok_or_else(|| SamlError::Invalid("the request has no issuer".to_owned()))?;
        let issued_at = match doc.attr(root, "IssueInstant") {
            Some(instant) => parse_instant(instant)?,
            None => Utc::now(),
        };
        let mut response = FakeResponse::new(self.clone(), acs, &audience, issued_at);
        response.in_response_to = doc.attr(root, "ID").map(str::to_owned);
        response.relay_state = relay_state;
        Ok(response)
    }

    /// An IdP-initiated response to `sp` (no request), issued at
    /// `issued_at`.
    pub fn unsolicited(&self, sp: &ServiceProvider, issued_at: DateTime<Utc>) -> FakeResponse {
        FakeResponse::new(self.clone(), &sp.acs_url, &sp.entity_id, issued_at)
    }
}

/// A response from [`SsoFake`]. By default: success, the assertion signed,
/// the name id `user@sso-fake.test` (email format), valid for five
/// minutes.
#[derive(Clone, Debug)]
pub struct FakeResponse {
    idp: SsoFake,
    acs_url: String,
    audience: String,
    issued_at: DateTime<Utc>,
    lifetime: Duration,
    in_response_to: Option<String>,
    relay_state: Option<String>,
    name_id: String,
    name_id_format: String,
    attributes: Vec<(String, Vec<String>)>,
    sign_response: bool,
    sign_assertion: bool,
    status: Option<(String, Option<String>)>,
    assertion_id: String,
}

impl FakeResponse {
    fn new(idp: SsoFake, acs_url: &str, audience: &str, issued_at: DateTime<Utc>) -> FakeResponse {
        FakeResponse {
            idp,
            acs_url: acs_url.to_owned(),
            audience: audience.to_owned(),
            issued_at,
            lifetime: Duration::from_secs(300),
            in_response_to: None,
            relay_state: None,
            name_id: "user@sso-fake.test".to_owned(),
            name_id_format: "urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress".to_owned(),
            attributes: Vec::new(),
            sign_response: false,
            sign_assertion: true,
            status: None,
            assertion_id: new_id(),
        }
    }

    /// The subject's name id.
    #[must_use]
    pub fn name_id(mut self, name_id: impl Into<String>) -> Self {
        self.name_id = name_id.into();
        self
    }

    /// The name id's format (`urn:...:persistent`, say).
    #[must_use]
    pub fn name_id_format(mut self, format: impl Into<String>) -> Self {
        self.name_id_format = format.into();
        self
    }

    /// Adds attribute `name` with `values`.
    #[must_use]
    pub fn attribute<I, V>(mut self, name: impl Into<String>, values: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<String>,
    {
        self.attributes
            .push((name.into(), values.into_iter().map(Into::into).collect()));
        self
    }

    /// When the response was issued (by default the request's
    /// `IssueInstant`).
    #[must_use]
    pub fn issued_at(mut self, instant: DateTime<Utc>) -> Self {
        self.issued_at = instant;
        self
    }

    /// How long the assertion stays valid.
    #[must_use]
    pub fn valid_for(mut self, lifetime: Duration) -> Self {
        self.lifetime = lifetime;
        self
    }

    /// Whether the response element is signed (default no).
    #[must_use]
    pub fn sign_response(mut self, sign: bool) -> Self {
        self.sign_response = sign;
        self
    }

    /// Whether the assertion is signed (default yes).
    #[must_use]
    pub fn sign_assertion(mut self, sign: bool) -> Self {
        self.sign_assertion = sign;
        self
    }

    /// The audience (by default the request's issuer).
    #[must_use]
    pub fn audience(mut self, audience: impl Into<String>) -> Self {
        self.audience = audience.into();
        self
    }

    /// The `InResponseTo` (by default the request's id).
    #[must_use]
    pub fn in_response_to(mut self, request_id: Option<String>) -> Self {
        self.in_response_to = request_id;
        self
    }

    /// The relay state posted back (by default the request's).
    #[must_use]
    pub fn relay_state(mut self, relay_state: Option<String>) -> Self {
        self.relay_state = relay_state;
        self
    }

    /// The id of the assertion (fresh by default; reuse one to test
    /// replays).
    #[must_use]
    pub fn assertion_id(mut self, id: impl Into<String>) -> Self {
        self.assertion_id = id.into();
        self
    }

    /// A failed response with status `code` (a SAML status URI) and an
    /// optional message, and no assertion.
    #[must_use]
    pub fn failure(mut self, code: impl Into<String>, message: Option<&str>) -> Self {
        self.status = Some((code.into(), message.map(str::to_owned)));
        self
    }

    /// The response XML, signed.
    pub fn xml(&self) -> String {
        let at = format_instant(self.issued_at);
        let until = format_instant(
            self.issued_at + TimeDelta::from_std(self.lifetime).unwrap_or(TimeDelta::zero()),
        );
        let not_before = format_instant(self.issued_at - TimeDelta::seconds(30));
        let in_response_to = self
            .in_response_to
            .as_deref()
            .map(|id| format!(" InResponseTo=\"{}\"", escape(id)))
            .unwrap_or_default();
        let entity = escape(&self.idp.entity_id);
        let acs = escape(&self.acs_url);
        let response_id = new_id();
        let (status, assertion) = match &self.status {
            Some((code, message)) => {
                let message = message
                    .as_deref()
                    .map(|m| format!("<samlp:StatusMessage>{}</samlp:StatusMessage>", escape(m)))
                    .unwrap_or_default();
                (
                    format!("<samlp:StatusCode Value=\"{}\"/>{message}", escape(code)),
                    String::new(),
                )
            }
            None => (
                format!("<samlp:StatusCode Value=\"{STATUS_SUCCESS}\"/>"),
                self.assertion_xml(&at, &until, &not_before, &in_response_to),
            ),
        };
        let mut xml = format!(
            "<samlp:Response xmlns:samlp=\"{PROTOCOL_NS}\" xmlns:saml=\"{ASSERTION_NS}\" \
             ID=\"{response_id}\" Version=\"2.0\" IssueInstant=\"{at}\" Destination=\"{acs}\"{in_response_to}>\
             <saml:Issuer>{entity}</saml:Issuer><samlp:Status>{status}</samlp:Status>{assertion}</samlp:Response>"
        );
        if self.sign_assertion && self.status.is_none() {
            xml = sign_enveloped(&xml, &self.assertion_id, &self.idp.key, Some(&self.certificate()))
                .expect("the fake signs its assertion");
        } else {
            xml = xml.replacen(SIGNATURE_PLACEHOLDER, "", 1);
        }
        if self.sign_response {
            let issuer_end = xml.find("</saml:Issuer>").expect("the response has an issuer")
                + "</saml:Issuer>".len();
            xml.insert_str(issuer_end, SIGNATURE_PLACEHOLDER);
            xml = sign_enveloped(&xml, &response_id, &self.idp.key, Some(&self.certificate()))
                .expect("the fake signs its response");
        }
        xml
    }

    fn assertion_xml(&self, at: &str, until: &str, not_before: &str, in_response_to: &str) -> String {
        let entity = escape(&self.idp.entity_id);
        let acs = escape(&self.acs_url);
        let mut attributes = String::new();
        if !self.attributes.is_empty() {
            attributes.push_str("<saml:AttributeStatement>");
            for (name, values) in &self.attributes {
                attributes.push_str(&format!("<saml:Attribute Name=\"{}\">", escape(name)));
                for value in values {
                    attributes.push_str(&format!(
                        "<saml:AttributeValue>{}</saml:AttributeValue>",
                        escape(value)
                    ));
                }
                attributes.push_str("</saml:Attribute>");
            }
            attributes.push_str("</saml:AttributeStatement>");
        }
        format!(
            "<saml:Assertion ID=\"{id}\" Version=\"2.0\" IssueInstant=\"{at}\">\
             <saml:Issuer>{entity}</saml:Issuer>{SIGNATURE_PLACEHOLDER}\
             <saml:Subject><saml:NameID Format=\"{format}\">{name_id}</saml:NameID>\
             <saml:SubjectConfirmation Method=\"{CM_BEARER}\">\
             <saml:SubjectConfirmationData{in_response_to} NotOnOrAfter=\"{until}\" Recipient=\"{acs}\"/>\
             </saml:SubjectConfirmation></saml:Subject>\
             <saml:Conditions NotBefore=\"{not_before}\" NotOnOrAfter=\"{until}\">\
             <saml:AudienceRestriction><saml:Audience>{audience}</saml:Audience></saml:AudienceRestriction>\
             </saml:Conditions>\
             <saml:AuthnStatement AuthnInstant=\"{at}\" SessionIndex=\"{id}\"><saml:AuthnContext>\
             <saml:AuthnContextClassRef>urn:oasis:names:tc:SAML:2.0:ac:classes:PasswordProtectedTransport</saml:AuthnContextClassRef>\
             </saml:AuthnContext></saml:AuthnStatement>{attributes}</saml:Assertion>",
            id = escape(&self.assertion_id),
            format = escape(&self.name_id_format),
            name_id = escape(&self.name_id),
            audience = escape(&self.audience),
        )
    }

    fn certificate(&self) -> Vec<u8> {
        crate::saml::certificate_der(SsoFake::CERTIFICATE_PEM).expect("the fake's certificate")
    }

    /// The base64 `SAMLResponse`.
    pub fn encoded(&self) -> String {
        STANDARD.encode(self.xml())
    }

    /// The form to POST to the assertion consumer service:
    /// `SAMLResponse`, and `RelayState` when there is one.
    pub fn form(&self) -> Vec<(String, String)> {
        let mut form = vec![("SAMLResponse".to_owned(), self.encoded())];
        if let Some(relay_state) = &self.relay_state {
            form.push(("RelayState".to_owned(), relay_state.clone()));
        }
        form
    }

    /// The assertion consumer service URL the response is for.
    pub fn acs_url(&self) -> &str {
        &self.acs_url
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saml::{RequestOptions, ResponseCheck, SamlError, validate_response};

    fn sp() -> ServiceProvider {
        ServiceProvider {
            entity_id: "https://api.acme.test/sso/acme/metadata".to_owned(),
            acs_url: "https://api.acme.test/sso/acme/acs".to_owned(),
        }
    }

    fn check<'a>(
        sp: &'a ServiceProvider,
        idp: &'a IdpMetadata,
        request_id: Option<&'a str>,
        now: DateTime<Utc>,
    ) -> ResponseCheck<'a> {
        ResponseCheck {
            sp,
            idp,
            request_id,
            now,
            clock_skew: Duration::from_secs(60),
            allow_idp_initiated: true,
            require_signed_assertion: false,
        }
    }

    #[test]
    fn responses_to_a_request_pass_the_checks() {
        let fake = SsoFake::new();
        let (sp, idp) = (sp(), fake.metadata());
        let now = Utc::now();
        let request = AuthnRequest::new(&sp, &idp, "rs", now, &RequestOptions::default()).unwrap();
        assert!(request.url.starts_with("https://idp.sso-fake.test/sso?"));
        for (sign_response, sign_assertion) in [(false, true), (true, false), (true, true)] {
            let response = fake
                .respond_to(&request.url)
                .unwrap()
                .name_id("alice@acme.test")
                .attribute("groups", ["Admins", "Sales & Ops"])
                .sign_response(sign_response)
                .sign_assertion(sign_assertion);
            let assertion =
                validate_response(&response.encoded(), &check(&sp, &idp, Some(&request.id), now))
                    .unwrap();
            assert_eq!(assertion.name_id, "alice@acme.test");
            assert_eq!(assertion.values("groups"), ["Admins", "Sales & Ops"]);
            assert_eq!(response.form()[1].1, "rs");
        }
    }

    #[test]
    fn failures_unsigned_and_unsolicited_responses() {
        let fake = SsoFake::new();
        let (sp, idp) = (sp(), fake.metadata());
        let now = Utc::now();
        let failed = fake
            .unsolicited(&sp, now)
            .failure("urn:oasis:names:tc:SAML:2.0:status:Requester", Some("no"));
        assert!(matches!(
            validate_response(&failed.encoded(), &check(&sp, &idp, None, now)),
            Err(SamlError::Status { .. })
        ));
        let unsigned = fake.unsolicited(&sp, now).sign_assertion(false);
        assert!(matches!(
            validate_response(&unsigned.encoded(), &check(&sp, &idp, None, now)),
            Err(SamlError::Signature(_))
        ));
        let unsolicited = fake.unsolicited(&sp, now).name_id("bob@acme.test");
        let assertion =
            validate_response(&unsolicited.encoded(), &check(&sp, &idp, None, now)).unwrap();
        assert_eq!(assertion.name_id, "bob@acme.test");
        assert!(unsolicited.form().len() == 1);
    }

    #[test]
    fn another_entity_id_moves_the_sign_on_url() {
        let fake = SsoFake::with_entity_id("https://idp.other.test/");
        assert_eq!(fake.sso_url(), "https://idp.other.test/sso");
        assert_eq!(fake.metadata().entity_id, "https://idp.other.test/");
    }
}
