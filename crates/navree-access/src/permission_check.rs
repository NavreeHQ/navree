//! The module's `dyn PermissionCheck`: app-wide roles and permissions
//! answering permission questions asked outside a route.

use std::sync::Arc;

use navree_contracts::{BoxError, BoxFuture, PermissionCheck, Principal};
use uuid::Uuid;

use crate::factory::AbilityFactory;

/// Answers [`PermissionCheck`] questions with the user's app-wide roles
/// and permissions (see [`AbilityFactory::has_permission`]). An app-wide
/// grant holds in every workspace, so the workspace is not consulted and
/// the answer is always `Some`.
pub(crate) struct AccessPermissionCheck(pub(crate) Arc<AbilityFactory>);

impl PermissionCheck for AccessPermissionCheck {
    fn check<'a>(
        &'a self,
        principal: &'a Principal,
        permission: &'a str,
        _workspace: Option<Uuid>,
    ) -> BoxFuture<'a, Result<Option<bool>, BoxError>> {
        Box::pin(async move { Ok(Some(self.0.has_permission(principal, permission).await?)) })
    }
}
