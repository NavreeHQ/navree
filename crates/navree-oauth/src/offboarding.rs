//! Ending every OAuth credential of some users, for navree-auth's
//! `AuthService::revoke_credentials_of_domain` (offboarding).

use std::sync::Arc;

use navree_contracts::{BoxError, BoxFuture, CredentialRevoker, RevokedByKind};
use navree_kernel::{Injectable, KernelError, ProviderKey, Resolver};
use navree_orm::Model as _;
use uuid::Uuid;

use crate::core::Core;
use crate::entity::{
    access_token, authorization, code, device_code, personal_access_token, refresh_token,
};

/// The `dyn CredentialRevoker` of this module: revokes the users'
/// personal access tokens, access and refresh tokens, unused authorization
/// and device codes, and forgets the consent they remembered for each app.
pub(crate) struct OAuthCredentialRevoker {
    core: Arc<Core>,
}

impl Injectable for OAuthCredentialRevoker {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Core>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(OAuthCredentialRevoker {
                core: r.get::<Core>().await?,
            })
        })
    }
}

impl CredentialRevoker for OAuthCredentialRevoker {
    fn name(&self) -> &'static str {
        "oauth"
    }

    fn revoke_for_users<'a>(
        &'a self,
        users: &'a [Uuid],
    ) -> BoxFuture<'a, Result<RevokedByKind, BoxError>> {
        Box::pin(async move {
            let now = self.core.now();
            let uow = self.core.db.uow(None, None).await;
            let work = async {
                let mut revoked = RevokedByKind::new();
                revoked.insert(
                    "personal_access_tokens",
                    personal_access_token::Model::query()
                        .where_(personal_access_token::Model::USER_ID.is_in(users.to_vec()))
                        .where_null(personal_access_token::Model::REVOKED_AT)
                        .update(&uow, [personal_access_token::Model::REVOKED_AT.set(now)])
                        .await?,
                );
                revoked.insert(
                    "oauth_access_tokens",
                    access_token::Model::query()
                        .where_(access_token::Model::USER_ID.is_in(users.to_vec()))
                        .where_null(access_token::Model::REVOKED_AT)
                        .update(&uow, [access_token::Model::REVOKED_AT.set(now)])
                        .await?,
                );
                revoked.insert(
                    "oauth_refresh_tokens",
                    refresh_token::Model::query()
                        .where_(refresh_token::Model::USER_ID.is_in(users.to_vec()))
                        .where_null(refresh_token::Model::REVOKED_AT)
                        .update(&uow, [refresh_token::Model::REVOKED_AT.set(now)])
                        .await?,
                );
                // A code approved before and not exchanged yet would
                // become a fresh token pair.
                revoked.insert(
                    "oauth_codes",
                    code::Model::query()
                        .where_(code::Model::USER_ID.is_in(users.to_vec()))
                        .where_null(code::Model::USED_AT)
                        .update(&uow, [code::Model::USED_AT.set(now)])
                        .await?,
                );
                revoked.insert(
                    "oauth_device_codes",
                    device_code::Model::query()
                        .where_(device_code::Model::USER_ID.is_in(users.to_vec()))
                        .where_null(device_code::Model::USED_AT)
                        .update(&uow, [device_code::Model::USED_AT.set(now)])
                        .await?,
                );
                revoked.insert(
                    "oauth_authorizations",
                    authorization::Model::query()
                        .where_(authorization::Model::USER_ID.is_in(users.to_vec()))
                        .delete(&uow)
                        .await?,
                );
                Ok::<_, BoxError>(revoked)
            };
            match uow.scope(work).await {
                Ok(revoked) => {
                    uow.commit().await?;
                    // Looked-up tokens may still be cached.
                    self.core.forget_all();
                    Ok(revoked)
                }
                Err(error) => {
                    if let Err(rollback) = uow.rollback().await {
                        tracing::warn!(target: "navree::oauth", error = %rollback, "rollback failed");
                    }
                    Err(error)
                }
            }
        })
    }
}
