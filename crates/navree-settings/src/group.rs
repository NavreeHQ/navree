//! [`SettingsGroup`]: a settings type, stored under one key; and
//! [`SettingsDefinition`], what the module knows about one.

use std::fmt;

use navree_validation::Validate;
use navree_validation::{Messages, ValidationErrors, validate_json};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::ScopeKind;

/// A group of settings stored under one key: a struct whose fields are
/// the settings. Implement it with `#[derive(Settings)]` (navree-macros,
/// re-exported as `navree::settings::Settings`):
///
/// ```ignore
/// use navree::settings::Settings;
///
/// /// What the member portal shows.
/// #[derive(Settings, Serialize, Deserialize, Validate, Default)]
/// #[settings(key = "portal", scopes(global, organization, workspace))]
/// pub struct PortalSettings {
///     #[validate(skip)]
///     pub show_leaderboard: bool,
///     #[validate(length(max = 500))]
///     pub welcome_text: String,
/// }
/// ```
///
/// By hand, the same:
///
/// ```
/// use navree_validation::Validate;
/// use navree_settings::{ScopeKind, SettingsGroup};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize, Validate, Default)]
/// pub struct PortalSettings {
///     #[validate(skip)]
///     pub show_leaderboard: bool,
///     #[validate(length(max = 500))]
///     pub welcome_text: String,
/// }
///
/// impl SettingsGroup for PortalSettings {
///     const KEY: &'static str = "portal";
///     const SCOPES: &'static [ScopeKind] =
///         &[ScopeKind::Global, ScopeKind::Organization, ScopeKind::Workspace];
///     const DESCRIPTION: &'static str = "What the member portal shows.";
/// }
/// ```
///
/// The type serializes to a JSON object whose top-level fields are the
/// settings (serde's renames apply); each field is stored, inherited and
/// reported on its own. `Default` gives the values when nothing is stored,
/// and its validation rules check every write. Register the type with
/// `SettingsModule::for_feature::<T>()` or `m.settings::<T>()`.
pub trait SettingsGroup:
    Serialize + DeserializeOwned + Default + Validate + Send + Sync + 'static
{
    /// The stored key (`portal`): lower-case letters, digits, `-`, `_` and
    /// `.`, at most [`MAX_KEY_LEN`] characters. Values are keyed by it, so
    /// keep it stable. Also the path segment of the routes
    /// (`/api/settings/portal`).
    const KEY: &'static str;
    /// The levels the settings may be stored at, in any order (default:
    /// global, organization and workspace). Writes at another level are
    /// refused; resolution skips the levels not listed.
    const SCOPES: &'static [ScopeKind] = &[
        ScopeKind::Global,
        ScopeKind::Organization,
        ScopeKind::Workspace,
    ];
    /// What the settings are for (the doc comment, with the derive), shown
    /// by `settings:list` and `GET /api/settings`.
    const DESCRIPTION: &'static str = "";
    /// The permission a write needs at every level but the user's (which
    /// is the user's own). `None` (the default): `settings.manage` at the
    /// global level, `organization.update` in an organization and
    /// `workspace.update` in a workspace.
    const PERMISSION: Option<&'static str> = None;
    /// The permission a read through the routes needs, at its level. `None`
    /// (the default): any signed-in user reads the global values, any
    /// member an organization's or a workspace's, and users their own.
    const READ_PERMISSION: Option<&'static str> = None;
}

/// The longest settings key.
pub const MAX_KEY_LEN: usize = 100;

/// Checks a settings key: 1 to [`MAX_KEY_LEN`] characters, lower-case
/// ASCII letters, digits, `-`, `_` and `.`, starting with a letter or a
/// digit.
pub fn check_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("a settings key must not be empty".to_owned());
    }
    if key.len() > MAX_KEY_LEN {
        return Err(format!(
            "the settings key `{key}` is longer than {MAX_KEY_LEN} characters"
        ));
    }
    let valid =
        |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.');
    let first = key.chars().next().unwrap_or('-');
    if !key.chars().all(valid) || !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err(format!(
            "the settings key `{key}` may only hold lower-case letters, digits, `-`, `_` and `.`, \
             and must start with a letter or a digit"
        ));
    }
    Ok(())
}

