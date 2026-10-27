//! The settings types the app's modules registered with
//! `SettingsModule::for_feature::<T>()`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, PoisonError, RwLock};

use serde_json::{Map, Value};

use crate::{SettingsDefinition, SettingsError, check_key};

/// A `SettingsModule::for_feature::<T>()` registration (a contribution).
#[derive(Clone, Copy, Debug)]
pub(crate) struct SettingsRegistration(pub(crate) SettingsDefinition);

/// A registered settings type, with its defaults worked out once.
#[derive(Clone, Debug)]
pub struct RegisteredSettings {
    /// What the type declares.
    pub definition: SettingsDefinition,
    /// Its default values.
    pub defaults: Map<String, Value>,
    /// Its fields (the defaults' keys).
    pub fields: BTreeSet<String>,
}

impl RegisteredSettings {
    /// Checks `definition` (its key, levels and defaults) and works out its
    /// fields.
    pub fn new(definition: SettingsDefinition) -> Result<Self, String> {
        check_key(definition.key).map_err(|error| format!("{error} ({})", definition.type_name))?;
        if definition.scopes.is_empty() {
            return Err(format!(
                "the `{}` settings ({}) allow no level: list at least one of global, \
                 organization, workspace and user",
                definition.key, definition.type_name
            ));
        }
        let defaults = definition.defaults()?;
        if let Err(error) = definition.canonical(&Value::Object(defaults.clone())) {
            return Err(format!(
                "the defaults of {} do not deserialize back: {error}",
                definition.type_name
            ));
        }
        let fields = defaults.keys().cloned().collect();
        Ok(RegisteredSettings {
            definition,
            defaults,
            fields,
        })
    }

    /// Whether the settings have `field`.
    pub fn has_field(&self, field: &str) -> bool {
        self.fields.contains(field)
    }
}

/// Every registered settings type, by key.
#[derive(Debug, Default)]
pub struct SettingsRegistry {
    entries: RwLock<BTreeMap<&'static str, Arc<RegisteredSettings>>>,
    /// Whether typed calls register unknown types on the fly (the fake).
    open: bool,
}

impl SettingsRegistry {
    /// The registry of `definitions`. The same type twice counts once; two
    /// types with one key, an invalid key, no level or a type that does not
    /// serialize to a JSON object is an error naming them.
    pub fn new(definitions: impl IntoIterator<Item = SettingsDefinition>) -> Result<Self, String> {
        let mut entries: BTreeMap<&'static str, Arc<RegisteredSettings>> = BTreeMap::new();
        for definition in definitions {
            match entries.get(definition.key) {
                Some(first) if first.definition.type_name != definition.type_name => {
                    return Err(format!(
                        "two settings types use the key `{}`: {} and {}; give one of them another \
                         `#[settings(key = \"..\")]`",
                        definition.key, first.definition.type_name, definition.type_name
                    ));
                }
                Some(_) => {}
                None => {
                    entries.insert(
                        definition.key,
                        Arc::new(RegisteredSettings::new(definition)?),
                    );
                }
            }
        }
        Ok(SettingsRegistry {
            entries: RwLock::new(entries),
            open: false,
        })
    }

    /// A registry that registers every type a typed call names (the fake's).
    pub(crate) fn open() -> Self {
        SettingsRegistry {
            entries: RwLock::default(),
            open: true,
        }
    }

    /// The settings registered under `key`, if any.
    pub fn get(&self, key: &str) -> Option<Arc<RegisteredSettings>> {
        self.entries
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned()
    }

    /// Whether settings are registered under `key`.
    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Every registered settings type, by key.
    pub fn all(&self) -> Vec<Arc<RegisteredSettings>> {
        self.entries
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }

