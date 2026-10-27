//! Eager loading: the relations a query loads into the records it reads,
//! one batched query per relation and level (`with("company.owner")`),
//! with constraints, column selections and per-type loads of `morph_to`
//! relations.

use std::any::Any;
use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use crate::column::Filter;
use crate::error::{OrmError, Result};
use crate::exec::Target;
use crate::ir::{ColumnRef, Expr, SelectItem};
use crate::model::Model;
use crate::query::Builder;
use crate::record::Record;
use crate::relations::access::AccessConstraint;
use crate::relations::core::RelationCore;
use crate::relations::morph::MorphWith;
use crate::relations::path::{ConstrainPath, ParsedPath, RelationPath};
use crate::relations::registry;

/// A constraint closure of an eager load, its type erased: a
/// `ConstraintFn<Q>` where `Q` is the relation type or a `Builder` of the
/// related model.
#[derive(Clone)]
pub(crate) struct Constraint {
    pub(crate) closure: Arc<dyn Any + Send + Sync>,
    /// What the closure takes, for error messages.
    pub(crate) takes: &'static str,
}

impl fmt::Debug for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Constraint").field(&self.takes).finish()
    }
}

/// A constraint closure on `Q`.
pub(crate) struct ConstraintFn<Q>(pub(crate) Box<dyn Fn(Q) -> Q + Send + Sync>);

/// A constraint adding a condition to the related query, whatever its
/// model (`with_where_relation`).
pub(crate) struct FilterConstraint(pub(crate) Filter);

impl Constraint {
    /// `closure`, constraining a `Q`.
    pub(crate) fn new<Q: 'static>(closure: impl Fn(Q) -> Q + Send + Sync + 'static) -> Self {
        Constraint {
            closure: Arc::new(ConstraintFn::<Q>(Box::new(closure))),
            takes: std::any::type_name::<Q>(),
        }
    }

    /// A constraint adding `filter` to the related query.
    pub(crate) fn new_filter(filter: Filter) -> Self {
        Constraint {
            closure: Arc::new(FilterConstraint(filter)),
            takes: "a condition",
        }
    }

    /// The condition of a filter constraint.
    pub(crate) fn filter(&self) -> Option<&Filter> {
        self.closure
            .downcast_ref::<FilterConstraint>()
            .map(|constraint| &constraint.0)
    }

    /// A constraint keeping the related records `access` lets a viewer
    /// read (`with_accessible`).
    pub(crate) fn new_access(access: AccessConstraint) -> Self {
        Constraint {
            closure: Arc::new(access),
            takes: "an access filter",
        }
    }

    /// The access filter of an access constraint.
    pub(crate) fn access(&self) -> Option<&AccessConstraint> {
        self.closure.downcast_ref::<AccessConstraint>()
    }
}

/// A relation to load, and what to load under it.
#[derive(Clone, Default)]
pub struct EagerNode {
    /// The relation's name.
    pub(crate) name: String,
    /// Constraints on its query, in the order given.
    pub(crate) constraints: Vec<Constraint>,
    /// The columns to select (`"company:id,name"`).
    pub(crate) columns: Option<Vec<String>>,
    /// Relations to load into the related records.
    pub(crate) children: EagerTree,
    /// Per-type loads of a `morph_to` relation.
    pub(crate) morph: Option<MorphWith>,
    /// Give the loaded records an automatic-loading group (records of a
    /// group load a relation for all of them at once).
    pub(crate) autoload: bool,
}

impl fmt::Debug for EagerNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("EagerNode");
        debug.field("name", &self.name);
        if !self.constraints.is_empty() {
            let takes: Vec<&str> = self.constraints.iter().map(|c| c.takes).collect();
            debug.field("constraints", &takes);
        }
        if let Some(columns) = &self.columns {
            debug.field("columns", columns);
        }
        if !self.children.nodes.is_empty() {
            debug.field("children", &self.children);
        }
        debug.finish()
    }
}

/// The relations a query loads, by name, in the order asked for.
#[derive(Clone, Debug, Default)]
pub struct EagerTree {
    pub(crate) nodes: Vec<EagerNode>,
}

