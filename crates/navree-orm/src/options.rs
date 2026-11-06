//! [`OrmOptions`]: the `[orm]` configuration section.

use navree_config::{ConfigSection, Environment};
use serde::{Deserialize, Serialize};

/// The default number of records `model:prune` handles per chunk.
pub const DEFAULT_PRUNE_CHUNK: u64 = 1000;

/// The ORM's options, the `[orm]` section of the configuration.
///
/// ```toml
/// [orm]
/// strict = true                          # every strict mode below at once
/// prevent_missing_attributes = true      # a column a partial select left out is an error
/// prevent_discarding_attributes = true   # filling a column that is not fillable is an error
/// prevent_lazy_loading = true            # loading a relation per record of a result is an error
/// auto_eager_load = false                # a lazy load loads the relation for the whole result
/// prune_chunk = 1000                     # records `model:prune` handles per chunk
/// ```
///
/// `strict` turns the three `prevent_*` modes on or off together; each one
/// set on its own wins over it. Unset, `strict` is on, except when the
/// app's environment (top-level `env`, `APP_ENV`, or `app.env`) is
/// `production`: development and tests are strict, production lenient.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ConfigSection)]
#[config(key = "orm", docs = "/docs/orm/getting-started/", validate = Self::check)]
#[serde(default)]
pub struct OrmOptions {
    /// Every strict mode at once; unset: on unless the environment is
    /// `production`.
    pub strict: Option<bool>,
    /// A column a partial select left out is an error
    /// ([`OrmError::MissingAttribute`](crate::OrmError::MissingAttribute))
    /// instead of the field's default.
    pub prevent_missing_attributes: Option<bool>,
    /// Filling a column that is not fillable is an error
    /// ([`OrmError::MassAssignment`](crate::OrmError::MassAssignment))
    /// instead of silently skipping it.
    pub prevent_discarding_attributes: Option<bool>,
    /// Loading a relation lazily for a record that came with others is an
    /// error ([`OrmError::LazyLoading`](crate::OrmError::LazyLoading)).
    pub prevent_lazy_loading: Option<bool>,
    /// A lazy load on one record loads the relation for every record of
    /// the result it came from.
    pub auto_eager_load: bool,
    /// How many records `model:prune` handles per chunk (unless `--chunk`
    /// or the model says otherwise).
    pub prune_chunk: u64,
}

impl Default for OrmOptions {
    fn default() -> Self {
        OrmOptions {
            strict: None,
            prevent_missing_attributes: None,
            prevent_discarding_attributes: None,
            prevent_lazy_loading: None,
            auto_eager_load: false,
            prune_chunk: DEFAULT_PRUNE_CHUNK,
        }
    }
}

impl OrmOptions {
    /// Every strict mode on (development).
    pub fn strict() -> Self {
        OrmOptions {
            strict: Some(true),
            ..OrmOptions::default()
        }
    }

    /// Every strict mode off (production).
    pub fn lenient() -> Self {
        OrmOptions {
            strict: Some(false),
            ..OrmOptions::default()
        }
    }

    /// Whether `strict` is on in environment `env` (`None` when the app
    /// names none): its own value, else on unless `env` is `production`.
    pub fn is_strict_in(&self, env: Option<&str>) -> bool {
        self.strict.unwrap_or_else(|| {
            !env.is_some_and(|env| Environment::parse(env).behaves_like_production())
        })
    }
}

impl OrmOptions {
    /// Checks the values (the boot calls this).
    fn check(&self) -> Result<(), String> {
        if self.prune_chunk == 0 {
            return Err("prune_chunk must be at least 1".to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_section_describes_itself() {
        let schema = OrmOptions::schema().expect("schema");
        assert_eq!(schema.key, "orm");
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        for name in [
            "strict",
            "prevent_missing_attributes",
            "prevent_discarding_attributes",
            "prevent_lazy_loading",
            "auto_eager_load",
            "prune_chunk",
        ] {
            assert!(schema.fields.iter().any(|f| f.name == name), "{name}");
        }
        let defaults = OrmOptions::defaults().expect("defaults");
        assert_eq!(defaults["prune_chunk"], DEFAULT_PRUNE_CHUNK);
        assert!(defaults["strict"].is_null());
    }

    #[test]
    fn strict_by_default_except_in_production() {
        let options = OrmOptions::default();
        assert!(options.is_strict_in(None));
        assert!(options.is_strict_in(Some("local")));
        assert!(options.is_strict_in(Some("testing")));
        assert!(!options.is_strict_in(Some("production")));
        assert!(!options.is_strict_in(Some("Production")));
        assert!(OrmOptions::strict().is_strict_in(Some("production")));
        assert!(!OrmOptions::lenient().is_strict_in(None));
    }

    #[test]
    fn the_section_reads_partial_tables() {
        let options: OrmOptions =
            serde_json::from_value(serde_json::json!({"strict": false, "prune_chunk": 50}))
                .unwrap();
        assert_eq!(options.strict, Some(false));
        assert_eq!(options.prune_chunk, 50);
        assert_eq!(options.prevent_lazy_loading, None);
        assert_eq!(
            OrmOptions {
                prune_chunk: 0,
                ..OrmOptions::default()
            }
            .validate()
            .unwrap_err(),
            "prune_chunk must be at least 1"
        );
    }
}
