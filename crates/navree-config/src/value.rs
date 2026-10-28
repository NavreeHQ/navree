//! The configuration tree: [`Value`], the conversions that build it from
//! TOML, JSON and environment variables, and the merge that stacks layers.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt;
use std::sync::Arc;

use crate::redact;

/// Where a value came from, quoted in type errors: a file path,
/// `environment` or `ConfigSources::set`. Shared by every value of a layer.
pub(crate) type Origin = Arc<str>;

/// A table. Ordered by key, so `Debug`, redacted output and "first unknown
/// field" errors are the same on every run.
pub(crate) type Table = BTreeMap<String, Value>;

/// A node of the configuration tree.
#[derive(Clone)]
pub(crate) struct Value {
    /// The layer the node came from; `None` for tables built while merging
    /// and for map keys.
    pub(crate) origin: Option<Origin>,
    pub(crate) kind: Kind,
}

/// What a node holds.
#[derive(Clone, PartialEq)]
pub(crate) enum Kind {
    /// An explicit JSON `null`.
    Null,
    Bool(bool),
    /// An integer that fits `i64` (every TOML integer).
    Int(i64),
    /// An integer above `i64::MAX` (only JSON can write one).
    UInt(u64),
    Float(f64),
    String(String),
    Array(Vec<Value>),
    Table(Table),
}

/// A layer nests deeper than the limit.
#[derive(Debug)]
pub(crate) struct TooDeep;

impl Value {
    pub(crate) fn new(origin: Option<&Origin>, kind: Kind) -> Self {
        Value {
            origin: origin.cloned(),
            kind,
        }
    }

    pub(crate) fn empty_table() -> Self {
        Value {
            origin: None,
            kind: Kind::Table(Table::new()),
        }
    }

    /// A string without an origin: how map keys are handed to serde.
    pub(crate) fn key(key: String) -> Self {
        Value {
            origin: None,
            kind: Kind::String(key),
        }
    }

    pub(crate) fn is_table(&self) -> bool {
        matches!(self.kind, Kind::Table(_))
    }

    /// The value at dotted path `key`; empty segments are skipped, so `""` is
    /// the node itself. Only tables are walked through.
    pub(crate) fn lookup(&self, key: &str) -> Option<&Value> {
        let mut node = self;
        for segment in key.split('.').filter(|s| !s.is_empty()) {
            match &node.kind {
                Kind::Table(map) => node = map.get(segment)?,
                _ => return None,
            }
        }
        Some(node)
    }

    /// The scalar as a string (numbers and booleans formatted); `None` for
    /// null, tables and arrays.
    pub(crate) fn scalar_string(&self) -> Option<String> {
        match &self.kind {
            Kind::String(s) => Some(s.clone()),
            Kind::Bool(b) => Some(b.to_string()),
            Kind::Int(i) => Some(i.to_string()),
            Kind::UInt(u) => Some(u.to_string()),
            Kind::Float(f) => Some(f.to_string()),
            Kind::Null | Kind::Array(_) | Kind::Table(_) => None,
        }
    }

    /// The greatest nesting of tables and arrays below this node, counting
    /// the node itself when it is one (a scalar is 0).
    #[cfg(test)]
    pub(crate) fn depth(&self) -> usize {
        match &self.kind {
            Kind::Array(items) => 1 + items.iter().map(Value::depth).max().unwrap_or(0),
            Kind::Table(map) => 1 + map.values().map(Value::depth).max().unwrap_or(0),
            _ => 0,
        }
    }
}

/// Prints the redacted tree: a stray `{:?}` of a value must not leak a secret.
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", redact::redacted_json(self))
    }
}

/// Equality of what the trees hold; where the values came from is not compared.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

/// Deep-merges `layer` over `base`: tables merge key by key; anything else
/// (scalars, arrays, null, a table replacing a scalar or the reverse) is
/// replaced by the layer's value.
///
/// Folding layers left to right with this function is how a load stacks its
/// sources. The merge is associative for layers that agree on which paths
/// are tables; see [`ConfigSources`](crate::ConfigSources) for what happens
/// when they do not. The recursion is as deep as the layers, which the load
/// limits.
pub(crate) fn merge(base: &mut Value, layer: Value) {
    let Value { origin, kind } = layer;
    match kind {
        Kind::Table(layer_map) => match &mut base.kind {
            Kind::Table(base_map) => {
                for (key, value) in layer_map {
                    match base_map.entry(key) {
                        Entry::Occupied(mut slot) => merge(slot.get_mut(), value),
                        Entry::Vacant(slot) => {
                            slot.insert(value);
                        }
                    }
                }
            }
            _ => {
                *base = Value {
                    origin,
                    kind: Kind::Table(layer_map),
                };
            }
        },
        kind => *base = Value { origin, kind },
    }
}