impl EagerTree {
    /// The node at `segments` (created, with its ancestors, when missing).
    pub(crate) fn entry(&mut self, segments: &[String]) -> &mut EagerNode {
        let (first, rest) = segments
            .split_first()
            .expect("a relation path has at least one segment");
        let index = match self.nodes.iter().position(|node| node.name == *first) {
            Some(index) => index,
            None => {
                self.nodes.push(EagerNode {
                    name: first.clone(),
                    ..EagerNode::default()
                });
                self.nodes.len() - 1
            }
        };
        let node = &mut self.nodes[index];
        if rest.is_empty() {
            node
        } else {
            node.children.entry(rest)
        }
    }

    /// Removes the node at `segments` (and what is loaded under it).
    pub(crate) fn remove(&mut self, segments: &[String]) {
        let Some((first, rest)) = segments.split_first() else {
            return;
        };
        if rest.is_empty() {
            self.nodes.retain(|node| node.name != *first);
        } else if let Some(node) = self.nodes.iter_mut().find(|node| node.name == *first) {
            node.children.remove(rest);
        }
    }

    /// Adds `other`'s loads (its constraints after this tree's).
    pub(crate) fn merge(&mut self, other: &EagerTree) {
        for node in &other.nodes {
            let entry = self.entry(std::slice::from_ref(&node.name));
            entry.constraints.extend(node.constraints.iter().cloned());
            if node.columns.is_some() {
                entry.columns.clone_from(&node.columns);
            }
            if let Some(morph) = &node.morph {
                match &mut entry.morph {
                    Some(existing) => existing.merge(morph),
                    None => entry.morph = Some(morph.clone()),
                }
            }
            entry.autoload |= node.autoload;
            entry.children.merge(&node.children);
        }
    }

    /// The dotted paths of every node, depth first.
    pub(crate) fn paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        for node in &self.nodes {
            paths.push(node.name.clone());
            for child in node.children.paths() {
                paths.push(format!("{}.{child}", node.name));
            }
        }
        paths
    }

    /// Whether nothing is loaded.
    pub(crate) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// What a model query loads besides its rows: the eager-load tree, the
/// relation counts added by the model's defaults, and how deep the query
/// is nested in existence subqueries (for table aliases).
#[derive(Clone, Debug, Default)]
pub(crate) struct EagerState {
    pub(crate) tree: EagerTree,
    /// `(relation, alias)` of the counts `#[model(with_count)]` added.
    pub(crate) default_counts: Vec<(String, String)>,
    /// A relation's query: the model's default counts are added when it
    /// reads records (adding them up front would resolve relations while
    /// relations are being defined).
    pub(crate) pending_default_counts: bool,
    /// Default counts `without` took off before they were added.
    pub(crate) skipped_default_counts: Vec<String>,
    /// How many existence subqueries this query is nested in.
    pub(crate) depth: u32,
}

impl<M: Model> Builder<M> {
    /// Eager loads relation `path` into the records the query reads: one
    /// batched query per relation and level, whatever the number of
    /// records (`"company"`, nested `"company.owner"`, with a column
    /// selection `"company:id,name"`, or a relation method,
    /// `Deal::company`).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let deals = Deal::query().with("company").with("activities").get(&db).await?;
    /// let company = deals[0].relation::<Option<Record<Company>>>("company");
    /// # Ok(()) }
    /// ```
    pub fn with(self, path: impl RelationPath<M>) -> Self {
        let text = path.relation_path().into_owned();
        self.with_path(&text, None)
    }

    /// Eager loads every relation path of `paths`.
    pub fn with_all(self, paths: impl IntoIterator<Item = impl RelationPath<M>>) -> Self {
        paths.into_iter().fold(self, |query, path| query.with(path))
    }

