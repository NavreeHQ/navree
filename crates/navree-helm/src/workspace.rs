//! Models that belong to workspaces (`#[model(workspace)]`) in the panel.
//!
//! The panel works in one workspace at a time. Which one is decided before
//! a screen runs: the workspace the request's unit of work has entered
//! (the app enters it for requests that are inside a workspace), else the
//! one the request names with the `X-Workspace` header or the panel's
//! workspace cookie (set by the form in the sidebar). The panel then enters
//! it the way a workspace request does: the unit of work remembers it, which
//! scopes the ORM's queries and fills the records it creates, and on
//! PostgreSQL the transaction's `navree.workspace` setting is set, which is
//! what row-level security policies read.
//!
//! Without a workspace, a resource of a workspace-scoped model answers with
//! a 400 that asks for one: it never falls back to every workspace.
//! [`Workspaces::Every`] is the explicit way for a resource to work across
//! workspaces; writes still run inside the workspace of the record they
//! change.

use std::future::Future;

use http::HeaderMap;
use navree_db::{Backend, Executor as _, Uow};
use navree_http::AppError;
use uuid::Uuid;

use crate::session::cookie;

/// The cookie that holds the workspace the panel works in, set by the form
/// in the sidebar (`POST {path}/workspace`).
pub const WORKSPACE_COOKIE: &str = "navree_helm_workspace";

/// The header an API client names its workspace with (the same one
/// navree-workspace reads).
pub(crate) const WORKSPACE_HEADER: &str = "x-workspace";

/// Where a resource of a workspace-scoped model (`#[model(workspace)]`)
/// works. Models without a workspace column ignore it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Workspaces {
    /// In one workspace: the one the request works in (see
    /// [`WORKSPACE_COOKIE`]). Without one, every screen of the resource
    /// answers with a 400 that asks for a workspace. This is the default,
    /// and the only scope that holds under PostgreSQL row-level security
    /// with a role that does not bypass it.
    #[default]
    Current,
    /// Across every workspace: the queries leave out the workspace scope,
    /// so the database alone decides what they see (a role that bypasses
    /// row-level security sees every workspace, one that does not sees
    /// none), and a create form must name the `workspace_id` of the record.
    /// Every write still runs inside the workspace of the record it
    /// changes.
    Every,
}

/// The workspace the request names: the `X-Workspace` header, else the
/// panel's workspace cookie. Anything but a UUID names none.
pub(crate) fn requested(headers: &HeaderMap) -> Option<Uuid> {
    let from_header = headers
        .get(WORKSPACE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match from_header {
        Some(value) => Uuid::parse_str(value).ok(),
        None => {
            cookie(headers, WORKSPACE_COOKIE).and_then(|value| Uuid::parse_str(value.trim()).ok())
        }
    }
}

/// Makes `workspace` the one `uow` works in: the unit of work remembers it
/// and, on PostgreSQL and MySQL, the transaction's `navree.workspace`
/// setting holds it (empty for none).
pub(crate) async fn enter(uow: &Uow, workspace: Option<Uuid>) -> Result<(), AppError> {
    uow.set_workspace(workspace);
    if uow.backend() == Backend::Sqlite {
        return Ok(());
    }
    let value = workspace.map(|id| id.to_string()).unwrap_or_default();
    uow.set_local("navree.workspace", &value)
        .await
        .map_err(AppError::internal)?;
    Ok(())
}

/// Runs `work` inside `workspace` (the ORM's current workspace and the
/// transaction's setting), then puts back the workspace `uow` had.
pub(crate) async fn within<T>(
    uow: &Uow,
    workspace: Uuid,
    work: impl Future<Output = Result<T, AppError>>,
) -> Result<T, AppError> {
    let previous = uow.workspace();
    if previous != Some(workspace) {
        enter(uow, Some(workspace)).await?;
    }
    let result = navree_orm::workspace::scope(workspace, work).await;
    if previous != Some(workspace) {
        let restored = enter(uow, previous).await;
        return result.and_then(|value| restored.map(|()| value));
    }
    result
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;
    use http::header::COOKIE;

    use super::*;

    #[test]
    fn the_header_comes_before_the_cookie_and_only_a_uuid_counts() {
        let acme = Uuid::now_v7();
        let globex = Uuid::now_v7();
        let mut headers = HeaderMap::new();
        assert_eq!(requested(&headers), None);
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("{WORKSPACE_COOKIE}={acme}")).unwrap(),
        );
        assert_eq!(requested(&headers), Some(acme));
        headers.insert(
            WORKSPACE_HEADER,
            HeaderValue::from_str(&format!(" {globex} ")).unwrap(),
        );
        assert_eq!(requested(&headers), Some(globex));
        // A header that is not a UUID does not fall back to the cookie.
        headers.insert(WORKSPACE_HEADER, HeaderValue::from_static("acme"));
        assert_eq!(requested(&headers), None);
        headers.remove(WORKSPACE_HEADER);
        headers.insert(
            COOKIE,
            HeaderValue::from_static("navree_helm_workspace=nope"),
        );
        assert_eq!(requested(&headers), None);
    }
}
