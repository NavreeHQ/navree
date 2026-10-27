//! `INSERT` (with `RETURNING`, `INSERT … SELECT`, insert-or-ignore and
//! upserts), `UPDATE` (with JSON path assignments, and `UPDATE … FROM`),
//! `DELETE` and `TRUNCATE`.

use std::borrow::Cow;

use sea_query::{Alias, IntoIden, TableName};

use super::{Compiler, Dialect, Statement, build};
use crate::error::OrmError;
use crate::ir::{
    ColumnRef, DeleteIr, Expr, InsertIr, Join, JoinKind, JsonSegment, OnConflict, TableRef,
    UpdateIr, WhereClause,
};
use crate::value::{Value, to_json};

/// The SQL of an insert. `returning` columns come back on PostgreSQL and
/// SQLite (`RETURNING`); on MySQL the caller reads `last_insert_id`.
/// Conflicting rows fail the statement, are skipped or update the existing
/// row, as [`OnConflict`] says.
///
/// ```
/// use navree_orm::Value;
/// use navree_orm::ir::{Expr, InsertIr, OnConflict};
/// use navree_orm::sql::{Dialect, compile_insert};
///
/// let insert = InsertIr::new("deals", vec!["title".into()], vec![vec![Expr::Value(Value::from("Renewal"))]])
///     .returning(vec!["id".into()]);
/// let sql = compile_insert(&insert, Dialect::Postgres).unwrap();
/// assert_eq!(sql.sql, r#"INSERT INTO "deals" ("title") VALUES ($1) RETURNING "id""#);
///
/// let ignore = insert.on_conflict(OnConflict::Ignore { target: vec![] });
/// assert_eq!(
///     compile_insert(&ignore, Dialect::MySql).unwrap().sql,
///     "INSERT IGNORE INTO `deals` (`title`) VALUES (?)"
/// );
/// ```
pub fn compile_insert(insert: &InsertIr, dialect: Dialect) -> Result<Statement, OrmError> {
    let compiler = Compiler::new(dialect);
    let mut statement = sea_query::Query::insert();
    statement.into_table(Alias::new(insert.table.to_string()));
    match &insert.select {
        Some(query) => {
            if insert.columns.is_empty() {
                return Err(OrmError::invalid(format!(
                    "an insert into `{}` from a query names no columns",
                    insert.table
                )));
            }
            if !query.columns.is_empty() && query.columns.len() != insert.columns.len() {
                return Err(OrmError::invalid(format!(
                    "an insert into `{}` fills {} columns but its query selects {}",
                    insert.table,
                    insert.columns.len(),
                    query.columns.len()
                )));
            }
            statement.columns(
                insert
                    .columns
                    .iter()
                    .map(|column| Alias::new(column.to_string())),
            );
            let mut select = compiler.select(query)?;
            // SQLite reads `ON` right after a `FROM` as a join constraint:
            // a `WHERE` before the conflict clause settles it.
            if dialect == Dialect::Sqlite && insert.on_conflict.is_some() && query.wheres.is_empty()
            {
                select.and_where(sea_query::Expr::cust("true"));
            }
            statement
                .select_from(select)
                .map_err(|error| OrmError::invalid(error.to_string()))?;
        }
        None if insert.columns.is_empty() => {
            statement.or_default_values();
        }
        None => {
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
    }
    let mut ignore = false;
    // Clauses written after sea-query's statement: the conflict clause
    // without a target, then `RETURNING`.
    let mut tail = String::new();
    match &insert.on_conflict {
        None => {}
        Some(OnConflict::Ignore { .. }) if dialect == Dialect::MySql => ignore = true,
        Some(OnConflict::Ignore { target }) if target.is_empty() => {
            tail.push_str(" ON CONFLICT DO NOTHING");
        }
        Some(OnConflict::Ignore { target }) => {
            let mut clause = conflict_target(target);
            clause.do_nothing();
            statement.on_conflict(clause);
        }
        Some(OnConflict::Update { target, sets }) => {
            if sets.is_empty() {
                return Err(OrmError::invalid(format!(
                    "an upsert into `{}` updates no column",
                    insert.table
                )));
            }
            if target.is_empty() && dialect != Dialect::MySql {
                return Err(OrmError::invalid(format!(
                    "an upsert into `{}` on {dialect} needs the unique columns its rows conflict on",
                    insert.table
                )));
            }
            let mut clause = if dialect == Dialect::MySql {
                sea_query::OnConflict::new()
            } else {
                conflict_target(target)
            };
            let mut values = Vec::with_capacity(sets.len());
            for (column, value) in sets {
                values.push((Alias::new(column.to_string()), compiler.expr(value)?));
            }
            clause.values(values);
            statement.on_conflict(clause);
        }
    }
    if !insert.returning.is_empty() && dialect.supports_returning() {
        let columns: Vec<String> = insert
            .returning
            .iter()
            .map(|column| super::quote_identifier(column, dialect))
            .collect();
        tail.push_str(" RETURNING ");
        tail.push_str(&columns.join(", "));
    }
    let mut built = build(&statement, dialect);
    if ignore && built.sql.starts_with("INSERT INTO") {
        built.sql = built.sql.replacen("INSERT INTO", "INSERT IGNORE INTO", 1);
    }
    built.sql.push_str(&tail);
    Ok(built)
}

/// `ON CONFLICT (target)`, or `ON CONFLICT` for any unique index.
fn conflict_target(target: &[Cow<'static, str>]) -> sea_query::OnConflict {
    if target.is_empty() {
        sea_query::OnConflict::new()
    } else {
        sea_query::OnConflict::columns(target.iter().map(|column| Alias::new(column.to_string())))
    }
}

/// The SQL of an update. Assignments to JSON paths (`meta->status`) are
/// grouped per column into one call that sets them all: `jsonb_set` on
/// PostgreSQL, `json_set` on MySQL and SQLite (a NULL column starts as an
/// empty object).
///
/// ```
/// use navree_orm::ir::{Expr, TableRef, UpdateIr};
/// use navree_orm::sql::{Dialect, compile_update};
///
/// let update = UpdateIr {
///     table: TableRef::new("deals"),
///     sets: vec![("meta->status".into(), Expr::val("won")), ("meta->score".into(), Expr::val(9))],
///     wheres: vec![],
/// };
/// assert_eq!(
///     compile_update(&update, Dialect::Sqlite).unwrap().sql,
///     r#"UPDATE "deals" SET "meta" = json_set(ifnull("meta", json_object()), '$."status"', json(?), '$."score"', json(?))"#
/// );
/// ```
pub fn compile_update(update: &UpdateIr, dialect: Dialect) -> Result<Statement, OrmError> {
    if update.sets.is_empty() {
        return Err(OrmError::invalid(format!(
            "an update of `{}` sets no column",
            update.table.name
        )));
    }
    let compiler = Compiler::new(dialect);
    let mut statement = sea_query::Query::update();
    statement.table(table_ref(&update.table));
    for (column, value) in compiler.update_sets(&update.table.name, &update.sets)? {
        statement.value(Alias::new(column), value);
    }
    if !update.wheres.is_empty() {
        statement.cond_where(compiler.clauses(&update.wheres)?);
    }
    Ok(build(&statement, dialect))
}

/// The SQL of PostgreSQL's (and SQLite's) `UPDATE table SET .. FROM
/// joined, .. WHERE ..`: the joined tables become the `FROM` list and their
/// join conditions join the `WHERE`. MySQL writes it as `UPDATE table JOIN
/// joined ON .. SET ..`, for one joined table. Only inner and cross joins
/// can be expressed this way.
pub fn compile_update_from(
    table: &TableRef,
    sets: &[(Cow<'static, str>, Expr)],
    joins: &[Join],
    wheres: &[WhereClause],
    dialect: Dialect,
) -> Result<Statement, OrmError> {
    if sets.is_empty() {
        return Err(OrmError::invalid(format!(
            "an update of `{}` sets no column",
            table.name
        )));
    }
    if joins.is_empty() {
        return Err(OrmError::invalid(format!(
            "update_from on `{}` needs a joined table to update from",
            table.name
        )));
    }
    if dialect == Dialect::MySql && joins.len() > 1 {
        return Err(OrmError::unsupported(
            "update_from with more than one joined table",
            dialect.name(),
        ));
    }
    let compiler = Compiler::new(dialect);
    let mut statement = sea_query::Query::update();
    statement.table(table_ref(table));
    for (column, value) in compiler.update_sets(&table.name, sets)? {
        statement.value(Alias::new(column), value);
    }
    let mut conditions: Vec<WhereClause> = Vec::new();
    for join in joins {
        match join.kind {
            JoinKind::Inner | JoinKind::Cross => {}
            other => {
                return Err(OrmError::invalid(format!(
                    "update_from takes inner and cross joins only, not a {other:?} join (`{}`)",
                    join.table.qualifier()
                )));
            }
        }
        if join.table.subquery.is_some() {
            return Err(OrmError::invalid(format!(
                "update_from cannot update from the subquery `{}`; join a table",
                join.table.qualifier()
            )));
        }
        statement.from(table_ref(&join.table));
        if !join.on.is_empty() {
            conditions.push(WhereClause::and(crate::ir::Where::Group(join.on.clone())));
        }
    }
    if !wheres.is_empty() {
        conditions.push(WhereClause::and(crate::ir::Where::Group(wheres.to_vec())));
    }
    if !conditions.is_empty() {
        statement.cond_where(compiler.clauses(&conditions)?);
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

/// The statements that empty `table` and reset its identity:
/// `TRUNCATE TABLE … RESTART IDENTITY [CASCADE]` on PostgreSQL (`CASCADE`
/// also empties the tables whose foreign keys point at it), `TRUNCATE
/// TABLE` on MySQL (which commits the open transaction, as DDL does
/// there), and on SQLite a `DELETE` of every row plus resetting the
/// table's `AUTOINCREMENT` sequence (run only when the database has a
/// sequence table).
pub fn compile_truncate(table: &str, cascade: bool, dialect: Dialect) -> Vec<Statement> {
    let quoted = super::quote_identifier(table, dialect);
    match dialect {
        Dialect::Postgres => vec![Statement {
            sql: format!(
                "TRUNCATE TABLE {quoted} RESTART IDENTITY{}",
                if cascade { " CASCADE" } else { "" }
            ),
            values: Vec::new(),
            dialect,
        }],
        Dialect::MySql => vec![Statement {
            sql: format!("TRUNCATE TABLE {quoted}"),
            values: Vec::new(),
            dialect,
        }],
        Dialect::Sqlite => vec![
            Statement {
                sql: r#"DELETE FROM "sqlite_sequence" WHERE "name" = ?"#.to_owned(),
                values: vec![Value::from(table.to_owned())],
                dialect,
            },
            Statement {
                sql: format!("DELETE FROM {quoted}"),
                values: Vec::new(),
                dialect,
            },
        ],
    }
}

/// A table (with its alias) as sea-query names it.
fn table_ref(table: &TableRef) -> sea_query::TableRef {
    let name = TableName(None, Alias::new(table.name.to_string()).into_iden());
    sea_query::TableRef::Table(
        name,
        table
            .alias
            .as_ref()
            .map(|alias| Alias::new(alias.to_string()).into_iden()),
    )
}

/// One column's assignment in a `SET` list: a value, or JSON paths inside
/// it.
enum SetItem<'a> {
    Plain(&'a Expr),
    Paths(Vec<(&'a [JsonSegment], &'a Expr)>),
}

impl Compiler {
    /// The `SET` list of an update: plain assignments as they are, the
    /// assignments to JSON paths of one column merged into one expression,
    /// in the order the columns first appear.
    pub(crate) fn update_sets(
        &self,
        table: &str,
        sets: &[(Cow<'static, str>, Expr)],
    ) -> Result<Vec<(String, sea_query::Expr)>, OrmError> {
        let parsed: Vec<(ColumnRef, &Expr)> = sets
            .iter()
            .map(|(column, value)| (ColumnRef::parse(column.clone()), value))
            .collect();
        let mut items: Vec<(&str, SetItem<'_>)> = Vec::new();
        for (column, value) in &parsed {
            let name = column.name.as_ref();
            let existing = items.iter_mut().find(|(existing, _)| *existing == name);
            match (existing, column.path.is_empty()) {
                (None, true) => items.push((name, SetItem::Plain(value))),
                (None, false) => {
                    items.push((name, SetItem::Paths(vec![(column.path.as_slice(), value)])));
                }
                (Some((_, SetItem::Paths(paths))), false) => {
                    paths.push((column.path.as_slice(), value));
                }
                (Some(_), _) => {
                    return Err(OrmError::invalid(format!(
                        "an update of `{table}` sets `{name}` twice, or both the column and a JSON path inside it"
                    )));
                }
            }
        }
        let mut out = Vec::with_capacity(items.len());
        for (name, item) in items {
            let expr = match item {
                SetItem::Plain(value) => self.expr(value)?,
                SetItem::Paths(paths) => self.json_set(name, &paths)?,
            };
            out.push((name.to_owned(), expr));
        }
        Ok(out)
    }

    /// Column `name` with the JSON values at `paths` replaced (or added).
    fn json_set(
        &self,
        name: &str,
        paths: &[(&[JsonSegment], &Expr)],
    ) -> Result<sea_query::Expr, OrmError> {
        let column = sea_query::Expr::col(Alias::new(name.to_owned()));
        match self.dialect {
            Dialect::Postgres => {
                let mut target = self.custom("COALESCE({}::jsonb, jsonb_build_object())", vec![column]);
                for (path, value) in paths {
                    let value = match value {
                        Expr::Value(value) => {
                            sea_query::Expr::val(Value::Json(Some(Box::new(to_json(value)))))
                        }
                        other => self.custom("to_jsonb({})", vec![self.expr(other)?]),
                    };
                    target = self.custom(
                        "jsonb_set({}, {}, {})",
                        vec![target, sea_query::Expr::cust(postgres_path_array(path)), value],
                    );
                }
                Ok(target)
            }
            Dialect::MySql | Dialect::Sqlite => {
                let (empty, wrap) = match self.dialect {
                    Dialect::MySql => ("COALESCE({}, JSON_OBJECT())", "CAST({} AS JSON)"),
                    _ => ("ifnull({}, json_object())", "json({})"),
                };
                let mut exprs = vec![self.custom(empty, vec![column])];
                for (path, value) in paths {
                    exprs.push(sea_query::Expr::cust(self.json_path(path)));
                    exprs.push(match value {
                        Expr::Value(value) => self.custom(
                            wrap,
                            vec![sea_query::Expr::val(Value::from(to_json(value).to_string()))],
                        ),
                        other => self.expr(other)?,
                    });
                }
                let markers = vec!["{}"; exprs.len()].join(", ");
                Ok(self.custom(&format!("json_set({markers})"), exprs))
            }
        }
    }
}

/// A JSON path as PostgreSQL's text array literal for `jsonb_set`:
/// `'{"owner","name"}'`, indexes as numbers (`'{"items",0}'`).
fn postgres_path_array(path: &[JsonSegment]) -> String {
    let elements: Vec<String> = path
        .iter()
        .map(|segment| match segment {
            JsonSegment::Key(key) => {
                format!("\"{}\"", key.replace('\\', "\\\\").replace('"', "\\\""))
            }
            JsonSegment::Index(index) => index.to_string(),
        })
        .collect();
    format!("'{{{}}}'", elements.join(",").replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_paths_are_text_array_literals() {
        assert_eq!(
            postgres_path_array(&[
                JsonSegment::Key("owner".into()),
                JsonSegment::Key("na\"me's".into()),
                JsonSegment::Index(2)
            ]),
            r#"'{"owner","na\"me''s",2}'"#
        );
    }
}
