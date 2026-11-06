//! What the database says about a workspace table: its columns, which
//! columns lead an index, and (PostgreSQL) whether row-level security is on.
//!
//! Read from the catalogs with statements: these are questions about the
//! database, not rows of a model, and the row-level security flags are not
//! part of what the schema builder's introspection reports.

use navree_db::{Backend, Db, DbError, Executor, Row, Statement, Value};

/// The facts about one table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TableFacts {
    /// Whether the table exists.
    pub(crate) exists: bool,
    /// Its columns.
    pub(crate) columns: Vec<String>,
    /// The first column of each of its indexes (primary key included).
    pub(crate) index_leads: Vec<String>,
    /// PostgreSQL: row-level security enabled and forced; `None` elsewhere.
    pub(crate) rls: Option<(bool, bool)>,
}

/// Reads the facts about `table`, or `None` on a database this check does
/// not read (anything but SQLite and PostgreSQL).
pub(crate) async fn table_facts(db: &Db, table: &str) -> Result<Option<TableFacts>, DbError> {
    match Executor::backend(db) {
        Backend::Sqlite => sqlite(db, table).await.map(Some),
        Backend::Postgres => postgres(db, table).await.map(Some),
        Backend::MySql => Ok(None),
    }
}

async fn rows(db: &Db, statement: Statement) -> Result<Vec<Row>, DbError> {
    db.fetch_all(&statement).await
}

/// `"name"` with embedded quotes doubled, for PRAGMA arguments.
fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

async fn sqlite(db: &Db, table: &str) -> Result<TableFacts, DbError> {
    let pragma = Statement::text;
    let columns: Vec<String> = rows(db, pragma(format!("PRAGMA table_info({})", quoted(table))))
        .await?
        .iter()
        .map(|row| row.get::<String>("name"))
        .collect::<Result<_, _>>()?;
    if columns.is_empty() {
        return Ok(TableFacts::default());
    }
    let mut index_leads = Vec::new();
    let indexes = rows(db, pragma(format!("PRAGMA index_list({})", quoted(table)))).await?;
    for index in indexes {
        let name: String = index.get("name")?;
        let info = rows(db, pragma(format!("PRAGMA index_info({})", quoted(&name)))).await?;
        let lead = info
            .iter()
            .map(|row| Ok::<_, DbError>((row.get::<i64>("seqno")?, row.get::<String>("name")?)))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .min_by_key(|(seqno, _)| *seqno)
            .map(|(_, column)| column);
        index_leads.extend(lead);
    }
    Ok(TableFacts {
        exists: true,
        columns,
        index_leads,
        rls: None,
    })
}

async fn postgres(db: &Db, table: &str) -> Result<TableFacts, DbError> {
    let statement = |sql: &str| Statement::new(Backend::Postgres, sql, [Value::from(table)]);
    let columns: Vec<String> = rows(
        db,
        statement(
            "SELECT column_name::text AS name FROM information_schema.columns \
             WHERE table_schema = current_schema() AND table_name = $1 ORDER BY ordinal_position",
        ),
    )
    .await?
    .iter()
    .map(|row| row.get::<String>("name"))
    .collect::<Result<_, _>>()?;
    if columns.is_empty() {
        return Ok(TableFacts::default());
    }
    let index_leads = rows(
        db,
        statement(
            "SELECT a.attname::text AS name FROM pg_index i \
             JOIN pg_class c ON c.oid = i.indrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = i.indkey[0] \
             WHERE c.relname = $1 AND n.nspname = current_schema()",
        ),
    )
    .await?
    .iter()
    .map(|row| row.get::<String>("name"))
    .collect::<Result<_, _>>()?;
    let rls = rows(
        db,
        statement(
            "SELECT c.relrowsecurity AS enabled, c.relforcerowsecurity AS forced FROM pg_class c \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE c.relname = $1 AND n.nspname = current_schema()",
        ),
    )
    .await?
    .first()
    .map(|row| Ok::<_, DbError>((row.get::<bool>("enabled")?, row.get::<bool>("forced")?)))
    .transpose()?;
    Ok(TableFacts {
        exists: true,
        columns,
        index_leads,
        rls,
    })
}
