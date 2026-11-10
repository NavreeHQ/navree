//! Test migrations, the `notes` record and database inspection helpers.

#![allow(dead_code)]

use navree_db::{Db, ExecutorExt, Statement};
use navree_kernel::BoxFuture;
use navree_schema::{Dialect, NamedMigration, Schema, SchemaError, SchemaResult};

/// A migration creating `table` (`id` uuid primary key, `title` unique), with
/// an optional `owner_id` foreign key to `references(id)`.
pub struct CreateTable {
    pub name: &'static str,
    pub table: &'static str,
    pub references: Option<&'static str>,
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
    ) -> Box<dyn NamedMigration> {
        Box::new(Self {
            name,
            table,
            references: Some(references),
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
                    t.uuid("id").primary();
                    t.string("title").unique();
                    if let Some(parent) = self.references {
                        t.uuid("owner_id").nullable();
                        t.foreign("owner_id").references("id").on(parent);
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

/// A migration that creates `table`, then fails.
pub struct FailAfterCreating {
    pub name: &'static str,
    pub table: &'static str,
    pub transactional: bool,
}

impl NamedMigration for FailAfterCreating {
    fn name(&self) -> &str {
        self.name
    }

    fn up<'a>(&'a self, schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult> {
        Box::pin(async move {
            schema
                .create(self.table, |t| {
                    t.integer("id").primary();
                })
                .await?;
            Err(SchemaError::Definition {
                table: self.table.to_owned(),
                message: "boom".to_owned(),
            })
        })
    }

    fn down<'a>(&'a self, _schema: &'a Schema<'_>) -> BoxFuture<'a, SchemaResult> {
        Box::pin(async { Ok(()) })
    }

    fn transactional(&self, _dialect: Dialect) -> bool {
        self.transactional
    }
}

/// The user tables of the database, sorted.
pub async fn tables(db: &Db) -> Vec<String> {
    db.pool()
        .fetch_column::<String>(&Statement::text(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        ))
        .await
        .expect("list tables")
}

/// `SELECT count(*) FROM table`.
pub async fn count(db: &Db, table: &str) -> i64 {
    db.pool()
        .fetch_scalar::<i64>(&Statement::text(format!(
            "SELECT count(*) AS n FROM \"{table}\""
        )))
        .await
        .expect("count")
}

navree_db::record! {
    /// A note: uuid id, unique title, body.
    #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
    pub struct Note in "notes" {
        pub id: uuid::Uuid,
        pub title: String,
        pub body: String,
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
                    t.uuid("id").primary();
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

/// Creates the `notes` and `audit` tables on `db`.
pub async fn create_notes(db: &Db) {
    CreateNotes
        .up(&Schema::new(db.pool()))
        .await
        .expect("creates tables");
}

/// A connected database with the `notes` and `audit` tables.
pub async fn notes_db(temp: &crate::support::TempDb) -> Db {
    let db = Db::connect(&temp.options()).await.expect("connects");
    create_notes(&db).await;
    db
}

/// A new note.
pub fn new_note(title: &str) -> Note {
    Note {
        id: uuid::Uuid::now_v7(),
        title: title.to_owned(),
        body: format!("body of {title}"),
    }
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
