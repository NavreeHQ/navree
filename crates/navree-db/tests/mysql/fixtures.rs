//! The `notes` entity and the migrations the MySQL suite runs.

#![allow(dead_code)]

use navree_db::Db;
use sea_orm::sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Table};
use sea_orm::{DbErr, Set};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use uuid::Uuid;

/// The `notes` entity: UUID id (`BINARY(16)` on MySQL), unique title,
/// body.
pub mod note {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "notes")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        #[sea_orm(unique)]
        pub title: String,
        pub body: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// A new note active model.
pub fn new_note(title: &str) -> note::ActiveModel {
    note::ActiveModel {
        id: Set(Uuid::now_v7()),
        title: Set(title.to_owned()),
        body: Set(format!("body of {title}")),
    }
}

/// Creates `notes` and `audit` (written by test change observers).
pub struct CreateNotes;

impl MigrationName for CreateNotes {
    fn name(&self) -> &str {
        "m0001_create_notes"
    }
}

#[async_trait]
impl MigrationTrait for CreateNotes {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("notes")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("title").string().not_null().unique_key())
                    .col(ColumnDef::new("body").string().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("audit")
                    .col(
                        ColumnDef::new("id")
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new("entity").string().not_null())
                    .col(ColumnDef::new("entity_id").string().not_null())
                    .col(ColumnDef::new("op").string().not_null())
                    .col(ColumnDef::new("actor").string().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("audit").to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table("notes").to_owned())
            .await
    }
}

/// Creates `table` (`id` UUID primary key, unique `title`), with a
/// nullable `owner_id` referencing `references(id)` when given.
pub struct CreateTable {
    pub name: &'static str,
    pub table: &'static str,
    pub references: Option<(&'static str, ForeignKeyAction)>,
}

impl CreateTable {
    pub fn boxed(name: &'static str, table: &'static str) -> Box<dyn MigrationTrait> {
        Box::new(Self {
            name,
            table,
            references: None,
        })
    }

    pub fn referencing(
        name: &'static str,
        table: &'static str,
        references: &'static str,
        on_delete: ForeignKeyAction,
    ) -> Box<dyn MigrationTrait> {
        Box::new(Self {
            name,
            table,
            references: Some((references, on_delete)),
        })
    }
}

impl MigrationName for CreateTable {
    fn name(&self) -> &str {
        self.name
    }
}

#[async_trait]
impl MigrationTrait for CreateTable {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let mut table = Table::create();
        table
            .table(self.table)
            .col(ColumnDef::new("id").uuid().not_null().primary_key())
            .col(ColumnDef::new("title").string().not_null().unique_key());
        if let Some((parent, on_delete)) = self.references {
            table
                .col(ColumnDef::new("owner_id").uuid().null())
                .foreign_key(
                    ForeignKey::create()
                        .from(self.table, "owner_id")
                        .to(parent, "id")
                        .on_delete(on_delete),
                );
        }
        manager.create_table(table).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(self.table).to_owned())
            .await
    }
}

/// Creates `notes` and `audit` on `db` directly (no migration record).
pub async fn create_notes(db: &Db) {
    CreateNotes
        .up(&SchemaManager::new(db.conn()))
        .await
        .expect("creates the notes and audit tables");
}

/// A migration set of SeaORM migrations (boxed, as the fixtures build them).
pub fn set(
    module: &'static str,
    after: impl IntoIterator<Item = &'static str>,
    migrations: Vec<Box<dyn MigrationTrait>>,
) -> navree_db::MigrationSet {
    migrations.into_iter().fold(
        navree_db::MigrationSet::new(module).after(after),
        |set, migration| set.migration(Boxed(migration)),
    )
}

/// A boxed SeaORM migration as a migration of its own.
struct Boxed(Box<dyn MigrationTrait>);

impl MigrationName for Boxed {
    fn name(&self) -> &str {
        self.0.name()
    }
}

#[async_trait]
impl MigrationTrait for Boxed {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        self.0.up(manager).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        self.0.down(manager).await
    }

    fn use_transaction(&self) -> Option<bool> {
        self.0.use_transaction()
    }
}
