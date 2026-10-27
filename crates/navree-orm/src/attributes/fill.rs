//! Mass assignment: setting many columns from untyped input ([`Attributes`],
//! a request body), guarded by the model's fillable and guarded columns.
//!
//! ```
//! use navree_orm::prelude::*;
//! use serde::Serialize;
//!
//! #[derive(Model, Clone, Debug, Default, Serialize)]
//! #[model(table = "deals", fillable(title, amount_cents))]
//! struct Deal {
//!     id: i64,
//!     title: String,
//!     amount_cents: i64,
//!     owner_id: Option<i64>,
//! }
//!
//! let mut deal = Record::new(Deal::default());
//! deal.fill(attrs! { "title" => "Renewal", "amount_cents" => 12_000, "owner_id" => 7 })?;
//! assert_eq!((deal.title.as_str(), deal.amount_cents), ("Renewal", 12_000));
//! assert_eq!(deal.owner_id, None, "not fillable: left alone");
//!
//! deal.force_fill(attrs! { "owner_id" => 7 })?;
//! assert_eq!(deal.owner_id, Some(7));
//! # Ok::<(), navree_orm::OrmError>(())
//! ```
//!
//! The rules:
//!
//! - Only real columns can be filled: unknown keys and JSON paths into
//!   columns that are not JSON never reach the model, let alone SQL.
//! - `#[model(fillable(..))]` lists what `fill` may set. Without it,
//!   `#[model(guarded(..))]` lists what it may not set, and every other
//!   column is fillable. A model that declares neither is *totally guarded*:
//!   `fill` is an error naming the model; `force_fill` still works.
//! - Values are converted to each field's type the way untyped input is
//!   (numbers, numeric strings, RFC 3339 text...), through the field's cast
//!   (an encrypted field takes the plain text) and mutator
//!   (`#[model(set_with = ..)]`).
//! - Keys `fill` may not set are left out silently, unless the process or
//!   the task refuses to discard them ([`set_discard_policy`],
//!   [`with_discard_policy`]): then `fill` fails with
//!   [`OrmError::MassAssignment`] before changing anything.
//! - [`unguarded`] turns guarding off for a task; `#[model(unguarded)]` for
//!   a model.

use std::future::Future;
use std::sync::{Arc, RwLock};

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, Serializer};

use crate::attributes::CastContext;
use crate::column::ColumnName;
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor};
use crate::model::{Model, ModelMeta};
use crate::query::Builder;
use crate::record::Record;

/// Untyped column values for mass assignment, in order: what `fill`,
/// `force_fill`, `create_from` and `update_attributes` take. Keys are
/// column names, or JSON paths into JSON columns (`"meta->status"`); values
/// are JSON.
///
/// Build them with [`attrs!`](crate::attrs), from a JSON object
/// ([`from_json`](Attributes::from_json), or as a request body: it
/// deserializes from one), or from any serializable struct
/// ([`from_serialize`](Attributes::from_serialize)).
///
/// ```
/// use navree_orm::attributes::Attributes;
/// use serde_json::json;
///
/// let attributes = Attributes::from_json(json!({"title": "Renewal", "amount_cents": 1200}))?;
/// assert_eq!(attributes.get("title"), Some(&json!("Renewal")));
/// assert_eq!(attributes.keys().collect::<Vec<_>>(), ["title", "amount_cents"]);
/// # Ok::<(), navree_orm::OrmError>(())
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attributes {
    entries: Vec<(String, serde_json::Value)>,
}

impl Attributes {
    /// No values.
    pub fn new() -> Self {
        Attributes::default()
    }

    /// The values of `pairs` (a later key replaces an earlier one).
    pub fn from_pairs<K: Into<String>>(
        pairs: impl IntoIterator<Item = (K, serde_json::Value)>,
    ) -> Self {
        pairs.into_iter().collect()
    }

