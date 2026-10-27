//! One record out of many: the latest, the oldest, or the one an aggregate
//! of some columns picks (`latest_of_many`, `oldest_of_many`, `of_many`),
//! compiled as a join on grouped subqueries.
//!
//! For `of_many([(created_at, Max), (id, Max)])` on a deal's activities:
//!
//! ```text
//! SELECT activities.* FROM activities
//! INNER JOIN (
//!     SELECT activities.deal_id, MAX(activities.id) AS id_aggregate, MIN(activities.created_at) AS created_at_aggregate
//!     FROM activities
//!     INNER JOIN (
//!         SELECT activities.deal_id, MAX(activities.created_at) AS created_at_aggregate
//!         FROM activities WHERE activities.deal_id = ? GROUP BY activities.deal_id
//!     ) AS latest_activity ON latest_activity.created_at_aggregate = activities.created_at
//!                          AND latest_activity.deal_id = activities.deal_id
//!     GROUP BY activities.deal_id
//! ) AS latest_activity ON latest_activity.id_aggregate = activities.id
//!                      AND latest_activity.created_at_aggregate = activities.created_at
//!                      AND latest_activity.deal_id = activities.deal_id
//! ```
//!
//! The relation's own conditions (the parent's key, a morph type) go into
//! the innermost subquery; each column after the first breaks the ties of
//! the ones before.

use std::borrow::Cow;
use std::sync::Arc;

use crate::ir::{AggregateFunction, ColumnRef, Expr, QueryIr, SelectItem, Where};
use crate::model::Model;
use crate::query::Builder;
use crate::relations::core::{constrain, qualified};

/// A constraint on the subqueries of a one-of-many relation.
pub(crate) type OfManyConstraint<R> = Arc<dyn Fn(Builder<R>) -> Builder<R> + Send + Sync>;

/// How a `has_one` picks its record among many.
pub(crate) struct OneOfMany<R: Model> {
    /// The columns and aggregates, the key last (added with `MAX` when it
    /// is not listed).
    pub(crate) columns: Vec<(Cow<'static, str>, AggregateFunction)>,
    /// A constraint on every subquery.
    pub(crate) constraint: Option<OfManyConstraint<R>>,
    /// The join's alias; the relation's name by default.
    pub(crate) alias: Option<Cow<'static, str>>,
}

impl<R: Model> Clone for OneOfMany<R> {
    fn clone(&self) -> Self {
        OneOfMany {
            columns: self.columns.clone(),
            constraint: self.constraint.clone(),
            alias: self.alias.clone(),
        }
    }
}

impl<R: Model> OneOfMany<R> {
    /// Picks by `columns` (with the key appended, by `MAX`, when it is not
    /// among them).
    pub(crate) fn new(mut columns: Vec<(Cow<'static, str>, AggregateFunction)>) -> Self {
        let key = R::meta().key_column();
        if !columns.iter().any(|(column, _)| column == key) {
            columns.push((Cow::Borrowed(key), AggregateFunction::Max));
        }
        OneOfMany {
            columns,
            constraint: None,
            alias: None,
        }
    }

    /// The join alias: the given one, else the relation's name (with
    /// `_of_many` when it is the related table's name).
    pub(crate) fn alias(&self, relation: &str) -> String {
        match &self.alias {
            Some(alias) => alias.to_string(),
            None if relation == R::meta().table() => format!("{relation}_of_many"),
            None => relation.to_owned(),
        }
    }

    /// `query` joined with the grouped subqueries that pick one record of
    /// each group (`group_by`: the foreign key, and the morph type);
    /// `constraints` go into the innermost subquery. `joins` adds what each
    /// subquery needs to reach the grouping columns (a through relation's
    /// intermediate table).
    pub(crate) fn apply(
        &self,
        query: Builder<R>,
        relation: &str,
        group_by: &[ColumnRef],
        constraints: Vec<Where>,
        joins: &dyn Fn(Builder<R>) -> Builder<R>,
    ) -> Builder<R> {
        let table = R::meta().table();
        let alias = self.alias(relation);
        let mut previous: Option<(QueryIr, Vec<String>)> = None;
        for (index, (column, function)) in self.columns.iter().enumerate() {
            let mut accumulated = vec![column.to_string()];
            if let Some((_, columns)) = &previous {
                accumulated.extend(columns.iter().cloned());
            }
            let mut base = Builder::<R>::new();
            base.removed = query.removed.clone();
            let mut sub = joins(base);
            sub.ir.columns = group_by
                .iter()
                .map(|column| SelectItem {
                    expr: Expr::Column(column.clone()),
                    alias: None,
                })
                .collect();
            for (position, name) in accumulated.iter().enumerate() {
                sub.ir.columns.push(SelectItem {
                    expr: Expr::Aggregate {
                        function: if position == 0 {
                            *function
                        } else {
                            AggregateFunction::Min
                        },
                        arg: Some(Box::new(Expr::Column(qualified(table, name)))),
                        distinct: false,
                    },
                    alias: Some(Cow::Owned(format!("{name}_aggregate"))),
                });
            }
            sub.ir.groups = group_by
                .iter()
                .map(|column| Expr::Column(column.clone()))
                .collect();
            if index == 0 {
                sub = constrain(sub, constraints.clone());
            }
            if let Some((inner, columns)) = previous.take() {
                sub = join(sub, inner, &alias, &columns, group_by);
            }
            if let Some(constraint) = &self.constraint {
                sub = constraint(sub);
            }
            previous = Some((sub.into_ir(), accumulated));
        }
        match previous {
            Some((last, columns)) => join(query, last, &alias, &columns, group_by),
            None => query,
        }
    }
}

/// `query INNER JOIN (subquery) AS alias ON alias.{c}_aggregate = table.c
/// ... AND alias.{group} = table.{group}`.
fn join<R: Model>(
    query: Builder<R>,
    subquery: QueryIr,
    alias: &str,
    columns: &[String],
    group_by: &[ColumnRef],
) -> Builder<R> {
    let table = R::meta().table();
    let alias_owned = alias.to_owned();
    query.join_sub_on(subquery, alias_owned.clone(), move |mut join| {
        for column in columns {
            join = join.on(
                ColumnRef::new(Some(alias_owned.clone()), format!("{column}_aggregate")),
                "=",
                qualified(table, column),
            );
        }
        for group in group_by {
            join = join.on(
                ColumnRef::new(Some(alias_owned.clone()), group.name.to_string()),
                "=",
                group.clone(),
            );
        }
        join
    })
}
