//! [`SsoProfile`]: who signed in, read from a SAML assertion or ID token
//! claims with a connection's [`AttributeMapping`].

use serde::Serialize;
use serde_json::{Map, Value};

use crate::AttributeMapping;
use crate::saml::SamlAssertion;

/// The person an identity provider signed in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SsoProfile {
    /// The identity provider's id for them: the SAML name id or the
    /// OpenID Connect `sub`.
    pub subject: String,
    /// The email address, lower-cased.
    pub email: Option<String>,
    /// `Some(false)` when the identity provider says the email is not
    /// verified (OpenID Connect `email_verified`); such an email never
    /// links to an existing user.
    pub email_verified: Option<bool>,
    /// The display name.
    pub name: Option<String>,
    /// The groups.
    pub groups: Vec<String>,
    /// Every attribute or claim, as the identity provider sent it.
    pub attributes: Value,
}

impl SsoProfile {
    /// The email's domain (after the last `@`) in its normalized form
    /// ([`navree_auth::email_domain`]: lower-case ASCII, no trailing dot),
    /// the form verified domains are kept in.
    pub fn email_domain(&self) -> Option<String> {
        self.email.as_deref().and_then(navree_auth::email_domain)
    }

    /// The profile of a SAML assertion.
    pub fn from_saml(assertion: &SamlAssertion, mapping: &AttributeMapping) -> SsoProfile {
        let first = |names: &[String]| {
            names
                .iter()
                .find_map(|name| assertion.attribute(name))
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        let email = first(&mapping.email)
            .or_else(|| {
                let format = assertion.name_id_format.as_deref().unwrap_or_default();
                let email_like = assertion.name_id.contains('@')
                    && !format.ends_with(":transient")
                    && !format.ends_with(":persistent");
                email_like.then(|| assertion.name_id.clone())
            })
            .map(|email| email.to_lowercase());
        let name = first(&mapping.name)
            .or_else(|| join_name(first(&mapping.first_name), first(&mapping.last_name)));
        let groups = mapping
            .groups
            .iter()
            .find(|name| !assertion.values(name).is_empty())
            .map(|name| assertion.values(name).to_vec())
            .unwrap_or_default();
        SsoProfile {
            subject: assertion.name_id.clone(),
            email,
            email_verified: None,
            name,
            groups,
            attributes: serde_json::to_value(&assertion.attributes).unwrap_or(Value::Null),
        }
    }

    /// The profile of verified ID token (and userinfo) claims.
    pub fn from_claims(claims: &Map<String, Value>, mapping: &AttributeMapping) -> SsoProfile {
        let text = |value: &Value| match value {
            Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
            _ => None,
        };
        let first = |names: &[String]| {
            names
                .iter()
                .find_map(|name| claims.get(name).and_then(text))
        };
        let name = first(&mapping.name)
            .or_else(|| join_name(first(&mapping.first_name), first(&mapping.last_name)));
        let groups = mapping
            .groups
            .iter()
            .find_map(|name| match claims.get(name) {
                Some(Value::Array(values)) => Some(values.iter().filter_map(text).collect()),
                Some(value) => text(value).map(|g| vec![g]),
                None => None,
            })
            .unwrap_or_default();
        let email_verified = match claims.get("email_verified") {
            Some(Value::Bool(verified)) => Some(*verified),
            Some(Value::String(verified)) => Some(verified == "true"),
            _ => None,
        };
        SsoProfile {
            subject: claims
                .get("sub")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            email: first(&mapping.email).map(|email| email.to_lowercase()),
            email_verified,
            name,
            groups,
            attributes: Value::Object(claims.clone()),
        }
    }
}

fn join_name(first: Option<String>, last: Option<String>) -> Option<String> {
    match (first, last) {
        (Some(first), Some(last)) => Some(format!("{first} {last}")),
        (first, last) => first.or(last),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::Utc;
    use serde_json::json;

    use super::*;

    fn assertion(name_id: &str, format: &str, attributes: &[(&str, &[&str])]) -> SamlAssertion {
        SamlAssertion {
            id: "_a".to_owned(),
            issuer: "idp".to_owned(),
            name_id: name_id.to_owned(),
            name_id_format: Some(format.to_owned()),
            session_index: None,
            authn_instant: None,
            in_response_to: None,
            not_on_or_after: Utc::now(),
            attributes: attributes
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.iter().map(|s| (*s).to_owned()).collect()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    #[test]
    fn saml_profiles_read_the_mapped_attributes() {
        let profile = SsoProfile::from_saml(
            &assertion(
                "00u1abcd",
                "urn:oasis:names:tc:SAML:2.0:nameid-format:persistent",
                &[
                    (
                        "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/emailaddress",
                        &["Bob@Acme.test"],
                    ),
                    ("firstName", &["Bob"]),
                    ("lastName", &["Builder"]),
                    ("groups", &["Admins", "Sales"]),
                ],
            ),
            &AttributeMapping::default(),
        );
        assert_eq!(profile.subject, "00u1abcd");
        assert_eq!(profile.email.as_deref(), Some("bob@acme.test"));
        assert_eq!(profile.email_domain().as_deref(), Some("acme.test"));
        assert_eq!(profile.name.as_deref(), Some("Bob Builder"));
        assert_eq!(profile.groups, ["Admins", "Sales"]);
    }

    #[test]
    fn an_email_name_id_stands_in_for_a_missing_email_attribute() {
        let email = "urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress";
        let profile = SsoProfile::from_saml(
            &assertion("Ann@Acme.test", email, &[]),
            &AttributeMapping::default(),
        );
        assert_eq!(profile.email.as_deref(), Some("ann@acme.test"));
        // A transient id that happens to contain `@` is not an email.
        let transient = "urn:oasis:names:tc:SAML:2.0:nameid-format:transient";
        let profile = SsoProfile::from_saml(
            &assertion("x@y", transient, &[]),
            &AttributeMapping::default(),
        );
        assert_eq!(profile.email, None);
    }

    /// The profile's domain is the one navree-auth gives policies.
    #[test]
    fn the_email_domain_is_normalized() {
        for email in ["Eve@ACME.test.", "eve@\u{ff41}cme\u{3002}test"] {
            let claims = json!({ "sub": "u1", "email": email });
            let profile =
                SsoProfile::from_claims(claims.as_object().unwrap(), &AttributeMapping::default());
            assert_eq!(
                profile.email_domain().as_deref(),
                Some("acme.test"),
                "{email}"
            );
        }
    }

    #[test]
    fn claims_profiles_read_standard_claims() {
        let claims = json!({
            "sub": "248289761001",
            "email": "Jane@Acme.test",
            "email_verified": true,
            "given_name": "Jane",
            "family_name": "Doe",
            "groups": ["Engineering"],
        });
        let profile =
            SsoProfile::from_claims(claims.as_object().unwrap(), &AttributeMapping::default());
        assert_eq!(profile.subject, "248289761001");
        assert_eq!(profile.email.as_deref(), Some("jane@acme.test"));
        assert_eq!(profile.email_verified, Some(true));
        assert_eq!(profile.name.as_deref(), Some("Jane Doe"));
        assert_eq!(profile.groups, ["Engineering"]);
        // A single group as a string.
        let claims = json!({ "sub": "1", "groups": "Admins", "email_verified": "false" });
        let profile =
            SsoProfile::from_claims(claims.as_object().unwrap(), &AttributeMapping::default());
        assert_eq!(profile.groups, ["Admins"]);
        assert_eq!(profile.email_verified, Some(false));
    }
}