    /// The members of a JSON object; anything else is an error.
    pub fn from_json(json: serde_json::Value) -> Result<Self> {
        match json {
            serde_json::Value::Object(object) => Ok(object.into_iter().collect()),
            other => Err(OrmError::InvalidQuery(format!(
                "mass assignment takes a JSON object, not {}",
                json_kind(&other)
            ))),
        }
    }

    /// The fields of any serializable value that serializes as a map (a
    /// typed request struct): `Option` fields serialized as `null` stay,
    /// fields serde skips are left out.
    pub fn from_serialize<T: Serialize + ?Sized>(value: &T) -> Result<Self> {
        let json = serde_json::to_value(value).map_err(|error| {
            OrmError::InvalidQuery(format!("the attributes do not serialize to JSON: {error}"))
        })?;
        Attributes::from_json(json)
    }

    /// The values with `key` set to `value`.
    pub fn with(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.insert(key, value);
        self
    }

    /// Sets `key` to `value`, in place when it is already present; returns
    /// the value it replaced.
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Option<serde_json::Value> {
        let key = key.into();
        let value = value.into();
        match self
            .entries
            .iter_mut()
            .find(|(existing, _)| *existing == key)
        {
            Some(entry) => Some(std::mem::replace(&mut entry.1, value)),
            None => {
                self.entries.push((key, value));
                None
            }
        }
    }

    /// The value of `key`.
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.entries
            .iter()
            .find(|(existing, _)| existing == key)
            .map(|(_, value)| value)
    }

    /// Removes `key`; returns its value.
    pub fn remove(&mut self, key: &str) -> Option<serde_json::Value> {
        let index = self
            .entries
            .iter()
            .position(|(existing, _)| existing == key)?;
        Some(self.entries.remove(index).1)
    }

    /// Whether `key` is present.
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// The keys, in order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(key, _)| key.as_str())
    }

    /// The keys and values, in order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &serde_json::Value)> {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value))
    }

    /// How many values.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn json_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

impl<K: Into<String>> FromIterator<(K, serde_json::Value)> for Attributes {
    fn from_iter<I: IntoIterator<Item = (K, serde_json::Value)>>(iter: I) -> Self {
        let mut attributes = Attributes::new();
        attributes.extend(iter);
        attributes
    }
}

impl<K: Into<String>> Extend<(K, serde_json::Value)> for Attributes {
    fn extend<I: IntoIterator<Item = (K, serde_json::Value)>>(&mut self, iter: I) {
        for (key, value) in iter {
            self.insert(key, value);
        }
    }
}

impl IntoIterator for Attributes {
    type Item = (String, serde_json::Value);
    type IntoIter = std::vec::IntoIter<(String, serde_json::Value)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl From<serde_json::Map<String, serde_json::Value>> for Attributes {
    fn from(object: serde_json::Map<String, serde_json::Value>) -> Self {
        object.into_iter().collect()
    }
}

impl Serialize for Attributes {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (key, value) in &self.entries {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// A JSON object, so handlers can take `Json<Attributes>` bodies.
impl<'de> Deserialize<'de> for Attributes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct AttributesVisitor;

        impl<'de> Visitor<'de> for AttributesVisitor {
            type Value = Attributes;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object of column values")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Attributes, A::Error> {
                let mut attributes = Attributes::new();
                while let Some((key, value)) = map.next_entry::<String, serde_json::Value>()? {
                    attributes.insert(key, value);
                }
                Ok(attributes)
            }
        }

        deserializer.deserialize_map(AttributesVisitor)
    }
}

