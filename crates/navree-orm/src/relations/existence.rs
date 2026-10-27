//! Existence queries: records that have (or lack) related records, through
//! nested paths, with conditions on them: `has`, `where_has`,
//! `doesnt_have`, `where_relation`, `where_belongs_to`, `where_attached_to`
//! and their `or_` forms. Each compiles to a correlated `EXISTS` subquery
//! (a count subquery for "at least n"), in which the related model's global
//! scopes apply.

use std::any::Any;
use std::borrow::Cow;

use crate::collection::Collection;
use crate::column::Filter;
use crate::error::{OrmError, Result};
use crate::ir::{
    AggregateFunction, Boolean, ColumnRef, Expr, Operator, QueryIr, SelectItem, TableRef, Where,
    WhereClause,
};
use crate::model::{Model, ModelMeta, snake_case};
use crate::query::Builder;
use crate::query::filters::HasWheres;
use crate::record::Record;
use crate::relations::path::{ParsedPath, RelationPath, RelationPathTo};
use crate::relations::registry::{self, RelationKind};
use crate::relations::requalify::requalify;
use crate::value::Value;

/// What an existence subquery selects.
pub(crate) enum ExistsSelect {
    /// `1`: for `EXISTS`.
    One,
    /// `COUNT(*)`: relation counts, "at least n" checks.
    Count,
    /// An aggregate of a related column: relation sums, averages, minima
    /// and maxima.
    Aggregate {
        function: AggregateFunction,
        column: Expr,
    },
}

/// How the related query is narrowed before it is correlated.
pub(crate) enum Refine {
    /// Not at all.
    None,
    /// By a `Builder` of the related model the caller's closure refined
    /// (its conditions, joins, groups and removed scopes are merged in).
    Builder(Box<dyn Any>),
    /// By an existence check of its own (the rest of a nested path).
    Nested {
        segments: Vec<String>,
        op: Operator,
        count: u64,
        refine: Box<Refine>,
    },
    /// By one condition.
    Filter(Filter),
}

impl Refine {
    /// Applies the refinement to `query` (of the related model `R`).
    pub(crate) fn apply<R: Model>(self, query: Builder<R>, relation: &str) -> Result<Builder<R>> {
        match self {
            Refine::None => Ok(query),
            Refine::Builder(refined) => match refined.downcast::<Builder<R>>() {
                Ok(refined) => Ok(merge_refined(query, *refined)),
                Err(_) => Err(OrmError::invalid(format!(
                    "the closure for relation `{relation}` refines another model's query: the relation leads to {} (take a Builder<{}>)",
                    R::meta().name(),
                    R::meta().name()
                ))),
            },
            Refine::Nested {
                segments,
                op,
                count,
                refine,
            } => {
                let depth = query.eager.depth;
                let filter = has_filter::<R>(depth, &segments, op, count, *refine)?;
                Ok(query.where_(Filter::from(filter)))
            }
            Refine::Filter(filter) => Ok(query.where_(filter)),
        }
    }
}

/// `query` with the parts of `refined` an existence check keeps: its
/// conditions (grouped when they contain an `OR`), joins, groups, havings
/// and removed global scopes.
pub(crate) fn merge_refined<R: Model>(mut query: Builder<R>, refined: Builder<R>) -> Builder<R> {
    let ir = refined.ir;
    if ir.wheres.iter().any(|clause| clause.boolean == Boolean::Or) {
        query
            .ir
            .wheres
            .push(WhereClause::and(Where::Group(ir.wheres)));
    } else {
        query.ir.wheres.extend(ir.wheres);
    }
    query.ir.joins.extend(ir.joins);
    query.ir.groups.extend(ir.groups);
    query.ir.havings.extend(ir.havings);
    query.ir.errors.extend(ir.errors);
    query.scopes.merge(refined.scopes);
    query
}

