//! [`WorkspaceError`] and its HTTP mapping.

use std::collections::BTreeMap;

use http::StatusCode;
use navree_http::AppError;
use navree_kernel::BoxError;
use sea_orm::DbErr;

/// The error code of a write refused by a read-only demo workspace
/// (`AppError::code`).
pub const DEMO_READ_ONLY: &str = "demo_read_only";

/// A workspace-module error. Returned from handlers with `?`, it becomes an
/// error response in Navree's JSON error shape:
///
/// | Error | Status | Message |
/// |---|---|---|
/// | `NoWorkspace` | 400 | `The request names no workspace.` |
/// | `WorkspaceNotFound` | 404 | `Workspace not found.` |
/// | `NotMember` | 403 | `You are not a member of this workspace.` |
/// | `Invalid` | 400 | the field's message, under `errors.<field>` |
/// | `AlreadyMember` | 409 | `The user is already a member of this workspace.` |
/// | `MemberNotFound` | 404 | `Member not found.` |
/// | `OwnerRequired` | 409 | `The workspace owner can't be removed or demoted; transfer ownership first.` |
/// | `InvitationNotFound` | 404 | `Invitation not found.` |
/// | `InvitationExpired` | 410 | `The invitation has expired.` |
/// | `InvitationEmailMismatch` | 403 | `This invitation was sent to a different email address.` |
/// | `DemoReadOnly` | 403, code `demo_read_only` | `Demo workspaces are read-only.` |
/// | `NotDemo` | 409 | `This workspace is not a demo workspace.` |
/// | `Db` | as navree-db maps it (404, 409) or 500 | |
/// | `Other` | 500 | |
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
    /// A write to a read-only demo workspace (`workspace.demo.mode =
    /// "read_only"`).
    #[error("demo workspaces are read-only")]
    DemoReadOnly,
    /// The workspace (a slug or an id) is not a demo workspace.
    #[error("workspace `{0}` is not a demo workspace")]
    NotDemo(String),
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
        WorkspaceError::NoWorkspace => AppError::bad_request("The request names no workspace."),
        WorkspaceError::WorkspaceNotFound(_) => {
            AppError::not_found().with_detail("Workspace not found.")
        }
        WorkspaceError::NotMember => {
            AppError::forbidden().with_detail("You are not a member of this workspace.")
        }
        WorkspaceError::Invalid { field, message } => AppError::validation(BTreeMap::from([(
            (*field).to_owned(),
            vec![message.clone()],
        )])),
        WorkspaceError::AlreadyMember => {
            AppError::conflict("The user is already a member of this workspace.")
        }
        WorkspaceError::MemberNotFound => AppError::not_found().with_detail("Member not found."),
        WorkspaceError::OwnerRequired => AppError::conflict(
            "The workspace owner can't be removed or demoted; transfer ownership first.",
        ),
        WorkspaceError::InvitationNotFound => {
            AppError::not_found().with_detail("Invitation not found.")
        }
        WorkspaceError::InvitationExpired => {
            AppError::from_status(StatusCode::GONE).with_detail("The invitation has expired.")
        }
        WorkspaceError::InvitationEmailMismatch => AppError::forbidden()
            .with_detail("This invitation was sent to a different email address."),
        WorkspaceError::DemoReadOnly => {
            AppError::new(StatusCode::FORBIDDEN, DEMO_READ_ONLY, "Forbidden")
                .with_detail("Demo workspaces are read-only.")
        }
        WorkspaceError::NotDemo(_) => AppError::conflict("This workspace is not a demo workspace."),
        WorkspaceError::Db(db) => return navree_db::map_db_error(db),
        WorkspaceError::Other(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_map_to_statuses() {
        let cases = [
            (WorkspaceError::NoWorkspace, 400),
            (WorkspaceError::WorkspaceNotFound("acme".into()), 404),
            (WorkspaceError::NotMember, 403),
            (
                WorkspaceError::invalid("slug", "slug has already been taken"),
                400,
            ),
            (WorkspaceError::AlreadyMember, 409),
            (WorkspaceError::MemberNotFound, 404),
            (WorkspaceError::OwnerRequired, 409),
            (WorkspaceError::InvitationNotFound, 404),
            (WorkspaceError::InvitationExpired, 410),
            (WorkspaceError::InvitationEmailMismatch, 403),
            (WorkspaceError::DemoReadOnly, 403),
            (WorkspaceError::NotDemo("acme".into()), 409),
            (
                WorkspaceError::Db(DbErr::RecordNotFound("workspaces x".into())),
                404,
            ),
            (WorkspaceError::Other("boom".into()), 500),
        ];
        for (error, status) in cases {
            assert_eq!(error.to_app_error().status.as_u16(), status, "{error}");
        }
        let body = WorkspaceError::invalid("slug", "slug has already been taken")
            .to_app_error()
            .body();
        assert_eq!(body["errors"]["slug"][0], "slug has already been taken");
        let body = WorkspaceError::NotMember.to_app_error().body();
        assert_eq!(body["message"], "You are not a member of this workspace.");
    }

    #[test]
    fn read_only_demos_refuse_with_their_code() {
        let error = WorkspaceError::DemoReadOnly.to_app_error();
        assert_eq!(error.status, StatusCode::FORBIDDEN);
        assert_eq!(error.code, "demo_read_only");
        assert_eq!(error.body()["message"], "Demo workspaces are read-only.");
        assert_eq!(error.body()["error"], "Forbidden");
        let error = WorkspaceError::NotDemo("acme".into()).to_app_error();
        assert_eq!(
            error.body()["message"],
            "This workspace is not a demo workspace."
        );
    }
}