/// The defaults of a settings type as a JSON object.
type DefaultsFn = fn() -> Result<Map<String, Value>, String>;
/// Deserializes a JSON object into the type and back (its canonical form).
type CanonicalFn = fn(&Value) -> Result<Value, String>;
/// Deserializes and validates a JSON object, returning its canonical form.
type ValidateFn = fn(&Value, &Messages) -> Result<Value, ValidationErrors>;

/// A registered settings type: its key, levels, description and
/// permissions, plus the functions the module needs to read and check its
/// values without knowing the type.
#[derive(Clone, Copy)]
pub struct SettingsDefinition {
    /// The stored key.
    pub key: &'static str,
    /// The levels it may be stored at.
    pub scopes: &'static [ScopeKind],
    /// What it is for.
    pub description: &'static str,
    /// The write permission, if the type names one.
    pub permission: Option<&'static str>,
    /// The read permission, if the type names one.
    pub read_permission: Option<&'static str>,
    /// The Rust type (for error messages).
    pub type_name: &'static str,
    pub(crate) defaults: DefaultsFn,
    pub(crate) canonical: CanonicalFn,
    pub(crate) validate: ValidateFn,
}

impl SettingsDefinition {
    /// The definition of `T`.
    pub fn of<T: SettingsGroup>() -> Self {
        SettingsDefinition {
            key: T::KEY,
            scopes: T::SCOPES,
            description: T::DESCRIPTION,
            permission: T::PERMISSION,
            read_permission: T::READ_PERMISSION,
            type_name: std::any::type_name::<T>(),
            defaults: defaults_of::<T>,
            canonical: canonical_of::<T>,
            validate: validate_of::<T>,
        }
    }

    /// Whether the settings may be stored at `kind`.
    pub fn allows(&self, kind: ScopeKind) -> bool {
        self.scopes.contains(&kind)
    }

    /// The levels, from the most general to the most specific.
    pub fn levels(&self) -> Vec<ScopeKind> {
        ScopeKind::ALL
            .into_iter()
            .filter(|kind| self.allows(*kind))
            .collect()
    }

    /// The levels, comma-separated (`global, organization, workspace`).
    pub(crate) fn levels_list(&self) -> String {
        self.levels()
            .into_iter()
            .map(ScopeKind::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The default values as a JSON object.
    pub fn defaults(&self) -> Result<Map<String, Value>, String> {
        (self.defaults)()
    }

    /// `value` deserialized into the type and serialized back.
    pub fn canonical(&self, value: &Value) -> Result<Value, String> {
        (self.canonical)(value)
    }

    /// `value` deserialized into the type and checked with its rules;
    /// failures name the field.
    pub fn validate(&self, value: &Value, messages: &Messages) -> Result<Value, ValidationErrors> {
        (self.validate)(value, messages)
    }
}

impl fmt::Debug for SettingsDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsDefinition")
            .field("key", &self.key)
            .field("scopes", &self.scopes)
            .field("type_name", &self.type_name)
            .finish_non_exhaustive()
    }
}

impl PartialEq for SettingsDefinition {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.type_name == other.type_name
    }
}

fn defaults_of<T: SettingsGroup>() -> Result<Map<String, Value>, String> {
    match serde_json::to_value(T::default()) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(other) => Err(format!(
            "{} must serialize to a JSON object (its fields are the settings), not `{other}`",
            std::any::type_name::<T>()
        )),
        Err(error) => Err(format!(
            "{} could not be serialized: {error}",
            std::any::type_name::<T>()
        )),
    }
}

fn canonical_of<T: SettingsGroup>(value: &Value) -> Result<Value, String> {
    let typed = T::deserialize(value).map_err(|error| error.to_string())?;
    serde_json::to_value(typed).map_err(|error| error.to_string())
}

