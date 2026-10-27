//! [`WorkspaceError`] and its HTTP mapping.

use std::collections::BTreeMap;

use http::StatusCode;
use navree_http::AppError;
use navree_kernel::BoxError;
use sea_orm::DbErr;

/// A workspace-module error. Returned from handlers with `?`, it becomes an
/// error response in Navree's JSON error shape:
///
/// | Error | Status | `code` | Message |
/// |---|---|---|---|
/// | `NoWorkspace` | 400 | `workspace_required` | `The request names no workspace.` |
/// | `WorkspaceNotFound` | 404 | `workspace_not_found` | `Workspace not found.` |
/// | `NotMember` | 403 | `not_a_member` | `You are not a member of this workspace.` |
/// | `Invalid` | 400 | `validation_failed` | the field's message, under `errors.<field>` |
/// | `AlreadyMember` | 409 | `already_a_member` | `The user is already a member of this workspace.` |
/// | `MemberNotFound` | 404 | `member_not_found` | `Member not found.` |
/// | `OwnerRequired` | 409 | `owner_required` | `The workspace owner can't be removed or demoted; transfer ownership first.` |
/// | `InvitationNotFound` | 404 | `invitation_not_found` | `Invitation not found.` |
/// | `InvitationExpired` | 410 | `invitation_expired` | `The invitation has expired.` |
/// | `InvitationEmailMismatch` | 403 | `invitation_email_mismatch` | `This invitation was sent to a different email address.` |
/// | `Db` | as navree-db maps it (404, 409) or 500 | | |
/// | `Other` | 500 | `internal_error` | |
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WorkspaceError {
    /// The request names no workspace (no resolver found one).
    #[error("the request names no workspace")]
    NoWorkspace,
    /// No workspace with this slug or id.
    #[error("workspace `{0}` does not exist")]
    WorkspaceNotFound(String),
    /// The user is not a member of the workspace.
    #[error("the user is not a member of the workspace")]
    NotMember,
    /// An input value is invalid (a 400 naming the field).
    #[error("invalid {field}: {message}")]
    Invalid {
        /// The input field (`name`, `slug`, `role`, `email`, ...).
        field: &'static str,
        /// What is wrong, as shown to the client.
        message: String,
    },
    /// The user is already a member.
    #[error("the user is already a member of the workspace")]
    AlreadyMember,
    /// The user is not a member (for member changes).
    #[error("no such member")]
    MemberNotFound,
    /// The change would leave the workspace without its owner.
    #[error("the workspace owner can't be removed or demoted")]
    OwnerRequired,
    /// No pending invitation with this token or id.
    #[error("no such invitation")]
    InvitationNotFound,
    /// The invitation has expired.
    #[error("the invitation has expired")]
    InvitationExpired,
    /// The invitation was sent to another email address.
    #[error("the invitation was sent to another email address")]
    InvitationEmailMismatch,
    /// A database error.
    #[error("workspace database error: {0}")]
    Db(#[from] DbErr),
    /// Anything else (a user directory or tenant database failure).
    #[error(transparent)]
    Other(BoxError),
}

impl WorkspaceError {
    /// An [`Invalid`](WorkspaceError::Invalid) error for `field`.
    pub fn invalid(field: &'static str, message: impl Into<String>) -> Self {
        WorkspaceError::Invalid {
            field,
            message: message.into(),
        }
    }

    /// The error response (also what `map_error` registers).
    pub fn to_app_error(&self) -> AppError {
        map_workspace_error(self).unwrap_or_else(|| AppError::internal(self.to_string()))
    }
}

/// The error response for a [`WorkspaceError`], `None` for errors that stay
/// a 500.
pub(crate) fn map_workspace_error(error: &WorkspaceError) -> Option<AppError> {
    Some(match error {
        WorkspaceError::NoWorkspace => {
            AppError::bad_request("The request names no workspace.").with_code("workspace_required")
        }
        WorkspaceError::WorkspaceNotFound(_) => AppError::not_found()
            .with_code("workspace_not_found")
            .with_detail("Workspace not found."),
        WorkspaceError::NotMember => AppError::forbidden()
            .with_code("not_a_member")
            .with_detail("You are not a member of this workspace."),
        WorkspaceError::Invalid { field, message } => AppError::validation(BTreeMap::from([(
            (*field).to_owned(),
            vec![message.clone()],
        )])),
        WorkspaceError::AlreadyMember => {
            AppError::conflict("The user is already a member of this workspace.")
                .with_code("already_a_member")
        }
        WorkspaceError::MemberNotFound => AppError::not_found()
            .with_code("member_not_found")
            .with_detail("Member not found."),
        WorkspaceError::OwnerRequired => AppError::conflict(
            "The workspace owner can't be removed or demoted; transfer ownership first.",
        )
        .with_code("owner_required"),
        WorkspaceError::InvitationNotFound => AppError::not_found()
            .with_code("invitation_not_found")
            .with_detail("Invitation not found."),
        WorkspaceError::InvitationExpired => AppError::from_status(StatusCode::GONE)
            .with_code("invitation_expired")
            .with_detail("The invitation has expired."),
        WorkspaceError::InvitationEmailMismatch => AppError::forbidden()
            .with_code("invitation_email_mismatch")
            .with_detail("This invitation was sent to a different email address."),
        WorkspaceError::Db(db) => return navree_db::map_db_error(db),
        WorkspaceError::Other(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_map_to_statuses_and_codes() {
        let cases = [
            (WorkspaceError::NoWorkspace, 400, "workspace_required"),
            (
                WorkspaceError::WorkspaceNotFound("acme".into()),
                404,
                "workspace_not_found",
            ),
            (WorkspaceError::NotMember, 403, "not_a_member"),
            (
                WorkspaceError::invalid("slug", "slug has already been taken"),
                400,
                "validation_failed",
            ),
            (WorkspaceError::AlreadyMember, 409, "already_a_member"),
            (WorkspaceError::MemberNotFound, 404, "member_not_found"),
            (WorkspaceError::OwnerRequired, 409, "owner_required"),
            (
                WorkspaceError::InvitationNotFound,
                404,
                "invitation_not_found",
            ),
            (WorkspaceError::InvitationExpired, 410, "invitation_expired"),
            (
                WorkspaceError::InvitationEmailMismatch,
                403,
                "invitation_email_mismatch",
            ),
            (
                WorkspaceError::Db(DbErr::RecordNotFound("workspaces x".into())),
                404,
                "not_found",
            ),
            (WorkspaceError::Other("boom".into()), 500, "internal_error"),
        ];
        for (error, status, code) in cases {
            let app_error = error.to_app_error();
            assert_eq!(app_error.status.as_u16(), status, "{error}");
            assert_eq!(app_error.body()["code"], code, "{error}");
            assert!(navree_http::codes::is_valid(code), "{code}");
        }
        let body = WorkspaceError::invalid("slug", "slug has already been taken")
            .to_app_error()
            .body();
        assert_eq!(body["errors"]["slug"][0], "slug has already been taken");
        let body = WorkspaceError::NotMember.to_app_error().body();
        assert_eq!(body["message"], "You are not a member of this workspace.");
    }
}
