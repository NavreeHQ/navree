//! Validation rules that query models (feature `validation`): [`unique`]
//! and [`exists`] build navree-validation [`AsyncRule`]s, checked by
//! `ValidAsync` in the request's unit of work after the input's other rules
//! pass.
//!
//! The rules query through the model, so its global scopes hold: on a
//! workspace-scoped model a value is unique, or exists, within the current
//! workspace. `unique` also counts soft-deleted records (a unique index
//! still covers their rows; [`without_trashed`](ModelRule::without_trashed)
//! leaves them out), while `exists` does not (a deleted record is gone;
//! [`with_trashed`](ModelRule::with_trashed) counts it).
//!
//! ```
//! # use navree_orm::__doc_factories::*;
//! use navree_orm::validation::{exists, unique};
//! use navree_validation::{AsyncRule, AsyncRules};
//!
//! /// The body of `POST /deals` and `PATCH /deals/{deal}`.
//! struct DealInput {
//!     slug: String,
//!     company_id: Option<i64>,
//! }
//!
//! impl AsyncRules for DealInput {
//!     fn async_rules(&self) -> Vec<AsyncRule> {
//!         vec![
//!             // An update keeps its own slug: the route's `{deal}` is ignored.
//!             unique(Deal::SLUG, self.slug.clone())
//!                 .ignore_path_param("deal")
//!                 .into(),
//!             // `None` skips the rule; the error goes to `company_id`.
//!             exists(Company::ID, self.company_id)
//!                 .field("company_id")
//!                 .where_(Company::ACTIVE.eq(true))
//!                 .into(),
//!         ]
//!     }
//! }
//! ```

use std::fmt;
use std::sync::Arc;

use navree_db::Uow;
use navree_kernel::{BoxError, BoxFuture};
use navree_validation::{AsyncRule, UowCheck};

use crate::binding::route_value;
use crate::column::{Column, Filter, col};
use crate::model::Model;
use crate::query::Builder;
use crate::value::is_null;

/// What a [`ModelRule`] checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Unique,
    Exists,
}

impl Kind {
    fn rule(self) -> &'static str {
        match self {
            Kind::Unique => "unique",
            Kind::Exists => "exists",
        }
    }
}

/// A change to a rule's query.
type QueryFn<M> = Arc<dyn Fn(Builder<M>) -> Builder<M> + Send + Sync>;

/// `field` must be unique in `column` of model `M`: no record has
/// `value`. A `None` value (or a null one) passes. The error goes to the
/// column's name unless [`field`](ModelRule::field) names another; the
/// record being updated is skipped with [`ignore`](ModelRule::ignore) or
/// [`ignore_path_param`](ModelRule::ignore_path_param). Turn it into an
/// [`AsyncRule`] with `.into()`.
pub fn unique<M: Model, T>(column: Column<M, T>, value: impl Into<Option<T>>) -> ModelRule<M> {
    ModelRule::new(Kind::Unique, column, value.into())
}

/// `field` must name a record of model `M`: some record has `value` in
/// `column` (`exists(Company::ID, input.company_id)`). A `None` value (or
/// a null one) passes; make the field required to demand one.
pub fn exists<M: Model, T>(column: Column<M, T>, value: impl Into<Option<T>>) -> ModelRule<M> {
    ModelRule::new(Kind::Exists, column, value.into())
}

/// A [`unique`] or [`exists`] rule on model `M`: the filters of its query,
/// then `.into()` an [`AsyncRule`].
pub struct ModelRule<M: Model> {
    kind: Kind,
    field: String,
    /// `column = value`; `None` for a null value (the rule passes).
    filter: Option<Filter>,
    ignore: Option<M::Key>,
    ignore_path_param: Option<String>,
    conditions: Vec<Filter>,
    trashed: Option<bool>,
    query: Option<QueryFn<M>>,
}

impl<M: Model> fmt::Debug for ModelRule<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelRule")
            .field("rule", &self.kind.rule())
            .field("model", &M::meta().name())
            .field("field", &self.field)
            .field("filter", &self.filter)
            .field("ignore", &self.ignore)
            .field("ignore_path_param", &self.ignore_path_param)
            .field("conditions", &self.conditions)
            .field("trashed", &self.trashed)
            .finish_non_exhaustive()
    }
}

impl<M: Model> ModelRule<M> {
    fn new<T>(kind: Kind, column: Column<M, T>, value: Option<T>) -> Self {
        let null = match &value {
            None => true,
            // A value the column cannot encode stays: its filter reports
            // the error when the rule runs.
            Some(value) => column.encode(value).is_ok_and(|value| is_null(&value)),
        };
        let filter = value.filter(|_| !null).map(|value| column.eq(value));
        Self {
            kind,
            field: crate::column::ColumnName::column_name(&column).into_owned(),
            filter,
            ignore: None,
            ignore_path_param: None,
            conditions: Vec::new(),
            trashed: None,
            query: None,
        }
    }

    /// The field the error goes to (default: the column's name).
    pub fn field(mut self, field: impl Into<String>) -> Self {
        self.field = field.into();
        self
    }

    /// For `unique`: skip the record with key `key` (the one being
    /// updated).
    pub fn ignore(mut self, key: impl Into<M::Key>) -> Self {
        self.ignore = Some(key.into());
        self
    }

    /// For `unique` under `ValidAsync`: skip the record whose route key
    /// ([`Model::route_key_name`]) is the route's path parameter `name`
    /// (`PATCH /deals/{deal}`), unless [`ignore`](ModelRule::ignore) named
    /// one.
    pub fn ignore_path_param(mut self, name: impl Into<String>) -> Self {
        self.ignore_path_param = Some(name.into());
        self
    }

