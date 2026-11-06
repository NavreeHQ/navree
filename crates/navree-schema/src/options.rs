//! [`SchemaOptions`]: the `schema` configuration section.

use navree_config::ConfigSection;
use serde::{Deserialize, Serialize};

/// The key type of polymorphic columns made by `morphs` and
/// `nullable_morphs` (`{name}_type` plus `{name}_id`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MorphKey {
    /// `{name}_id` is an unsigned big integer (like [`foreign_id`](crate::Blueprint::foreign_id)).
    #[default]
    Int,
    /// `{name}_id` is a UUID.
    Uuid,
    /// `{name}_id` is a ULID (`CHAR(26)`).
    Ulid,
}

/// When the schema builder uses TimescaleDB.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimescaleMode {
    /// Use TimescaleDB when the database has it (installing the extension
    /// when it is available and allowed); otherwise fall back to regular
    /// tables with a notice.
    #[default]
    Auto,
    /// Fail when a hypertable is declared and TimescaleDB is not usable.
    Require,
    /// Never use TimescaleDB: hypertables become regular tables.
    Off,
}

/// What to do with a hypertable's primary key when it lacks the time (or
/// space-partition) column, which TimescaleDB requires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyStrategy {
    /// Widen it: `(id)` becomes `(id, occurred_at)`.
    #[default]
    Extend,
    /// Stop with an error that explains the fix.
    Reject,
}

/// Defaults of the schema builder: configuration section `schema`.
///
/// ```toml
/// [schema]
/// default_string_length = 255   # `string` columns without `.length(n)`
/// default_time_precision = 6    # fractional seconds of time columns; omit for the database default
/// morph_key = "int"             # int, uuid or ulid: the `_id` column of `morphs`
/// timescaledb = "auto"          # auto, require or off
/// create_extensions = true      # may run CREATE EXTENSION timescaledb
/// hypertable_keys = "extend"    # extend or reject primary keys missing the time column
/// mysql_charset = "utf8mb4"     # MySQL tables' default character set
/// # mysql_collation = "utf8mb4_0900_ai_ci"
/// # mysql_engine = "InnoDB"
/// dump_path = "database/schema" # where `schema:dump` writes {driver}-schema.sql
/// ```
///
/// ```
/// use navree_schema::{MorphKey, SchemaOptions, TimescaleMode};
///
/// let options = SchemaOptions::default();
/// assert_eq!(options.default_string_length, 255);
/// assert_eq!(options.default_time_precision, Some(6));
/// assert_eq!(options.morph_key, MorphKey::Int);
/// assert_eq!(options.timescaledb, TimescaleMode::Auto);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigSection)]
#[config(key = "schema", docs = "/docs/database/schema-builder/", validate = Self::check)]
#[serde(default)]
pub struct SchemaOptions {
    /// The length of `string` columns without an explicit `.length(n)`.
    pub default_string_length: u32,
    /// The fractional-second precision of `timestamp`, `date_time` and
    /// `time` columns without `.precision(p)`; `None` leaves each
    /// database's default (MySQL's is whole seconds).
    pub default_time_precision: Option<u8>,
    /// The key type of `morphs` and `nullable_morphs`.
    #[config(one_of = "int, uuid, ulid")]
    pub morph_key: MorphKey,
    /// When to use TimescaleDB.
    #[config(one_of = "auto, require, off")]
    pub timescaledb: TimescaleMode,
    /// Whether the builder may run `CREATE EXTENSION IF NOT EXISTS
    /// timescaledb` when the extension is available but not installed.
    pub create_extensions: bool,
    /// What to do with a hypertable's primary key that lacks the time
    /// column.
    #[config(one_of = "extend, reject")]
    pub hypertable_keys: KeyStrategy,
    /// The default character set of MySQL tables (`utf8mb4`: every Unicode
    /// character, emoji included); `None` leaves the server's default.
    pub mysql_charset: Option<String>,
    /// The default collation of MySQL tables; `None` leaves the server's
    /// default for the character set.
    pub mysql_collation: Option<String>,
    /// The default storage engine of MySQL tables; `None` leaves the
    /// server's default (InnoDB).
    pub mysql_engine: Option<String>,
    /// The directory (relative to the working directory) where `navree
    /// schema:dump` writes `{driver}-schema.sql`, and where `navree
    /// migrate` looks for it when the database is empty.
    pub dump_path: String,
}

