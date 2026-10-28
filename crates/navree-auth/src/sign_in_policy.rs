//! [`SignInPolicy`]: one decision, consulted by every way to a session.
//!
//! Every flow that signs a person in asks the registered policies before
//! it issues anything (a session, a token pair, a two-factor challenge):
//! passwords (`POST /auth/login`, registration, a password change),
//! magic links and emailed codes (when one is requested and when it is
//! used), passkeys, trusted token exchange, the second step of a
//! two-factor sign-in, social sign-in (navree-social), single sign-on
//! (navree-sso) and the platform audience. Each passes a
//! [`SignInAttempt`]: the email of the account, its id when known, the
//! [`SignInMethod`] and the [`SignInAudience`].
//!
//! Single sign-on registers a policy that refuses every method but its own
//! connection for the verified domains of an enforcing connection (403
//! `sso_required`).

use std::fmt;

use navree_contracts::AuditKind;
use navree_kernel::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::service::{entry, normalize_email};
use crate::{AuthError, AuthService, ClientMeta, TokenPair, User};

/// How a person proves who they are.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SignInMethod {
    /// An email and a password (sign-in, registration, a password change).
    Password,
    /// A magic link (`POST /auth/magic-link`).
    MagicLink,
    /// An emailed sign-in code (`POST /auth/otp`).
    Otp,
    /// A passkey (`POST /auth/passkeys/login`).
    Passkey,
    /// A social sign-in provider (navree-social), by its key (`github`).
    Social {
        /// The provider's key.
        provider: String,
    },
    /// A single sign-on connection (navree-sso), by its id.
    Sso {
        /// The connection's id.
        connection: String,
    },
    /// A token of a trusted issuer (`POST /auth/exchange`), by the
    /// issuer's name under `auth.trusted_issuers`.
    TokenExchange {
        /// The issuer's name.
        issuer: String,
    },
}

impl SignInMethod {
    /// The method's name, as audit entries carry it: `password`,
    /// `magic_link`, `otp`, `passkey`, `social`, `sso`, `token_exchange`.
    pub fn name(&self) -> &'static str {
        match self {
            SignInMethod::Password => "password",
            SignInMethod::MagicLink => "magic_link",
            SignInMethod::Otp => "otp",
            SignInMethod::Passkey => "passkey",
            SignInMethod::Social { .. } => "social",
            SignInMethod::Sso { .. } => "sso",
            SignInMethod::TokenExchange { .. } => "token_exchange",
        }
    }

    /// A social sign-in with `provider`.
    pub fn social(provider: impl Into<String>) -> Self {
        SignInMethod::Social {
            provider: provider.into(),
        }
    }

    /// A single sign-on through `connection` (its id).
    pub fn sso(connection: impl fmt::Display) -> Self {
        SignInMethod::Sso {
            connection: connection.to_string(),
        }
    }

    /// The audit metadata of the method: its name plus the provider,
    /// connection or issuer.
    fn metadata(&self) -> serde_json::Value {
        let mut value = json!({ "method": self.name() });
        match self {
            SignInMethod::Social { provider } => value["provider"] = json!(provider),
            SignInMethod::Sso { connection } => value["connection"] = json!(connection),
            SignInMethod::TokenExchange { issuer } => value["issuer"] = json!(issuer),
            _ => {}
        }
        value
    }
}

/// Where a sign-in leads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SignInAudience {
    /// The app.
    App,
    /// The platform console (`auth.platform`).
    Platform,
}

/// A sign-in a [`SignInPolicy`] decides on.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SignInAttempt {
    /// The account's email, trimmed and lower-cased (the address a
    /// sign-in asks for, before the account is known).
    pub email: String,
    /// The account, once known.
    pub user_id: Option<Uuid>,
    /// How the person signs in.
    pub method: SignInMethod,
    /// Where the sign-in leads.
    pub audience: SignInAudience,
}

impl SignInAttempt {
    /// A sign-in to the app as `email` (trimmed and lower-cased here) with
    /// `method`.
    pub fn new(email: &str, method: SignInMethod) -> Self {
        Self {
            email: normalize_email(email),
            user_id: None,
            method,
            audience: SignInAudience::App,
        }
    }

    /// The sign-in of `user` with `method`.
    pub fn of(user: &User, method: SignInMethod) -> Self {
        Self::new(&user.email, method).user(user.id)
    }

    /// The account the sign-in is for.
    pub fn user(mut self, user_id: Uuid) -> Self {
        self.user_id = Some(user_id);
        self
    }

    /// Where the sign-in leads.
    pub fn audience(mut self, audience: SignInAudience) -> Self {
        self.audience = audience;
        self
    }

    /// The email's domain (after the last `@`), if it has one.
    pub fn domain(&self) -> Option<&str> {
        self.email.rsplit_once('@').map(|(_, domain)| domain)
    }
}

/// Decides whether a sign-in may go on. Every flow that signs a person in
/// asks every registered policy before it issues a session, tokens or a
/// two-factor challenge (see the [module documentation](self)).
///
/// Register implementations as `dyn SignInPolicy` multi-bindings, most
/// simply with
/// [`ModuleBuilderAuthExt::sign_in_policy`](crate::ModuleBuilderAuthExt::sign_in_policy).
/// Every registered policy must allow; the first refusal is the answer,
/// usually [`AuthError::SignInRefused`] (a 403 with the policy's code),
/// audited as `auth.login_refused` with the method, the email and the code.
pub trait SignInPolicy: Send + Sync + 'static {
    /// `Ok(())` lets `attempt` go on.
    fn check<'a>(&'a self, attempt: &'a SignInAttempt) -> BoxFuture<'a, Result<(), AuthError>>;
}