    /// Eager loads relation `path`, constrained by `constraint`. The
    /// closure takes the relation (`HasMany<Deal, Activity>`: pivot
    /// conditions and every builder method) or a `Builder` of the related
    /// model; a relation method fixes the type, a string path needs the
    /// closure's parameter annotated. A limit applies to each parent's
    /// records (the latest three activities of every deal).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let deals = Deal::query()
    ///     .with_constrained(Deal::activities, |q| q.latest().limit(3))
    ///     .with_constrained("company", |q: Builder<Company>| q.where_(Company::ACTIVE.eq(true)))
    ///     .get(&db)
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub fn with_constrained<Q: 'static>(
        self,
        path: impl ConstrainPath<M, Q>,
        constraint: impl Fn(Q) -> Q + Send + Sync + 'static,
    ) -> Self {
        let text = path.relation_path().into_owned();
        self.with_path(&text, Some(Constraint::new(constraint)))
    }

    /// Adds `path` to the eager loads, with `constraint` on its last
    /// relation.
    pub(crate) fn with_path(mut self, text: &str, constraint: Option<Constraint>) -> Self {
        match ParsedPath::parse(text) {
            Ok(path) => {
                let node = self.eager.tree.entry(&path.segments);
                if let Some(columns) = path.columns {
                    node.columns = Some(columns);
                }
                if let Some(constraint) = constraint {
                    node.constraints.push(constraint);
                }
                self
            }
            Err(error) => self.with_error(error.to_string()),
        }
    }

    /// Stops eager loading relation `path` (and what loads under it); a
    /// default eager load or count of the model is removed too.
    pub fn without(mut self, path: impl RelationPath<M>) -> Self {
        let text = path.relation_path().into_owned();
        match ParsedPath::parse(&text) {
            Ok(path) => {
                self.eager.tree.remove(&path.segments);
                if path.segments.len() == 1 {
                    self.remove_default_count(&path.segments[0]);
                    if self.eager.pending_default_counts {
                        self.eager
                            .skipped_default_counts
                            .push(path.segments[0].clone());
                    }
                }
                self
            }
            Err(error) => self.with_error(error.to_string()),
        }
    }

    /// Stops eager loading every relation path of `paths`.
    pub fn without_eager_load(self, paths: impl IntoIterator<Item = impl RelationPath<M>>) -> Self {
        paths
            .into_iter()
            .fold(self, |query, path| query.without(path))
    }

    /// Eager loads exactly `paths`, replacing the loads asked for so far
    /// (the model's defaults included).
    pub fn with_only(self, paths: impl IntoIterator<Item = impl RelationPath<M>>) -> Self {
        self.without_eager_loads().with_all(paths)
    }

    /// Eager loads nothing: removes every eager load, and the model's
    /// default relation counts.
    pub fn without_eager_loads(mut self) -> Self {
        self.eager.tree = EagerTree::default();
        self.eager.pending_default_counts = false;
        let counts: Vec<String> = self
            .eager
            .default_counts
            .iter()
            .map(|(relation, _)| relation.clone())
            .collect();
        for relation in counts {
            self.remove_default_count(&relation);
        }
        self
    }

    /// The relation paths the query eager loads, dotted, depth first.
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// let query = Deal::query().with("company").with("activities");
    /// assert_eq!(query.eager_loads(), ["company", "activities"]);
    /// ```
    pub fn eager_loads(&self) -> Vec<String> {
        self.eager.tree.paths()
    }

    /// The model's default eager loads and relation counts
    /// (`#[model(with = [..], with_count = [..])]`); `Model::query`
    /// applies them.
    pub fn with_model_defaults(self) -> Self {
        let meta = M::meta();
        let mut query = meta
            .default_with()
            .iter()
            .fold(self, |query, path| query.with(*path));
        for relation in meta.default_with_count() {
            let alias = format!("{relation}_count");
            query = query.with_count(*relation);
            query
                .eager
                .default_counts
                .push(((*relation).to_owned(), alias));
        }
        query
    }

    /// What a relation's query of `M` starts with: the model's default eager
    /// loads now, its default counts when the query reads records.
    pub(crate) fn with_relation_defaults(self) -> Self {
        let meta = M::meta();
        let mut query = meta
            .default_with()
            .iter()
            .fold(self, |query, path| query.with(*path));
        query.eager.pending_default_counts = !meta.default_with_count().is_empty();
        query
    }

    /// Adds the model's default counts a relation's query left for when it
    /// reads records (but the ones `without` took off).
    pub(crate) fn with_pending_default_counts(mut self) -> Self {
        if !self.eager.pending_default_counts {
            return self;
        }
        self.eager.pending_default_counts = false;
        let skipped = std::mem::take(&mut self.eager.skipped_default_counts);
        for relation in M::meta().default_with_count() {
            if skipped.iter().any(|name| name == relation) {
                continue;
            }
            self = self.with_count(*relation);
            self.eager
                .default_counts
                .push(((*relation).to_owned(), format!("{relation}_count")));
        }
        self
    }

    /// Removes the default count of `relation` from the select list.
    fn remove_default_count(&mut self, relation: &str) {
        let Some(index) = self
            .eager
            .default_counts
            .iter()
            .position(|(counted, _)| counted == relation)
        else {
            return;
        };
        let (_, alias) = self.eager.default_counts.remove(index);
        self.ir
            .columns
            .retain(|item| item.alias.as_deref() != Some(alias.as_str()));
    }

    /// Takes the eager-load tree off the query (`fetch` loads it after
    /// reading the rows).
    pub(crate) fn take_eager(&mut self) -> EagerTree {
        std::mem::take(&mut self.eager.tree)
    }
}

