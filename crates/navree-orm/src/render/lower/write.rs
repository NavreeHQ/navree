//! `INSERT` (with `RETURNING`, `INSERT … SELECT`, insert-or-ignore and
//! upserts), `UPDATE` (with JSON path assignments, and `UPDATE … FROM`),
//! `DELETE` and emptying a table.

use std::borrow::Cow;

use super::Lowerer;
use crate::error::{OrmError, Result};
use crate::ir::{
    ColumnRef, DeleteIr, Expr, InsertIr, Join, JoinKind, JsonSegment, OnConflict, TableRef,
    UpdateIr, Where, WhereClause,
};
use crate::render::ast::{self, SqlType};
use crate::sql::Dialect;
use crate::value::{Value, to_json};

/// An insert as a tree: `returning` columns come back on PostgreSQL and SQLite only
/// (MySQL reads `last_insert_id`); conflicting rows fail the statement,
/// are skipped or update the existing row, as [`OnConflict`] says.
pub fn insert(insert: &InsertIr, dialect: Dialect) -> Result<ast::Insert> {
    let lowerer = Lowerer::new(dialect);
    let columns: Vec<ast::Ident> = insert
        .columns
        .iter()
        .map(|column| ast::Ident(column.clone()))
        .collect();
    let source = match &insert.select {
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
            ast::InsertSource::Query(Box::new(lowerer.select(query)?))
        }
        None if insert.columns.is_empty() => ast::InsertSource::DefaultValues,
        None => {
            let mut rows = Vec::with_capacity(insert.rows.len());
            for row in &insert.rows {
                if row.len() != insert.columns.len() {
                    return Err(OrmError::invalid(format!(
                        "an insert into `{}` has {} columns but a row of {} values",
                        insert.table,
                        insert.columns.len(),
                        row.len()
                    )));
                }
                rows.push(
                    row.iter()
                        .map(|value| lowerer.expr(value))
                        .collect::<Result<Vec<_>>>()?,
                );
            }
            ast::InsertSource::Values(rows)
        }
    };
    let target = |target: &[Cow<'static, str>]| {
        target
            .iter()
            .map(|column| ast::Ident(column.clone()))
            .collect::<Vec<_>>()
    };
    let on_conflict = match &insert.on_conflict {
        None => None,
        Some(OnConflict::Ignore { target: columns }) => Some(ast::OnConflict {
            target: target(columns),
            action: ast::ConflictAction::Ignore,
        }),
        Some(OnConflict::Update {
            target: columns,
            sets,
        }) => {
            if sets.is_empty() {
                return Err(OrmError::invalid(format!(
                    "an upsert into `{}` updates no column",
                    insert.table
                )));
            }
            if columns.is_empty() && dialect != Dialect::MySql {
                return Err(OrmError::invalid(format!(
                    "an upsert into `{}` on {dialect} needs the unique columns its rows conflict on",
                    insert.table
                )));
            }
            let mut assignments = Vec::with_capacity(sets.len());
            for (column, value) in sets {
                assignments.push(ast::Assignment {
                    column: ast::Ident(column.clone()),
                    value: lowerer.expr(value)?,
                });
            }
            Some(ast::OnConflict {
                target: target(columns),
                action: ast::ConflictAction::Update {
                    sets: assignments,
                    filter: None,
                },
            })
        }
    };
    let returning = if dialect.supports_returning() {
        insert
            .returning
            .iter()
            .map(|column| ast::SelectItem::new(ast::Expr::col(column.clone())))
            .collect()
    } else {
        Vec::new()
    };
    Ok(ast::Insert {
        table: ast::ObjectName::parse(insert.table.clone()),
        columns,
        source,
        on_conflict,
        returning,
    })
}

/// An update as a tree: assignments to JSON paths (`meta->status`) are grouped per
/// column into one call that sets them all (`jsonb_set` on PostgreSQL,
/// `json_set` on MySQL and SQLite; a NULL column starts as an empty
/// object).
pub fn update(update: &UpdateIr, dialect: Dialect) -> Result<ast::Update> {
    if update.sets.is_empty() {
        return Err(OrmError::invalid(format!(
            "an update of `{}` sets no column",
            update.table.name
        )));
    }
    let lowerer = Lowerer::new(dialect);
    let mut lowered = ast::Update::new(
        ast::ObjectName::parse(update.table.name.clone()),
        lowerer.update_sets(&update.table.name, &update.sets)?,
    );
    lowered.alias = update.table.alias.clone().map(ast::Ident);
    if !update.wheres.is_empty() {
        lowered.filter = Some(lowerer.clauses(&update.wheres)?);
    }
    Ok(lowered)
}

