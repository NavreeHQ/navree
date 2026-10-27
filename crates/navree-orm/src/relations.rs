//! Relationships between models: the [`Relation`] trait and the
//! [`Relations`] a record carries once they are loaded.
//!
//! A relation is a typed description of how one model's records reach
//! another's (`Deal` → its `Company`, `Deal` → its `Activity`s). Asking a
//! relation for a parent's [`query`](Relation::query_for) gives an ordinary
//! [`Builder`] of the related model, so every query method works on it:
//!
//! ```ignore
//! let overdue = deal.activities().query_for(&deal).where_(Activity::DUE_AT.lt(now)).get(&db).await?;
//! ```
//!
//! Loaded relations live on the record ([`Record::relations`]), keyed by
//! the relation's name, and serialize with it:
//!
//! ```
//! use navree_orm::relations::Relations;
//!
//! let mut relations = Relations::new();
//! relations.set("tags", vec!["vip".to_owned()]);
//! assert!(relations.is_loaded("tags"));
//! assert_eq!(relations.get::<Vec<String>>("tags").unwrap(), &["vip"]);
//! assert!(relations.get::<u32>("tags").is_none(), "another type");
//! assert_eq!(relations.to_json().unwrap(), serde_json::json!({"tags": ["vip"]}));
//! ```
//!
//! [`Record::relations`]: crate::Record::relations

use std::any::Any;
use std::borrow::Cow;
use std::fmt;

use serde::Serialize;

use crate::model::Model;
use crate::query::Builder;
use crate::record::Record;

/// A relationship from model `P` (the parent) to model
/// [`Related`](Relation::Related).
pub trait Relation<P: Model>: Send + Sync + 'static {
    /// The related model.
    type Related: Model;

    /// The relation's name: the key its records are loaded under on the
    /// parent and serialized as, and its step in relation paths
    /// (`"company.owner"`).
    fn name(&self) -> &'static str;

    /// The related records of `parent`, as a query to refine and run.
    fn query_for(&self, parent: &Record<P>) -> Builder<Self::Related>;
}

/// A loaded relation: the value, how to clone it and how to serialize it.
struct Slot {
    name: Cow<'static, str>,
    value: Box<dyn Any + Send + Sync>,
    clone: fn(&(dyn Any + Send + Sync)) -> Box<dyn Any + Send + Sync>,
    to_json: fn(&(dyn Any + Send + Sync)) -> Result<serde_json::Value, serde_json::Error>,
}

/// The relations loaded on a record, by name. A value is usually an
/// `Option<Record<R>>` (to-one), or a [`Collection<R>`](crate::Collection)
/// (to-many), but any cloneable, serializable value can be stored.
#[derive(Default)]
pub struct Relations {
    slots: Vec<Slot>,
}

impl Relations {
    /// No relation loaded.
    pub fn new() -> Self {
        Relations::default()
    }

    /// Stores `value` as relation `name`, replacing what was loaded.
    pub fn set<T>(&mut self, name: impl Into<Cow<'static, str>>, value: T)
    where
        T: Clone + Serialize + Send + Sync + 'static,
    {
        fn clone<T: Clone + Send + Sync + 'static>(
            value: &(dyn Any + Send + Sync),
        ) -> Box<dyn Any + Send + Sync> {
            match value.downcast_ref::<T>() {
                Some(value) => Box::new(value.clone()),
                // A slot only ever holds the type it was stored with.
                None => Box::new(()),
            }
        }
        fn to_json<T: Serialize + 'static>(
            value: &(dyn Any + Send + Sync),
        ) -> Result<serde_json::Value, serde_json::Error> {
            match value.downcast_ref::<T>() {
                Some(value) => serde_json::to_value(value),
                None => Ok(serde_json::Value::Null),
            }
        }
        let name = name.into();
        let slot = Slot {
            name: name.clone(),
            value: Box::new(value),
            clone: clone::<T>,
            to_json: to_json::<T>,
        };
        match self.slots.iter_mut().find(|slot| slot.name == name) {
            Some(existing) => *existing = slot,
            None => self.slots.push(slot),
        }
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

    /// Removes relation `name` and returns it, when it is loaded as a `T`.
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
}

impl Clone for Relations {
    fn clone(&self) -> Self {
        Relations {
            slots: self
                .slots
                .iter()
                .map(|slot| Slot {
                    name: slot.name.clone(),
                    value: (slot.clone)(&*slot.value),
                    clone: slot.clone,
                    to_json: slot.to_json,
                })
                .collect(),
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
