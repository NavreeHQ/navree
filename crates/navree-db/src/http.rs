//! HTTP integration: the unit-of-work layer, the [`Uow`] extractor and the
//! mapping of database errors to error responses.

use std::sync::Arc;

use axum::extract::{FromRequestParts, Request, State};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use http::request::Parts;
use navree_contracts::{AuthState, RequestId};
use navree_http::{AppError, GlobalLayer, LayerSlot};
use navree_kernel::AppContext;
use sea_orm::sqlx::error::ErrorKind;
use sea_orm::{DbErr, RuntimeErr, SqlErr};
use tokio::sync::OnceCell;

use crate::{Db, Uow};

/// The unit of work of the current request, opened by the `DatabaseModule`
/// layer. A 500 error when `DatabaseModule` is not in the app.
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

/// The error response for a database error, used by `DatabaseModule` for
/// errors returned from handlers with `?`: `RecordNotFound` and
/// `RecordNotUpdated` → 404, unique and foreign-key violations → 409,
/// anything else stays a 500. No SQL detail reaches the client.
///
/// Violations are recognised on every database: MySQL and MariaDB error
/// numbers (1062 duplicate entry; 1451 and 1452 foreign key failures, and
/// their relatives), PostgreSQL SQLSTATEs (`23505`, `23503`) and SQLite's
/// extended codes.
///
/// ```
/// use navree_db::map_db_error;
/// use sea_orm::DbErr;
///
/// let error = map_db_error(&DbErr::RecordNotFound("deals 42".into())).unwrap();
/// assert_eq!(error.status.as_u16(), 404);
/// assert!(map_db_error(&DbErr::Custom("boom".into())).is_none());
/// ```
pub fn map_db_error(error: &DbErr) -> Option<AppError> {
    match error {
        DbErr::RecordNotFound(_) | DbErr::RecordNotUpdated => Some(AppError::not_found()),
        _ => match violation(error)? {
            Violation::Unique => Some(AppError::conflict(
                "A record with the same unique value already exists.",
            )),
            Violation::ForeignKey => Some(AppError::conflict(
                "The change conflicts with a related record.",
            )),
        },
    }
}

/// A constraint a write broke.
enum Violation {
    Unique,
    ForeignKey,
}

/// The constraint `error` broke: SeaORM's reading of the driver's error
/// code, else SQLx's (`DatabaseError::kind`), which knows a few codes
/// SeaORM's list leaves out (MySQL's 1859, a duplicate found while
/// `ALTER TABLE` builds a unique index).
fn violation(error: &DbErr) -> Option<Violation> {
    match error.sql_err() {
        Some(SqlErr::UniqueConstraintViolation(_)) => return Some(Violation::Unique),
        Some(SqlErr::ForeignKeyConstraintViolation(_)) => return Some(Violation::ForeignKey),
        _ => {}
    }
    let (DbErr::Exec(RuntimeErr::SqlxError(error)) | DbErr::Query(RuntimeErr::SqlxError(error))) =
        error
    else {
        return None;
    };
    match error.as_database_error()?.kind() {
        ErrorKind::UniqueViolation => Some(Violation::Unique),
        ErrorKind::ForeignKeyViolation => Some(Violation::ForeignKey),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::sync::Arc;

    use sea_orm::RuntimeErr;
    use sea_orm::sqlx::error::{DatabaseError, ErrorKind};

    use super::*;

    /// A driver error SeaORM does not recognise, classified by SQLx: MySQL's
    /// 1859, a duplicate found while `ALTER TABLE` builds a unique index.
    #[derive(Debug)]
    struct DriverError {
        message: &'static str,
        kind: ErrorKind,
    }

    impl std::fmt::Display for DriverError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.message)
        }
    }

    impl std::error::Error for DriverError {}

    impl DatabaseError for DriverError {
        fn message(&self) -> &str {
            self.message
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed("23000"))
        }

        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> ErrorKind {
            match self.kind {
                ErrorKind::UniqueViolation => ErrorKind::UniqueViolation,
                ErrorKind::ForeignKeyViolation => ErrorKind::ForeignKeyViolation,
                _ => ErrorKind::Other,
            }
        }
    }

    fn driver_error(kind: ErrorKind, wrap: fn(RuntimeErr) -> DbErr) -> DbErr {
        let error = DriverError {
            message: "Duplicate entry 'x' found while adding the unique index",
            kind,
        };
        wrap(RuntimeErr::SqlxError(Arc::new(
            sea_orm::sqlx::Error::Database(Box::new(error)),
        )))
    }

    fn message(error: &DbErr) -> Option<(u16, serde_json::Value)> {
        map_db_error(error).map(|app| (app.status.as_u16(), app.body()["message"].clone()))
    }

    #[test]
    fn violations_the_driver_classifies_are_conflicts() {
        for wrap in [DbErr::Exec as fn(RuntimeErr) -> DbErr, DbErr::Query] {
            assert_eq!(
                message(&driver_error(ErrorKind::UniqueViolation, wrap)),
                Some((
                    409,
                    "A record with the same unique value already exists.".into()
                ))
            );
            assert_eq!(
                message(&driver_error(ErrorKind::ForeignKeyViolation, wrap)),
                Some((409, "The change conflicts with a related record.".into()))
            );
            assert_eq!(message(&driver_error(ErrorKind::Other, wrap)), None);
        }
    }

    #[test]
    fn missing_records_are_not_found_and_other_errors_stay_500s() {
        assert_eq!(
            message(&DbErr::RecordNotFound("notes 1".into())),
            Some((404, "Not Found".into()))
        );
        assert_eq!(
            message(&DbErr::RecordNotUpdated),
            Some((404, "Not Found".into()))
        );
        assert_eq!(message(&DbErr::Custom("boom".into())), None);
    }
}