/// What one existence subquery is for.
pub struct ExistsRequest {
    /// How deep the outer query is nested.
    pub(crate) depth: u32,
    /// What the subquery selects.
    pub(crate) select: ExistsSelect,
    /// How the related query is narrowed.
    pub(crate) refine: Refine,
}

/// The table alias of a subquery nested `depth` levels deep, for a related
/// table that is also the outer query's.
pub(crate) fn alias_at(depth: u32) -> String {
    format!("navree_{depth}")
}

/// Builds the existence subquery of a relation from `P` to `R`: the
/// relation definition's query (`definition`), its joins and fixed
/// conditions (`prepare`, which gets the alias of the aliased table, if
/// any), the caller's refinement, `R`'s global scopes and the select; then
/// references to `aliased` (the related or intermediate table, when it is
/// the outer query's table too) move to the alias, and `correlate`'s
/// conditions (which name the outer table) come first.
pub(crate) fn build_subquery<R: Model>(
    parent: &'static ModelMeta,
    definition: &Builder<R>,
    request: ExistsRequest,
    relation: &str,
    aliased: Option<&str>,
    prepare: impl FnOnce(Builder<R>, Option<&str>) -> Builder<R>,
    correlate: impl FnOnce(Option<&str>) -> Vec<Where>,
) -> Result<QueryIr> {
    if R::meta().connection_name() != parent.connection_name() {
        return Err(OrmError::unsupported(
            format!(
                "an existence check from {} (connection {}) to {} (connection {}): the relation `{relation}` crosses connections, which cannot be joined; load the related keys and filter with where_in",
                parent.name(),
                parent.connection_name().unwrap_or("default"),
                R::meta().name(),
                R::meta().connection_name().unwrap_or("default"),
            ),
            "SQL",
        ));
    }
    let depth = request.depth + 1;
    let alias = aliased.map(|_| alias_at(depth));
    let mut query = definition.clone();
    query.eager = crate::relations::eager::EagerState {
        depth,
        ..Default::default()
    };
    if let (Some(table), Some(alias)) = (aliased, &alias)
        && table == R::meta().table()
    {
        query.ir.table = TableRef::aliased(table.to_owned(), alias.clone());
    }
    let query = prepare(query, alias.as_deref());
    let query = request.refine.apply::<R>(query, relation)?;
    let qualifier = query.ir.table.qualifier().to_owned();
    let mut ir = query.into_ir();
    ir.orders.clear();
    ir.lock = None;
    ir.columns = vec![SelectItem {
        expr: match request.select {
            ExistsSelect::One => Expr::Raw {
                sql: Cow::Borrowed("1"),
                values: Vec::new(),
            },
            ExistsSelect::Count => Expr::count_all(),
            ExistsSelect::Aggregate { function, column } => Expr::Aggregate {
                function,
                arg: Some(Box::new(qualify(column, &qualifier))),
                distinct: false,
            },
        },
        alias: None,
    }];
    if let (Some(table), Some(alias)) = (aliased, &alias) {
        requalify(&mut ir, table, alias);
    }
    let correlation = correlate(alias.as_deref());
    let refinements = std::mem::take(&mut ir.wheres);
    ir.wheres = correlation.into_iter().map(WhereClause::and).collect();
    if refinements
        .iter()
        .any(|clause| clause.boolean == Boolean::Or)
    {
        ir.wheres.push(WhereClause::and(Where::Group(refinements)));
    } else {
        ir.wheres.extend(refinements);
    }
    Ok(ir)
}

/// `column` qualified by `qualifier` when it names no table.
fn qualify(column: Expr, qualifier: &str) -> Expr {
    match column {
        Expr::Column(mut column) if column.table.is_none() => {
            column.table = Some(Cow::Owned(qualifier.to_owned()));
            Expr::Column(column)
        }
        other => other,
    }
}

