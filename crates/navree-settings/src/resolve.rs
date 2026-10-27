//! Resolution: the stored values of a chain merged over the defaults, one
//! field at a time, with the source of every field. [`ResolvedSettings`]
//! (JSON, what the routes return) and [`Resolved<T>`] (typed).

use std::collections::BTreeMap;
use std::ops::Deref;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;

use crate::{Scope, SettingsDefinition, SettingsError, SettingsGroup, Source};

/// Resolved settings as JSON: the values, where each came from, and the
/// fields set at the scope's own level. What `GET /api/settings/{key}`
/// returns.
///
/// ```json
/// {
///   "data": { "show_leaderboard": true, "welcome_text": "Welcome to Acme" },
///   "sources": { "show_leaderboard": "organization", "welcome_text": "workspace" },
///   "overridden": ["welcome_text"]
/// }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ResolvedSettings {
    /// Every setting's value.
    #[schema(value_type = Object)]
    pub data: Map<String, Value>,
    /// Where each value came from: `default`, `global`, `organization`,
    /// `workspace` or `user`.
    pub sources: BTreeMap<String, Source>,
    /// The fields stored at the scope's own level (its target), which
    /// [`inherit`](crate::Settings::inherit) resets; sorted.
    pub overridden: Vec<String>,
}

impl ResolvedSettings {
    /// Where `field` came from (`None` for a field the settings do not
    /// have).
    pub fn source(&self, field: &str) -> Option<Source> {
        self.sources.get(field).copied()
    }

    /// Whether `field` is stored at the scope's own level.
    pub fn is_overridden(&self, field: &str) -> bool {
        self.overridden.iter().any(|name| name == field)
    }

    /// The values deserialized into `T`.
    pub fn typed<T: DeserializeOwned>(&self) -> Result<T, SettingsError> {
        serde_json::from_value(Value::Object(self.data.clone()))
            .map_err(|error| SettingsError::Corrupt(error.to_string()))
    }
}

/// Typed settings resolved for a scope: the value, and where each field
/// came from. Derefs to the value; serializes as `{data, sources,
/// overridden}`, so a handler can return it as JSON.
///
/// ```ignore
/// let portal = settings.resolve::<PortalSettings>(Scope::workspace(acme.id)).await?;
/// if portal.show_leaderboard { /* .. */ }
/// assert_eq!(portal.source("show_leaderboard"), Some(Source::Organization));
/// ```
#[derive(Clone, Debug)]
pub struct Resolved<T> {
    /// The settings.
    pub value: T,
    /// Where each field came from.
    pub sources: BTreeMap<String, Source>,
    /// The fields stored at the scope's own level, sorted.
    pub overridden: Vec<String>,
    /// The scope they were resolved for (with its organization, once looked
    /// up).
    pub scope: Scope,
}

impl<T> Resolved<T> {
    /// Where `field` came from (`None` for a field the settings do not
    /// have).
    pub fn source(&self, field: &str) -> Option<Source> {
        self.sources.get(field).copied()
    }

    /// Whether `field` is stored at the scope's own level.
    pub fn is_overridden(&self, field: &str) -> bool {
        self.overridden.iter().any(|name| name == field)
    }

    /// The settings.
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T: SettingsGroup> Resolved<T> {
    /// The typed form of `resolved`, for `scope`.
    pub(crate) fn from_json(
        resolved: ResolvedSettings,
        scope: Scope,
    ) -> Result<Self, SettingsError> {
        Ok(Resolved {
            value: resolved.typed::<T>()?,
            sources: resolved.sources,
            overridden: resolved.overridden,
            scope,
        })
    }
}

impl<T> Deref for Resolved<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T: Serialize> Serialize for Resolved<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut out = serializer.serialize_struct("Resolved", 3)?;
        out.serialize_field("data", &self.value)?;
        out.serialize_field("sources", &self.sources)?;
        out.serialize_field("overridden", &self.overridden)?;
        out.end()
    }
}

/// The stored values of one level of a chain.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Layer<'a> {
    /// Where its values come from.
    pub(crate) source: Source,
    /// The fields stored at the level.
    pub(crate) values: &'a Map<String, Value>,
}

/// A merged chain.
#[derive(Debug)]
pub(crate) struct Merged {
    /// The values, in canonical form.
    pub(crate) data: Map<String, Value>,
    /// Where each came from.
    pub(crate) sources: BTreeMap<String, Source>,
    /// Stored values left out because the type no longer accepts them:
    /// (field, level, why).
    pub(crate) skipped: Vec<(String, Source, String)>,
}

