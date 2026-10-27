//! `two_factor_enabled` on the user resource.

use navree::auth::{User, UserResourceExtension};
use navree::kernel::BoxError;
use navree::{BoxFuture, injectable};
use serde_json::{Map, Value};

/// Adds `two_factor_enabled` to the user resource (`GET /api/auth/me`, the
/// sign-in answer), so a front end's security settings know whether to offer
/// turning two-factor authentication on or off.
#[injectable]
pub struct TwoFactorStatus;

impl UserResourceExtension for TwoFactorStatus {
    fn extend<'a>(
        &'a self,
        user: &'a User,
        fields: &'a mut Map<String, Value>,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            fields.insert(
                "two_factor_enabled".to_owned(),
                Value::Bool(user.two_factor_confirmed_at.is_some()),
            );
            Ok(())
        })
    }
}
