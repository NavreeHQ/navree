//! [`Relations`]: the relations loaded on a record, and [`RelationValue`]:
//! what a loaded relation holds.

use std::any::Any;
use std::borrow::Cow;
use std::fmt;

use navree_kernel::BoxFuture;
use serde::Serialize;

use crate::collection::Collection;
use crate::error::Result;
use crate::exec::Target;
use crate::model::Model;
use crate::record::Record;
use crate::relations::any::AnyRecord;
use crate::value::Value;

type AnyValue = dyn Any + Send + Sync;

/// Saves a loaded value and the records it holds (`Record::push`).
type PushFn = for<'a, 'b> fn(&'a mut AnyValue, &'a Target<'b>) -> BoxFuture<'a, Result<bool>>;

/// Reads a column of a loaded value that is one row.
type ColumnFn = fn(&AnyValue, &str) -> Option<Value>;

pub(crate) mod sealed {
    /// Only the ORM's relation values are relation values.
    pub trait Sealed {}
}

/// What a relation holds once it is loaded on a record:
/// `Option<Record<R>>` for a to-one relation (`belongs_to`, `has_one`,
/// `has_one_through`, `morph_one`), [`Collection<R>`] for a to-many one
/// (`has_many`, `belongs_to_many`, `has_many_through`, `morph_many`,
/// `morph_to_many`), `Option<AnyRecord>` for `morph_to`, and the pivot of a
/// many-to-many record ([`Pivot`](crate::relations::Pivot), or the
/// `Record` of a typed pivot model).
///
/// Loaded values serialize with the record, and [`Record::push`] saves the
/// records they hold.
pub trait RelationValue: Clone + Serialize + Send + Sync + 'static + sealed::Sealed {
    /// Saves the records the value holds, and theirs (`Record::push`).
    #[doc(hidden)]
    fn push_on<'a, 'b>(&'a mut self, target: &'a Target<'b>) -> BoxFuture<'a, Result<bool>>;

    /// Column `column` of a value that is one row (a pivot, or the record
    /// of a typed pivot model); `None` for other values.
    #[doc(hidden)]
    fn column(&self, column: &str) -> Option<Value> {
        let _ = column;
        None
    }
}

impl<R: Model> sealed::Sealed for Option<Record<R>> {}

impl<R: Model> RelationValue for Option<Record<R>> {
    fn push_on<'a, 'b>(&'a mut self, target: &'a Target<'b>) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            match self {
                Some(record) => crate::relations::lazy::push_record(record, target).await,
                None => Ok(true),
            }
        })
    }
}

impl<R: Model> sealed::Sealed for Record<R> {}

impl<R: Model> RelationValue for Record<R> {
    fn push_on<'a, 'b>(&'a mut self, target: &'a Target<'b>) -> BoxFuture<'a, Result<bool>> {
        crate::relations::lazy::push_record(self, target)
    }

    fn column(&self, column: &str) -> Option<Value> {
        self.model.value(column).ok().flatten()
    }
}

impl<R: Model> sealed::Sealed for Collection<R> {}

impl<R: Model> RelationValue for Collection<R> {
    fn push_on<'a, 'b>(&'a mut self, target: &'a Target<'b>) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            for record in self.iter_mut() {
                if !crate::relations::lazy::push_record(record, target).await? {
                    return Ok(false);
                }
            }
            Ok(true)
        })
    }
}

impl sealed::Sealed for Option<AnyRecord> {}

impl RelationValue for Option<AnyRecord> {
    fn push_on<'a, 'b>(&'a mut self, target: &'a Target<'b>) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            match self {
                Some(record) => record.push_on(target).await,
                None => Ok(true),
            }
        })
    }
}

/// A loaded relation: the value, how to clone it, how to serialize it and,
/// for relation values, how to save the records it holds and read a
/// column of a one-row value.
struct Slot {
    name: Cow<'static, str>,
    value: Box<AnyValue>,
    clone: fn(&AnyValue) -> Box<AnyValue>,
    to_json: fn(&AnyValue) -> Result<serde_json::Value, serde_json::Error>,
    push: Option<PushFn>,
    column: Option<ColumnFn>,
}

