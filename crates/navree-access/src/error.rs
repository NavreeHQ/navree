//! [`AccessError`] and its HTTP mapping.

use std::collections::BTreeMap;

use navree_http::AppError;
use sea_orm::DbErr;

use crate::engine::RuleError;

/// An access-module error. Returned from handlers with `?`, it becomes an
/// error response: unknown role → 404, existing role → 409, invalid names
/// or unknown roles in a request body → 400, database errors as navree-db maps
/// them, anything else 500.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AccessError {
    /// No role with this name.
    #[error("role `{0}` does not exist")]
    RoleNotFound(String),
    /// A role with this name already exists.
    #[error("role `{0}` already exists")]
    RoleExists(String),
    /// No record of `subject` with this id.
    #[error("no `{subject}` record with id `{id}`")]
    RecordNotFound {
        /// The subject type.
        subject: String,
        /// The id.
        id: String,
    },
    /// Roles named in a request do not exist.
    #[error("unknown roles: {}", .0.join(", "))]
    UnknownRoles(Vec<String>),
    /// A role or permission name is malformed.
    #[error("invalid {field} `{name}`: {reason}")]
    InvalidName {
        /// The input field (`name`, `permissions`, `roles`, ...).
        field: &'static str,
        /// The rejected name.
        name: String,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// The abilities function produced an invalid rule.
    #[error("invalid ability rules: {0}")]
    Rules(#[from] RuleError),
    /// A configuration problem (for example two subjects with one name).
    #[error("{0}")]
    Config(String),
    /// A database error.
    #[error("access database error: {0}")]
    Db(#[from] DbErr),
}

/// The error response for an [`AccessError`] (registered with
/// `map_error`), `None` for errors that stay a 500.
pub(crate) fn map_access_error(error: &AccessError) -> Option<AppError> {
    match error {
        AccessError::RoleNotFound(name) => {
            Some(AppError::not_found().with_detail(format!("The role `{name}` does not exist.")))
        }
        AccessError::RecordNotFound { .. } => Some(AppError::not_found()),
        AccessError::RoleExists(name) => Some(AppError::conflict(format!(
            "The role `{name}` already exists."
        ))),
        AccessError::UnknownRoles(names) => Some(validation(
            "roles",
            names
                .iter()
                .map(|name| format!("The role `{name}` does not exist."))
                .collect(),
        )),
        AccessError::InvalidName {
            field,
            name,
            reason,
        } => Some(validation(field, vec![format!("`{name}` {reason}.")])),
        AccessError::Db(db) => navree_db::map_db_error(db),
        AccessError::Rules(_) | AccessError::Config(_) => None,
    }
}

fn validation(field: &str, messages: Vec<String>) -> AppError {
    AppError::validation(BTreeMap::from([(field.to_owned(), messages)]))
}

/// Checks a role name: 1 to 64 ASCII letters, digits, `_`, `-`, `.` or `:`,
/// starting with a letter or digit.
pub(crate) fn check_role_name(name: &str) -> Result<(), AccessError> {
    check_name("name", name, 64, false)
}

/// Checks a permission string: `<prefix>.<action>`, up to 128 ASCII
/// letters, digits, `_`, `-`, `.`, `:` or `*`.
pub(crate) fn check_permission(field: &'static str, name: &str) -> Result<(), AccessError> {
    check_name(field, name, 128, true)?;
    match name.rsplit_once('.') {
        Some((prefix, action)) if !prefix.is_empty() && !action.is_empty() => Ok(()),
        _ => Err(AccessError::InvalidName {
            field,
            name: name.to_owned(),
            reason: "must look like `<prefix>.<action>`, e.g. `posts.publish`",
        }),
    }
}

fn check_name(
    field: &'static str,
    name: &str,
    max: usize,
    permission: bool,
) -> Result<(), AccessError> {
    let invalid = |reason| {
        Err(AccessError::InvalidName {
            field,
            name: name.to_owned(),
            reason,
        })
    };
    if name.is_empty() || name.len() > max {
        return invalid(if permission {
            "must be 1 to 128 characters long"
        } else {
            "must be 1 to 64 characters long"
        });
    }
    let allowed = |c: char| {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':') || (permission && c == '*')
    };
    if !name.chars().all(allowed) || !name.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        return invalid(
            "may only contain ASCII letters, digits, `_`, `-`, `.` and `:` and must start with a letter or digit",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_names_are_checked() {
        assert!(check_role_name("sales-manager").is_ok());
        assert!(check_role_name("team:admin").is_ok());
        assert!(check_role_name("").is_err());
        assert!(check_role_name("-x").is_err());
        assert!(check_role_name("has space").is_err());
        assert!(check_role_name(&"a".repeat(65)).is_err());
    }

    #[test]
    fn permissions_need_a_prefix_and_an_action() {
        assert!(check_permission("permissions", "posts.publish").is_ok());
        assert!(check_permission("permissions", "posts.*").is_ok());
        assert!(check_permission("permissions", "posts").is_err());
        assert!(check_permission("permissions", "posts.").is_err());
        assert!(check_permission("permissions", ".publish").is_err());
        assert!(check_permission("permissions", "posts publish").is_err());
    }
}
