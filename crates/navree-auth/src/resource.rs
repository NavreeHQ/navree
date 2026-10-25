//! [`UserResource`], the API representation of a user, and
//! [`UserResourceExtension`]s that add app-specific fields to it.

use chrono::{DateTime, Utc};
use navree_contracts::{BoxError, BoxFuture};
use serde::Serialize;
use serde_json::{Map, Value};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::User;

/// The subject type of users in resources and abilities (CASL subject);
/// `<user::Entity as navree_access::Subject>::SUBJECT`.
pub const USER_SUBJECT: &str = "User";

/// The prefix of permission strings about users (`users.manage`);
/// `<user::Entity as navree_access::Subject>::PERMISSION_PREFIX`.
pub const USER_PERMISSION_PREFIX: &str = "users";

/// Users are the access subject `User` with permission prefix `users`.
/// Import `AccessModule::for_feature::<navree_auth::user::Entity>()` to load
/// users for `can(.., User, from = "id")` checks and to make `users.*`
/// permissions grant abilities on `User`.
impl navree_access::Subject for crate::user::Entity {
    const SUBJECT: &'static str = USER_SUBJECT;
    const PERMISSION_PREFIX: &'static str = USER_PERMISSION_PREFIX;
}

/// The API representation of a [`User`]:
/// `{"__type": "User", "id", "name", "email", "email_verified_at", "created_at"}`,
/// plus the fields of the registered [`UserResourceExtension`]s when built
/// with [`AuthService::present`](crate::AuthService::present).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct UserResource {
    /// Always `"User"` (the CASL subject type).
    #[serde(rename = "__type")]
    #[schema(example = "User")]
    pub resource_type: String,
    /// The id.
    pub id: Uuid,
    /// The display name.
    pub name: String,
    /// The email address.
    pub email: String,
    /// When the email address was verified, if it was.
    pub email_verified_at: Option<DateTime<Utc>>,
    /// When the account was created.
    pub created_at: DateTime<Utc>,
}

impl From<&User> for UserResource {
    fn from(user: &User) -> Self {
        Self {
            resource_type: USER_SUBJECT.to_owned(),
            id: user.id,
            name: user.name.clone(),
            email: user.email.clone(),
            email_verified_at: user.email_verified_at,
            created_at: user.created_at,
        }
    }
}

impl From<User> for UserResource {
    fn from(user: User) -> Self {
        Self::from(&user)
    }
}

/// Adds fields to every user resource the auth routes return
/// (`GET /auth/me`, and `user` in the login and register responses), e.g.
/// the user's roles from the access module. Register implementations as
/// `dyn UserResourceExtension` multi-bindings, most simply with
/// [`ModuleBuilderAuthExt::user_resource_extension`](crate::ModuleBuilderAuthExt::user_resource_extension);
/// they run in module dependency order and may overwrite fields.
pub trait UserResourceExtension: Send + Sync + 'static {
    /// Adds or replaces fields of `user`'s resource.
    fn extend<'a>(
        &'a self,
        user: &'a User,
        fields: &'a mut Map<String, Value>,
    ) -> BoxFuture<'a, Result<(), BoxError>>;
}
