//! The module's migrations (`MigrationSet` `i18n`) and its table.
//!
//! Written with the schema builder, with the name and the column types the
//! module's migration always had, so a database migrated by an earlier
//! version finds nothing pending: the scope id is `BINARY(16)` on MySQL and
//! MariaDB (where navree-db writes UUIDs as 16 bytes) and a native `uuid`
//! elsewhere.

use navree_db::MigrationSet;
use navree_db::navree_schema::{Dialect, Migration, Schema, SchemaMigration, SchemaResult};

/// The name of the localization module's migration set.
pub const I18N_MIGRATIONS: &str = "i18n";

/// The localization module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(I18N_MIGRATIONS).migration(SchemaMigration::new(
        "m20261027_000001_create_locale_preferences",
        CreateLocalePreferences,
    ))
}

/// Creates `locale_preferences`.
struct CreateLocalePreferences;

impl Migration for CreateLocalePreferences {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create("locale_preferences", |t| {
                t.string("scope_type").length(16);
                if mysql {
                    t.binary("scope_id").length(16).fixed();
                } else {
                    t.uuid("scope_id");
                }
                t.string("locale").length(35);
                t.timestamp_tz("updated_at");
                t.primary(["scope_type", "scope_id"]);
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("locale_preferences").await
    }
}

navree_db::record! {
    /// `locale_preferences(scope_type, scope_id, locale, updated_at)`,
    /// primary key `(scope_type, scope_id)`: the saved locale of a user
    /// (`user`) or a workspace (`workspace`). Ids are not foreign keys, so
    /// the table works with any user and workspace store. Write it through
    /// [`I18n`](crate::I18n).
    #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    pub struct LocalePreferenceRow in "locale_preferences" key "scope_id" {
        /// `user` or `workspace`.
        pub scope_type: String,
        /// The user's or workspace's id.
        pub scope_id: uuid::Uuid,
        /// The locale's tag (`fr`, `pt-BR`).
        pub locale: String,
        /// When it was saved.
        pub updated_at: chrono::DateTime<chrono::Utc>,
    }
}
