//! The `notes` record and the migrations the MySQL suite runs.

#![allow(dead_code)]

use navree_db::Db;
use navree_kernel::BoxFuture;
use navree_schema::{Dialect, NamedMigration, ReferentialAction, Schema, SchemaResult};
use uuid::Uuid;

navree_db::record! {
    /// A note: UUID id (`BINARY(16)` on MySQL), unique title, body.
    #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
    pub struct Note in "notes" {
        pub id: Uuid,
        pub title: String,
        pub body: String,
    }
}

/// A new note.
pub fn new_note(title: &str) -> Note {
    Note {
        id: Uuid::now_v7(),
        title: title.to_owned(),
        body: format!("body of {title}"),
    }
}

/// Creates `notes` and `audit` (written by test change observers).
pub struct CreateNotes;

impl NamedMigration for CreateNotes {
    fn name(&self) -> &str {
        "m0001_create_notes"
    }

    fn up<'a>(&'a self, schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult> {
        Box::pin(async move {
            schema
                .create("notes", |t| {
                    t.binary("id").length(16).fixed().primary();
                    t.string("title").unique();
                    t.string("body");
                })
                .await?;
            schema
                .create("audit", |t| {
                    t.increments("id");
                    t.string("entity");
                    t.string("entity_id");
                    t.string("op");
                    t.string("actor").nullable();
                })
                .await
        })
    }

    fn down<'a>(&'a self, schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult> {
        Box::pin(async move {
            schema.drop("audit").await?;
            schema.drop("notes").await
        })
    }

    fn transactional(&self, dialect: Dialect) -> bool {
        dialect != Dialect::MySql
    }
}

/// Creates `table` (`id` UUID primary key, unique `title`), with a
/// nullable `owner_id` referencing `references(id)` when given.
pub struct CreateTable {
    pub name: &'static str,
    pub table: &'static str,
    pub references: Option<(&'static str, ReferentialAction)>,
}

impl CreateTable {
    pub fn boxed(name: &'static str, table: &'static str) -> Box<dyn NamedMigration> {
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
        on_delete: ReferentialAction,
    ) -> Box<dyn NamedMigration> {
        Box::new(Self {
            name,
            table,
            references: Some((references, on_delete)),
        })
    }
}

impl NamedMigration for CreateTable {
    fn name(&self) -> &str {
        self.name
    }

    fn up<'a>(&'a self, schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult> {
        Box::pin(async move {
            schema
                .create(self.table, |t| {
                    t.binary("id").length(16).fixed().primary();
                    t.string("title").unique();
                    if let Some((parent, on_delete)) = self.references {
                        t.binary("owner_id").length(16).fixed().nullable();
                        t.foreign("owner_id")
                            .references("id")
                            .on(parent)
                            .on_delete(on_delete);
                    }
                })
                .await
        })
    }

    fn down<'a>(&'a self, schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult> {
        Box::pin(async move { schema.drop(self.table).await })
    }

    fn transactional(&self, dialect: Dialect) -> bool {
        dialect != Dialect::MySql
    }
}

/// Creates `notes` and `audit` on `db` directly (no migration record).
pub async fn create_notes(db: &Db) {
    CreateNotes
        .up(&Schema::new(db.pool()))
        .await
        .expect("creates the notes and audit tables");
}

/// A migration set of boxed migrations (as the fixtures build them).
pub fn set(
    module: &'static str,
    after: impl IntoIterator<Item = &'static str>,
    migrations: Vec<Box<dyn NamedMigration>>,
) -> navree_db::MigrationSet {
    migrations.into_iter().fold(
        navree_db::MigrationSet::new(module).after(after),
        |set, migration| set.migration(Boxed(migration)),
    )
}

/// A boxed migration as a migration of its own.
struct Boxed(Box<dyn NamedMigration>);

impl NamedMigration for Boxed {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn up<'a>(&'a self, schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult> {
        self.0.up(schema)
    }

    fn down<'a>(&'a self, schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult> {
        self.0.down(schema)
    }

    fn transactional(&self, dialect: Dialect) -> bool {
        self.0.transactional(dialect)
    }
}