    /// How many settings types are registered.
    pub fn len(&self) -> usize {
        self.entries
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Whether no settings type is registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The registration of `definition`'s type (registering it first in an
    /// open registry): [`SettingsError::Unregistered`] when another module
    /// did not register it.
    pub(crate) fn of(
        &self,
        definition: &SettingsDefinition,
    ) -> Result<Arc<RegisteredSettings>, SettingsError> {
        if let Some(entry) = self.get(definition.key)
            && entry.definition.type_name == definition.type_name
        {
            return Ok(entry);
        }
        if !self.open || self.contains(definition.key) {
            return Err(SettingsError::unregistered(
                definition.key,
                definition.type_name,
            ));
        }
        let entry = Arc::new(RegisteredSettings::new(*definition).map_err(SettingsError::Definition)?);
        self.entries
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(definition.key, entry.clone());
        Ok(entry)
    }

    /// The settings registered under `key`: [`SettingsError::UnknownKey`]
    /// otherwise.
    pub(crate) fn by_key(&self, key: &str) -> Result<Arc<RegisteredSettings>, SettingsError> {
        self.get(key)
            .ok_or_else(|| SettingsError::UnknownKey(key.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use garde::Validate;
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{ScopeKind, SettingsGroup};

    #[derive(Serialize, Deserialize, Validate, Default)]
    #[garde(allow_unvalidated)]
    struct Portal {
        show_leaderboard: bool,
    }

    impl SettingsGroup for Portal {
        const KEY: &'static str = "portal";
    }

    mod other {
        use super::*;

        #[derive(Serialize, Deserialize, Validate, Default)]
        #[garde(allow_unvalidated)]
        pub struct Portal {
            pub colour: String,
        }

        impl SettingsGroup for Portal {
            const KEY: &'static str = "portal";
        }
    }

    #[derive(Serialize, Deserialize, Validate, Default)]
    #[garde(allow_unvalidated)]
    struct Nowhere {
        x: bool,
    }

    impl SettingsGroup for Nowhere {
        const KEY: &'static str = "nowhere";
        const SCOPES: &'static [ScopeKind] = &[];
    }

    #[derive(Serialize, Deserialize, Validate, Default)]
    #[garde(allow_unvalidated)]
    struct BadKey {
        x: bool,
    }

    impl SettingsGroup for BadKey {
        const KEY: &'static str = "Bad Key";
    }

    #[test]
    fn the_same_type_twice_counts_once() {
        let registry = SettingsRegistry::new([
            SettingsDefinition::of::<Portal>(),
            SettingsDefinition::of::<Portal>(),
        ])
        .unwrap();
        assert_eq!(registry.len(), 1);
        let portal = registry.get("portal").unwrap();
        assert!(portal.has_field("show_leaderboard"));
        assert!(!portal.has_field("colour"));
        assert!(registry.of(&SettingsDefinition::of::<Portal>()).is_ok());
    }

    #[test]
    fn two_types_with_one_key_are_named() {
        let error = SettingsRegistry::new([
            SettingsDefinition::of::<Portal>(),
            SettingsDefinition::of::<other::Portal>(),
        ])
        .unwrap_err();
        assert!(
            error.contains("registry::tests::Portal") && error.contains("other::Portal"),
            "{error}"
        );
    }

    #[test]
    fn invalid_definitions_are_refused() {
        let error = SettingsRegistry::new([SettingsDefinition::of::<Nowhere>()]).unwrap_err();
        assert!(error.contains("allow no level"), "{error}");
        let error = SettingsRegistry::new([SettingsDefinition::of::<BadKey>()]).unwrap_err();
        assert!(error.contains("BadKey"), "{error}");
    }

    #[test]
    fn unregistered_types_and_unknown_keys_are_errors() {
        let registry = SettingsRegistry::new([SettingsDefinition::of::<Portal>()]).unwrap();
        assert!(matches!(
            registry.of(&SettingsDefinition::of::<other::Portal>()),
            Err(SettingsError::Unregistered { .. })
        ));
        assert!(matches!(
            registry.by_key("nope"),
            Err(SettingsError::UnknownKey(key)) if key == "nope"
        ));

        // An open registry (the fake's) registers on first use, still one
        // type per key.
        let open = SettingsRegistry::open();
        assert!(open.of(&SettingsDefinition::of::<Portal>()).is_ok());
        assert!(open.contains("portal"));
        assert!(open.of(&SettingsDefinition::of::<other::Portal>()).is_err());
    }
}