impl Slot {
    /// A copy of the slot.
    fn duplicate(&self) -> Slot {
        Slot {
            name: self.name.clone(),
            value: (self.clone)(&*self.value),
            clone: self.clone,
            to_json: self.to_json,
            push: self.push,
            column: self.column,
        }
    }
}

fn column_of<V: RelationValue>(value: &AnyValue, column: &str) -> Option<Value> {
    value.downcast_ref::<V>()?.column(column)
}

fn clone_value<T: Clone + Send + Sync + 'static>(value: &AnyValue) -> Box<AnyValue> {
    match value.downcast_ref::<T>() {
        Some(value) => Box::new(value.clone()),
        // A slot only ever holds the type it was stored with.
        None => Box::new(()),
    }
}

fn value_to_json<T: Serialize + 'static>(
    value: &AnyValue,
) -> Result<serde_json::Value, serde_json::Error> {
    match value.downcast_ref::<T>() {
        Some(value) => serde_json::to_value(value),
        None => Ok(serde_json::Value::Null),
    }
}

fn push_value<'a, 'b, V: RelationValue>(
    value: &'a mut AnyValue,
    target: &'a Target<'b>,
) -> BoxFuture<'a, Result<bool>> {
    match value.downcast_mut::<V>() {
        Some(value) => value.push_on(target),
        None => Box::pin(async { Ok(true) }),
    }
}

/// The relations loaded on a record, by name, in loading order.
///
/// Values are typed: [`get`](Relations::get) returns a relation only as
/// the type it was stored as. The ORM stores [`RelationValue`]s
/// (`Option<Record<R>>`, `Collection<R>`, ...); any other cloneable,
/// serializable value can be stored with [`set`](Relations::set) (it
/// serializes with the record, and is not saved by `push`).
///
/// ```
/// use navree_orm::relations::Relations;
///
/// let mut relations = Relations::new();
/// relations.set("tags", vec!["vip".to_owned()]);
/// assert!(relations.is_loaded("tags"));
/// assert_eq!(relations.get::<Vec<String>>("tags").unwrap(), &["vip"]);
/// assert!(relations.get::<u32>("tags").is_none(), "another type");
/// assert_eq!(relations.to_json().unwrap(), serde_json::json!({"tags": ["vip"]}));
/// ```
#[derive(Default)]
pub struct Relations {
    slots: Vec<Slot>,
}

impl Relations {
    /// No relation loaded.
    pub fn new() -> Self {
        Relations::default()
    }

    fn put(&mut self, slot: Slot) {
        match self
            .slots
            .iter_mut()
            .find(|existing| existing.name == slot.name)
        {
            Some(existing) => *existing = slot,
            None => self.slots.push(slot),
        }
    }

    /// Stores `value` as relation `name`, replacing what was loaded. Any
    /// cloneable, serializable value can be stored; it serializes with the
    /// record. Relation values are stored with
    /// [`set_relation`](Relations::set_relation), which `push` saves.
    pub fn set<T>(&mut self, name: impl Into<Cow<'static, str>>, value: T)
    where
        T: Clone + Serialize + Send + Sync + 'static,
    {
        self.put(Slot {
            name: name.into(),
            value: Box::new(value),
            clone: clone_value::<T>,
            to_json: value_to_json::<T>,
            push: None,
            column: None,
        });
    }

    /// Stores relation value `value` as relation `name`, replacing what
    /// was loaded.
    pub fn set_relation<V: RelationValue>(&mut self, name: impl Into<Cow<'static, str>>, value: V) {
        self.put(Slot {
            name: name.into(),
            value: Box::new(value),
            clone: clone_value::<V>,
            to_json: value_to_json::<V>,
            push: Some(push_value::<V>),
            column: Some(column_of::<V>),
        });
    }

    /// Column `column` of relation `name`, when it holds one row (a pivot,
    /// or a typed pivot model's record).
    pub(crate) fn column(&self, name: &str, column: &str) -> Option<Value> {
        let slot = self.slots.iter().find(|slot| slot.name == name)?;
        (slot.column?)(&*slot.value, column)
    }

    /// Relation `name`, when it is loaded as a `T`.
    pub fn get<T: 'static>(&self, name: &str) -> Option<&T> {
        self.slots
            .iter()
            .find(|slot| slot.name == name)
            .and_then(|slot| slot.value.downcast_ref::<T>())
    }