/// [`Attributes`] from `"column" => value` pairs. Keys are string
/// literals; each value is what `serde_json::json!` takes: an expression
/// of any serializable type, `null`, or JSON written inline.
///
/// ```
/// use navree_orm::attrs;
///
/// let owner = 7;
/// let attributes = attrs! {
///     "title" => "Renewal",
///     "owner_id" => owner,
///     "closed_at" => null,
///     "meta" => { "source": "partner", "tags": ["a", "b"] },
/// };
/// assert_eq!(attributes.len(), 4);
/// assert_eq!(attributes.get("closed_at"), Some(&serde_json::Value::Null));
/// assert_eq!(attributes.get("meta"), Some(&serde_json::json!({"source": "partner", "tags": ["a", "b"]})));
/// ```
#[macro_export]
macro_rules! attrs {
    () => {
        $crate::attributes::Attributes::new()
    };
    (@pairs [$($pairs:tt)*]) => {
        $crate::attributes::Attributes::from_pairs([$($pairs)*])
    };
    (@pairs [$($pairs:tt)*] $key:literal => $($rest:tt)*) => {
        $crate::attrs!(@value [$($pairs)*] $key [] $($rest)*)
    };
    (@value [$($pairs:tt)*] $key:literal [$($value:tt)+] , $($rest:tt)*) => {
        $crate::attrs!(@pairs [$($pairs)* ($key, $crate::__private::serde_json::json!($($value)+)),] $($rest)*)
    };
    (@value [$($pairs:tt)*] $key:literal [$($value:tt)*] $next:tt $($rest:tt)*) => {
        $crate::attrs!(@value [$($pairs)*] $key [$($value)* $next] $($rest)*)
    };
    (@value [$($pairs:tt)*] $key:literal [$($value:tt)+]) => {
        $crate::attrs!(@pairs [$($pairs)* ($key, $crate::__private::serde_json::json!($($value)+)),])
    };
    ($($tokens:tt)+) => {
        $crate::attrs!(@pairs [] $($tokens)+)
    };
}

// -------------------------------------------------------------------------
// Guarding
// -------------------------------------------------------------------------

/// `key` split into its column and the JSON path inside it
/// (`"meta->status"` is `("meta", Some("status"))`).
pub(crate) fn split_key(key: &str) -> (&str, Option<&str>) {
    match key.split_once("->") {
        Some((column, path)) => (column.trim(), Some(path.trim())),
        None => (key, None),
    }
}

/// Whether `key` names a real column, or a path into a JSON column.
fn is_assignable(meta: &ModelMeta, key: &str) -> bool {
    let (column, path) = split_key(key);
    match meta.column(column) {
        Some(found) => path.is_none() || (found.is_json() && !path.is_some_and(str::is_empty)),
        None => false,
    }
}

fn listed(list: &[&str], key: &str) -> bool {
    let (column, path) = split_key(key);
    list.iter()
        .any(|name| *name == key || (path.is_some() && *name == column))
}

/// Whether `key` is guarded against mass assignment for `meta`.
pub(crate) fn is_guarded(meta: &ModelMeta, key: &str, unguarded: bool) -> bool {
    if unguarded || meta.is_unguarded() {
        return false;
    }
    match meta.guarded_columns() {
        None => true,
        Some([]) => !is_assignable(meta, key),
        Some(list) => listed(list, key) || !is_assignable(meta, key),
    }
}

/// Whether `fill` may set `key` on `meta`'s model.
pub(crate) fn is_fillable(meta: &ModelMeta, key: &str, unguarded: bool) -> bool {
    if !is_assignable(meta, key) {
        return false;
    }
    if unguarded || meta.is_unguarded() {
        return true;
    }
    if listed(meta.fillable_columns(), key) {
        return true;
    }
    if is_guarded(meta, key, false) {
        return false;
    }
    meta.fillable_columns().is_empty()
}

/// Whether `meta`'s model refuses every `fill`.
pub(crate) fn totally_guarded(meta: &ModelMeta, unguarded: bool) -> bool {
    !(unguarded || meta.is_unguarded())
        && meta.fillable_columns().is_empty()
        && meta.guarded_columns().is_none()
}

tokio::task_local! {
    static UNGUARDED: bool;
}

