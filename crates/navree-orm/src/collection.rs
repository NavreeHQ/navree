//! [`Collection`]: the records a query returns.

use std::fmt;
use std::hash::Hash;
use std::ops::{Deref, DerefMut};

use indexmap::IndexMap;
use serde::{Serialize, Serializer};

use crate::column::Column;
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor};
use crate::model::Model;
use crate::query::Builder;
use crate::record::Record;
use crate::row::Row;

/// The records of a query, in order.
///
/// A collection dereferences to a slice of [`Record`]s (`len`, indexing,
/// `iter`, `first`, `last`, ...), iterates by value or by reference, and
/// serializes as an array. Its own methods work with keys and columns:
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db) -> Result<()> {
/// let deals = Deal::query().where_(Deal::STAGE.eq(Stage::Open)).get(&db).await?;
/// let by_owner = deals.clone().group_by(|deal| deal.owner_id);
/// let titles = deals.pluck(Deal::TITLE)?;
/// let big = deals.filter(|deal| deal.amount_cents > 100_000);
/// # Ok(()) }
/// ```
pub struct Collection<M: Model> {
    records: Vec<Record<M>>,
}

impl<M: Model> Collection<M> {
    /// An empty collection.
    pub fn new() -> Self {
        Collection {
            records: Vec::new(),
        }
    }

    /// The records, by value.
    pub fn into_vec(self) -> Vec<Record<M>> {
        self.records
    }

    /// The models, without the records' bookkeeping.
    pub fn into_models(self) -> Vec<M> {
        self.records.into_iter().map(Record::into_inner).collect()
    }

    /// Appends a record.
    pub fn push(&mut self, record: Record<M>) {
        self.records.push(record);
    }

    /// The record with key `key`.
    pub fn find(&self, key: impl Into<M::Key>) -> Option<&Record<M>> {
        let key = key.into();
        self.records.iter().find(|record| record.key() == key)
    }

    /// The record with key `key`, for changes.
    pub fn find_mut(&mut self, key: impl Into<M::Key>) -> Option<&mut Record<M>> {
        let key = key.into();
        self.records.iter_mut().find(|record| record.key() == key)
    }

    /// Whether a record has key `key`.
    pub fn contains(&self, key: impl Into<M::Key>) -> bool {
        self.find(key).is_some()
    }

    /// The records' keys, in order.
    pub fn model_keys(&self) -> Vec<M::Key> {
        self.records.iter().map(Record::key).collect()
    }

    /// `f` of every record, in order.
    pub fn map<U>(&self, f: impl FnMut(&Record<M>) -> U) -> Vec<U> {
        self.records.iter().map(f).collect()
    }

    /// The records for which `keep` holds.
    pub fn filter(self, mut keep: impl FnMut(&Record<M>) -> bool) -> Self {
        Collection {
            records: self
                .records
                .into_iter()
                .filter(|record| keep(record))
                .collect(),
        }
    }

    /// The records for which `test` holds, and the others.
    pub fn partition(self, test: impl FnMut(&Record<M>) -> bool) -> (Self, Self) {
        let (yes, no): (Vec<_>, Vec<_>) = self.records.into_iter().partition(test);
        (Collection { records: yes }, Collection { records: no })
    }

    /// The values of `column`, in order (decoded as the field is, casts
    /// included).
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// # fn main() -> Result<()> {
    /// let deals = Collection::from(vec![
    ///     Record::new(Deal { title: "Renewal".into(), ..Deal::default() }),
    ///     Record::new(Deal { title: "Upsell".into(), ..Deal::default() }),
    /// ]);
    /// let titles: Vec<String> = deals.pluck(Deal::TITLE)?;
    /// assert_eq!(titles, ["Renewal", "Upsell"]);
    /// # Ok(()) }
    /// ```
    pub fn pluck<T>(&self, column: Column<M, T>) -> Result<Vec<T>> {
        let name = column.column_ref().name.into_owned();
        self.records
            .iter()
            .map(|record| {
                let value = record
                    .model
                    .value(&name)?
                    .ok_or_else(|| crate::model::derive_support::unknown_column::<M>(&name))?;
                let row = Row::from_values(vec![(name.clone(), value)]);
                column.decode(&row).map_err(|error| {
                    crate::row::decode_error(
                        M::meta().name(),
                        &name,
                        "the column's type".into(),
                        &error,
                    )
                })
            })
            .collect()
    }

    /// The records by `key` (a later record with the same key replaces an
    /// earlier one, keeping its position).
    pub fn key_by<K: Hash + Eq>(
        self,
        mut key: impl FnMut(&Record<M>) -> K,
    ) -> IndexMap<K, Record<M>> {
        let mut map = IndexMap::with_capacity(self.records.len());
        for record in self.records {
            map.insert(key(&record), record);
        }
        map
    }

    /// The records grouped by `key`, groups in order of first appearance.
    pub fn group_by<K: Hash + Eq>(
        self,
        mut key: impl FnMut(&Record<M>) -> K,
    ) -> IndexMap<K, Collection<M>> {
        let mut groups: IndexMap<K, Collection<M>> = IndexMap::new();
        for record in self.records {
            groups.entry(key(&record)).or_default().push(record);
        }
        groups
    }

    /// The records whose key is not in `other`.
    pub fn diff(self, other: &Collection<M>) -> Self {
        let keys = other.model_keys();
        self.filter(|record| !keys.contains(&record.key()))
    }

    /// The records whose key is also in `other`.
    pub fn intersect(self, other: &Collection<M>) -> Self {
        let keys = other.model_keys();
        self.filter(|record| keys.contains(&record.key()))
    }

    /// The first record of each key.
    pub fn unique(self) -> Self {
        let mut seen: Vec<M::Key> = Vec::with_capacity(self.records.len());
        self.filter(|record| {
            let key = record.key();
            if seen.contains(&key) {
                false
            } else {
                seen.push(key);
                true
            }
        })
    }

    /// The records with these keys.
    pub fn only(self, keys: impl IntoIterator<Item = impl Into<M::Key>>) -> Self {
        let keys: Vec<M::Key> = keys.into_iter().map(Into::into).collect();
        self.filter(|record| keys.contains(&record.key()))
    }

    /// The records without these keys.
    pub fn except(self, keys: impl IntoIterator<Item = impl Into<M::Key>>) -> Self {
        let keys: Vec<M::Key> = keys.into_iter().map(Into::into).collect();
        self.filter(|record| !keys.contains(&record.key()))
    }

    /// The records sorted by `compare` (stable).
    pub fn sort_by(
        mut self,
        compare: impl FnMut(&Record<M>, &Record<M>) -> std::cmp::Ordering,
    ) -> Self {
        self.records.sort_by(compare);
        self
    }

    /// The records sorted by `key` (stable).
    pub fn sort_by_key<K: Ord>(mut self, key: impl FnMut(&Record<M>) -> K) -> Self {
        self.records.sort_by_key(key);
        self
    }

    /// The records in reverse order.
    pub fn reverse(mut self) -> Self {
        self.records.reverse();
        self
    }

    /// A query of exactly these records (by key), e.g. for a mass update.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db, uow: Uow) -> Result<()> {
    /// let stale = Deal::query().where_(Deal::CREATED_AT.lt(Utc::now() - Duration::days(90))).get(&db).await?;
    /// stale.to_query().update(&uow, [Deal::STAGE.set(Stage::Lost)]).await?;
    /// # Ok(()) }
    /// ```
    pub fn to_query(&self) -> Builder<M> {
        Builder::new().where_key_in(self.model_keys())
    }

    /// The records as they are now in the database (ignoring global
    /// scopes), in this collection's order; records that are gone or were
    /// never stored are left out.
    pub async fn fresh<E: Executor + ?Sized>(&self, exec: &E) -> Result<Collection<M>> {
        let keys: Vec<M::Key> = self
            .records
            .iter()
            .filter(|record| record.exists())
            .map(Record::key)
            .collect();
        if keys.is_empty() {
            return Ok(Collection::new());
        }
        let target = exec
            .target_routed(Access::Read, crate::query::model_route::<M>())
            .await?;
        let mut fresh = Builder::<M>::new()
            .without_global_scopes()
            .where_key_in(keys.clone())
            .fetch(&target)
            .await?
            .into_vec();
        let mut ordered = Vec::with_capacity(fresh.len());
        for key in keys {
            if let Some(index) = fresh.iter().position(|record| record.key() == key) {
                ordered.push(fresh.swap_remove(index));
            }
        }
        Ok(Collection { records: ordered })
    }

    /// The single record, or an error naming how many there are.
    pub fn sole(self) -> Result<Record<M>> {
        let count = self.records.len();
        let mut records = self.records.into_iter();
        match (records.next(), count) {
            (Some(record), 1) => Ok(record),
            (None, _) => Err(OrmError::not_found(M::meta().name(), Vec::new())),
            (Some(_), _) => Err(OrmError::MultipleRecordsFound {
                model: M::meta().name().into(),
            }),
        }
    }
}

impl<M: Model> Default for Collection<M> {
    fn default() -> Self {
        Collection::new()
    }
}

impl<M: Model> Clone for Collection<M> {
    fn clone(&self) -> Self {
        Collection {
            records: self.records.clone(),
        }
    }
}

impl<M: Model + fmt::Debug> fmt::Debug for Collection<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.records).finish()
    }
}

