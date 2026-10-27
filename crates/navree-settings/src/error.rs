//! [`SettingsError`].

use std::collections::BTreeMap;

use navree_http::AppError;
use navree_validation::ValidationErrors;
use sea_orm::DbErr;

use crate::ScopeKind;

/// What can go wrong reading or writing settings.
///
/// Returned from a handler (`?`), [`UnknownKey`](SettingsError::UnknownKey)
/// is a 404; [`Invalid`](SettingsError::Invalid),
/// [`UnknownFields`](SettingsError::UnknownFields) and
/// [`InvalidPatch`](SettingsError::InvalidPatch) are 400s with the fields
/// in `errors`; [`ScopeNotAllowed`](SettingsError::ScopeNotAllowed) is a
/// 400; the rest are 500s (logged, never shown).
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// The settings type was read or written, but no module registered it
    /// with `SettingsModule::for_feature::<T>()`.
    #[error(
        "the settings `{key}` ({type_name}) are not registered: import \
         SettingsModule::for_feature::<{short}>() in the module that owns them"
    )]
    Unregistered {
        /// The settings key.
        key: String,
        /// The Rust type.
        type_name: String,
        /// The type's last path segment, for the hint.
        short: String,
    },
    /// No registered settings have this key (routes, commands).
    #[error("unknown settings `{0}`")]
    UnknownKey(String),
    /// The settings cannot be stored at this level.
    #[error("the `{key}` settings cannot be stored at the {scope} level; they allow {allowed}")]
    ScopeNotAllowed {
        /// The settings key.
        key: String,
        /// The level of the write.
        scope: ScopeKind,
        /// The levels they allow, comma-separated.
        allowed: String,
    },
    /// A patch or an inherit named fields the settings do not have.
    #[error("the `{key}` settings have no field {}", quoted(.fields))]
    UnknownFields {
        /// The settings key.
        key: String,
        /// The unknown fields.
        fields: Vec<String>,
    },
    /// A patch that is not a JSON object.
    #[error("{0}")]
    InvalidPatch(String),
    /// The values after a patch fail to deserialize or break a garde rule;
    /// the errors are keyed by field.
    #[error("invalid settings: {}", .0.summary().unwrap_or_default())]
    Invalid(ValidationErrors),
    /// The settings type is unusable (it does not serialize to an object, or
    /// its defaults do not read back).
    #[error("{0}")]
    Definition(String),
    /// The database failed.
    #[error("settings storage failed: {0}")]
    Database(#[from] DbErr),
    /// A stored value could not be read.
    #[error("settings storage holds an invalid value: {0}")]
    Corrupt(String),
    /// Something else failed.
    #[error("{0}")]
    Other(String),
}

fn quoted(fields: &[String]) -> String {
    fields
        .iter()
        .map(|field| format!("`{field}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

impl SettingsError {
    /// `Unregistered` for type `type_name`.
    pub(crate) fn unregistered(key: &str, type_name: &str) -> SettingsError {
        SettingsError::Unregistered {
            key: key.to_owned(),
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
            SettingsError::UnknownKey(_) => {
                Some(AppError::not_found().with_detail(self.to_string()))
            }
            SettingsError::ScopeNotAllowed { .. } => Some(AppError::bad_request(self.to_string())),
            SettingsError::InvalidPatch(message) => Some(AppError::validation(BTreeMap::from([(
                "input".to_owned(),
                vec![message.clone()],
            )]))),
            SettingsError::UnknownFields { key, fields } => Some(AppError::validation(
                fields
                    .iter()
                    .map(|field| {
                        (
                            field.clone(),
                            vec![format!("{field} is not a setting of {key}")],
                        )
                    })
                    .collect(),
            )),
            SettingsError::Invalid(errors) => Some(errors.clone().into_app_error()),
            _ => None,
        }
    }
}

/// The `map_error` hook of the module.
pub(crate) fn map_settings_error(error: &SettingsError) -> Option<AppError> {
    error.to_app_error()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_errors_have_statuses_and_name_the_fields() {
        let unknown = SettingsError::UnknownKey("x".into())
            .to_app_error()
            .unwrap();
        assert_eq!(unknown.status.as_u16(), 404);
        assert_eq!(unknown.body()["message"], "unknown settings `x`");

        let scope = SettingsError::ScopeNotAllowed {
            key: "portal".into(),
            scope: ScopeKind::User,
            allowed: "global, workspace".into(),
        };
        assert_eq!(
            scope.to_string(),
            "the `portal` settings cannot be stored at the user level; they allow global, workspace"
        );
        assert_eq!(scope.to_app_error().unwrap().status.as_u16(), 400);

        let fields = SettingsError::UnknownFields {
            key: "portal".into(),
            fields: vec!["colour".into(), "size".into()],
        };
        assert_eq!(
            fields.to_string(),
            "the `portal` settings have no field `colour`, `size`"
        );
        let body = fields.to_app_error().unwrap().body();
        assert_eq!(body["statusCode"], 400);
        assert_eq!(
            body["errors"]["colour"][0],
            "colour is not a setting of portal"
        );

        let mut errors = ValidationErrors::new();
        errors.add("welcome_text", "welcome_text should not be empty");
        let invalid = SettingsError::Invalid(errors).to_app_error().unwrap();
        assert_eq!(invalid.status.as_u16(), 400);
        assert_eq!(
            invalid.body()["errors"]["welcome_text"][0],
            "welcome_text should not be empty"
        );

        assert!(SettingsError::Other("x".into()).to_app_error().is_none());
        assert!(SettingsError::Corrupt("x".into()).to_app_error().is_none());
    }

    #[test]
    fn unregistered_names_the_registration() {
        let error = SettingsError::unregistered("portal", "app::portal::PortalSettings");
        assert_eq!(
            error.to_string(),
            "the settings `portal` (app::portal::PortalSettings) are not registered: import \
             SettingsModule::for_feature::<PortalSettings>() in the module that owns them"
        );
        assert!(error.to_app_error().is_none());
    }
}
