//! Sign-ins as telemetry: every sign-in flow writes its outcome to the audit
//! trail, and this turns those entries into [`SignInEvent`]s for the sinks
//! (Logbook shows them). The events hold the method, the outcome, a reason
//! code and, for a sign-in that worked, the user's id; never the email
//! address the audit entry carries.

use navree_contracts::AuditEntry;
use navree_telemetry::{SignInEvent, SignInMethod};
use serde_json::Value;

/// The sign-in event an audit entry stands for, or `None` when the entry is
/// about something else.
pub(crate) fn sign_in_of(entry: &AuditEntry) -> Option<SignInEvent> {
    let text = |key: &str| entry.metadata.get(key).and_then(Value::as_str);
    let method = method_of(text("method"));
    let event = match entry.action.as_str() {
        "auth.login" => {
            let method = if entry.metadata.get("two_factor").is_some() {
                SignInMethod::TwoFactor
            } else {
                method
            };
            SignInEvent::succeeded(method, entry.actor_id?.to_string())
        }
        "auth.token_exchanged" => {
            SignInEvent::succeeded(SignInMethod::TokenExchange, entry.actor_id?.to_string())
        }
        "auth.login_failed" => {
            SignInEvent::refused(method, text("reason").unwrap_or("invalid_credentials"))
        }
        "auth.login_refused" => SignInEvent::refused(method, text("code").unwrap_or("refused")),
        "auth.two_factor_failed" => SignInEvent::refused(SignInMethod::TwoFactor, "invalid_code"),
        "auth.throttled" if text("flow") == Some("login") => {
            SignInEvent::refused(SignInMethod::Password, "throttled")
        }
        _ => return None,
    };
    let provider = ["provider", "connection", "issuer"]
        .into_iter()
        .find_map(text);
    Some(match provider {
        Some(provider) => event.provider(provider),
        None => event,
    })
}

/// Reports that `user` signed in with `method` through
/// [`AuthService::sign_in`](crate::AuthService::sign_in) or
/// [`sign_in_to_session`](crate::AuthService::sign_in_to_session): the way
/// social sign-in, single sign-on and an app's own flows end.
pub(crate) fn report_signed_in(user: &crate::User, method: &crate::SignInMethod) {
    navree_telemetry::Telemetry::current().sign_in(|| {
        let (kind, provider) = match method {
            crate::SignInMethod::Password => (SignInMethod::Password, None),
            crate::SignInMethod::MagicLink => (SignInMethod::MagicLink, None),
            crate::SignInMethod::Otp => (SignInMethod::Code, None),
            crate::SignInMethod::Passkey => (SignInMethod::Passkey, None),
            crate::SignInMethod::Social { provider } => (SignInMethod::Social, Some(provider)),
            crate::SignInMethod::Sso { connection } => (SignInMethod::Sso, Some(connection)),
            crate::SignInMethod::TokenExchange { issuer } => {
                (SignInMethod::TokenExchange, Some(issuer))
            }
            // A sign-in handed over to an app: a token exchange, named by the app.
            crate::SignInMethod::AppHandOver { app, .. } => {
                (SignInMethod::TokenExchange, Some(app))
            }
        };
        let event = SignInEvent::succeeded(kind, user.id.to_string());
        match provider {
            Some(provider) => event.provider(provider.as_str()),
            None => event,
        }
    });
}

/// The method an audit entry's `method` names; a password when it names none.
fn method_of(name: Option<&str>) -> SignInMethod {
    match name {
        Some("magic_link") => SignInMethod::MagicLink,
        Some("otp") => SignInMethod::Code,
        Some("passkey") => SignInMethod::Passkey,
        Some("social") => SignInMethod::Social,
        Some("sso") => SignInMethod::Sso,
        Some("token_exchange") => SignInMethod::TokenExchange,
        _ => SignInMethod::Password,
    }
}