/// Sets `value` at `path` below `root`, turning every non-table node on the
/// way into a table (so `a` set to a scalar and then `a.b` makes `a` a table).
pub(crate) fn insert_at(root: &mut Value, path: &[String], value: Value) {
    let mut node = root;
    for segment in path {
        node = table_mut(node)
            .entry(segment.clone())
            .or_insert_with(Value::empty_table);
    }
    *node = value;
}

/// The table in `node`, replacing a node of another kind by an empty table.
fn table_mut(node: &mut Value) -> &mut Table {
    if !node.is_table() {
        *node = Value::empty_table();
    }
    match &mut node.kind {
        Kind::Table(map) => map,
        // Replaced by a table just above.
        _ => unreachable!("node is a table"),
    }
}

/// Converts a parsed TOML value. `level` is the nesting level of `value` if
/// it is a table or an array (the root table is level 1); a container below
/// `max_depth` is [`TooDeep`]. Datetimes become their TOML text.
pub(crate) fn from_toml(
    value: toml::Value,
    origin: &Origin,
    level: usize,
    max_depth: usize,
) -> Result<Value, TooDeep> {
    let kind = match value {
        toml::Value::String(s) => Kind::String(s),
        toml::Value::Integer(i) => Kind::Int(i),
        toml::Value::Float(f) => Kind::Float(f),
        toml::Value::Boolean(b) => Kind::Bool(b),
        toml::Value::Datetime(d) => Kind::String(d.to_string()),
        toml::Value::Array(items) => {
            check(level, max_depth)?;
            Kind::Array(
                items
                    .into_iter()
                    .map(|v| from_toml(v, origin, level + 1, max_depth))
                    .collect::<Result<_, _>>()?,
            )
        }
        toml::Value::Table(table) => {
            check(level, max_depth)?;
            Kind::Table(
                table
                    .into_iter()
                    .map(|(k, v)| Ok((k, from_toml(v, origin, level + 1, max_depth)?)))
                    .collect::<Result<_, _>>()?,
            )
        }
    };
    Ok(Value::new(Some(origin), kind))
}

/// Converts a JSON value, like [`from_toml`]. Numbers keep their kind:
/// integers that fit `i64`, then `u64`, else floating point. The value is
/// borrowed, so a too-deep one is refused before anything recurses into it.
pub(crate) fn from_json(
    value: &serde_json::Value,
    origin: &Origin,
    level: usize,
    max_depth: usize,
) -> Result<Value, TooDeep> {
    use serde_json::Value as Json;
    let kind = match value {
        Json::Null => Kind::Null,
        Json::Bool(b) => Kind::Bool(*b),
        Json::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => Kind::Int(i),
            (None, Some(u)) => Kind::UInt(u),
            (None, None) => n.as_f64().map_or(Kind::Null, Kind::Float),
        },
        Json::String(s) => Kind::String(s.clone()),
        Json::Array(items) => {
            check(level, max_depth)?;
            Kind::Array(
                items
                    .iter()
                    .map(|v| from_json(v, origin, level + 1, max_depth))
                    .collect::<Result<_, _>>()?,
            )
        }
        Json::Object(map) => {
            check(level, max_depth)?;
            Kind::Table(
                map.iter()
                    .map(|(k, v)| Ok((k.clone(), from_json(v, origin, level + 1, max_depth)?)))
                    .collect::<Result<_, _>>()?,
            )
        }
    };
    Ok(Value::new(Some(origin), kind))
}

