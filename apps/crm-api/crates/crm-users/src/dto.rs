//! Request bodies and queries, with their validation rules.

use crm_core::UserRole;
use navree::openapi::ToSchema;
use navree::orm::list::{ListResource, ListSpec};
use navree::validation::Validate;
use serde::{Deserialize, Deserializer};

/// The body of `POST /api/v1/users`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[schema(as = UserCreateInput)]
pub struct CreateUser {
    /// The display name.
    #[validate(length(min = 1, max = 255))]
    pub name: String,
    /// The email address (trimmed; stored lower-cased; unique).
    #[serde(deserialize_with = "trimmed")]
    #[validate(email, length(max = 255))]
    pub email: String,
    /// The password (at least `auth.password_min_length` characters).
    #[validate(length(min = 1, max = 1024))]
    pub password: String,
    /// The CRM role.
    #[validate(skip)]
    pub role: UserRole,
}

/// A string without surrounding whitespace.
fn trimmed<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    String::deserialize(deserializer).map(|value| value.trim().to_owned())
}

/// The body of `PATCH /api/v1/users/{id}`: only the fields present change.
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[schema(as = UserUpdateInput)]
pub struct UpdateUser {
    /// The display name.
    #[validate(length(min = 1, max = 255))]
    pub name: Option<String>,
    /// The CRM role (replaces the user's roles).
    #[validate(skip)]
    pub role: Option<UserRole>,
}

/// The list query of `GET /api/v1/users`, in the list-query grammar:
///
/// - `search`: part of the name or email, ignoring case.
/// - `sort`: `name`, `email` or `created_at`, with `-` in front for
///   descending. The default is `name`.
/// - `page` and `per_page` (default 15, at most 100).
pub struct UserList;

impl ListResource for UserList {
    const SPEC: ListSpec = ListSpec::new(&[])
        .sortable(&["name", "email", "created_at"])
        .default_sort(&["name"])
        .searchable(&["name", "email"]);
}
