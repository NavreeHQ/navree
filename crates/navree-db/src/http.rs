//! HTTP integration: the unit-of-work layer, the [`Uow`] extractor and the
//! mapping of database errors to problem responses.

use std::sync::Arc;

use axum::extract::{FromRequestParts, Request, State};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use http::request::Parts;
use navree_contracts::{AuthState, RequestId};
use navree_http::{AppError, GlobalLayer, LayerSlot};
use navree_kernel::AppContext;
use sea_orm::{DbErr, SqlErr};
use tokio::sync::OnceCell;

use crate::{Db, Uow};

/// The unit of work of the current request, opened by the `DatabaseModule`
/// layer. A 500 problem when `DatabaseModule` is not in the app.
impl<S: Send + Sync> FromRequestParts<S> for Uow {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<Uow>().cloned().ok_or_else(|| {
            AppError::internal(
                "no unit of work in the request: add DatabaseModule to the app (its layer opens one per request)",
            )
        })
    }
}

/// The [`LayerSlot::Uow`] layer: a lazy [`Uow`] per request with the actor
/// (from `AuthState`) and the request id. The rest of the pipeline runs in a
/// task on the app's tracker, so it completes (and commits) even if the
/// client disconnects, with the unit of work as [`Uow::current`]. Commits
/// when the status is below 400, rolls back otherwise; a failed commit
/// becomes an error response.
pub(crate) fn uow_layer() -> GlobalLayer {
    GlobalLayer::new(LayerSlot::Uow, "uow", |_ctx, router| {
        let db: Arc<OnceCell<Arc<Db>>> = Arc::new(OnceCell::new());
        Ok(router.layer(from_fn_with_state(db, unit_of_work)))
    })
}

async fn unit_of_work(
    State(db): State<Arc<OnceCell<Arc<Db>>>>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(ctx) = request.extensions().get::<AppContext>().cloned() else {
        return AppError::internal("the unit-of-work layer runs outside the navree pipeline")
            .into_response();
    };
    let db = match db.get_or_try_init(|| ctx.get::<Db>()).await {
        Ok(db) => db.clone(),
        Err(error) => return AppError::internal(error).into_response(),
    };
    let actor = request
        .extensions()
        .get::<AuthState>()
        .and_then(|state| state.0.as_ref())
        .map(|principal| principal.user_id);
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .map(|id| id.as_str().to_owned());
    let uow = db.uow(actor, request_id).await;
    request.extensions_mut().insert(uow.clone());

    let task = ctx.tracker().spawn(async move {
        // After-commit hooks run outside the scope: by then it is finished.
        let response = uow.scope(next.run(request)).await;
        finish(&uow, response).await
    });
    match task.await {
        Ok(response) => response,
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(error) => AppError::internal(error).into_response(),
    }
}

async fn finish(uow: &Uow, response: Response) -> Response {
    if response.status().as_u16() < 400 {
        match uow.commit().await {
            Ok(()) => response,
            Err(error) => AppError::internal(error).into_response(),
        }
    } else {
        if let Err(error) = uow.rollback().await {
            tracing::warn!(target: "navree_db::uow", %error, "rolling back the unit of work failed");
        }
        response
    }
}

/// The problem response for a database error, used by `DatabaseModule` for
/// errors returned from handlers with `?`: `RecordNotFound` and
/// `RecordNotUpdated` → 404, unique and foreign-key violations → 409,
/// anything else stays a 500. No SQL detail reaches the client.
pub fn map_db_error(error: &DbErr) -> Option<AppError> {
    match error {
        DbErr::RecordNotFound(_) | DbErr::RecordNotUpdated => Some(AppError::not_found()),
        _ => match error.sql_err() {
            Some(SqlErr::UniqueConstraintViolation(_)) => Some(AppError::conflict(
                "A record with the same unique value already exists.",
            )),
            Some(SqlErr::ForeignKeyConstraintViolation(_)) => Some(AppError::conflict(
                "The change conflicts with a related record.",
            )),
            _ => None,
        },
    }
}