fn check(level: usize, max_depth: usize) -> Result<(), TooDeep> {
    if level > max_depth {
        Err(TooDeep)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn tree(json: serde_json::Value) -> Value {
        from_json(&json, &Origin::from("test"), 1, 64).expect("shallow enough")
    }

    fn merged(base: serde_json::Value, layer: serde_json::Value) -> Value {
        let mut base = tree(base);
        merge(&mut base, tree(layer));
        base
    }

    #[test]
    fn tables_merge_and_everything_else_is_replaced() {
        assert_eq!(
            merged(
                json!({ "a": { "x": 1, "y": [1, 2] }, "b": 1, "c": { "d": 1 } }),
                json!({ "a": { "y": [3], "z": null }, "b": { "e": 1 }, "c": 2 }),
            ),
            tree(json!({ "a": { "x": 1, "y": [3], "z": null }, "b": { "e": 1 }, "c": 2 }))
        );
    }

    #[test]
    fn the_empty_table_is_the_identity_on_both_sides() {
        let layer = json!({ "a": { "b": [1] }, "c": "d" });
        assert_eq!(merged(json!({}), layer.clone()), tree(layer.clone()));
        assert_eq!(merged(layer.clone(), json!({})), tree(layer));
    }

    #[test]
    fn a_table_does_not_remember_what_it_replaced() {
        // The documented limit of associativity: a scalar between two tables
        // cuts the earlier table off when layers are merged left to right,
        // but not when the later two are merged first.
        let (a, b, c) = (
            json!({ "x": { "p": 1 } }),
            json!({ "x": 5 }),
            json!({ "x": { "q": 2 } }),
        );
        let mut left = tree(a.clone());
        merge(&mut left, tree(b.clone()));
        merge(&mut left, tree(c.clone()));
        let mut bc = tree(b);
        merge(&mut bc, tree(c));
        let mut right = tree(a);
        merge(&mut right, bc);
        assert_eq!(left, tree(json!({ "x": { "q": 2 } })));
        assert_eq!(right, tree(json!({ "x": { "p": 1, "q": 2 } })));
    }

    #[test]
    fn the_merged_value_keeps_the_origin_of_the_layer_that_won() {
        let (first, second) = (Origin::from("first"), Origin::from("second"));
        let mut base = from_json(&json!({ "a": 1, "b": 2 }), &first, 1, 8).unwrap();
        merge(
            &mut base,
            from_json(&json!({ "a": 3 }), &second, 1, 8).unwrap(),
        );
        let Kind::Table(map) = &base.kind else {
            panic!("a table")
        };
        assert_eq!(map["a"].origin.as_deref(), Some("second"));
        assert_eq!(map["b"].origin.as_deref(), Some("first"));
    }

    #[test]
    fn insert_at_turns_scalars_on_the_way_into_tables() {
        let mut root = tree(json!({ "a": 1 }));
        insert_at(
            &mut root,
            &["a".to_owned(), "b".to_owned()],
            Value::new(None, Kind::Int(2)),
        );
        assert_eq!(root, tree(json!({ "a": { "b": 2 } })));
        insert_at(&mut root, &[], tree(json!({ "z": true })));
        assert_eq!(root, tree(json!({ "z": true })));
    }

    #[test]
    fn conversions_refuse_containers_below_the_limit() {
        let origin = Origin::from("test");
        let deep = json!({ "a": { "b": { "c": 1 } } });
        assert!(from_json(&deep, &origin, 1, 3).is_ok());
        assert!(from_json(&deep, &origin, 1, 2).is_err());
        assert!(from_json(&deep, &origin, 2, 3).is_err());
        // Scalars are not containers: a scalar at the limit is fine.
        assert!(from_json(&json!(1), &origin, 9, 3).is_ok());
        let toml: toml::Table = "a = { b = [1, [2]] }".parse().unwrap();
        assert!(from_toml(toml::Value::Table(toml.clone()), &origin, 1, 4).is_ok());
        assert!(from_toml(toml::Value::Table(toml), &origin, 1, 3).is_err());
    }

    #[test]
    fn json_numbers_keep_their_kind() {
        let value = tree(json!({ "i": -1, "u": u64::MAX, "f": 1.5, "w": 2.0 }));
        let Kind::Table(map) = value.kind else {
            panic!("a table")
        };
        assert!(matches!(map["i"].kind, Kind::Int(-1)));
        assert!(matches!(map["u"].kind, Kind::UInt(u64::MAX)));
        assert!(matches!(map["f"].kind, Kind::Float(f) if f == 1.5));
        assert!(matches!(map["w"].kind, Kind::Float(f) if f == 2.0));
    }

    #[test]
    fn depth_counts_containers() {
        assert_eq!(tree(json!(1)).depth(), 0);
        assert_eq!(tree(json!({})).depth(), 1);
        assert_eq!(tree(json!({ "a": [[1]] })).depth(), 3);
    }

    #[test]
    fn debug_never_prints_secrets() {
        let value =
            tree(json!({ "password": "hunter2", "db": { "url": "postgres://u:hunter2@h/d" } }));
        let debug = format!("{value:?}");
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(debug.contains("postgres://u:[redacted]@h/d"), "{debug}");
    }
}
