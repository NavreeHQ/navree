//! Relation aggregates: counts, sums, averages, minima, maxima and
//! existence flags of related records, selected as correlated subqueries
//! (`with_count`, `with_sum`, ...) or loaded onto records afterwards
//! (`load_count`, ...). Values land in the records' extras, under
//! `{relation}_count`, `{relation}_{function}_{column}`,
//! `{relation}_exists`, or the alias given with `"relation as alias"`.

use std::borrow::Cow;

use crate::column::IntoExpr;
use crate::error::{OrmError, Result};
use crate::exec::Target;
use crate::ir::{AggregateFunction, Expr, SelectItem};
use crate::model::Model;
use crate::query::Builder;
use crate::record::Record;
use crate::relations::existence::{ExistsRequest, ExistsSelect, Refine};
use crate::relations::keys::key_of;
use crate::relations::path::{ParsedPath, RelationPath, RelationPathTo};
use crate::relations::registry;

/// One relation aggregate.
#[derive(Clone, Debug)]
pub struct AggregateSpec {
    kind: AggregateKind,
}

#[derive(Clone, Debug)]
enum AggregateKind {
    Count,
    Exists,
    Function {
        function: AggregateFunction,
        column: Expr,
        name: String,
    },
}

impl AggregateSpec {
    /// The number of related records.
    pub(crate) fn count() -> Self {
        AggregateSpec {
            kind: AggregateKind::Count,
        }
    }

    /// Whether a related record exists.
    pub(crate) fn exists() -> Self {
        AggregateSpec {
            kind: AggregateKind::Exists,
        }
    }

    /// `function` of related column `column`.
    pub(crate) fn function(function: AggregateFunction, column: impl IntoExpr) -> Result<Self> {
        let column = column.into_expr();
        let name = match &column {
            Expr::Column(column) => column.name.to_string(),
            other => {
                return Err(OrmError::invalid(format!(
                    "a relation aggregate takes a column, not {other:?}"
                )));
            }
        };
        Ok(AggregateSpec {
            kind: AggregateKind::Function {
                function,
                column,
                name,
            },
        })
    }

    /// The result column's default name for relation `relation`.
    fn alias(&self, relation: &str) -> String {
        match &self.kind {
            AggregateKind::Count => format!("{relation}_count"),
            AggregateKind::Exists => format!("{relation}_exists"),
            AggregateKind::Function { function, name, .. } => format!(
                "{relation}_{}_{name}",
                function.sql_name().to_ascii_lowercase()
            ),
        }
    }

    fn select(&self) -> ExistsSelect {
        match &self.kind {
            AggregateKind::Count => ExistsSelect::Count,
            AggregateKind::Exists => ExistsSelect::One,
            AggregateKind::Function {
                function, column, ..
            } => ExistsSelect::Aggregate {
                function: *function,
                column: column.clone(),
            },
        }
    }
}

impl<M: Model> Builder<M> {
    /// Adds the aggregate `spec` of relation `text` to the select list.
    pub(crate) fn push_aggregate(
        self,
        text: &str,
        spec: &AggregateSpec,
        refine: impl FnOnce(u32) -> Refine,
    ) -> Self {
        let path = match ParsedPath::parse(text) {
            Ok(path) => path,
            Err(error) => return self.with_error(error.to_string()),
        };
        if path.segments.len() != 1 || path.columns.is_some() {
            return self.with_error(format!(
                "`{text}`: a relation aggregate takes one relation of {} (optionally `as alias`), not a path or a column selection",
                M::meta().name()
            ));
        }
        let name = &path.segments[0];
        let alias = path.alias.clone().unwrap_or_else(|| spec.alias(name));
        let relation = match registry::find::<M>(name) {
            Ok(relation) => relation,
            Err(error) => return self.with_error(error.to_string()),
        };
        let depth = self.eager.depth;
        let query = match relation.inner.exists_query(ExistsRequest {
            depth,
            select: spec.select(),
            refine: refine(depth + 1),
        }) {
            Ok(query) => query,
            Err(error) => return self.with_error(error.to_string()),
        };
        match spec.kind {
            AggregateKind::Exists => self.select_expr(Expr::Exists(Box::new(query)), alias),
            AggregateKind::Count | AggregateKind::Function { .. } => self.select_sub(query, alias),
        }
    }