impl<M: Model> Deref for Collection<M> {
    type Target = [Record<M>];

    fn deref(&self) -> &[Record<M>] {
        &self.records
    }
}

impl<M: Model> DerefMut for Collection<M> {
    fn deref_mut(&mut self) -> &mut [Record<M>] {
        &mut self.records
    }
}

impl<M: Model> From<Vec<Record<M>>> for Collection<M> {
    fn from(records: Vec<Record<M>>) -> Self {
        Collection { records }
    }
}

impl<M: Model> FromIterator<Record<M>> for Collection<M> {
    fn from_iter<I: IntoIterator<Item = Record<M>>>(iter: I) -> Self {
        Collection {
            records: iter.into_iter().collect(),
        }
    }
}

impl<M: Model> Extend<Record<M>> for Collection<M> {
    fn extend<I: IntoIterator<Item = Record<M>>>(&mut self, iter: I) {
        self.records.extend(iter);
    }
}

impl<M: Model> IntoIterator for Collection<M> {
    type Item = Record<M>;
    type IntoIter = std::vec::IntoIter<Record<M>>;

    fn into_iter(self) -> Self::IntoIter {
        self.records.into_iter()
    }
}

impl<'a, M: Model> IntoIterator for &'a Collection<M> {
    type Item = &'a Record<M>;
    type IntoIter = std::slice::Iter<'a, Record<M>>;

    fn into_iter(self) -> Self::IntoIter {
        self.records.iter()
    }
}

impl<'a, M: Model> IntoIterator for &'a mut Collection<M> {
    type Item = &'a mut Record<M>;
    type IntoIter = std::slice::IterMut<'a, Record<M>>;

    fn into_iter(self) -> Self::IntoIter {
        self.records.iter_mut()
    }
}

/// An array of the records.
impl<M: Model> Serialize for Collection<M> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.records.serialize(serializer)
    }
}
