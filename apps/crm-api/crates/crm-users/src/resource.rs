//! The API's user shapes and the `roles` the auth routes gain.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use crm_core::models::User;
use navree::BoxFuture;
use navree::access::AccessService;
use navree::api::ApiResource;
use navree::auth::{USER_SUBJECT, UserResourceExtension};
use navree::orm::Record;
use navree::orm::attributes::Resource;
use navree::prelude::*;
use serde::Serialize;
use serde_json::{Map, Value};
use utoipa::ToSchema;
use uuid::Uuid;

/// A user as `/api/v1/users` returns it:
/// `{__type: "User", id, name, email, email_verified_at, roles, created_at}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = User)]
pub struct UserView {
    /// Always `"User"` (the CASL subject type).
    #[serde(rename = "__type")]
    #[schema(example = "User")]
    pub resource_type: &'static str,
    /// The id.
    pub id: Uuid,
    /// The display name.
    pub name: String,
    /// The email address.
    pub email: String,
    /// When the email address was verified, if it was.
    pub email_verified_at: Option<DateTime<Utc>>,
    /// The user's role names (`admin`, `manager`, `rep`), sorted.
    pub roles: Vec<String>,
    /// When the account was created.
    pub created_at: DateTime<Utc>,
}

impl UserView {
    /// The relations a user's view embeds: load them with the records
    /// (`User::query().with_all(UserView::EMBEDS)`).
    pub const EMBEDS: [&'static str; 1] = ["roles"];
}

impl ApiResource for UserView {
    const TYPE: &'static str = USER_SUBJECT;
}

impl Resource<User> for UserView {
    /// `user` with the names of its loaded roles, sorted.
    fn from_record(user: &Record<User>) -> Self {
        let mut roles: Vec<String> = user
            .loaded(User::roles)
            .map(|roles| roles.iter().map(|role| role.name.clone()).collect())
            .unwrap_or_default();
        roles.sort();
        roles.dedup();
        UserView {
            resource_type: USER_SUBJECT,
            id: user.id,
            name: user.name.clone(),
            email: user.email.clone(),
            email_verified_at: user.email_verified_at,
            roles,
            created_at: user.created_at,
        }
    }
}

/// The OpenAPI shape of `GET /api/v1/users` (a `Page<UserView>`):
/// `{data: User[], meta}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct UserPage {
    /// The users on this page, by name.
    pub data: Vec<UserView>,
    // The pagination metadata, inlined (a plain comment: a doc comment
    // would wrap it in an `allOf`).
    #[schema(inline)]
    pub meta: UserPageMeta,
}

/// The pagination metadata of a [`UserPage`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct UserPageMeta {
    /// The page number (1-based).
    pub current_page: u64,
    /// The page size (default 15, at most 100).
    pub per_page: u64,
    /// The number of matching users.
    pub total: u64,
    /// The last page number (at least 1).
    pub last_page: u64,
}

/// `GET /api/v1/me`: `{id, name, email, roles}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Me {
    /// The id.
    pub id: Uuid,
    /// The display name.
    pub name: String,
    /// The email address.
    pub email: String,
    /// The user's role names, sorted.
    pub roles: Vec<String>,
}

/// Adds `roles` to the user resource of navree-auth's routes
/// (`GET /api/auth/me`, `user` in the login response).
#[injectable]
pub struct UserRolesExtension {
    access: Arc<AccessService>,
}

impl UserResourceExtension for UserRolesExtension {
    fn extend<'a>(
        &'a self,
        user: &'a navree::auth::User,
        fields: &'a mut Map<String, Value>,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let roles = self.access.roles_of(user.id).await?;
            fields.insert("roles".to_owned(), Value::from(roles));
            Ok(())
        })
    }
}