/// Runs `future` with mass assignment unguarded: `fill` sets every real
/// column. Only the task running `future` is affected, never another
/// request.
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db) -> Result<()> {
/// use navree_orm::attributes::unguarded;
///
/// let deal = unguarded(Deal::create_from(&db, navree_orm::attrs! { "title" => "Imported", "owner_id" => 3 })).await?;
/// # Ok(()) }
/// ```
pub async fn unguarded<F: Future>(future: F) -> F::Output {
    UNGUARDED.scope(true, future).await
}

/// Whether the current task runs inside [`unguarded`].
pub fn is_unguarded() -> bool {
    UNGUARDED.try_with(|on| *on).unwrap_or(false)
}

// -------------------------------------------------------------------------
// Discarded attributes
// -------------------------------------------------------------------------

/// A handler for keys `fill` discarded: the model's name and the keys.
pub type DiscardHandler = Arc<dyn Fn(&'static str, &[String]) + Send + Sync>;

/// What `fill` does with keys it may not set (a totally guarded model
/// always fails instead).
#[derive(Clone, Default)]
pub enum DiscardPolicy {
    /// Leave them out (the default).
    #[default]
    Ignore,
    /// Fail with [`OrmError::MassAssignment`], changing nothing.
    Fail,
    /// Leave them out and call the handler (log them, count them).
    Report(DiscardHandler),
}

impl std::fmt::Debug for DiscardPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DiscardPolicy::Ignore => "Ignore",
            DiscardPolicy::Fail => "Fail",
            DiscardPolicy::Report(_) => "Report(..)",
        })
    }
}

static DISCARD_POLICY: RwLock<Option<DiscardPolicy>> = RwLock::new(None);

tokio::task_local! {
    static TASK_DISCARD_POLICY: DiscardPolicy;
}

/// Sets what `fill` does with keys it may not set, for the whole process
/// (at boot, from the app's strictness settings).
pub fn set_discard_policy(policy: DiscardPolicy) {
    *DISCARD_POLICY
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(policy);
}

/// Runs `future` with `policy` for discarded keys (tests, one-off imports).
pub async fn with_discard_policy<F: Future>(policy: DiscardPolicy, future: F) -> F::Output {
    TASK_DISCARD_POLICY.scope(policy, future).await
}

/// The policy in force: the task's, else the process's, else
/// [`DiscardPolicy::Ignore`].
pub fn discard_policy() -> DiscardPolicy {
    TASK_DISCARD_POLICY
        .try_with(Clone::clone)
        .ok()
        .or_else(|| {
            DISCARD_POLICY
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        })
        .unwrap_or_default()
}

// -------------------------------------------------------------------------
// Filling records
// -------------------------------------------------------------------------

/// Which keys a fill may set.
enum Guard<'a> {
    /// The model's fillable and guarded columns.
    Model,
    /// Every real column (`force_fill`).
    Off,
    /// Only these columns (`fill_only`).
    Only(&'a [&'a str]),
}