/// `outer = inner`, comparing a column of the outer query with one of the
/// subquery.
pub(crate) fn columns_equal(outer: ColumnRef, inner: ColumnRef) -> Where {
    Where::Compare {
        left: Expr::Column(outer),
        op: Operator::Eq,
        right: Expr::Column(inner),
    }
}

/// The condition of an existence check of path `segments` from `M`: `op`
/// and `count` apply to the last relation (`EXISTS` for "at least one"
/// and "none", a count otherwise); outer relations of a nested path only
/// need to exist (or, for "none", not to).
pub(crate) fn has_filter<M: Model>(
    depth: u32,
    segments: &[String],
    op: Operator,
    count: u64,
    refine: Refine,
) -> Result<Where> {
    let Some((first, rest)) = segments.split_first() else {
        return Err(OrmError::invalid("an existence check needs a relation"));
    };
    let relation = registry::find::<M>(first)?;
    if let Some(morph) = relation.inner.morph_to() {
        if !rest.is_empty() {
            return Err(OrmError::invalid(format!(
                "the path `{}` continues after `{first}`, a morph_to relation of {} whose model varies: check each type with has_morph / where_has_morph",
                segments.join("."),
                M::meta().name()
            )));
        }
        let condition = refine.into_morph()?;
        return morph.has_morph(depth, None, op, count, &mut |_| condition.clone());
    }
    let (op, count, refine) = if rest.is_empty() {
        (op, count, refine)
    } else {
        let none = op == Operator::Lt && count == 1;
        let inner = Refine::Nested {
            segments: rest.to_vec(),
            op: if none { Operator::Gte } else { op },
            count: if none { 1 } else { count },
            refine: Box::new(refine),
        };
        (if none { Operator::Lt } else { Operator::Gte }, 1, inner)
    };
    let exists = count == 1 && matches!(op, Operator::Gte | Operator::Lt);
    let query = relation.inner.exists_query(ExistsRequest {
        depth,
        select: if exists {
            ExistsSelect::One
        } else {
            ExistsSelect::Count
        },
        refine,
    })?;
    Ok(if exists {
        Where::Exists {
            query: Box::new(query),
            not: op == Operator::Lt,
        }
    } else {
        Where::Compare {
            left: Expr::Subquery(Box::new(query)),
            op,
            right: Expr::Value(Value::BigInt(Some(
                i64::try_from(count).unwrap_or(i64::MAX),
            ))),
        }
    })
}

impl Refine {
    /// The refinement as a per-type one of a `morph_to` check: only a
    /// plain condition can apply to every type.
    pub(crate) fn into_morph(self) -> Result<Option<Filter>> {
        match self {
            Refine::None => Ok(None),
            Refine::Filter(filter) => Ok(Some(filter)),
            Refine::Builder(_) | Refine::Nested { .. } => Err(OrmError::invalid(
                "a morph_to relation's models vary: constrain each type with where_has_morph",
            )),
        }
    }
}

impl<M: Model> Builder<M> {
    /// Adds an existence check of `path`.
    fn push_has(
        mut self,
        boolean: Boolean,
        text: &str,
        op: Operator,
        count: u64,
        refine: impl FnOnce(u32) -> Refine,
    ) -> Self {
        let path = match ParsedPath::parse(text) {
            Ok(path) => path,
            Err(error) => return self.with_error(error.to_string()),
        };
        if path.columns.is_some() || path.alias.is_some() {
            return self.with_error(format!(
                "`{text}`: an existence check takes relation names, without a column selection or alias"
            ));
        }
        let depth = self.eager.depth;
        let nested_depth = depth + u32::try_from(path.segments.len()).unwrap_or(u32::MAX);
        let refine = refine(nested_depth);
        match has_filter::<M>(depth, &path.segments, op, count, refine) {
            Ok(filter) => {
                self.push_where(boolean, false, filter);
                self
            }
            Err(error) => self.with_error(error.to_string()),
        }
    }