fn validate_of<T: SettingsGroup>(
    value: &Value,
    messages: &Messages,
) -> Result<Value, ValidationErrors> {
    let typed = validate_json::<T>(value, messages)?;
    serde_json::to_value(typed).map_err(|error| {
        let mut errors = ValidationErrors::new();
        errors.add("input", error.to_string());
        errors
    })
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    #[derive(Serialize, Deserialize, Validate, Default)]
    struct PortalSettings {
        #[validate(skip)]
        show_leaderboard: bool,
        #[validate(length(max = 5))]
        welcome_text: String,
    }

    impl SettingsGroup for PortalSettings {
        const KEY: &'static str = "portal";
        const DESCRIPTION: &'static str = "What the portal shows.";
    }

    #[derive(Serialize, Deserialize, Validate, Default)]
    #[validate(skip_unmarked)]
    struct NotAnObject(u32);

    impl SettingsGroup for NotAnObject {
        const KEY: &'static str = "number";
        const SCOPES: &'static [ScopeKind] = &[ScopeKind::User, ScopeKind::Global];
        const PERMISSION: Option<&'static str> = Some("numbers.manage");
    }

    #[test]
    fn definitions_carry_the_constants() {
        let portal = SettingsDefinition::of::<PortalSettings>();
        assert_eq!(portal.key, "portal");
        assert_eq!(portal.description, "What the portal shows.");
        assert_eq!(
            portal.levels(),
            [
                ScopeKind::Global,
                ScopeKind::Organization,
                ScopeKind::Workspace
            ]
        );
        assert!(!portal.allows(ScopeKind::User));
        assert_eq!(portal.permission, None);
        assert!(portal.type_name.ends_with("PortalSettings"));

        let number = SettingsDefinition::of::<NotAnObject>();
        assert_eq!(number.levels(), [ScopeKind::Global, ScopeKind::User]);
        assert_eq!(number.permission, Some("numbers.manage"));
        assert_ne!(portal, number);
    }

    #[test]
    fn defaults_are_a_json_object() {
        let portal = SettingsDefinition::of::<PortalSettings>();
        assert_eq!(
            Value::Object(portal.defaults().unwrap()),
            json!({"show_leaderboard": false, "welcome_text": ""})
        );
        let error = SettingsDefinition::of::<NotAnObject>()
            .defaults()
            .unwrap_err();
        assert!(error.contains("must serialize to a JSON object"), "{error}");
    }

    #[test]
    fn values_are_made_canonical_and_validated() {
        let portal = SettingsDefinition::of::<PortalSettings>();
        assert_eq!(
            portal
                .canonical(&json!({"show_leaderboard": true, "welcome_text": "Hi", "extra": 1}))
                .unwrap(),
            json!({"show_leaderboard": true, "welcome_text": "Hi"})
        );
        assert!(
            portal
                .canonical(&json!({"show_leaderboard": "yes", "welcome_text": ""}))
                .is_err()
        );

        let messages = Messages::default();
        let errors = portal
            .validate(
                &json!({"show_leaderboard": true, "welcome_text": "far too long"}),
                &messages,
            )
            .unwrap_err();
        assert_eq!(
            errors.get("welcome_text").unwrap(),
            ["welcome_text must be shorter than or equal to 5 characters"]
        );
        let errors = portal
            .validate(
                &json!({"show_leaderboard": "yes", "welcome_text": ""}),
                &messages,
            )
            .unwrap_err();
        assert!(errors.get("show_leaderboard").is_some(), "{errors:?}");
        assert_eq!(
            portal
                .validate(
                    &json!({"show_leaderboard": true, "welcome_text": "Hi"}),
                    &messages
                )
                .unwrap(),
            json!({"show_leaderboard": true, "welcome_text": "Hi"})
        );
    }

    #[test]
    fn keys_are_checked() {
        for good in [
            "portal",
            "team_portal",
            "team-portal",
            "mail.delivery",
            "2fa",
        ] {
            assert_eq!(check_key(good), Ok(()), "{good}");
        }
        for bad in ["", "Portal", "team portal", "-x", ".x", "émoji"] {
            assert!(check_key(bad).is_err(), "{bad}");
        }
        assert!(check_key(&"a".repeat(MAX_KEY_LEN)).is_ok());
        assert!(check_key(&"a".repeat(MAX_KEY_LEN + 1)).is_err());
    }
}