/// Fills `record` from `attributes` under `guard`; changes nothing when it
/// fails.
fn fill_record<M: Model>(
    record: &mut Record<M>,
    attributes: Attributes,
    guard: &Guard<'_>,
) -> Result<()> {
    let meta = M::meta();
    let unguarded = is_unguarded();
    let mut refused = Vec::new();
    for key in attributes.keys() {
        let allowed = match guard {
            Guard::Model => is_fillable(meta, key, unguarded),
            Guard::Off => {
                if !is_assignable(meta, key) {
                    return Err(not_assignable::<M>(key));
                }
                true
            }
            Guard::Only(columns) => is_assignable(meta, key) && listed(columns, key),
        };
        if !allowed {
            refused.push(key.to_owned());
        }
    }
    if !refused.is_empty() {
        if matches!(guard, Guard::Model) && totally_guarded(meta, unguarded) {
            return Err(OrmError::MassAssignment {
                model: meta.name().into(),
                columns: refused,
                totally_guarded: true,
            });
        }
        match discard_policy() {
            DiscardPolicy::Ignore => {}
            DiscardPolicy::Fail => {
                return Err(OrmError::MassAssignment {
                    model: meta.name().into(),
                    columns: refused,
                    totally_guarded: false,
                });
            }
            DiscardPolicy::Report(handler) => handler(meta.name(), &refused),
        }
    }
    // Convert on a copy, so a value that does not fit changes nothing. The
    // columns of a field stored across several columns are set together,
    // so the field is checked whole.
    let model = record.in_cast_context(|| -> Result<M> {
        let mut model = record.model.clone();
        let mut groups: Vec<(&'static str, Vec<(String, serde_json::Value)>)> = Vec::new();
        for (key, value) in attributes {
            if refused.contains(&key) {
                continue;
            }
            match split_key(&key) {
                (column, Some(path)) => set_json_path_on(&mut model, column, path, value)?,
                (column, None) => match multi_column_field(meta, column) {
                    Some(field) => match groups.iter_mut().find(|(known, _)| *known == field) {
                        Some((_, inputs)) => inputs.push((column.to_owned(), value)),
                        None => groups.push((field, vec![(column.to_owned(), value)])),
                    },
                    None => model.set_attribute(column, value)?,
                },
            }
        }
        for (field, inputs) in groups {
            model.set_attribute_parts(field, inputs)?;
        }
        Ok(model)
    })?;
    record.model = model;
    Ok(())
}

/// The cast context `M`'s records get on `exec` (for models whose casts use
/// one), so a record filled before it is first saved converts its input
/// with the app's services.
async fn context_for<M: Model, E: Executor + ?Sized>(exec: &E) -> Result<Option<Arc<CastContext>>> {
    if !M::meta().uses_cast_context() {
        return Ok(None);
    }
    let target = exec.target(Access::Write).await?;
    Ok(CastContext::for_model::<M>(&target.context())
        .await?
        .map(Arc::new))
}

/// The field `column` belongs to, when that field is stored across several
/// columns.
fn multi_column_field(meta: &ModelMeta, column: &str) -> Option<&'static str> {
    let field = meta.column(column)?.field();
    let columns = meta
        .all_columns()
        .iter()
        .filter(|candidate| candidate.field() == field)
        .count();
    (columns > 1).then_some(field)
}

fn not_assignable<M: Model>(key: &str) -> OrmError {
    let (column, path) = split_key(key);
    match (M::meta().column(column), path) {
        (Some(_), Some(_)) => OrmError::InvalidQuery(format!(
            "cannot fill `{key}`: `{column}` of {} is not a JSON column",
            M::meta().name()
        )),
        _ => crate::model::derive_support::unknown_column::<M>(column),
    }
}

/// Sets `value` at `path` (`a->b->0`) inside JSON column `column`.
fn set_json_path_on<M: Model>(
    model: &mut M,
    column: &str,
    path: &str,
    value: serde_json::Value,
) -> Result<()> {
    let meta = M::meta();
    match meta.column(column) {
        Some(found) if found.is_json() => {}
        Some(_) | None => return Err(not_assignable::<M>(&format!("{column}->{path}"))),
    }
    let mut json = model
        .json_attribute(column)?
        .unwrap_or(serde_json::Value::Null);
    let segments: Vec<&str> = path.split("->").map(str::trim).collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(OrmError::InvalidQuery(format!(
            "`{column}->{path}` has an empty step"
        )));
    }
    set_path(&mut json, &segments, value).map_err(|message| {
        OrmError::InvalidQuery(format!(
            "cannot set `{column}->{path}` of {}: {message}",
            meta.name()
        ))
    })?;
    model.set_attribute(column, json)
}

/// Sets `value` at `segments` inside `json`, creating objects on the way;
/// a numeric step indexes an array (or appends at its length).
fn set_path(
    json: &mut serde_json::Value,
    segments: &[&str],
    value: serde_json::Value,
) -> std::result::Result<(), String> {
    let Some((first, rest)) = segments.split_first() else {
        *json = value;
        return Ok(());
    };
    if json.is_null() {
        *json = serde_json::Value::Object(serde_json::Map::new());
    }
    let next = match json {
        serde_json::Value::Object(object) => object
            .entry((*first).to_owned())
            .or_insert(serde_json::Value::Null),
        serde_json::Value::Array(items) => {
            let index: usize = first
                .parse()
                .map_err(|_| format!("`{first}` is not an index into an array"))?;
            match index.cmp(&items.len()) {
                std::cmp::Ordering::Less => &mut items[index],
                std::cmp::Ordering::Equal => {
                    items.push(serde_json::Value::Null);
                    &mut items[index]
                }
                std::cmp::Ordering::Greater => {
                    return Err(format!(
                        "index {index} is past the end of an array of {}",
                        items.len()
                    ));
                }
            }
        }
        other => {
            return Err(format!(
                "`{first}` is inside {}, not an object",
                json_kind(other)
            ));
        }
    };
    set_path(next, rest, value)
}

