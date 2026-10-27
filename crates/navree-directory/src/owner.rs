//! [`DirectoryOwner`]: who the request manages directory connections for.

use std::sync::Arc;

use axum::extract::FromRequestParts;
use http::request::Parts;
use navree_http::AppError;
use navree_integrations::{Level, Owner};
use navree_kernel::AppContext;
use navree_workspace::{CurrentOrganization, CurrentWorkspace};

use crate::Directory;

/// Who the request acts for, by `directory.level`: the request's workspace
/// (resolved and checked like `CurrentWorkspace`), its organization, or
/// the app. Answers like those extractors: 400 when the request names no
/// workspace, 404 for an unknown one, 403 for a non-member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectoryOwner(pub Owner);

impl<S: Send + Sync> FromRequestParts<S> for DirectoryOwner {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        if let Some(owner) = parts.extensions.get::<DirectoryOwner>() {
            return Ok(*owner);
        }
        let directory = directory(parts).await?;
        let owner = match directory.level()? {
            Level::App => Owner::App,
            Level::Workspace => Owner::Workspace(
                CurrentWorkspace::from_request_parts(parts, state)
                    .await?
                    .id(),
            ),
            Level::Organization => Owner::Organization(
                CurrentOrganization::from_request_parts(parts, state)
                    .await?
                    .id(),
            ),
        };
        parts.extensions.insert(DirectoryOwner(owner));
        Ok(DirectoryOwner(owner))
    }
}

/// The app's `Directory` service, from the request.
pub(crate) async fn directory(parts: &Parts) -> Result<Arc<Directory>, AppError> {
    let ctx = parts
        .extensions
        .get::<AppContext>()
        .ok_or_else(|| AppError::internal("DirectoryOwner needs the navree request pipeline"))?;
    ctx.get::<Directory>()
        .await
        .map_err(|_| AppError::internal("DirectoryOwner needs DirectoryModule in the app"))
}
