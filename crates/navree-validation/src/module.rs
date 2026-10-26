//! [`ValidationModule`] and [`ValidationOptions`].

use std::collections::BTreeMap;
use std::sync::Arc;

use navree_config::Config;
use navree_kernel::{Factory, Module, ModuleBuilder, ModuleId, ProviderKey, Resolver};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::Messages;

/// The configuration section of [`ValidationModule::from_config`].
pub const CONFIG_SECTION: &str = "validation";

/// Message and attribute-name overrides (section `validation`).
///
/// ```toml
/// [validation.messages]
/// required = ":attribute est obligatoire"
/// "email.unique" = "This e-mail is already registered"
///
/// [validation.attributes]
/// first_name = "first name"
/// ```
///
/// Nested tables flatten to dotted keys, so `[validation.messages.min]
/// string = "..."` sets `min.string`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ValidationOptions {
    /// Templates by rule (`"required"`, `"min.string"`) or by field and rule
    /// (`"email.unique"`); see [`DEFAULT_MESSAGES`](crate::DEFAULT_MESSAGES).
    #[serde(deserialize_with = "flat_strings")]
    pub messages: BTreeMap<String, String>,
    /// Display names by field path (`"address.city"`), used for `:attribute`.
    #[serde(deserialize_with = "flat_strings")]
    pub attributes: BTreeMap<String, String>,
}

impl From<ValidationOptions> for Messages {
    fn from(options: ValidationOptions) -> Self {
        Messages::new(options.messages, options.attributes)
    }
}

/// A string map whose nested maps flatten to dotted keys.
fn flat_strings<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    fn flatten(
        prefix: &str,
        value: Value,
        out: &mut BTreeMap<String, String>,
    ) -> Result<(), String> {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    let key = if prefix.is_empty() {
                        key
                    } else {
                        format!("{prefix}.{key}")
                    };
                    flatten(&key, value, out)?;
                }
                Ok(())
            }
            Value::String(text) => {
                out.insert(prefix.to_owned(), text);
                Ok(())
            }
            Value::Null => Ok(()),
            other => Err(format!("`{prefix}` must be a string, found {other}")),
        }
    }
    let value = Value::deserialize(deserializer)?;
    let mut out = BTreeMap::new();
    flatten("", value, &mut out).map_err(serde::de::Error::custom)?;
    Ok(out)
}

/// The validation module. Global; provides and exports the app's
/// [`Messages`], which the [`Valid`](crate::Valid) and
/// [`ValidAsync`](crate::ValidAsync) extractors render errors with (without
/// the module they use the built-in messages).
///
/// - [`for_root`](ValidationModule::for_root): the built-in messages.
/// - [`with_options`](ValidationModule::with_options): explicit overrides.
/// - [`from_config`](ValidationModule::from_config) or a plain
///   `.module(ValidationModule)`: overrides from the `validation`
///   configuration section, so the app must include `ConfigModule`.
///
/// ```no_run
/// use navree_kernel::Navree;
/// use navree_validation::ValidationModule;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app().module(ValidationModule::for_root()).run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct ValidationModule;

impl ValidationModule {
    /// The module with the built-in English messages.
    pub fn for_root() -> impl Module {
        Self::with_options(ValidationOptions::default())
    }

    /// The module with explicit message and attribute overrides.
    pub fn with_options(options: ValidationOptions) -> impl Module {
        ConfiguredValidationModule {
            options: Some(options),
        }
    }

    /// The module configured from the `validation` section of `Config`.
    pub fn from_config() -> impl Module {
        ConfiguredValidationModule { options: None }
    }
}

impl Module for ValidationModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

/// `ValidationModule::for_root` / `with_options` / `from_config`: same module id, configured.
struct ConfiguredValidationModule {
    options: Option<ValidationOptions>,
}

impl Module for ConfiguredValidationModule {
    fn id(&self) -> ModuleId {
        ModuleId::of::<ValidationModule>()
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn register(&self, m: &mut ModuleBuilder) {
        register(m, self.options.clone());
    }
}

fn register(m: &mut ModuleBuilder, options: Option<ValidationOptions>) {
    m.global();
    match options {
        Some(options) => m.value(Messages::from(options)),
        None => m.factory(
            Factory::<Messages>::new(|r: Resolver| async move {
                let config = r.get::<Config>().await?;
                let options: ValidationOptions = config.section_or_default(CONFIG_SECTION)?;
                Ok(Arc::new(Messages::from(options)))
            })
            .deps(vec![ProviderKey::of::<Config>()]),
        ),
    };
    m.exports::<Messages>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_flatten_nested_tables() {
        let options: ValidationOptions = serde_json::from_value(serde_json::json!({
            "messages": { "required": "R", "min": { "string": "S" }, "email": { "unique": "U" } },
        }))
        .unwrap();
        assert_eq!(
            options.messages,
            BTreeMap::from([
                ("email.unique".to_owned(), "U".to_owned()),
                ("min.string".to_owned(), "S".to_owned()),
                ("required".to_owned(), "R".to_owned()),
            ])
        );
        assert!(options.attributes.is_empty());
        let error = serde_json::from_value::<ValidationOptions>(serde_json::json!({
            "messages": { "required": 5 },
        }))
        .unwrap_err();
        assert!(
            error.to_string().contains("`required` must be a string"),
            "{error}"
        );
    }
}
