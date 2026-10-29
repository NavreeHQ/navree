//! [`WorkspaceError`] and its HTTP mapping.

use std::collections::BTreeMap;

use http::StatusCode;
use navree_db::DbError;
use navree_http::AppError;
use navree_kernel::BoxError;

/// The error code of a write refused by a read-only demo workspace
/// (`AppError::code`).
pub const DEMO_READ_ONLY: &str = navree_http::codes::DEMO_READ_ONLY;

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
/// | `NoOrganization` | 400 | `organization_required` | `The request names no organization.` |
/// | `OrganizationNotFound` | 404 | `organization_not_found` | `Organization not found.` |
/// | `NotOrganizationMember` | 403 | `not_an_organization_member` | `You are not a member of this organization.` |
/// | `AlreadyOrganizationMember` | 409 | `already_an_organization_member` | `The user is already a member of this organization.` |
/// | `OrganizationOwnerRequired` | 409 | `organization_owner_required` | `The organization owner can't be removed, demoted or limited; transfer ownership first.` |
/// | `AlreadyAttached` | 409 | `workspace_already_attached` | `The workspace belongs to another organization.` |
/// | `NotAttached` | 404 | `workspace_not_attached` | `The workspace is not part of this organization.` |
/// | `AccessRequestNotFound` | 404 | `access_request_not_found` | `Access request not found.` |
/// | `AccessRequestPending` | 409 | `access_request_pending` | `An access request is already pending.` |
/// | `AccessRequestDecided` | 409 | `access_request_decided` | `The access request has already been decided.` |
/// | `NoPartition` | 400 | `partition_required` | `The request names no partition.` |
/// | `PartitionNotFound` | 404 | `partition_not_found` | `Partition not found.` |
/// | `PartitionForbidden` | 403 | `partition_forbidden` | `You don't have access to this partition.` |
/// | `DemoReadOnly` | 403 | `demo_read_only` | `Demo workspaces are read-only.` |
/// | `NotDemo` | 409 | `not_a_demo_workspace` | `This workspace is not a demo workspace.` |
/// | `LibraryReadOnly` | 403 | `library_read_only` | `This library item is shared: clone it to change it.` |
/// | `FeatureDisabled` | 500 | `internal_error` | (the setting to turn on, in the log) |
/// | `Db` | as navree-db maps it (404, 409) or 500 | | |
/// | `Orm` | as its source maps (a missing record 404, a hook's refusal as that refusal) or 500 | | |
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
    /// A write to a read-only demo workspace (`workspace.demo.mode =
    /// "read_only"`).
    #[error("demo workspaces are read-only")]
    DemoReadOnly,
    /// The workspace (a slug or an id) is not a demo workspace.
    #[error("workspace `{0}` is not a demo workspace")]
    NotDemo(String),
    /// A write to a library item the workspace (or organization) does not
    /// own: a global or an organization's item, seen but shared. The value
    /// names the item, for the log. Clone it to change it.
    #[error("{0} is shared and read-only here: clone it to change it")]
    LibraryReadOnly(String),
    /// An ORM error (a library query or write). Over HTTP it answers as the
    /// error in its source does: a 404 for a missing record, a 409 for a
    /// unique violation, a hook's refusal as that refusal.
    #[error(transparent)]
    Orm(#[from] navree_orm::OrmError),
    /// A service of a feature that is off was called; the value is the
    /// setting that turns it on (`workspace.organizations.enabled`).
    #[error("{0} is false: turn it on to use this")]
    FeatureDisabled(&'static str),
    /// A database error.
    #[error("workspace database error: {0}")]
    Db(#[from] DbError),
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

    /// A [`Db`](WorkspaceError::Db) error from anything navree-db turns
    /// into a [`DbError`] (what its connections and units of work return).
    pub(crate) fn db(error: impl Into<DbError>) -> Self {
        WorkspaceError::Db(error.into())
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
        WorkspaceError::NoOrganization => {
            AppError::bad_request("The request names no organization.")
                .with_code("organization_required")
        }
        WorkspaceError::OrganizationNotFound(_) => AppError::not_found()
            .with_code("organization_not_found")
            .with_detail("Organization not found."),
        WorkspaceError::NotOrganizationMember => AppError::forbidden()
            .with_code("not_an_organization_member")
            .with_detail("You are not a member of this organization."),
        WorkspaceError::AlreadyOrganizationMember => {
            AppError::conflict("The user is already a member of this organization.")
                .with_code("already_an_organization_member")
        }
        WorkspaceError::OrganizationOwnerRequired => AppError::conflict(
            "The organization owner can't be removed, demoted or limited; transfer ownership first.",
        )
        .with_code("organization_owner_required"),
        WorkspaceError::AlreadyAttached => {
            AppError::conflict("The workspace belongs to another organization.")
                .with_code("workspace_already_attached")
        }
        WorkspaceError::NotAttached => AppError::not_found()
            .with_code("workspace_not_attached")
            .with_detail("The workspace is not part of this organization."),
        WorkspaceError::AccessRequestNotFound => AppError::not_found()
            .with_code("access_request_not_found")
            .with_detail("Access request not found."),
        WorkspaceError::AccessRequestPending => {
            AppError::conflict("An access request is already pending.")
                .with_code("access_request_pending")
        }
        WorkspaceError::AccessRequestDecided => {
            AppError::conflict("The access request has already been decided.")
                .with_code("access_request_decided")
        }
        WorkspaceError::NoPartition => {
            AppError::bad_request("The request names no partition.").with_code("partition_required")
        }
        WorkspaceError::PartitionNotFound(_) => AppError::not_found()
            .with_code("partition_not_found")
            .with_detail("Partition not found."),
        WorkspaceError::PartitionForbidden => AppError::forbidden()
            .with_code("partition_forbidden")
            .with_detail("You don't have access to this partition."),
        WorkspaceError::DemoReadOnly => AppError::demo_read_only(),
        WorkspaceError::NotDemo(_) => {
            AppError::conflict("This workspace is not a demo workspace.")
                .with_code("not_a_demo_workspace")
        }
        WorkspaceError::LibraryReadOnly(_) => AppError::forbidden()
            .with_code(LIBRARY_READ_ONLY)
            .with_detail("This library item is shared: clone it to change it."),
        WorkspaceError::Db(db) => return navree_db::map_db_error(db),
        WorkspaceError::Orm(error) => return map_orm_error(error),
        WorkspaceError::FeatureDisabled(_) | WorkspaceError::Other(_) => return None,
    })
}

/// The error code of a write to a shared library item (`AppError::code`).
pub const LIBRARY_READ_ONLY: &str = "library_read_only";

/// The response of an ORM error: the first workspace error or database
/// error of its source chain, as each maps (`None`, a 500, otherwise).
fn map_orm_error(error: &navree_orm::OrmError) -> Option<AppError> {
    let mut next: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = next {
        if let Some(workspace) = cause.downcast_ref::<WorkspaceError>() {
            return map_workspace_error(workspace);
        }
        if let Some(db) = cause.downcast_ref::<navree_db::DbError>() {
            return navree_db::map_db_error(db);
        }
        next = cause.source();
    }
    None
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
            (WorkspaceError::NoOrganization, 400, "organization_required"),
            (
                WorkspaceError::OrganizationNotFound("acme".into()),
                404,
                "organization_not_found",
            ),
            (
                WorkspaceError::NotOrganizationMember,
                403,
                "not_an_organization_member",
            ),
            (
                WorkspaceError::AlreadyOrganizationMember,
                409,
                "already_an_organization_member",
            ),
            (
                WorkspaceError::OrganizationOwnerRequired,
                409,
                "organization_owner_required",
            ),
            (
                WorkspaceError::AlreadyAttached,
                409,
                "workspace_already_attached",
            ),
            (WorkspaceError::NotAttached, 404, "workspace_not_attached"),
            (
                WorkspaceError::AccessRequestNotFound,
                404,
                "access_request_not_found",
            ),
            (
                WorkspaceError::AccessRequestPending,
                409,
                "access_request_pending",
            ),
            (
                WorkspaceError::AccessRequestDecided,
                409,
                "access_request_decided",
            ),
            (WorkspaceError::NoPartition, 400, "partition_required"),
            (
                WorkspaceError::PartitionNotFound("eu".into()),
                404,
                "partition_not_found",
            ),
            (
                WorkspaceError::PartitionForbidden,
                403,
                "partition_forbidden",
            ),
            (
                WorkspaceError::FeatureDisabled("workspace.organizations.enabled"),
                500,
                "internal_error",
            ),
            (WorkspaceError::DemoReadOnly, 403, "demo_read_only"),
            (
                WorkspaceError::NotDemo("acme".into()),
                409,
                "not_a_demo_workspace",
            ),
            (
                WorkspaceError::Db(DbError::not_found("workspaces x")),
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

    #[test]
    fn library_errors_map_through_the_orm_error_s_source() {
        let read_only = WorkspaceError::LibraryReadOnly("Template x (global)".into());
        let response = read_only.to_app_error();
        assert_eq!(response.status, StatusCode::FORBIDDEN);
        assert_eq!(response.code, LIBRARY_READ_ONLY);
        assert_eq!(
            response.body()["message"],
            "This library item is shared: clone it to change it."
        );

        let cases = [
            (
                navree_orm::OrmError::refused(
                    "Template",
                    WorkspaceError::LibraryReadOnly("Template x (global)".into()),
                ),
                403,
                "library_read_only",
            ),
            (
                navree_orm::OrmError::refused(
                    "Template",
                    WorkspaceError::invalid("scope", "a workspace's item needs a workspace_id"),
                ),
                400,
                "validation_failed",
            ),
            (
                navree_orm::OrmError::not_found("Template", vec!["x".into()]),
                404,
                "not_found",
            ),
            (
                navree_orm::OrmError::InvalidQuery("bad".into()),
                500,
                "internal_error",
            ),
        ];
        for (error, status, code) in cases {
            let response = WorkspaceError::from(error).to_app_error();
            assert_eq!(response.status.as_u16(), status, "{code}");
            assert_eq!(response.code, code);
        }
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
