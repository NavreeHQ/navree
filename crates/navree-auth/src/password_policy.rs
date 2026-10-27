//! [`PasswordSignInPolicy`]: modules that keep some accounts off passwords.

use navree_kernel::BoxFuture;

use crate::AuthError;

/// Decides whether an email address may use a password: to sign in
/// (`POST /auth/login`, [`AuthService::attempt_login`](crate::AuthService::attempt_login))
/// or to register (`POST /auth/register`,
/// [`AuthService::register`](crate::AuthService::register)). Single sign-on
/// registers one that sends the people of an organization that enforces it
/// to their identity provider.
///
/// Register implementations as `dyn PasswordSignInPolicy` multi-bindings,
/// most simply with
/// [`ModuleBuilderAuthExt::password_sign_in_policy`](crate::ModuleBuilderAuthExt::password_sign_in_policy).
/// Every registered policy must allow; the first refusal is returned as is
/// (usually [`AuthError::PasswordSignInRefused`], a 403 with the policy's
/// code).
pub trait PasswordSignInPolicy: Send + Sync + 'static {
    /// `Ok(())` lets `email` (trimmed and lower-cased) use a password.
    fn check<'a>(&'a self, email: &'a str) -> BoxFuture<'a, Result<(), AuthError>>;
}
