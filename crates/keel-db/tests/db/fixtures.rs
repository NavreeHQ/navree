//! Test migrations and database inspection helpers.

#![allow(dead_code)]

use keel_db::Db;
use sea_orm::sea_query::{ColumnDef, ForeignKey, Table};
use sea_orm::{ConnectionTrait, DbErr, Statement};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// A migration creating `table` (`id` uuid primary key, `title` unique), with
/// an optional `owner_id` foreign key to `references(id)`.
pub struct CreateTable {
    pub name: &'static str,
    pub table: &'static str,
    pub references: Option<&'static str>,
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
    ) -> Box<dyn MigrationTrait> {
        Box::new(Self {
            name,
            table,
            references: Some(references),
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
        if let Some(parent) = self.references {
            table
                .col(ColumnDef::new("owner_id").uuid().null())
                .foreign_key(
                    ForeignKey::create()
                        .from(self.table, "owner_id")
                        .to(parent, "id"),
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

/// A migration that creates `table`, then fails.
pub struct FailAfterCreating {
    pub name: &'static str,
    pub table: &'static str,
    pub transactional: bool,
}

impl MigrationName for FailAfterCreating {
    fn name(&self) -> &str {
        self.name
    }
}

#[async_trait]
impl MigrationTrait for FailAfterCreating {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(self.table)
                    .col(ColumnDef::new("id").integer().not_null().primary_key())
                    .to_owned(),
            )
            .await?;
        Err(DbErr::Migration("boom".into()))
    }

    fn use_transaction(&self) -> Option<bool> {
        Some(self.transactional)
    }
}

/// The user tables of the database, sorted.
pub async fn tables(db: &Db) -> Vec<String> {
    let rows = db
        .conn()
        .query_all_raw(Statement::from_string(
            db.backend(),
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        ))
        .await
        .expect("list tables");
    rows.iter()
        .map(|row| row.try_get::<String>("", "name").expect("name"))
        .collect()
}

/// `SELECT count(*) FROM table`.
pub async fn count(db: &Db, table: &str) -> i64 {
    let row = db
        .conn()
        .query_one_raw(Statement::from_string(
            db.backend(),
            format!("SELECT count(*) AS n FROM \"{table}\""),
        ))
        .await
        .expect("count query")
        .expect("count row");
    row.try_get::<i64>("", "n").expect("count")
}

/// The `notes` entity: uuid id, unique title, body.
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

/// A connected database with the `notes` and `audit` tables.
pub async fn notes_db(temp: &crate::support::TempDb) -> Db {
    let db = Db::connect(&temp.options()).await.expect("connects");
    CreateNotes
        .up(&SchemaManager::new(db.conn()))
        .await
        .expect("creates tables");
    db
}

/// A new note active model.
pub fn new_note(title: &str) -> note::ActiveModel {
    use sea_orm::ActiveValue::Set;
    note::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        title: Set(title.to_owned()),
        body: Set(format!("body of {title}")),
    }
}