/// After a query read `records`: marks them for the lazy-loading rules and
/// loads `tree` into them.
pub(crate) async fn after_fetch<M: Model>(
    target: &Target<'_>,
    records: &mut [Record<M>],
    tree: &EagerTree,
) -> Result<()> {
    crate::relations::lazy::mark_result(records);
    load_tree(target, records, tree).await
}

/// Loads every relation of `tree` into `records`. The names of the first
/// level are checked even when there are no records (a misspelled
/// relation fails whatever the data); deeper names when their level loads.
pub(crate) async fn load_tree<M: Model>(
    target: &Target<'_>,
    records: &mut [Record<M>],
    tree: &EagerTree,
) -> Result<()> {
    let relations = tree
        .nodes
        .iter()
        .map(|node| registry::find::<M>(&node.name))
        .collect::<Result<Vec<_>>>()?;
    if records.is_empty() {
        return Ok(());
    }
    for (relation, node) in relations.iter().zip(&tree.nodes) {
        relation.inner.eager_load(target, records, node).await?;
    }
    Ok(())
}

/// A relation type whose query the generic eager-load and existence code
/// can constrain.
pub(crate) trait RelationParts<P: Model, R: Model>: Sized + 'static {
    /// The relation's shared state: its query of the related model.
    fn core_mut(&mut self) -> &mut RelationCore<P, R>;
}

/// `relation` with `node`'s constraints applied: closures on the relation
/// type, or on a `Builder` of the related model.
pub(crate) fn constrained<P, R, Rel>(mut relation: Rel, node: &EagerNode) -> Result<Rel>
where
    P: Model,
    R: Model,
    Rel: RelationParts<P, R>,
{
    for constraint in &node.constraints {
        if let Some(filter) = constraint.filter() {
            let query = std::mem::take(&mut relation.core_mut().query);
            relation.core_mut().query = query.where_(filter.clone());
        } else if let Some(access) = constraint.access() {
            let query = std::mem::take(&mut relation.core_mut().query);
            relation.core_mut().query = access.apply(query)?;
        } else if let Some(apply) = constraint.closure.downcast_ref::<ConstraintFn<Rel>>() {
            relation = (apply.0)(relation);
        } else if let Some(apply) = constraint
            .closure
            .downcast_ref::<ConstraintFn<Builder<R>>>()
        {
            let query = std::mem::take(&mut relation.core_mut().query);
            relation.core_mut().query = (apply.0)(query);
        } else {
            return Err(OrmError::invalid(format!(
                "the constraint on relation `{}` of {} takes `{}`, but the relation constrains `{}` or `{}`",
                node.name,
                P::meta().name(),
                constraint.takes,
                std::any::type_name::<Rel>(),
                std::any::type_name::<Builder<R>>(),
            )));
        }
    }
    Ok(relation)
}

/// The related query of an eager load: `query` (the relation's own, after
/// the constraints) with `node`'s column selection and nested loads.
/// `required` are the columns matching needs; a selection without them is
/// an error naming them. `qualifier` qualifies selected columns.
pub(crate) fn prepare_query<R: Model>(
    mut query: Builder<R>,
    node: &EagerNode,
    qualifier: &str,
    required: &[&str],
) -> Result<Builder<R>> {
    if let Some(columns) = &node.columns {
        let missing: Vec<&str> = required
            .iter()
            .filter(|column| !columns.iter().any(|selected| selected == *column))
            .copied()
            .collect();
        if !missing.is_empty() {
            return Err(OrmError::invalid(format!(
                "eager loading `{}` matches records by {}, so the selection `{}` must include {}",
                node.name,
                missing
                    .iter()
                    .map(|column| format!("`{column}`"))
                    .collect::<Vec<_>>()
                    .join(" and "),
                columns.join(","),
                if missing.len() == 1 { "it" } else { "them" }
            )));
        }
        query.ir.columns = columns
            .iter()
            .map(|column| SelectItem {
                expr: Expr::Column(ColumnRef::new(
                    Some(Cow::Owned(qualifier.to_owned())),
                    Cow::Owned(column.clone()),
                )),
                alias: None,
            })
            .collect();
    }
    query.eager.tree.merge(&node.children);
    Ok(query)
}
