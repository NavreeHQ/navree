//! [`ValueMap`]: the entries of a map value.

use std::collections::BTreeMap;

use super::Value;

/// The entries of a map: ordered by key (maps, dict literals), or in the
/// order a struct declares its fields.
#[derive(Clone)]
pub(crate) enum ValueMap {
    Sorted(BTreeMap<Value, Value>),
    Fields(Vec<(Value, Value)>),
}

impl Default for ValueMap {
    fn default() -> ValueMap {
        ValueMap::Sorted(BTreeMap::new())
    }
}

impl ValueMap {
    pub(crate) fn new() -> ValueMap {
        ValueMap::default()
    }

    pub(crate) fn get(&self, key: &Value) -> Option<&Value> {
        match self {
            ValueMap::Sorted(map) => map.get(key),
            ValueMap::Fields(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        }
    }

    pub(crate) fn contains_key(&self, key: &Value) -> bool {
        self.get(key).is_some()
    }

    pub(crate) fn insert(&mut self, key: Value, value: Value) {
        match self {
            ValueMap::Sorted(map) => {
                map.insert(key, value);
            }
            ValueMap::Fields(fields) => match fields.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = value,
                None => fields.push((key, value)),
            },
        }
    }

    /// How many keys a lookup compares at most.
    pub(crate) fn lookup_steps(&self) -> u64 {
        match self {
            ValueMap::Sorted(map) => u64::from(usize::BITS - map.len().leading_zeros()),
            ValueMap::Fields(fields) => fields.len() as u64,
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            ValueMap::Sorted(map) => map.len(),
            ValueMap::Fields(fields) => fields.len(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn iter(&self) -> Box<dyn Iterator<Item = (&Value, &Value)> + '_> {
        match self {
            ValueMap::Sorted(map) => Box::new(map.iter()),
            ValueMap::Fields(fields) => Box::new(fields.iter().map(|(k, v)| (k, v))),
        }
    }

    pub(crate) fn keys(&self) -> impl Iterator<Item = &Value> {
        self.iter().map(|(k, _)| k)
    }
}

impl FromIterator<(Value, Value)> for ValueMap {
    fn from_iter<I: IntoIterator<Item = (Value, Value)>>(iter: I) -> ValueMap {
        ValueMap::Sorted(iter.into_iter().collect())
    }
}

impl Extend<(Value, Value)> for ValueMap {
    fn extend<I: IntoIterator<Item = (Value, Value)>>(&mut self, iter: I) {
        for (key, value) in iter {
            self.insert(key, value);
        }
    }
}