    /// Adds `{relation}_count`, the number of related records of each
    /// record, to its extras (`"activities as open_count"` names it).
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// let sql = Deal::query().with_count("activities").to_sql(Dialect::Sqlite).unwrap();
    /// assert!(sql.sql.ends_with(
    ///     r#"(SELECT COUNT(*) FROM "activities" WHERE "deals"."id" = "activities"."deal_id") AS "activities_count" FROM "deals""#
    /// ));
    /// ```
    pub fn with_count(self, path: impl RelationPath<M>) -> Self {
        let text = path.relation_path().into_owned();
        self.push_aggregate(&text, &AggregateSpec::count(), |_| Refine::None)
    }

    /// Adds the number of related records matching `constraint`.
    pub fn with_count_constrained<T: Model>(
        self,
        path: impl RelationPathTo<M, T>,
        constraint: impl FnOnce(Builder<T>) -> Builder<T>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_aggregate(&text, &AggregateSpec::count(), move |depth| {
            let mut empty = Builder::<T>::new();
            empty.eager.depth = depth;
            Refine::Builder(Box::new(constraint(empty)))
        })
    }

    /// Adds `{relation}_sum_{column}`, the sum of related `column`.
    pub fn with_sum(self, path: impl RelationPath<M>, column: impl IntoExpr) -> Self {
        self.with_aggregate(path, column, AggregateFunction::Sum)
    }

    /// Adds `{relation}_avg_{column}`, the average of related `column`.
    pub fn with_avg(self, path: impl RelationPath<M>, column: impl IntoExpr) -> Self {
        self.with_aggregate(path, column, AggregateFunction::Avg)
    }

    /// Adds `{relation}_min_{column}`, the smallest related `column`.
    pub fn with_min(self, path: impl RelationPath<M>, column: impl IntoExpr) -> Self {
        self.with_aggregate(path, column, AggregateFunction::Min)
    }

    /// Adds `{relation}_max_{column}`, the largest related `column`.
    pub fn with_max(self, path: impl RelationPath<M>, column: impl IntoExpr) -> Self {
        self.with_aggregate(path, column, AggregateFunction::Max)
    }

    /// Adds `{relation}_exists`, whether a related record exists (read it
    /// as a `bool`).
    pub fn with_exists(self, path: impl RelationPath<M>) -> Self {
        let text = path.relation_path().into_owned();
        self.push_aggregate(&text, &AggregateSpec::exists(), |_| Refine::None)
    }

    /// Adds `function` of related `column`, as
    /// `{relation}_{function}_{column}`.
    pub fn with_aggregate(
        self,
        path: impl RelationPath<M>,
        column: impl IntoExpr,
        function: AggregateFunction,
    ) -> Self {
        let text = path.relation_path().into_owned();
        match AggregateSpec::function(function, column) {
            Ok(spec) => self.push_aggregate(&text, &spec, |_| Refine::None),
            Err(error) => self.with_error(error.to_string()),
        }
    }
}

/// Loads aggregate `spec` of every relation of `paths` onto `records`, with
/// one query of their keys.
pub(crate) async fn load_aggregate_on<M: Model>(
    target: &Target<'_>,
    records: &mut [Record<M>],
    paths: &[String],
    spec: &AggregateSpec,
) -> Result<()> {
    let keys: Vec<M::Key> = records
        .iter()
        .filter(|record| record.exists())
        .map(Record::key)
        .collect();
    if keys.is_empty() || paths.is_empty() {
        return Ok(());
    }
    let meta = M::meta();
    let mut query = Builder::<M>::new()
        .without_global_scopes()
        .where_key_in(keys)
        .select([SelectItem {
            expr: Expr::Column(crate::relations::core::qualified(meta.table(), meta.key_column())),
            alias: Some(Cow::Borrowed("navree_key")),
        }]);
    for path in paths {
        query = query.push_aggregate(path, spec, |_| Refine::None);
    }
    let rows = target.fetch(&query.into_ir()).await?;
    for row in rows {
        let values = row.values()?;
        let Some(key) = values
            .iter()
            .find(|(column, _)| column == "navree_key")
            .and_then(|(_, value)| key_of(value))
        else {
            continue;
        };
        for record in records.iter_mut() {
            let record_key = crate::Field::to_value(&record.key());
            if key_of(&record_key).as_ref() == Some(&key) {
                for (column, value) in &values {
                    if column != "navree_key" {
                        record.set_extra(column.clone(), value.clone());
                    }
                }
            }
        }
    }
    Ok(())
}