    /// Relation `name` for changes, when it is loaded as a `T`.
    pub fn get_mut<T: 'static>(&mut self, name: &str) -> Option<&mut T> {
        self.slots
            .iter_mut()
            .find(|slot| slot.name == name)
            .and_then(|slot| slot.value.downcast_mut::<T>())
    }

    /// Removes relation `name` and returns it, when it is loaded as a `T`
    /// (another type is left in place).
    pub fn take<T: 'static>(&mut self, name: &str) -> Option<T> {
        let index = self.slots.iter().position(|slot| slot.name == name)?;
        if !self.slots[index].value.is::<T>() {
            return None;
        }
        let slot = self.slots.remove(index);
        slot.value.downcast::<T>().ok().map(|value| *value)
    }

    /// Whether relation `name` is loaded.
    pub fn is_loaded(&self, name: &str) -> bool {
        self.slots.iter().any(|slot| slot.name == name)
    }

    /// Forgets relation `name`.
    pub fn unset(&mut self, name: &str) {
        self.slots.retain(|slot| slot.name != name);
    }

    /// Forgets every relation.
    pub fn clear(&mut self) {
        self.slots.clear();
    }

    /// The names of the loaded relations, in loading order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.slots.iter().map(|slot| slot.name.as_ref())
    }

    /// How many relations are loaded.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether no relation is loaded.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The loaded relations as a JSON object, by name.
    pub fn to_json(&self) -> Result<serde_json::Value, serde_json::Error> {
        let mut object = serde_json::Map::with_capacity(self.slots.len());
        for slot in &self.slots {
            object.insert(slot.name.to_string(), (slot.to_json)(&*slot.value)?);
        }
        Ok(serde_json::Value::Object(object))
    }

    /// Copies relation `name` of `other` (if loaded there) into these
    /// relations.
    pub(crate) fn copy_from(&mut self, other: &Relations, name: &str) {
        if let Some(slot) = other.slots.iter().find(|slot| slot.name == name) {
            self.put(slot.duplicate());
        }
    }

    /// Saves the records of every loaded relation value, in loading order;
    /// `false` when a hook cancelled one of the saves.
    pub(crate) fn push_all<'a, 'b>(
        &'a mut self,
        target: &'a Target<'b>,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            for slot in &mut self.slots {
                if let Some(push) = slot.push
                    && !push(&mut *slot.value, target).await?
                {
                    return Ok(false);
                }
            }
            Ok(true)
        })
    }
}

impl Clone for Relations {
    fn clone(&self) -> Self {
        Relations {
            slots: self.slots.iter().map(Slot::duplicate).collect(),
        }
    }
}

impl fmt::Debug for Relations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.names()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_typed_replaced_cloned_and_taken() {
        let mut relations = Relations::new();
        relations.set("owner", Some("ada".to_owned()));
        relations.set("owner", Some("grace".to_owned()));
        assert_eq!(relations.names().collect::<Vec<_>>(), ["owner"]);
        let copy = relations.clone();
        relations
            .get_mut::<Option<String>>("owner")
            .unwrap()
            .replace("linus".into());
        assert_eq!(
            copy.get::<Option<String>>("owner").unwrap().as_deref(),
            Some("grace")
        );
        assert!(relations.take::<u8>("owner").is_none());
        assert_eq!(relations.len(), 1, "a take of another type leaves it");
        assert_eq!(
            relations
                .take::<Option<String>>("owner")
                .unwrap()
                .as_deref(),
            Some("linus")
        );
        assert!(relations.is_empty());
        assert_eq!(format!("{copy:?}"), r#"["owner"]"#);
    }
}