impl<M: Model> Record<M> {
    /// Sets the fillable columns among `attributes` (see the module docs
    /// for the rules); other keys are left out, or refused when the app
    /// does not discard attributes silently. Nothing changes when it fails.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(uow: Uow, mut deal: Record<Deal>, body: navree_orm::attributes::Attributes) -> Result<()> {
    /// deal.fill(body)?.save(&uow).await?;
    /// # Ok(()) }
    /// ```
    pub fn fill(&mut self, attributes: impl Into<Attributes>) -> Result<&mut Self> {
        fill_record(self, attributes.into(), &Guard::Model)?;
        Ok(self)
    }

    /// Sets every key of `attributes`, ignoring the model's guards (keys
    /// must still be real columns or paths into JSON columns: anything
    /// else is an error).
    pub fn force_fill(&mut self, attributes: impl Into<Attributes>) -> Result<&mut Self> {
        fill_record(self, attributes.into(), &Guard::Off)?;
        Ok(self)
    }

    /// Sets the keys of `attributes` that `columns` lists (a per-call
    /// allow-list, whatever the model declares); other keys are left out
    /// (or refused, like `fill`).
    pub fn fill_only(
        &mut self,
        attributes: impl Into<Attributes>,
        columns: &[&str],
    ) -> Result<&mut Self> {
        fill_record(self, attributes.into(), &Guard::Only(columns))?;
        Ok(self)
    }

    /// Sets the value at `path` (`"owner->name"`, `"tags->0"`) inside JSON
    /// column `column` (a `serde_json::Value` field or a JSON cast),
    /// creating the objects on the way. A typed JSON field is converted
    /// back to its type, so the value must fit it.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # fn demo(mut setting: Record<Setting>) -> Result<()> {
    /// # let column = "value";
    /// setting.set_json_path(column, "theme->accent", "green")?;
    /// # Ok(()) }
    /// ```
    pub fn set_json_path(
        &mut self,
        column: impl ColumnName,
        path: &str,
        value: impl Serialize,
    ) -> Result<&mut Self> {
        let column = column.column_name();
        let value = serde_json::to_value(value).map_err(|error| OrmError::Serialize {
            model: M::meta().name().into(),
            column: Some(column.to_string()),
            message: error.to_string(),
        })?;
        let model = self.in_cast_context(|| -> Result<M> {
            let mut model = self.model.clone();
            set_json_path_on(&mut model, &column, path, value)?;
            Ok(model)
        })?;
        self.model = model;
        Ok(self)
    }

    /// Sets `column` from untyped input, through the field's cast and
    /// mutator (unguarded: [`fill`](Record::fill) applies the guards).
    pub fn set_attribute(
        &mut self,
        column: impl ColumnName,
        input: impl Into<serde_json::Value>,
    ) -> Result<&mut Self> {
        let column = column.column_name();
        let input = input.into();
        let context = self.attributes.context.clone();
        CastContext::with_record(context.as_ref(), || match split_key(&column) {
            (column, Some(path)) => set_json_path_on(&mut self.model, column, path, input),
            (column, None) => self.model.set_attribute(column, input),
        })?;
        Ok(self)
    }

    /// Fills the stored record from `attributes` and saves it. Returns
    /// `false` (and changes nothing) for a record that is not stored, or
    /// when a hook cancelled the save.
    pub async fn update_attributes<E: Executor + ?Sized>(
        &mut self,
        exec: &E,
        attributes: impl Into<Attributes>,
    ) -> Result<bool> {
        if !self.exists {
            return Ok(false);
        }
        self.fill(attributes)?;
        self.save(exec).await
    }
}

