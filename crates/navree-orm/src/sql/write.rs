//! `INSERT`, `UPDATE` and `DELETE`.

use sea_query::{Alias, ReturningClause};

use super::{Compiler, Dialect, Statement, build};
use crate::error::OrmError;
use crate::ir::{DeleteIr, InsertIr, UpdateIr};

/// The SQL of an insert. `returning` columns come back on PostgreSQL and
/// SQLite (`RETURNING`); on MySQL the caller reads `last_insert_id`.
///
/// ```
/// use navree_orm::Value;
/// use navree_orm::ir::{Expr, InsertIr};
/// use navree_orm::sql::{Dialect, compile_insert};
///
/// let insert = InsertIr {
///     table: "deals".into(),
///     columns: vec!["title".into()],
///     rows: vec![vec![Expr::Value(Value::from("Renewal"))]],
///     returning: vec!["id".into()],
/// };
/// let sql = compile_insert(&insert, Dialect::Postgres).unwrap();
/// assert_eq!(sql.sql, r#"INSERT INTO "deals" ("title") VALUES ($1) RETURNING "id""#);
/// ```
pub fn compile_insert(insert: &InsertIr, dialect: Dialect) -> Result<Statement, OrmError> {
    let compiler = Compiler::new(dialect);
    let mut statement = sea_query::Query::insert();
    statement.into_table(Alias::new(insert.table.to_string()));
    if insert.columns.is_empty() {
        statement.or_default_values();
    } else {
        statement.columns(
            insert
                .columns
                .iter()
                .map(|column| Alias::new(column.to_string())),
        );
        for row in &insert.rows {
            if row.len() != insert.columns.len() {
                return Err(OrmError::invalid(format!(
                    "an insert into `{}` has {} columns but a row of {} values",
                    insert.table,
                    insert.columns.len(),
                    row.len()
                )));
            }
            let values = row
                .iter()
                .map(|value| compiler.expr(value))
                .collect::<Result<Vec<_>, _>>()?;
            statement
                .values(values)
                .map_err(|error| OrmError::invalid(error.to_string()))?;
        }
    }
    if !insert.returning.is_empty() && dialect.supports_returning() {
        statement.returning(ReturningClause::Columns(
            insert
                .returning
                .iter()
                .map(|column| sea_query::ColumnRef::Column(Alias::new(column.to_string()).into()))
                .collect(),
        ));
    }
    Ok(build(&statement, dialect))
}

/// The SQL of an update.
pub fn compile_update(update: &UpdateIr, dialect: Dialect) -> Result<Statement, OrmError> {
    if update.sets.is_empty() {
        return Err(OrmError::invalid(format!(
            "an update of `{}` sets no column",
            update.table.name
        )));
    }
    let compiler = Compiler::new(dialect);
    let mut statement = sea_query::Query::update();
    statement.table(Alias::new(update.table.name.to_string()));
    for (column, value) in &update.sets {
        statement.value(Alias::new(column.to_string()), compiler.expr(value)?);
    }
    if !update.wheres.is_empty() {
        statement.cond_where(compiler.clauses(&update.wheres)?);
    }
    Ok(build(&statement, dialect))
}

/// The SQL of a delete.
pub fn compile_delete(delete: &DeleteIr, dialect: Dialect) -> Result<Statement, OrmError> {
    let compiler = Compiler::new(dialect);
    let mut statement = sea_query::Query::delete();
    statement.from_table(Alias::new(delete.table.name.to_string()));
    if !delete.wheres.is_empty() {
        statement.cond_where(compiler.clauses(&delete.wheres)?);
    }
    Ok(build(&statement, dialect))
}
