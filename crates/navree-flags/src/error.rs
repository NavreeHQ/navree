//! [`FlagsError`].

use navree_db::DbError;
use navree_http::AppError;
use navree_orm::OrmError;

/// What can go wrong checking or changing a flag.
///
/// Returned from a handler (`?`), [`Unknown`](FlagsError::Unknown) is a
/// 404 and [`InvalidScope`](FlagsError::InvalidScope),
/// [`InvalidRollout`](FlagsError::InvalidRollout) and
/// [`NoPlanResolver`](FlagsError::NoPlanResolver) are 400s; the rest are
/// 500s (logged, never shown).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FlagsError {
    /// The flag type was checked or changed, but no module registered it
    /// with `FlagsModule::for_feature::<F>()`.
    #[error(
        "the flag `{name}` ({type_name}) is not registered: import \
         FlagsModule::for_feature::<{short}>() in the module that owns it"
    )]
    Unregistered {
        /// The flag's name.
        name: String,
        /// The flag's Rust type.
        type_name: String,
        /// The type's last path segment, for the hint.
        short: String,
    },
    /// No registered flag has this name (CLI, admin routes).
    #[error("unknown flag `{0}`")]
    Unknown(String),
    /// An override names one user or one workspace, not both.
    #[error("{0}")]
    InvalidScope(String),
    /// A rollout percentage above 100 or a blank plan name.
    #[error("invalid rollout: {0}")]
    InvalidRollout(String),
    /// A rollout by plan, but the app registers no `dyn PlanResolver`.
    #[error(
        "rollouts by plan need a `dyn PlanResolver` in the app: register one with \
         ModuleBuilderFlagsExt::plan_resolver"
    )]
    NoPlanResolver,
    /// The database failed.
    #[error("feature flag storage failed: {0}")]
    Database(#[from] DbError),
    /// A navree-orm operation on the flags tables failed.
    #[error("flag storage failed: {0}")]
    Orm(#[from] OrmError),
    /// A stored value could not be read back.
    #[error("feature flag storage holds an invalid value: {0}")]
    Corrupt(String),
    /// Something else failed (the plan resolver, the app shutting down).
    #[error("{0}")]
    Other(String),
}

impl FlagsError {
    /// `Unregistered` for type `F`.
    pub(crate) fn unregistered(name: &str, type_name: &str) -> FlagsError {
        FlagsError::Unregistered {
            name: name.to_owned(),
            type_name: type_name.to_owned(),
            short: type_name
                .rsplit("::")
                .next()
                .unwrap_or(type_name)
                .to_owned(),
        }
    }

    /// The error response of this error, `None` for a 500.
    pub fn to_app_error(&self) -> Option<AppError> {
        match self {
            FlagsError::Unknown(_) => Some(AppError::not_found().with_detail(self.to_string())),
            FlagsError::InvalidScope(_)
            | FlagsError::InvalidRollout(_)
            | FlagsError::NoPlanResolver => Some(AppError::bad_request(self.to_string())),
            _ => None,
        }
    }
}

/// The `map_error` hook of the module.
pub(crate) fn map_flags_error(error: &FlagsError) -> Option<AppError> {
    error.to_app_error()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_errors_have_statuses() {
        let unknown = FlagsError::Unknown("x".into()).to_app_error().unwrap();
        assert_eq!(unknown.status.as_u16(), 404);
        assert_eq!(unknown.body()["message"], "unknown flag `x`");
        let scope = FlagsError::InvalidScope("bad".into())
            .to_app_error()
            .unwrap();
        assert_eq!(scope.status.as_u16(), 400);
        assert!(FlagsError::NoPlanResolver.to_app_error().is_some());
        assert!(FlagsError::Other("x".into()).to_app_error().is_none());
    }

    #[test]
    fn unregistered_names_the_registration() {
        let error = FlagsError::unregistered("new-pipeline", "app::flags::NewPipeline");
        assert_eq!(
            error.to_string(),
            "the flag `new-pipeline` (app::flags::NewPipeline) is not registered: import \
             FlagsModule::for_feature::<NewPipeline>() in the module that owns it"
        );
        assert!(error.to_app_error().is_none());
    }
}
