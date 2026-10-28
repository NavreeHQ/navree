//! [`CredentialRevoker`]: ending the credentials modules issued to users.

use std::collections::BTreeMap;
use std::sync::Arc;

use navree_kernel::{Factory, Injectable, ModuleBuilder, Resolver};

use uuid::Uuid;

use crate::{BoxError, BoxFuture};

/// How many credentials of each kind a [`CredentialRevoker`] ended, by kind
/// (`"personal_access_tokens"`, `"oauth_refresh_tokens"`, ...). Kinds that
/// had nothing to revoke may be left out.
pub type RevokedByKind = BTreeMap<&'static str, u64>;

/// A module that issues credentials to users (tokens, grants, sessions)
/// and can end all of them. Register an implementation as a
/// `dyn CredentialRevoker` multi-binding; navree-auth's
/// `AuthService::revoke_credentials_of_domain` and
/// `AuthService::revoke_all_credentials` ask every registered revoker
/// after revoking the sign-in sessions themselves, so a module such as
/// navree-oauth takes part without navree-auth (or navree-sso) depending
/// on it.
///
/// The credentials are revoked in the revoker's own transaction, and
/// credentials it caches (looked-up tokens) are forgotten before it
/// returns.
pub trait CredentialRevoker: Send + Sync + 'static {
    /// The module's name, for logs and audit entries (`"oauth"`).
    fn name(&self) -> &'static str;

    /// Ends every credential of `users` that the module issued: they stop
    /// working at once. The count of each kind it ended.
    fn revoke_for_users<'a>(
        &'a self,
        users: &'a [Uuid],
    ) -> BoxFuture<'a, Result<RevokedByKind, BoxError>>;
}

/// Revoker registrations on a [`ModuleBuilder`](navree_kernel::ModuleBuilder).
pub trait ModuleBuilderRevocationExt {
    /// Registers `R` as a `dyn CredentialRevoker` multi-binding, asked by
    /// navree-auth's offboarding calls.
    fn credential_revoker<R: CredentialRevoker + Injectable>(&mut self) -> &mut Self;
}

impl ModuleBuilderRevocationExt for ModuleBuilder {
    fn credential_revoker<R: CredentialRevoker + Injectable>(&mut self) -> &mut Self {
        self.multi(
            Factory::<dyn CredentialRevoker>::new(|r: Resolver| async move {
                Ok(Arc::new(R::construct(r).await?) as Arc<dyn CredentialRevoker>)
            })
            .deps(R::dependencies()),
        )
    }
}