#[cfg(test)]
mod tests {
    use navree_contracts::AuditKind;
    use navree_telemetry::Outcome;
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn entry(action: &str, metadata: Value) -> AuditEntry {
        AuditEntry::new(AuditKind::Auth, action).metadata(metadata)
    }

    #[test]
    fn a_password_sign_in_that_worked_names_the_user() {
        let user = Uuid::now_v7();
        let event = sign_in_of(&entry("auth.login", json!({})).actor(user)).unwrap();
        assert_eq!(event.method, SignInMethod::Password);
        assert_eq!(event.outcome, Outcome::Success);
        assert_eq!(event.user_id, Some(user.to_string()));
        assert_eq!(event.reason, None);
    }

    #[test]
    fn the_other_ways_in_have_their_own_methods() {
        let user = Uuid::now_v7();
        for (metadata, method) in [
            (json!({ "method": "passkey" }), SignInMethod::Passkey),
            (json!({ "method": "magic_link" }), SignInMethod::MagicLink),
            (json!({ "method": "otp" }), SignInMethod::Code),
            (json!({ "two_factor": "totp" }), SignInMethod::TwoFactor),
            (
                json!({ "via": "session", "remember": true }),
                SignInMethod::Password,
            ),
        ] {
            let event = sign_in_of(&entry("auth.login", metadata.clone()).actor(user)).unwrap();
            assert_eq!(event.method, method, "{metadata}");
        }
        let exchanged = sign_in_of(
            &entry("auth.token_exchanged", json!({ "issuer": "partner-idp" })).actor(user),
        )
        .unwrap();
        assert_eq!(exchanged.method, SignInMethod::TokenExchange);
        assert_eq!(exchanged.provider.as_deref(), Some("partner-idp"));
    }

    #[test]
    fn a_refusal_names_a_reason_and_never_the_email() {
        let failed = sign_in_of(&entry(
            "auth.login_failed",
            json!({ "email": "anu@example.com" }),
        ))
        .unwrap();
        assert_eq!(failed.outcome, Outcome::Failure);
        assert_eq!(failed.reason.as_deref(), Some("invalid_credentials"));
        assert_eq!(failed.user_id, None);

        let refused = sign_in_of(&entry(
            "auth.login_refused",
            json!({ "method": "sso", "connection": "okta-1", "email": "anu@example.com", "code": "sso_required" }),
        ))
        .unwrap();
        assert_eq!(refused.method, SignInMethod::Sso);
        assert_eq!(refused.provider.as_deref(), Some("okta-1"));
        assert_eq!(refused.reason.as_deref(), Some("sso_required"));

        let wrong_code = sign_in_of(&entry(
            "auth.two_factor_failed",
            json!({ "method": "totp" }),
        ))
        .unwrap();
        assert_eq!(wrong_code.method, SignInMethod::TwoFactor);
        assert_eq!(wrong_code.reason.as_deref(), Some("invalid_code"));

        for event in [&failed, &refused, &wrong_code] {
            let text = serde_json::to_string(event).unwrap();
            assert!(!text.contains("anu@example.com"), "{text}");
        }
    }

    #[test]
    fn a_throttled_login_is_a_refusal_and_other_flows_are_not() {
        let throttled = sign_in_of(&entry("auth.throttled", json!({ "flow": "login" }))).unwrap();
        assert_eq!(throttled.reason.as_deref(), Some("throttled"));
        assert!(
            sign_in_of(&entry(
                "auth.throttled",
                json!({ "flow": "password_reset" })
            ))
            .is_none()
        );
        assert!(sign_in_of(&entry("auth.logout", json!({}))).is_none());
        assert!(sign_in_of(&entry("auth.register", json!({}))).is_none());
    }

    #[test]
    fn a_sign_in_without_a_user_is_not_reported_as_a_success() {
        assert!(sign_in_of(&entry("auth.login", json!({}))).is_none());
    }
}
