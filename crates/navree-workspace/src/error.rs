//! [`WorkspaceError`] and its HTTP mapping.

use std::collections::BTreeMap;

use http::StatusCode;
use navree_http::AppError;
use navree_kernel::BoxError;
use sea_orm::DbErr;

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
/// | `NoOrganization` | 400 | `The request names no organization.` |
/// | `OrganizationNotFound` | 404 | `Organization not found.` |
/// | `NotOrganizationMember` | 403 | `You are not a member of this organization.` |
/// | `AlreadyOrganizationMember` | 409 | `The user is already a member of this organization.` |
/// | `OrganizationOwnerRequired` | 409 | `The organization owner can't be removed, demoted or limited; transfer ownership first.` |
/// | `AlreadyAttached` | 409 | `The workspace belongs to another organization.` |
/// | `NotAttached` | 404 | `The workspace is not part of this organization.` |
/// | `AccessRequestNotFound` | 404 | `Access request not found.` |
/// | `AccessRequestPending` | 409 | `An access request is already pending.` |
/// | `AccessRequestDecided` | 409 | `The access request has already been decided.` |
/// | `NoPartition` | 400 | `The request names no partition.` |
/// | `PartitionNotFound` | 404 | `Partition not found.` |
/// | `PartitionForbidden` | 403 | `You don't have access to this partition.` |
/// | `FeatureDisabled` | 500 | (the setting to turn on, in the log) |
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
    /// The request names no organization (no resolver found one).
    #[error("the request names no organization")]
    NoOrganization,
    /// No organization with this slug or id.
    #[error("organization `{0}` does not exist")]
    OrganizationNotFound(String),
    /// The user is not a member of the organization.
    #[error("the user is not a member of the organization")]
    NotOrganizationMember,
    /// The user is already a member of the organization.
    #[error("the user is already a member of the organization")]
    AlreadyOrganizationMember,
    /// The change would remove, demote or limit the organization's owner.
    #[error("the organization owner can't be removed, demoted or limited")]
    OrganizationOwnerRequired,
    /// The workspace is attached to another organization.
    #[error("the workspace belongs to another organization")]
    AlreadyAttached,
    /// The workspace is not attached to this organization.
    #[error("the workspace is not part of this organization")]
    NotAttached,
    /// No access request with this id in the organization.
    #[error("no such access request")]
    AccessRequestNotFound,
    /// The user has a pending access request already.
    #[error("an access request is already pending")]
    AccessRequestPending,
    /// The access request was approved or declined already.
    #[error("the access request has already been decided")]
    AccessRequestDecided,
    /// The request names no partition (no resolver found one).
    #[error("the request names no partition")]
    NoPartition,
    /// No partition with this slug or id in the workspace.
    #[error("partition `{0}` does not exist")]
    PartitionNotFound(String),
    /// The member may not see this partition.
    #[error("the member may not see this partition")]
    PartitionForbidden,
    /// A service of a feature that is off was called; the value is the
    /// setting that turns it on (`workspace.organizations.enabled`).
    #[error("{0} is false: turn it on to use this")]
    FeatureDisabled(&'static str),
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
        WorkspaceError::NoOrganization => {
            AppError::bad_request("The request names no organization.")
        }
        WorkspaceError::OrganizationNotFound(_) => {
            AppError::not_found().with_detail("Organization not found.")
        }
        WorkspaceError::NotOrganizationMember => {
            AppError::forbidden().with_detail("You are not a member of this organization.")
        }
        WorkspaceError::AlreadyOrganizationMember => {
            AppError::conflict("The user is already a member of this organization.")
        }
        WorkspaceError::OrganizationOwnerRequired => AppError::conflict(
            "The organization owner can't be removed, demoted or limited; transfer ownership first.",
        ),
        WorkspaceError::AlreadyAttached => {
            AppError::conflict("The workspace belongs to another organization.")
        }
        WorkspaceError::NotAttached => {
            AppError::not_found().with_detail("The workspace is not part of this organization.")
        }
        WorkspaceError::AccessRequestNotFound => {
            AppError::not_found().with_detail("Access request not found.")
        }
        WorkspaceError::AccessRequestPending => {
            AppError::conflict("An access request is already pending.")
        }
        WorkspaceError::AccessRequestDecided => {
            AppError::conflict("The access request has already been decided.")
        }
        WorkspaceError::NoPartition => AppError::bad_request("The request names no partition."),
        WorkspaceError::PartitionNotFound(_) => {
            AppError::not_found().with_detail("Partition not found.")
        }
        WorkspaceError::PartitionForbidden => {
            AppError::forbidden().with_detail("You don't have access to this partition.")
        }
        WorkspaceError::Db(db) => return navree_db::map_db_error(db),
        WorkspaceError::FeatureDisabled(_) | WorkspaceError::Other(_) => return None,
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
            (WorkspaceError::NoOrganization, 400),
            (WorkspaceError::OrganizationNotFound("acme".into()), 404),
            (WorkspaceError::NotOrganizationMember, 403),
            (WorkspaceError::AlreadyOrganizationMember, 409),
            (WorkspaceError::OrganizationOwnerRequired, 409),
            (WorkspaceError::AlreadyAttached, 409),
            (WorkspaceError::NotAttached, 404),
            (WorkspaceError::AccessRequestNotFound, 404),
            (WorkspaceError::AccessRequestPending, 409),
            (WorkspaceError::AccessRequestDecided, 409),
            (WorkspaceError::NoPartition, 400),
            (WorkspaceError::PartitionNotFound("eu".into()), 404),
            (WorkspaceError::PartitionForbidden, 403),
            (
                WorkspaceError::FeatureDisabled("workspace.organizations.enabled"),
                500,
            ),
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
}