impl<M: Model + Default> Builder<M> {
    /// Creates a record from the model's defaults filled with
    /// `attributes` (guarded, like [`Record::fill`]).
    pub async fn create_from<E: Executor + ?Sized>(
        self,
        exec: &E,
        attributes: impl Into<Attributes>,
    ) -> Result<Record<M>> {
        let attributes = attributes.into();
        let mut record = Record::new(M::default());
        record.attributes.context = context_for::<M, E>(exec).await?;
        record.fill(attributes)?;
        record.save(exec).await?;
        Ok(record)
    }

    /// Creates a record from the model's defaults with every key of
    /// `attributes` set (unguarded, like [`Record::force_fill`]).
    pub async fn force_create<E: Executor + ?Sized>(
        self,
        exec: &E,
        attributes: impl Into<Attributes>,
    ) -> Result<Record<M>> {
        let attributes = attributes.into();
        let mut record = Record::new(M::default());
        record.attributes.context = context_for::<M, E>(exec).await?;
        record.force_fill(attributes)?;
        record.save(exec).await?;
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn json_paths_create_objects_and_index_arrays() {
        let mut json = json!({"tags": ["a"]});
        set_path(&mut json, &["owner", "name"], json!("Ada")).unwrap();
        set_path(&mut json, &["tags", "1"], json!("b")).unwrap();
        set_path(&mut json, &["tags", "0"], json!("z")).unwrap();
        assert_eq!(json, json!({"tags": ["z", "b"], "owner": {"name": "Ada"}}));
        assert_eq!(
            set_path(&mut json, &["tags", "5"], json!(1)).unwrap_err(),
            "index 5 is past the end of an array of 2"
        );
        assert_eq!(
            set_path(&mut json, &["owner", "name", "first"], json!(1)).unwrap_err(),
            "`first` is inside a string, not an object"
        );
        let mut null = serde_json::Value::Null;
        set_path(&mut null, &["a"], json!(1)).unwrap();
        assert_eq!(null, json!({"a": 1}));
    }

    #[test]
    fn keys_split_into_column_and_path() {
        assert_eq!(split_key("meta->status"), ("meta", Some("status")));
        assert_eq!(split_key("meta -> a->b"), ("meta", Some("a->b")));
        assert_eq!(split_key("title"), ("title", None));
    }

    #[test]
    fn attributes_keep_order_and_replace_in_place() {
        let mut attributes = Attributes::from_pairs([("b", json!(1)), ("a", json!(2))]);
        assert_eq!(attributes.insert("b", 3), Some(json!(1)));
        assert_eq!(attributes.keys().collect::<Vec<_>>(), ["b", "a"]);
        assert_eq!(attributes.remove("b"), Some(json!(3)));
        assert!(!attributes.contains_key("b"));
        let parsed: Attributes = serde_json::from_value(json!({"x": 1, "y": [2]})).unwrap();
        assert_eq!(
            serde_json::to_value(&parsed).unwrap(),
            json!({"x": 1, "y": [2]})
        );
        assert!(Attributes::from_json(json!([1])).is_err());
        assert!(serde_json::from_value::<Attributes>(json!("text")).is_err());
    }

    #[tokio::test]
    async fn unguarded_and_discard_policies_are_task_local() {
        assert!(!is_unguarded());
        assert!(unguarded(async { is_unguarded() }).await);
        assert!(!is_unguarded());
        assert!(matches!(discard_policy(), DiscardPolicy::Ignore));
        let inside = with_discard_policy(DiscardPolicy::Fail, async { discard_policy() }).await;
        assert!(matches!(inside, DiscardPolicy::Fail));
        assert_eq!(
            format!("{:?}", DiscardPolicy::Report(Arc::new(|_, _| {}))),
            "Report(..)"
        );
    }
}