    /// `closure` applied to an empty query of `T` nested `depth` levels
    /// deep, as a refinement.
    fn refined<T: Model>(
        closure: impl FnOnce(Builder<T>) -> Builder<T>,
    ) -> impl FnOnce(u32) -> Refine {
        move |depth| {
            let mut empty = Builder::<T>::new();
            empty.eager.depth = depth;
            Refine::Builder(Box::new(closure(empty)))
        }
    }

    /// Only records that have at least one related record through `path`
    /// (`"activities"`, nested `"company.owner"`, or a relation method):
    /// `EXISTS (SELECT 1 FROM activities WHERE activities.deal_id =
    /// deals.id)`.
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// let sql = Deal::query().has("activities").to_sql(Dialect::Sqlite).unwrap();
    /// assert!(sql.sql.ends_with(
    ///     r#"FROM "deals" WHERE EXISTS(SELECT 1 FROM "activities" WHERE "deals"."id" = "activities"."deal_id")"#
    /// ));
    /// ```
    pub fn has(self, path: impl RelationPath<M>) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has(Boolean::And, &text, Operator::Gte, 1, |_| Refine::None)
    }

    /// Only records with a number of related records through `path` that
    /// compares to `count` with `op`: `has_count("activities", ">=", 3)`.
    pub fn has_count(
        self,
        path: impl RelationPath<M>,
        op: impl Into<Operator>,
        count: u64,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has(Boolean::And, &text, op.into(), count, |_| Refine::None)
    }

    /// `OR` records that have related records through `path`.
    pub fn or_has(self, path: impl RelationPath<M>) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has(Boolean::Or, &text, Operator::Gte, 1, |_| Refine::None)
    }

    /// `OR` records with a number of related records through `path` that
    /// compares to `count`.
    pub fn or_has_count(
        self,
        path: impl RelationPath<M>,
        op: impl Into<Operator>,
        count: u64,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has(Boolean::Or, &text, op.into(), count, |_| Refine::None)
    }

    /// Only records without related records through `path`.
    pub fn doesnt_have(self, path: impl RelationPath<M>) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has(Boolean::And, &text, Operator::Lt, 1, |_| Refine::None)
    }

    /// `OR` records without related records through `path`.
    pub fn or_doesnt_have(self, path: impl RelationPath<M>) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has(Boolean::Or, &text, Operator::Lt, 1, |_| Refine::None)
    }

    /// Only records with related records through `path` that match
    /// `constraint` (it applies to the last relation of a nested path). A
    /// relation method fixes the closure's type; with a string path,
    /// annotate it.
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// let overdue = Deal::query()
    ///     .where_has(Deal::activities, |q| q.where_(Activity::DUE_AT.lt(Utc::now())))
    ///     .where_has("company", |q: Builder<Company>| q.where_(Company::ACTIVE.eq(true)));
    /// # let _ = overdue;
    /// ```
    pub fn where_has<T: Model>(
        self,
        path: impl RelationPathTo<M, T>,
        constraint: impl FnOnce(Builder<T>) -> Builder<T>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has(
            Boolean::And,
            &text,
            Operator::Gte,
            1,
            Self::refined(constraint),
        )
    }

    /// `OR` records with related records through `path` that match
    /// `constraint`.
    pub fn or_where_has<T: Model>(
        self,
        path: impl RelationPathTo<M, T>,
        constraint: impl FnOnce(Builder<T>) -> Builder<T>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has(
            Boolean::Or,
            &text,
            Operator::Gte,
            1,
            Self::refined(constraint),
        )
    }

    /// Only records with a number of related records through `path`
    /// matching `constraint` that compares to `count` with `op`.
    pub fn where_has_count<T: Model>(
        self,
        path: impl RelationPathTo<M, T>,
        constraint: impl FnOnce(Builder<T>) -> Builder<T>,
        op: impl Into<Operator>,
        count: u64,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has(
            Boolean::And,
            &text,
            op.into(),
            count,
            Self::refined(constraint),
        )
    }

    /// Only records without related records through `path` that match
    /// `constraint`.
    pub fn where_doesnt_have<T: Model>(
        self,
        path: impl RelationPathTo<M, T>,
        constraint: impl FnOnce(Builder<T>) -> Builder<T>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has(
            Boolean::And,
            &text,
            Operator::Lt,
            1,
            Self::refined(constraint),
        )
    }

    /// `OR` records without related records through `path` that match
    /// `constraint`.
    pub fn or_where_doesnt_have<T: Model>(
        self,
        path: impl RelationPathTo<M, T>,
        constraint: impl FnOnce(Builder<T>) -> Builder<T>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_has(
            Boolean::Or,
            &text,
            Operator::Lt,
            1,
            Self::refined(constraint),
        )
    }

    /// Only records with related records through `path` matching
    /// `constraint`, and those related records eager loaded (constrained
    /// the same way).
    pub fn with_where_has<T: Model>(
        self,
        path: impl RelationPathTo<M, T>,
        constraint: impl Fn(Builder<T>) -> Builder<T> + Clone + Send + Sync + 'static,
    ) -> Self {
        let text = path.relation_path().into_owned();
        let eager = constraint.clone();
        self.push_has(
            Boolean::And,
            &text,
            Operator::Gte,
            1,
            Self::refined(constraint),
        )
        .with_path(&text, Some(crate::relations::eager::Constraint::new(eager)))
    }

    /// Only records with a related record through `path` matching
    /// `filter`: `where_relation("company", Company::NAME.eq("Acme"))`.
    pub fn where_relation(self, path: impl RelationPath<M>, filter: impl Into<Filter>) -> Self {
        let text = path.relation_path().into_owned();
        let filter = filter.into();
        self.push_has(Boolean::And, &text, Operator::Gte, 1, |_| {
            Refine::Filter(filter)
        })
    }

    /// `OR` records with a related record through `path` matching `filter`.
    pub fn or_where_relation(self, path: impl RelationPath<M>, filter: impl Into<Filter>) -> Self {
        let text = path.relation_path().into_owned();
        let filter = filter.into();
        self.push_has(Boolean::Or, &text, Operator::Gte, 1, |_| {
            Refine::Filter(filter)
        })
    }

    /// Only records with a related record through `path` matching
    /// `filter`, and those related records eager loaded.
    pub fn with_where_relation(
        self,
        path: impl RelationPath<M>,
        filter: impl Into<Filter>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        let filter = filter.into();
        let eager = filter.clone();
        self.push_has(Boolean::And, &text, Operator::Gte, 1, |_| {
            Refine::Filter(filter)
        })
        .with_path(&text, Some(eager_filter(eager)))
    }

    /// Only records without a related record through `path` matching
    /// `filter`.
    pub fn where_doesnt_have_relation(
        self,
        path: impl RelationPath<M>,
        filter: impl Into<Filter>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        let filter = filter.into();
        self.push_has(Boolean::And, &text, Operator::Lt, 1, |_| {
            Refine::Filter(filter)
        })
    }

    /// `OR` records without a related record through `path` matching
    /// `filter`.
    pub fn or_where_doesnt_have_relation(
        self,
        path: impl RelationPath<M>,
        filter: impl Into<Filter>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        let filter = filter.into();
        self.push_has(Boolean::Or, &text, Operator::Lt, 1, |_| {
            Refine::Filter(filter)
        })
    }

    /// Only records that belong to `owner`, through the `belongs_to`
    /// relation to `owner`'s model: the one named after the model
    /// (`company` for a `Company`), else the only one (an error names the
    /// candidates when there are several; pick one with
    /// [`where_belongs_to_named`](Builder::where_belongs_to_named)).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db, acme: Record<Company>) -> Result<()> {
    /// let deals = Deal::query().where_belongs_to(&acme).get(&db).await?;
    /// # Ok(()) }
    /// ```
    pub fn where_belongs_to<R: Model>(self, owner: &Record<R>) -> Self {
        self.push_belongs_to_default(Boolean::And, std::slice::from_ref(owner))
    }

    fn push_belongs_to_default<R: Model>(self, boolean: Boolean, owners: &[Record<R>]) -> Self {
        match relation_to::<M, R>(
            &snake_case(R::meta().name()),
            &[RelationKind::BelongsTo],
            "where_belongs_to",
        ) {
            Ok(name) => self.push_belongs_to(boolean, &name, owners),
            Err(error) => self.with_error(error.to_string()),
        }
    }

    /// Only records that belong to `owner` through relation `path` (a
    /// `belongs_to`).
    pub fn where_belongs_to_named<R: Model>(
        self,
        owner: &Record<R>,
        path: impl RelationPath<M>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_belongs_to(Boolean::And, &text, std::slice::from_ref(owner))
    }

    /// Only records that belong to one of `owners` (through the relation
    /// [`where_belongs_to`](Builder::where_belongs_to) picks).
    pub fn where_belongs_to_any<R: Model>(self, owners: &Collection<R>) -> Self {
        self.push_belongs_to_default(Boolean::And, owners)
    }

    /// `OR` records that belong to `owner`.
    pub fn or_where_belongs_to<R: Model>(self, owner: &Record<R>) -> Self {
        self.push_belongs_to_default(Boolean::Or, std::slice::from_ref(owner))
    }

    fn push_belongs_to<R: Model>(
        mut self,
        boolean: Boolean,
        name: &str,
        owners: &[Record<R>],
    ) -> Self {
        match belongs_to_filter::<M, R>(name, owners) {
            Ok(filter) => {
                self.push_where(boolean, false, filter);
                self
            }
            Err(error) => self.with_error(error.to_string()),
        }
    }

    /// Only records attached to `related` (many to many), through the
    /// many-to-many relation to `related`'s model: the one named after its
    /// table (`tags` for a `Tag`), else the only one (pick one with
    /// [`where_attached_to_named`](Builder::where_attached_to_named)).
    pub fn where_attached_to<R: Model>(self, related: &Record<R>) -> Self {
        self.push_attached_default(Boolean::And, std::slice::from_ref(related))
    }

    fn push_attached_default<R: Model>(self, boolean: Boolean, related: &[Record<R>]) -> Self {
        match relation_to::<M, R>(
            R::meta().table(),
            &[
                RelationKind::BelongsToMany,
                RelationKind::MorphToMany,
                RelationKind::MorphedByMany,
            ],
            "where_attached_to",
        ) {
            Ok(name) => self.push_attached(boolean, &name, related),
            Err(error) => self.with_error(error.to_string()),
        }
    }

    /// Only records attached to `related` through relation `path`.
    pub fn where_attached_to_named<R: Model>(
        self,
        related: &Record<R>,
        path: impl RelationPath<M>,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.push_attached(Boolean::And, &text, std::slice::from_ref(related))
    }

    /// Only records attached to one of `related`.
    pub fn where_attached_to_any<R: Model>(self, related: &Collection<R>) -> Self {
        self.push_attached_default(Boolean::And, related)
    }

    /// `OR` records attached to `related`.
    pub fn or_where_attached_to<R: Model>(self, related: &Record<R>) -> Self {
        self.push_attached_default(Boolean::Or, std::slice::from_ref(related))
    }

    fn push_attached<R: Model>(self, boolean: Boolean, name: &str, related: &[Record<R>]) -> Self {
        let kind = M::relation(name).map(|relation| relation.kind());
        if !matches!(
            kind,
            Some(
                RelationKind::BelongsToMany
                    | RelationKind::MorphToMany
                    | RelationKind::MorphedByMany
            )
        ) {
            let problem = match kind {
                Some(kind) => format!("is a {kind}"),
                None => "does not exist".to_owned(),
            };
            return self.with_error(format!(
                "where_attached_to needs a many-to-many relation of {} to {}: `{name}` {problem}",
                M::meta().name(),
                R::meta().name()
            ));
        }
        if related.is_empty() {
            return self.with_error(format!(
                "where_attached_to_any needs at least one {}",
                R::meta().name()
            ));
        }
        let keys: Vec<R::Key> = related.iter().map(Record::key).collect();
        self.push_has(boolean, name, Operator::Gte, 1, move |depth| {
            let mut query = Builder::<R>::new();
            query.eager.depth = depth;
            Refine::Builder(Box::new(query.where_key_in(keys)))
        })
    }

    /// Adds the conditions of `other` (a query of the same model) to this
    /// one, grouped when they contain an `OR`, and leaves out the global
    /// scopes `other` leaves out.
    pub fn merge_constraints_from(self, other: &Builder<M>) -> Self {
        merge_refined(self, other.clone())
    }
}