impl AuthService {
    /// Asks every registered [`SignInPolicy`] about `attempt`: `Ok(())`,
    /// or the first refusal, audited as `auth.login_refused` (with the
    /// email, the method, the audience and the refusal's code) and logged
    /// under `navree::security`. Every built-in sign-in flow calls it
    /// before it issues anything; call it from your own.
    pub async fn check_sign_in(
        &self,
        attempt: &SignInAttempt,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        let Err(refused) = self.evaluate_sign_in(attempt).await else {
            return Ok(());
        };
        if let AuthError::SignInRefused { code, .. } = &refused {
            self.audit_refusal(attempt, code, meta).await;
        }
        Err(refused)
    }

    /// Whether `email` may use a password: [`check_sign_in`](AuthService::check_sign_in)
    /// with [`SignInMethod::Password`], without the audit entry.
    pub async fn check_password_sign_in(&self, email: &str) -> Result<(), AuthError> {
        self.evaluate_sign_in(&SignInAttempt::new(email, SignInMethod::Password))
            .await
    }

    /// Signs `user` in with `method`: [`check_sign_in`](AuthService::check_sign_in),
    /// then [`issue_tokens`](AuthService::issue_tokens). What a sign-in flow
    /// of another crate (social sign-in, single sign-on) ends with.
    pub async fn sign_in(
        &self,
        user: &User,
        method: SignInMethod,
        meta: &ClientMeta,
    ) -> Result<TokenPair, AuthError> {
        self.check_sign_in(&SignInAttempt::of(user, method), meta)
            .await?;
        self.issue_tokens(user, meta.clone()).await
    }

    /// The two-factor challenge token of `user` signing in with `method`,
    /// for [`verify_two_factor`](AuthService::verify_two_factor) (`POST
    /// /auth/2fa/verify`), which asks the policies about `method` again
    /// before it starts the session. Ask them yourself before handing it
    /// out ([`check_sign_in`](AuthService::check_sign_in)).
    pub fn two_factor_challenge(
        &self,
        user: &User,
        method: &SignInMethod,
    ) -> Result<String, AuthError> {
        self.sign_challenge(user, method)
    }

    /// The first refusal of the registered policies, without auditing.
    pub(crate) async fn evaluate_sign_in(&self, attempt: &SignInAttempt) -> Result<(), AuthError> {
        for policy in self.sign_in_policies().await? {
            policy.check(attempt).await?;
        }
        Ok(())
    }

    /// Audits and logs a refusal of `attempt` with `code`.
    pub(crate) async fn audit_refusal(&self, attempt: &SignInAttempt, code: &str, meta: &ClientMeta) {
        tracing::warn!(
            target: "navree::security",
            ip = meta.ip.as_deref().unwrap_or("-"),
            method = attempt.method.name(),
            code,
            "sign-in refused by a sign-in policy"
        );
        let mut metadata = attempt.method.metadata();
        metadata["email"] = json!(attempt.email);
        metadata["code"] = json!(code);
        metadata["audience"] = json!(audience_name(attempt.audience));
        let mut refused = entry(AuditKind::Security, "auth.login_refused", meta).metadata(metadata);
        if let Some(user_id) = attempt.user_id {
            refused = refused.actor(user_id);
        }
        self.audit(refused).await;
    }
}

/// The audit name of `audience`.
pub(crate) fn audience_name(audience: SignInAudience) -> &'static str {
    match audience {
        SignInAudience::App => "app",
        SignInAudience::Platform => "platform",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempts_normalize_the_email_and_name_the_domain() {
        let attempt = SignInAttempt::new("  Ada@Corp.Test ", SignInMethod::Otp);
        assert_eq!(attempt.email, "ada@corp.test");
        assert_eq!(attempt.domain(), Some("corp.test"));
        assert_eq!(attempt.audience, SignInAudience::App);
        assert_eq!(attempt.user_id, None);
        assert_eq!(SignInAttempt::new("nobody", SignInMethod::Otp).domain(), None);
    }

    #[test]
    fn methods_have_stable_names_and_round_trip() {
        for (method, name) in [
            (SignInMethod::Password, "password"),
            (SignInMethod::MagicLink, "magic_link"),
            (SignInMethod::Otp, "otp"),
            (SignInMethod::Passkey, "passkey"),
            (SignInMethod::social("github"), "social"),
            (SignInMethod::sso("01a1"), "sso"),
            (
                SignInMethod::TokenExchange {
                    issuer: "legacy".to_owned(),
                },
                "token_exchange",
            ),
        ] {
            assert_eq!(method.name(), name);
            let value = serde_json::to_value(&method).unwrap();
            assert_eq!(value["method"], name);
            assert_eq!(serde_json::from_value::<SignInMethod>(value).unwrap(), method);
        }
        assert_eq!(
            SignInMethod::social("github").metadata(),
            json!({ "method": "social", "provider": "github" })
        );
    }
}