    /// Only records matching `filter` count (`Deal::STAGE.ne(Stage::Lost)`).
    pub fn where_(mut self, filter: impl Into<Filter>) -> Self {
        self.conditions.push(filter.into());
        self
    }

    /// Soft-deleted records count too (the default for `unique`).
    pub fn with_trashed(mut self) -> Self {
        self.trashed = Some(true);
        self
    }

    /// Soft-deleted records do not count (the default for `exists`).
    pub fn without_trashed(mut self) -> Self {
        self.trashed = Some(false);
        self
    }

    /// Any other change to the rule's query, applied last
    /// (`|query| query.without_workspace_scope()` for a value unique
    /// across every workspace).
    pub fn query(
        mut self,
        f: impl Fn(Builder<M>) -> Builder<M> + Send + Sync + 'static,
    ) -> Self {
        self.query = Some(Arc::new(f));
        self
    }
}

impl<M: Model> From<ModelRule<M>> for AsyncRule {
    fn from(rule: ModelRule<M>) -> AsyncRule {
        let null = rule.filter.is_none();
        let field = rule.field.clone();
        let kind = rule.kind;
        let ignore_path_param = rule.ignore_path_param.clone();
        let mut async_rule = AsyncRule::check(field, kind.rule(), ModelCheck(rule)).skipped_when(null);
        if let Some(name) = ignore_path_param {
            async_rule = async_rule.ignore_path_param(name);
        }
        async_rule
    }
}

/// The check of a [`ModelRule`].
struct ModelCheck<M: Model>(ModelRule<M>);

impl<M: Model> ModelCheck<M> {
    /// The rule's query: the records that make `unique` fail or `exists`
    /// pass. `ignore` is the path parameter `ignore_path_param` names.
    fn query(&self, ignore: Option<&str>) -> Option<Builder<M>> {
        let rule = &self.0;
        let mut query = M::query().where_(rule.filter.clone()?);
        if rule.trashed.unwrap_or(rule.kind == Kind::Unique) {
            query = query.with_trashed();
        }
        if rule.kind == Kind::Unique {
            if let Some(key) = &rule.ignore {
                query = query.where_key_not(key.clone());
            } else if let Some(text) = ignore {
                let column = M::route_key_name();
                // A parameter that cannot be a route key names no record.
                let value = M::meta()
                    .column(column)
                    .and_then(|meta| route_value(meta, text));
                if let Some(value) = value {
                    let qualified = format!("{}.{column}", M::meta().table());
                    query = query.where_(col(qualified).ne(value));
                }
            }
        }
        for condition in &rule.conditions {
            query = query.where_(condition.clone());
        }
        if let Some(f) = &rule.query {
            query = f(query);
        }
        Some(query)
    }
}

impl<M: Model> UowCheck for ModelCheck<M> {
    fn passes<'a>(
        &'a self,
        uow: &'a Uow,
        ignore: Option<&'a str>,
    ) -> BoxFuture<'a, Result<bool, BoxError>> {
        Box::pin(async move {
            let Some(query) = self.query(ignore) else {
                return Ok(true);
            };
            let found = query.exists(uow).await?;
            Ok(match self.0.kind {
                Kind::Unique => !found,
                Kind::Exists => found,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::__doc_factories::{Company, Deal};
    use crate::sql::Dialect;

    fn sql<M: Model>(rule: ModelRule<M>, ignore: Option<&str>) -> String {
        ModelCheck(rule)
            .query(ignore)
            .expect("a value")
            .to_sql(Dialect::Postgres)
            .expect("valid SQL")
            .sql
    }

    #[test]
    fn unique_counts_trashed_records_and_skips_the_ignored_one() {
        let query = sql(unique(Deal::SLUG, "acme".to_owned()), None);
        assert!(query.contains(r#""deals"."slug" = $1"#), "{query}");
        assert!(!query.contains(r#""deleted_at" IS NULL"#), "{query}");

        let query = sql(
            unique(Deal::SLUG, "acme".to_owned()).without_trashed(),
            None,
        );
        assert!(query.contains(r#""deleted_at" IS NULL"#), "{query}");

        let query = sql(
            unique(Deal::SLUG, "acme".to_owned()).ignore_path_param("deal"),
            Some("acme-renewal"),
        );
        assert!(query.contains(r#""deals"."slug" <> $2"#), "{query}");
    }

    #[test]
    fn exists_leaves_trashed_records_out_and_ignores_nothing() {
        let query = sql(exists(Deal::SLUG, "acme".to_owned()), Some("acme"));
        assert!(query.contains(r#""deleted_at" IS NULL"#), "{query}");
        assert!(!query.contains("<>"), "{query}");
        let query = sql(
            exists(Company::ID, 7).where_(Company::ACTIVE.eq(true)),
            None,
        );
        assert!(query.contains(r#""companies"."active" = $2"#), "{query}");
    }

    #[test]
    fn null_values_skip_the_rule() {
        let rule: AsyncRule = exists(Company::ID, None::<i64>).into();
        assert_eq!((rule.field(), rule.rule()), ("id", "exists"));
        assert!(ModelCheck(exists(Company::ID, None::<i64>)).query(None).is_none());
        let rule: AsyncRule = unique(Deal::SLUG, "acme".to_owned()).field("slug_text").into();
        assert_eq!((rule.field(), rule.rule()), ("slug_text", "unique"));
    }
}