/// An eager constraint adding `filter`.
fn eager_filter(filter: Filter) -> crate::relations::eager::Constraint {
    crate::relations::eager::Constraint::new_filter(filter)
}

/// The relation of `M` to `R` of one of `kinds` that `method` uses without
/// a name: `preferred` when it is one, else the only one.
fn relation_to<M: Model, R: Model>(
    preferred: &str,
    kinds: &[RelationKind],
    method: &str,
) -> Result<String> {
    let fits = |name: &str| {
        M::relation_meta(name).is_some_and(|meta| {
            kinds.contains(&meta.kind) && meta.related == Some(R::meta().name())
        })
    };
    if fits(preferred) {
        return Ok(preferred.to_owned());
    }
    let candidates: Vec<String> = M::relation_names()
        .into_iter()
        .filter(|name| fits(name))
        .map(Cow::into_owned)
        .collect();
    let kind = kinds[0].name();
    match candidates.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(OrmError::invalid(format!(
            "{method} needs a {kind} relation of {} to {}, and {} has none",
            M::meta().name(),
            R::meta().name(),
            M::meta().name()
        ))),
        several => Err(OrmError::invalid(format!(
            "{method} found several {kind} relations of {} to {} ({}): name one with {method}_named",
            M::meta().name(),
            R::meta().name(),
            several
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// `foreign_key IN (owner keys)` for `belongs_to` relation `name` of `M`
/// pointing at `R`.
fn belongs_to_filter<M: Model, R: Model>(name: &str, owners: &[Record<R>]) -> Result<Where> {
    let relation = registry::find::<M>(name)?;
    let meta = relation.meta();
    if meta.kind != RelationKind::BelongsTo || meta.related != Some(R::meta().name()) {
        return Err(OrmError::invalid(format!(
            "where_belongs_to needs a belongs_to relation of {} to {}: `{name}` is a {} to {}",
            M::meta().name(),
            R::meta().name(),
            meta.kind,
            meta.related.unwrap_or("models of several types"),
        )));
    }
    if owners.is_empty() {
        return Err(OrmError::invalid(format!(
            "where_belongs_to_any needs at least one {}",
            R::meta().name()
        )));
    }
    let (Some(foreign_key), Some(owner_key)) = (meta.foreign_key, meta.owner_key) else {
        return Err(OrmError::invalid(format!(
            "the relation `{name}` of {} has no keys",
            M::meta().name()
        )));
    };
    let values = owners
        .iter()
        .map(|owner| crate::relations::core::column_value(&owner.model, &owner_key, name))
        .collect::<Result<Vec<_>>>()?;
    let keys = crate::relations::keys::distinct_keys(values);
    Ok(crate::relations::keys::in_keys(
        ColumnRef::new(Some(M::meta().table()), foreign_key),
        &keys,
    ))
}
