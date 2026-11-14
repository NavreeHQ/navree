//! The sign-in policy: emails of an enforcing connection's verified
//! domains sign in through that connection only.

use std::sync::Arc;

use navree_auth::{AuthError, SignInAttempt, SignInMethod, SignInPolicy};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};

use crate::Sso;

/// The code of a refusal.
pub(crate) const SSO_REQUIRED: &str = "sso_required";

/// Refuses every sign-in (password, registration, magic link, emailed
/// code, passkey, social sign-in, token exchange, another connection) of
/// an email whose domain is a verified domain of an enabled connection
/// that enforces single sign-on (`enforce`, or
/// `sso.enforce_for_verified_domains`), with a 403 whose code is
/// `sso_required`; a single sign-on through that connection goes on, and so
/// does handing a browser's sign-in over to a native app.
pub(crate) struct SsoSignInPolicy {
    sso: Arc<Sso>,
}

impl Injectable for SsoSignInPolicy {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Sso>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(SsoSignInPolicy {
                sso: r.get::<Sso>().await?,
            })
        })
    }
}

impl SignInPolicy for SsoSignInPolicy {
    fn check<'a>(&'a self, attempt: &'a SignInAttempt) -> BoxFuture<'a, Result<(), AuthError>> {
        Box::pin(async move {
            let enforcing = self
                .sso
                .enforcing_connection(&attempt.email)
                .await
                .map_err(|error| AuthError::Extension(Box::new(error)))?;
            let Some((id, name)) = enforcing else {
                return Ok(());
            };
            // An app's session continues a browser session that was itself
            // asked when it was made, and keeps that session's sign-in time
            // and end: it carries no more than the browser session has.
            if matches!(attempt.method, SignInMethod::AppHandOver { .. }) {
                return Ok(());
            }
            if let SignInMethod::Sso { connection } = &attempt.method
                && *connection == id.to_string()
            {
                return Ok(());
            }
            Err(AuthError::SignInRefused {
                code: SSO_REQUIRED.to_owned(),
                message: format!("Sign in with {name}"),
            })
        })
    }
}
