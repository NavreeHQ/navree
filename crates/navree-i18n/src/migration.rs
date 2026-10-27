//! The module's migrations (`MigrationSet` `i18n`) and its table.

use navree_db::MigrationSet;
use sea_orm::DbErr;
use sea_orm::sea_query::{ColumnDef, Index, Table};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the localization module's migration set.
pub const I18N_MIGRATIONS: &str = "i18n";

/// The localization module's migration set.
pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(I18N_MIGRATIONS).migration(CreateLocalePreferences)
}

/// Creates `locale_preferences`.
struct CreateLocalePreferences;

impl MigrationName for CreateLocalePreferences {
    fn name(&self) -> &str {
        "m20261027_000001_create_locale_preferences"
    }
}

#[async_trait]
impl MigrationTrait for CreateLocalePreferences {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("locale_preferences")
                    .col(ColumnDef::new("scope_type").string_len(16).not_null())
                    .col(ColumnDef::new("scope_id").uuid().not_null())
                    .col(ColumnDef::new("locale").string_len(35).not_null())
                    .col(
                        ColumnDef::new("updated_at")
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .primary_key(Index::create().col("scope_type").col("scope_id"))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("locale_preferences").to_owned())
            .await
    }
}

/// `locale_preferences(scope_type, scope_id, locale, updated_at)`, primary
/// key `(scope_type, scope_id)`: the saved locale of a user (`user`) or a
/// workspace (`workspace`). Ids are not foreign keys, so the table works
/// with any user and workspace store. Write it through
/// [`I18n`](crate::I18n).
pub mod locale_preference {
    use sea_orm::entity::prelude::*;
    use serde::{Deserialize, Serialize};

    /// One saved locale.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "locale_preferences")]
    pub struct Model {
        /// `user` or `workspace`.
        #[sea_orm(primary_key, auto_increment = false)]
        pub scope_type: String,
        /// The user's or workspace's id.
        #[sea_orm(primary_key, auto_increment = false)]
        pub scope_id: Uuid,
        /// The locale's tag (`fr`, `pt-BR`).
        pub locale: String,
        /// When it was saved.
        pub updated_at: DateTimeUtc,
    }

    /// `locale_preferences` has no relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