/// PostgreSQL's (and SQLite's) `UPDATE table SET .. FROM joined, ..
/// WHERE ..` as a tree: the joined tables become the `FROM` list and their
/// join conditions join the `WHERE`; MySQL writes it as `UPDATE table,
/// joined SET ..`, for one joined table. Only
/// inner and cross joins can be expressed this way.
pub fn update_from(
    table: &TableRef,
    sets: &[(Cow<'static, str>, Expr)],
    joins: &[Join],
    wheres: &[WhereClause],
    dialect: Dialect,
) -> Result<ast::Update> {
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
    let lowerer = Lowerer::new(dialect);
    let mut lowered = ast::Update::new(
        ast::ObjectName::parse(table.name.clone()),
        lowerer.update_sets(&table.name, sets)?,
    );
    lowered.alias = table.alias.clone().map(ast::Ident);
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
        lowered.from.push(ast::Source::Table {
            name: ast::ObjectName::parse(join.table.name.clone()),
            alias: join.table.alias.clone().map(ast::Ident),
        });
        if !join.on.is_empty() {
            conditions.push(WhereClause::and(Where::Group(join.on.clone())));
        }
    }
    if !wheres.is_empty() {
        conditions.push(WhereClause::and(Where::Group(wheres.to_vec())));
    }
    if !conditions.is_empty() {
        lowered.filter = Some(lowerer.clauses(&conditions)?);
    }
    Ok(lowered)
}

/// A delete as a tree (the table is named without its alias).
pub fn delete(delete: &DeleteIr, dialect: Dialect) -> Result<ast::Delete> {
    let lowerer = Lowerer::new(dialect);
    let mut lowered = ast::Delete::new(ast::ObjectName::parse(delete.table.name.clone()));
    if !delete.wheres.is_empty() {
        lowered.filter = Some(lowerer.clauses(&delete.wheres)?);
    }
    Ok(lowered)
}

/// Emptying `table` and restarting its identity: `CASCADE`
/// (PostgreSQL) also empties the tables whose foreign keys point at it.
pub fn truncate(table: &str, cascade: bool) -> ast::Truncate {
    ast::Truncate {
        table: ast::ObjectName::parse(table.to_owned()),
        restart_identity: true,
        cascade,
    }
}

/// One column's assignment in a `SET` list: a value, or JSON paths inside
/// it.
enum SetItem<'a> {
    Plain(&'a Expr),
    Paths(Vec<(&'a [JsonSegment], &'a Expr)>),
}

impl Lowerer {
    /// The `SET` list of an update: plain assignments as they are, the
    /// assignments to JSON paths of one column merged into one expression,
    /// in the order the columns first appear.
    fn update_sets(
        &self,
        table: &str,
        sets: &[(Cow<'static, str>, Expr)],
    ) -> Result<Vec<ast::Assignment>> {
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
            let value = match item {
                SetItem::Plain(value) => self.expr(value)?,
                SetItem::Paths(paths) => self.json_set(name, &paths)?,
            };
            out.push(ast::Assignment {
                column: ast::Ident::new(name.to_owned()),
                value,
            });
        }
        Ok(out)
    }

    /// Column `name` with the JSON values at `paths` replaced (or added).
    fn json_set(&self, name: &str, paths: &[(&[JsonSegment], &Expr)]) -> Result<ast::Expr> {
        let column = ast::Expr::Column(ast::ColumnName {
            table: None,
            name: ast::Ident::new(name.to_owned()),
        });
        match self.dialect {
            Dialect::Postgres => {
                let mut target = ast::Expr::func(
                    "COALESCE",
                    vec![
                        column.cast(SqlType::Json),
                        ast::Expr::func("jsonb_build_object", Vec::new()),
                    ],
                );
                for (path, value) in paths {
                    let value = match value {
                        Expr::Value(value) => {
                            ast::Expr::Value(Value::Json(Some(Box::new(to_json(value)))))
                        }
                        other => ast::Expr::func("to_jsonb", vec![self.expr(other)?]),
                    };
                    target = ast::Expr::func(
                        "jsonb_set",
                        vec![target, ast::Expr::string(postgres_path_array(path)), value],
                    );
                }
                Ok(target)
            }
            Dialect::MySql | Dialect::Sqlite => {
                let mysql = self.dialect == Dialect::MySql;
                let mut args = vec![if mysql {
                    ast::Expr::func(
                        "COALESCE",
                        vec![column, ast::Expr::func("JSON_OBJECT", Vec::new())],
                    )
                } else {
                    ast::Expr::func(
                        "ifnull",
                        vec![column, ast::Expr::func("json_object", Vec::new())],
                    )
                }];
                for (path, value) in paths {
                    args.push(self.json_path(path));
                    args.push(match value {
                        Expr::Value(value) => {
                            let encoded = ast::Expr::Value(Value::from(to_json(value).to_string()));
                            if mysql {
                                encoded.cast(SqlType::Json)
                            } else {
                                ast::Expr::func("json", vec![encoded])
                            }
                        }
                        other => self.expr(other)?,
                    });
                }
                Ok(ast::Expr::func("json_set", args))
            }
        }
    }
}

/// A JSON path as PostgreSQL's text array for `jsonb_set`, before it is
/// quoted: `{"owner","name"}`, indexes as numbers (`{"items",0}`).
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
    format!("{{{}}}", elements.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_paths_are_text_arrays() {
        assert_eq!(
            postgres_path_array(&[
                JsonSegment::Key("owner".into()),
                JsonSegment::Key("na\"me's".into()),
                JsonSegment::Index(2)
            ]),
            r#"{"owner","na\"me's",2}"#
        );
    }
}
