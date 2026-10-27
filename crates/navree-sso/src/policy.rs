//! The password policy: emails of an enforcing connection's verified
//! domains sign in with single sign-on only.

use std::sync::Arc;

use navree_auth::{AuthError, PasswordSignInPolicy};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};

use crate::Sso;

/// Refuses passwords (sign-in and registration) for emails whose domain is
/// a verified domain of an enabled connection that enforces single
/// sign-on (`enforce`, or `sso.enforce_for_verified_domains`), with a 403
/// whose code is `sso_required`.
pub(crate) struct SsoPasswordPolicy {
    sso: Arc<Sso>,
}

impl Injectable for SsoPasswordPolicy {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Sso>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(SsoPasswordPolicy {
                sso: r.get::<Sso>().await?,
            })
        })
    }
}

impl PasswordSignInPolicy for SsoPasswordPolicy {
    fn check<'a>(&'a self, email: &'a str) -> BoxFuture<'a, Result<(), AuthError>> {
        Box::pin(async move {
            match self.sso.enforcing_connection(email).await {
                Ok(None) => Ok(()),
                Ok(Some(name)) => Err(AuthError::PasswordSignInRefused {
                    code: "sso_required".to_owned(),
                    message: format!("Sign in with {name}"),
                }),
                Err(error) => Err(AuthError::Extension(Box::new(error))),
            }
        })
    }
}