impl SchemaOptions {
    /// The schema dump file for `dialect`: `{dump_path}/{driver}-schema.sql`.
    ///
    /// ```
    /// use navree_schema::{Dialect, SchemaOptions};
    ///
    /// assert_eq!(
    ///     SchemaOptions::default().dump_file(Dialect::Postgres),
    ///     std::path::Path::new("database/schema").join("postgres-schema.sql")
    /// );
    /// ```
    pub fn dump_file(&self, dialect: crate::Dialect) -> std::path::PathBuf {
        std::path::Path::new(&self.dump_path).join(format!("{}-schema.sql", dialect.driver()))
    }
}

impl Default for SchemaOptions {
    fn default() -> SchemaOptions {
        SchemaOptions {
            default_string_length: 255,
            default_time_precision: Some(6),
            morph_key: MorphKey::Int,
            timescaledb: TimescaleMode::Auto,
            create_extensions: true,
            hypertable_keys: KeyStrategy::Extend,
            mysql_charset: Some("utf8mb4".to_owned()),
            mysql_collation: None,
            mysql_engine: None,
            dump_path: "database/schema".to_owned(),
        }
    }
}

impl SchemaOptions {
    /// Checks the values (the boot calls this).
    fn check(&self) -> Result<(), String> {
        let mut problems = Vec::new();
        if self.default_string_length == 0 {
            problems.push("default_string_length must be at least 1".to_owned());
        }
        if self.default_time_precision.is_some_and(|p| p > 6) {
            problems.push("default_time_precision must be between 0 and 6".to_owned());
        }
        if self.dump_path.trim().is_empty() {
            problems.push("dump_path must name a directory".to_owned());
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("; "))
        }
    }
}

#[cfg(test)]
mod tests {
    use navree_config::FieldType;

    use super::*;

    #[test]
    fn the_section_describes_itself() {
        let schema = SchemaOptions::schema().expect("schema");
        assert_eq!(schema.key, "schema");
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{name}"))
        };
        assert_eq!(
            field("morph_key").ty,
            FieldType::choice(&["int", "uuid", "ulid"])
        );
        assert_eq!(
            field("timescaledb").ty,
            FieldType::choice(&["auto", "require", "off"])
        );
        assert_eq!(
            field("hypertable_keys").ty,
            FieldType::choice(&["extend", "reject"])
        );
        assert_eq!(field("default_string_length").ty, FieldType::Integer);
        let defaults = SchemaOptions::defaults().expect("defaults");
        assert_eq!(defaults["default_string_length"], 255);
        assert_eq!(defaults["morph_key"], "int");
        assert_eq!(defaults["dump_path"], "database/schema");
    }

    #[test]
    fn validation_reports_bad_values() {
        let options = SchemaOptions {
            default_string_length: 0,
            default_time_precision: Some(9),
            ..SchemaOptions::default()
        };
        let problems = options.validate().unwrap_err();
        assert!(problems.contains("default_string_length"), "{problems}");
        assert!(problems.contains("default_time_precision"), "{problems}");
        assert!(SchemaOptions::default().validate().is_ok());
    }

    #[test]
    fn deserializes_lowercase_choices() {
        let options: SchemaOptions = serde_json::from_value(serde_json::json!({
            "morph_key": "uuid",
            "timescaledb": "require",
            "hypertable_keys": "reject",
            "default_time_precision": null,
        }))
        .unwrap();
        assert_eq!(options.morph_key, MorphKey::Uuid);
        assert_eq!(options.timescaledb, TimescaleMode::Require);
        assert_eq!(options.hypertable_keys, KeyStrategy::Reject);
        assert_eq!(options.default_time_precision, None);
        assert_eq!(options.default_string_length, 255);
    }
}