/// Merges `layers` (from the most general level to the most specific) over
/// `definition`'s defaults, one field at a time. A stored field the type
/// no longer has is ignored; a stored value the type no longer accepts (a
/// changed field type) is skipped, so the level below shows through.
pub(crate) fn merge(
    definition: &SettingsDefinition,
    layers: &[Layer<'_>],
) -> Result<Merged, SettingsError> {
    let defaults = definition.defaults().map_err(SettingsError::Definition)?;
    let mut data = defaults.clone();
    let mut sources: BTreeMap<String, Source> = defaults
        .keys()
        .map(|field| (field.clone(), Source::Default))
        .collect();
    for layer in layers {
        for (field, value) in layer.values {
            if defaults.contains_key(field) {
                data.insert(field.clone(), value.clone());
                sources.insert(field.clone(), layer.source);
            }
        }
    }
    if let Ok(Value::Object(canonical)) = definition.canonical(&Value::Object(data)) {
        return Ok(Merged {
            data: canonical,
            sources,
            skipped: Vec::new(),
        });
    }
    // Some stored value no longer fits the type: add the levels one field
    // at a time, keeping the values that do.
    let mut data = defaults.clone();
    let mut sources: BTreeMap<String, Source> = defaults
        .keys()
        .map(|field| (field.clone(), Source::Default))
        .collect();
    let mut skipped = Vec::new();
    for layer in layers {
        for (field, value) in layer.values {
            if !defaults.contains_key(field) {
                continue;
            }
            let previous = data.insert(field.clone(), value.clone());
            match definition.canonical(&Value::Object(data.clone())) {
                Ok(_) => {
                    sources.insert(field.clone(), layer.source);
                }
                Err(error) => {
                    if let Some(previous) = previous {
                        data.insert(field.clone(), previous);
                    }
                    skipped.push((field.clone(), layer.source, error));
                }
            }
        }
    }
    match definition.canonical(&Value::Object(data)) {
        Ok(Value::Object(canonical)) => Ok(Merged {
            data: canonical,
            sources,
            skipped,
        }),
        Ok(other) => Err(SettingsError::Definition(format!(
            "{} must serialize to a JSON object, not `{other}`",
            definition.type_name
        ))),
        Err(error) => Err(SettingsError::Definition(format!(
            "the defaults of {} do not deserialize: {error}",
            definition.type_name
        ))),
    }
}

#[cfg(test)]
mod tests {
    use garde::Validate;
    use serde_json::json;

    use super::*;

    #[derive(Debug, Serialize, Deserialize, Validate, Default, PartialEq)]
    struct PortalSettings {
        #[garde(skip)]
        show_leaderboard: bool,
        #[garde(skip)]
        welcome_text: String,
        #[garde(skip)]
        max_items: u32,
    }

    impl SettingsGroup for PortalSettings {
        const KEY: &'static str = "portal";
    }

    fn map(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            _ => unreachable!(),
        }
    }

    #[test]
    fn nothing_stored_is_the_defaults() {
        let merged = merge(&SettingsDefinition::of::<PortalSettings>(), &[]).unwrap();
        assert_eq!(
            Value::Object(merged.data),
            json!({"show_leaderboard": false, "welcome_text": "", "max_items": 0})
        );
        assert!(
            merged
                .sources
                .values()
                .all(|source| *source == Source::Default)
        );
    }

    #[test]
    fn the_most_specific_level_wins_field_by_field() {
        let global =
            map(json!({"show_leaderboard": true, "welcome_text": "Hello", "max_items": 5}));
        let organization = map(json!({"welcome_text": "Hello from the holding"}));
        let workspace = map(json!({"max_items": 9}));
        let merged = merge(
            &SettingsDefinition::of::<PortalSettings>(),
            &[
                Layer {
                    source: Source::Global,
                    values: &global,
                },
                Layer {
                    source: Source::Organization,
                    values: &organization,
                },
                Layer {
                    source: Source::Workspace,
                    values: &workspace,
                },
            ],
        )
        .unwrap();
        assert_eq!(
            Value::Object(merged.data),
            json!({"show_leaderboard": true, "welcome_text": "Hello from the holding", "max_items": 9})
        );
        assert_eq!(merged.sources["show_leaderboard"], Source::Global);
        assert_eq!(merged.sources["welcome_text"], Source::Organization);
        assert_eq!(merged.sources["max_items"], Source::Workspace);
        assert!(merged.skipped.is_empty());
    }

    #[test]
    fn unknown_and_unreadable_stored_values_are_left_out() {
        let global = map(json!({"max_items": 5, "removed_field": true}));
        // A value of the wrong type (the field's type changed since).
        let workspace = map(json!({"max_items": "nine", "welcome_text": "Hi"}));
        let merged = merge(
            &SettingsDefinition::of::<PortalSettings>(),
            &[
                Layer {
                    source: Source::Global,
                    values: &global,
                },
                Layer {
                    source: Source::Workspace,
                    values: &workspace,
                },
            ],
        )
        .unwrap();
        assert_eq!(
            Value::Object(merged.data),
            json!({"show_leaderboard": false, "welcome_text": "Hi", "max_items": 5})
        );
        assert_eq!(merged.sources["max_items"], Source::Global);
        assert_eq!(merged.sources["welcome_text"], Source::Workspace);
        assert!(!merged.sources.contains_key("removed_field"));
        assert_eq!(merged.skipped.len(), 1);
        assert_eq!(merged.skipped[0].0, "max_items");
        assert_eq!(merged.skipped[0].1, Source::Workspace);
    }

    #[test]
    fn resolved_values_serialize_and_report_sources() {
        let resolved = ResolvedSettings {
            data: map(json!({"show_leaderboard": true, "welcome_text": "", "max_items": 0})),
            sources: BTreeMap::from([
                ("show_leaderboard".to_owned(), Source::Workspace),
                ("welcome_text".to_owned(), Source::Default),
                ("max_items".to_owned(), Source::Default),
            ]),
            overridden: vec!["show_leaderboard".to_owned()],
        };
        assert_eq!(resolved.source("show_leaderboard"), Some(Source::Workspace));
        assert_eq!(resolved.source("nope"), None);
        assert!(resolved.is_overridden("show_leaderboard"));
        assert!(!resolved.is_overridden("welcome_text"));
        let typed: Resolved<PortalSettings> =
            Resolved::from_json(resolved.clone(), Scope::global()).unwrap();
        assert!(typed.show_leaderboard);
        assert_eq!(typed.source("show_leaderboard"), Some(Source::Workspace));
        assert_eq!(
            serde_json::to_value(&typed).unwrap(),
            serde_json::to_value(&resolved).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&resolved).unwrap()["sources"]["welcome_text"],
            "default"
        );
    }
}
