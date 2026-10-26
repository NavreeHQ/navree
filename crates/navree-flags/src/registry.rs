//! The flags the app's modules registered with
//! `FlagsModule::for_feature::<F>()`.

use std::collections::BTreeMap;

use crate::{FlagDefinition, check_name};

/// A `FlagsModule::for_feature::<F>()` registration (a contribution).
#[derive(Clone, Copy, Debug)]
pub(crate) struct FlagRegistration(pub(crate) FlagDefinition);

/// Every registered flag, by name.
#[derive(Clone, Debug, Default)]
pub struct FlagRegistry {
    flags: BTreeMap<&'static str, FlagDefinition>,
}

impl FlagRegistry {
    /// The registry of `definitions`. The same type twice counts once; two
    /// types with one name, or an invalid name, is an error naming them.
    pub fn new(definitions: impl IntoIterator<Item = FlagDefinition>) -> Result<Self, String> {
        let mut flags: BTreeMap<&'static str, FlagDefinition> = BTreeMap::new();
        for definition in definitions {
            check_name(definition.name)
                .map_err(|error| format!("{error} ({})", definition.type_name))?;
            match flags.get(definition.name) {
                Some(first) if first.type_name != definition.type_name => {
                    return Err(format!(
                        "two flags are named `{}`: {} and {}; give one of them another \
                         `#[flag(name = \"..\")]`",
                        definition.name, first.type_name, definition.type_name
                    ));
                }
                Some(_) => {}
                None => {
                    flags.insert(definition.name, definition);
                }
            }
        }
        Ok(FlagRegistry { flags })
    }

    /// The flag named `name`, if registered.
    pub fn get(&self, name: &str) -> Option<&FlagDefinition> {
        self.flags.get(name)
    }

    /// Whether a flag named `name` is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }

    /// Every registered flag, by name.
    pub fn iter(&self) -> impl Iterator<Item = &FlagDefinition> {
        self.flags.values()
    }

    /// How many flags are registered.
    pub fn len(&self) -> usize {
        self.flags.len()
    }

    /// Whether no flag is registered.
    pub fn is_empty(&self) -> bool {
        self.flags.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition(name: &'static str, type_name: &'static str) -> FlagDefinition {
        FlagDefinition {
            name,
            default: false,
            expose: false,
            description: "",
            type_name,
        }
    }

    #[test]
    fn the_same_type_twice_counts_once() {
        let registry = FlagRegistry::new([
            definition("new-pipeline", "app::NewPipeline"),
            definition("deal-export", "app::DealExport"),
            definition("new-pipeline", "app::NewPipeline"),
        ])
        .unwrap();
        assert_eq!(registry.len(), 2);
        assert!(registry.contains("new-pipeline"));
        let names: Vec<&str> = registry.iter().map(|d| d.name).collect();
        assert_eq!(names, ["deal-export", "new-pipeline"]);
    }

    #[test]
    fn two_types_with_one_name_are_named() {
        let error = FlagRegistry::new([
            definition("new-pipeline", "app::deals::NewPipeline"),
            definition("new-pipeline", "app::board::NewPipeline"),
        ])
        .unwrap_err();
        assert!(
            error.contains("app::deals::NewPipeline") && error.contains("app::board::NewPipeline"),
            "{error}"
        );
    }

    #[test]
    fn invalid_names_are_refused() {
        let error = FlagRegistry::new([definition("New Pipeline", "app::NewPipeline")]).unwrap_err();
        assert!(error.contains("app::NewPipeline"), "{error}");
    }
}
