//! [`Value`]: the data a template sees.
//!
//! A value is plain data (undefined, none, booleans, numbers, strings,
//! sequences and maps) or one of the engine's own objects (functions,
//! macros, imported modules, the `loop` variable). Templates reach nothing
//! else: there is no way from a template to a Rust method, a field that the
//! context did not serialize, or the file system.

use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use crate::error::{Error, ErrorKind};

mod convert;
mod kwargs;
mod map;
pub(crate) mod ops;
mod ser;

pub use kwargs::Kwargs;
pub(crate) use map::ValueMap;

/// A template value. Cheap to clone: strings, sequences and maps are shared.
#[derive(Clone)]
pub struct Value(pub(crate) Repr);

#[derive(Clone)]
pub(crate) enum Repr {
    /// `true` for the silent undefined of an `if` expression without `else`.
    Undefined(bool),
    None,
    Bool(bool),
    Int(i128),
    Float(f64),
    /// The text and what it is safe for.
    Str(Arc<str>, Safety),
    Seq(Arc<Vec<Value>>, SeqKind, Shape),
    Map(Arc<ValueMap>, Shape),
    Kwargs(Arc<ValueMap>, Shape),
    Function(Arc<crate::functions::NativeFunction>),
    Macro(Arc<crate::vm::MacroValue>),
    Module(Arc<Module>),
    Loop(Arc<crate::vm::LoopObject>),
}

/// What a string has already been made safe for. A mark holds only for the
/// mode it was made in: printed in another, the string is escaped again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Safety {
    /// Nothing: printing escapes it.
    Plain,
    /// The author's word (`|safe`, [`Value::from_safe_string`]): it prints as
    /// it is in every mode.
    Trusted,
    /// Escaped for HTML.
    Html,
    /// Printed as JSON.
    Json,
    /// Printed in a mode that is both.
    Both,
    /// JSON with `<`, `>`, `&` and `'` in its strings escaped (`|tojson`):
    /// fit for JSON, and for HTML once its quotes suit where it lands.
    Tojson,
}

/// The flavour of a sequence.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SeqKind {
    List,
    Tuple,
    /// A lazily produced sequence in other engines (`range`, `items`).
    Iterable,
}

/// The kind of a [`Value`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ValueKind {
    /// An undefined value.
    Undefined,
    /// `none`.
    None,
    /// A boolean.
    Bool,
    /// An integer or a float.
    Number,
    /// A string.
    String,
    /// A list or a tuple.
    Seq,
    /// A map.
    Map,
    /// A sequence produced on demand (`range(..)`, `|items`).
    Iterable,
    /// Anything else: a function, a macro, the `loop` variable.
    Plain,
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ValueKind::Undefined => "undefined",
            ValueKind::None => "none",
            ValueKind::Bool => "bool",
            ValueKind::Number => "number",
            ValueKind::String => "string",
            ValueKind::Seq => "sequence",
            ValueKind::Map => "map",
            ValueKind::Iterable => "iterator",
            ValueKind::Plain => "plain object",
        })
    }
}

/// How deep sequences and maps may nest in a value: printing, comparing and
/// dropping a value recurse into it.
pub(crate) const MAX_VALUE_DEPTH: u16 = 200;

/// How a container is built: printing or comparing a value goes through
/// every nested item, and items can be shared, so a small template could
/// otherwise build a value that takes forever to walk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Shape {
    /// How deep containers nest.
    pub(crate) depth: u16,
    /// How much a walk goes through: values, and text by the 16 bytes.
    pub(crate) weight: u64,
}

impl Shape {
    fn of<'a>(items: impl Iterator<Item = &'a Value>) -> Shape {
        let mut shape = Shape {
            depth: 0,
            weight: 1,
        };
        for item in items {
            let inner = item.shape();
            shape.depth = shape.depth.max(inner.depth);
            shape.weight = shape.weight.saturating_add(inner.weight);
        }
        shape.depth = shape.depth.saturating_add(1);
        shape
    }
}

/// An imported template: its exported names and its rendered output.
pub(crate) struct Module {
    pub(crate) values: ValueMap,
    pub(crate) captured: String,
    pub(crate) shape: Shape,
}

impl Module {
    pub(crate) fn new(values: ValueMap, captured: String) -> Module {
        let mut shape = Shape::of(values.iter().flat_map(|(key, value)| [key, value]));
        // Printing a module prints its output.
        shape.weight = shape.weight.saturating_add((captured.len() / 16) as u64);
        Module {
            values,
            captured,
            shape,
        }
    }
}

impl Value {
    /// The undefined value.
    pub const UNDEFINED: Value = Value(Repr::Undefined(false));

    /// The silent undefined of `a if b` without an `else`, which prints as
    /// nothing even where undefined values are errors.
    pub(crate) const SILENT_UNDEFINED: Value = Value(Repr::Undefined(true));

    /// `none`.
    pub const NONE: Value = Value(Repr::None);

    /// Serializes `value` into a template value. Structs and maps become
    /// maps, sequences and tuples become lists, unit and `None` become
    /// `none`, enum variants become their name (or a one-key map).
    pub fn from_serialize<T: serde::Serialize + ?Sized>(value: &T) -> Result<Value, Error> {
        value.serialize(ser::ValueSerializer)
    }

    /// A string printed as it is, even where printing escapes HTML.
    pub fn from_safe_string(value: String) -> Value {
        Value(Repr::Str(Arc::from(value), Safety::Trusted))
    }

    /// `text` with the mark `safety`. The string is shared, not copied.
    pub(crate) fn marked(text: Arc<str>, safety: Safety) -> Value {
        Value(Repr::Str(text, safety))
    }

    fn seq(values: Vec<Value>, kind: SeqKind) -> Value {
        let shape = Shape::of(values.iter());
        Value(Repr::Seq(Arc::new(values), kind, shape))
    }

    pub(crate) fn list(values: Vec<Value>) -> Value {
        Value::seq(values, SeqKind::List)
    }

    pub(crate) fn tuple(values: Vec<Value>) -> Value {
        Value::seq(values, SeqKind::Tuple)
    }

    pub(crate) fn iterable(values: Vec<Value>) -> Value {
        Value::seq(values, SeqKind::Iterable)
    }

    pub(crate) fn map(map: ValueMap) -> Value {
        let shape = Shape::of(map.iter().flat_map(|(key, value)| [key, value]));
        Value(Repr::Map(Arc::new(map), shape))
    }

    /// The shape of a container holding `items`.
    pub(crate) fn iterable_shape(items: &[Value]) -> Shape {
        Shape::of(items.iter())
    }

    /// How the value is built.
    pub(crate) fn shape(&self) -> Shape {
        match &self.0 {
            Repr::Seq(_, _, shape) | Repr::Map(_, shape) | Repr::Kwargs(_, shape) => *shape,
            Repr::Module(module) => module.shape,
            Repr::Loop(object) => object.shape(),
            Repr::Str(text, _) => Shape {
                depth: 0,
                weight: 1 + (text.len() / 16) as u64,
            },
            _ => Shape {
                depth: 0,
                weight: 1,
            },
        }
    }

    /// Fails when the value nests deeper than [`MAX_VALUE_DEPTH`].
    pub(crate) fn check_depth(self) -> Result<Value, Error> {
        if self.shape().depth > MAX_VALUE_DEPTH {
            return Err(Error::limit("value nests too deeply"));
        }
        Ok(self)
    }

    /// The value as text, failing when that exceeds `max_bytes`.
    pub(crate) fn to_string_capped(&self, max_bytes: usize) -> Result<String, Error> {
        use std::fmt::Write as _;
        if let Repr::Str(text, _) = &self.0 {
            if text.len() > max_bytes {
                return Err(Error::limit("a string is too large"));
            }
            return Ok(text.to_string());
        }
        let mut out = CappedString {
            text: String::new(),
            max_bytes,
        };
        write!(out, "{self}").map_err(|_| Error::limit("a string is too large"))?;
        Ok(out.text)
    }

    pub(crate) fn kwargs(map: ValueMap) -> Value {
        let shape = Shape::of(map.iter().flat_map(|(key, value)| [key, value]));
        Value(Repr::Kwargs(Arc::new(map), shape))
    }

    /// The kind of value.
    pub fn kind(&self) -> ValueKind {
        match &self.0 {
            Repr::Undefined(_) => ValueKind::Undefined,
            Repr::None => ValueKind::None,
            Repr::Bool(_) => ValueKind::Bool,
            Repr::Int(_) | Repr::Float(_) => ValueKind::Number,
            Repr::Str(..) => ValueKind::String,
            Repr::Seq(_, SeqKind::Iterable, _) => ValueKind::Iterable,
            Repr::Seq(..) => ValueKind::Seq,
            Repr::Map(..) | Repr::Kwargs(..) | Repr::Module(_) => ValueKind::Map,
            Repr::Function(_) | Repr::Macro(_) | Repr::Loop(_) => ValueKind::Plain,
        }
    }

    /// Whether the value is undefined.
    pub fn is_undefined(&self) -> bool {
        matches!(self.0, Repr::Undefined(_))
    }

    pub(crate) fn is_silent_undefined(&self) -> bool {
        matches!(self.0, Repr::Undefined(true))
    }

    /// Whether the value is `none`.
    pub fn is_none(&self) -> bool {
        matches!(self.0, Repr::None)
    }

    /// Whether the value is a safe string (printed without escaping).
    pub fn is_safe(&self) -> bool {
        matches!(&self.0, Repr::Str(_, safety) if *safety != Safety::Plain)
    }

    /// What the value is safe for, for a string.
    pub(crate) fn safety(&self) -> Safety {
        match &self.0 {
            Repr::Str(_, safety) => *safety,
            _ => Safety::Plain,
        }
    }

    /// Whether the value is an integer.
    pub fn is_integer(&self) -> bool {
        matches!(self.0, Repr::Int(_))
    }

    /// Whether the value is a number (an integer or a float).
    pub fn is_number(&self) -> bool {
        matches!(self.0, Repr::Int(_) | Repr::Float(_))
    }

    pub(crate) fn is_kwargs(&self) -> bool {
        matches!(self.0, Repr::Kwargs(..))
    }

    pub(crate) fn is_tuple(&self) -> bool {
        matches!(self.0, Repr::Seq(_, SeqKind::Tuple, _))
    }

    /// Whether the value is true in a condition: non-zero numbers, non-empty
    /// strings, sequences and maps, `true`, and functions or macros.
    pub fn is_true(&self) -> bool {
        match &self.0 {
            Repr::Bool(value) => *value,
            Repr::Int(value) => *value != 0,
            Repr::Float(value) => *value != 0.0,
            Repr::Str(value, _) => !value.is_empty(),
            Repr::None | Repr::Undefined(_) => false,
            Repr::Seq(values, ..) => !values.is_empty(),
            Repr::Map(map, _) | Repr::Kwargs(map, _) => !map.is_empty(),
            Repr::Module(module) => !module.values.is_empty(),
            Repr::Function(_) | Repr::Macro(_) | Repr::Loop(_) => true,
        }
    }

    /// The string, if the value is one.
    pub fn as_str(&self) -> Option<&str> {
        match &self.0 {
            Repr::Str(value, _) => Some(value),
            _ => None,
        }
    }

    /// The integer as an `i64`, if the value is an integer that fits.
    pub fn as_i64(&self) -> Option<i64> {
        match self.0 {
            Repr::Int(value) => i64::try_from(value).ok(),
            _ => None,
        }
    }

    pub(crate) fn as_usize(&self) -> Option<usize> {
        match self.0 {
            Repr::Int(value) => usize::try_from(value).ok(),
            _ => None,
        }
    }

    /// The number of characters of a string, of items of a sequence or of
    /// entries of a map.
    pub fn len(&self) -> Option<usize> {
        match &self.0 {
            Repr::Str(value, _) => Some(value.chars().count()),
            Repr::Seq(values, ..) => Some(values.len()),
            Repr::Map(map, _) | Repr::Kwargs(map, _) => Some(map.len()),
            Repr::Module(module) => Some(module.values.len()),
            _ => None,
        }
    }

    /// Whether [`len`](Value::len) is zero.
    pub fn is_empty(&self) -> bool {
        self.len() == Some(0)
    }

    /// Looks up attribute `key` of a map (or of the engine's objects).
    /// Undefined when there is no such attribute; an error on an undefined
    /// value.
    pub fn get_attr(&self, key: &str) -> Result<Value, Error> {
        if self.is_undefined() {
            return Err(Error::undefined());
        }
        Ok(self.get_attr_fast(key).unwrap_or(Value::UNDEFINED))
    }

    /// Looks up `key` in a map, or an index (negative from the end) in a
    /// sequence or string. Undefined when there is no such item; an error on
    /// an undefined value.
    pub fn get_item(&self, key: &Value) -> Result<Value, Error> {
        if self.is_undefined() {
            return Err(Error::undefined());
        }
        Ok(self.get_item_opt(key).unwrap_or(Value::UNDEFINED))
    }

    pub(crate) fn get_attr_fast(&self, key: &str) -> Option<Value> {
        match &self.0 {
            Repr::Map(map, _) | Repr::Kwargs(map, _) => map.get(&Value::from(key)).cloned(),
            Repr::Module(module) => module.values.get(&Value::from(key)).cloned(),
            Repr::Loop(object) => object.attr(key),
            Repr::Macro(object) => object.attr(key),
            _ => None,
        }
    }

    pub(crate) fn get_item_opt(&self, key: &Value) -> Option<Value> {
        fn index(key: &Value, len: usize) -> Option<usize> {
            let index = isize::try_from(key.as_i64()?).ok()?;
            if index < 0 {
                len.checked_sub(index.unsigned_abs())
            } else {
                Some(index.unsigned_abs())
            }
        }
        match &self.0 {
            Repr::Map(map, _) | Repr::Kwargs(map, _) => map.get(key).cloned(),
            Repr::Module(module) => module.values.get(key).cloned(),
            Repr::Seq(values, ..) => values.get(index(key, values.len())?).cloned(),
            Repr::Str(text, _) => {
                let position = index(key, text.chars().count())?;
                text.chars().nth(position).map(Value::from)
            }
            Repr::Loop(object) => object.attr(key.as_str()?),
            Repr::Macro(object) => object.attr(key.as_str()?),
            _ => None,
        }
    }

    /// The items iterating over the value gives: the characters of a
    /// string, the items of a sequence, the keys of a map, nothing for
    /// undefined and `none`.
    pub fn try_iter(&self) -> Result<std::vec::IntoIter<Value>, Error> {
        self.iter_items().map(Vec::into_iter)
    }

    pub(crate) fn iter_items(&self) -> Result<Vec<Value>, Error> {
        Ok(match &self.0 {
            Repr::None | Repr::Undefined(_) => Vec::new(),
            Repr::Str(text, _) => text.chars().map(Value::from).collect(),
            Repr::Seq(values, ..) => values.to_vec(),
            Repr::Map(map, _) | Repr::Kwargs(map, _) => map.keys().cloned().collect(),
            Repr::Module(module) => module.values.keys().cloned().collect(),
            _ => {
                return Err(Error::invalid(format!("{} is not iterable", self.kind())));
            }
        })
    }

    /// The number of items [`iter_items`](Value::iter_items) would give,
    /// without making them.
    pub(crate) fn iter_len(&self) -> Option<usize> {
        match &self.0 {
            Repr::None | Repr::Undefined(_) => Some(0),
            _ => self.len(),
        }
    }

    /// The entries of a map.
    pub(crate) fn as_map(&self) -> Option<&ValueMap> {
        match &self.0 {
            Repr::Map(map, _) | Repr::Kwargs(map, _) => Some(map),
            Repr::Module(module) => Some(&module.values),
            _ => None,
        }
    }
}

/// Text that stops growing at a limit.
struct CappedString {
    text: String,
    max_bytes: usize,
}

impl fmt::Write for CappedString {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.text.len().saturating_add(text.len()) > self.max_bytes {
            return Err(fmt::Error);
        }
        self.text.push_str(text);
        Ok(())
    }
}

/// The value as a float: booleans and integers convert (exactly unless
/// `lossy`), floats as they are.
pub(crate) fn as_f64(value: &Value, lossy: bool) -> Option<f64> {
    match value.0 {
        Repr::Bool(value) => Some(if value { 1.0 } else { 0.0 }),
        Repr::Float(value) => Some(value),
        Repr::Int(value) => {
            #[allow(clippy::cast_precision_loss)]
            let float = value as f64;
            #[allow(clippy::cast_possible_truncation)]
            (lossy || (float.is_finite() && float as i128 == value)).then_some(float)
        }
        _ => None,
    }
}

/// Two values brought to a common type for arithmetic and comparison.
pub(crate) enum Coerced<'a> {
    Int(i128, i128),
    Float(f64, f64),
    Str(&'a str, &'a str),
}

pub(crate) fn coerce<'a>(a: &'a Value, b: &'a Value, lossy: bool) -> Option<Coerced<'a>> {
    match (&a.0, &b.0) {
        (Repr::Str(a, _), Repr::Str(b, _)) => Some(Coerced::Str(a, b)),
        (Repr::Float(a), Repr::Float(b)) => Some(Coerced::Float(*a, *b)),
        (Repr::Float(a), _) => Some(Coerced::Float(*a, as_f64(b, lossy)?)),
        (_, Repr::Float(b)) => Some(Coerced::Float(as_f64(a, lossy)?, *b)),
        _ => Some(Coerced::Int(as_int(a)?, as_int(b)?)),
    }
}

/// Booleans and integers as an integer.
pub(crate) fn as_int(value: &Value) -> Option<i128> {
    match value.0 {
        Repr::Bool(value) => Some(i128::from(value)),
        Repr::Int(value) => Some(value),
        _ => None,
    }
}

fn cmp_f64(left: f64, right: f64) -> Ordering {
    if left == right {
        Ordering::Equal
    } else {
        left.total_cmp(&right)
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Value) -> bool {
        match (&self.0, &other.0) {
            (Repr::None, Repr::None) | (Repr::Undefined(_), Repr::Undefined(_)) => true,
            (Repr::Str(a, _), Repr::Str(b, _)) => a == b,
            _ => match coerce(self, other, false) {
                Some(Coerced::Int(a, b)) => a == b,
                Some(Coerced::Float(a, b)) => a == b,
                Some(Coerced::Str(a, b)) => a == b,
                None => container_eq(self, other),
            },
        }
    }
}

fn container_eq(a: &Value, b: &Value) -> bool {
    match (&a.0, &b.0) {
        (Repr::Seq(a, a_kind, _), Repr::Seq(b, b_kind, _)) => {
            (*a_kind == SeqKind::Tuple) == (*b_kind == SeqKind::Tuple) && a == b
        }
        (Repr::Map(..) | Repr::Kwargs(..) | Repr::Module(_), _)
        | (_, Repr::Map(..) | Repr::Kwargs(..) | Repr::Module(_)) => {
            match (a.as_map(), b.as_map()) {
                (Some(a), Some(b)) => {
                    a.len() == b.len() && a.iter().all(|(key, value)| b.get(key) == Some(value))
                }
                _ => false,
            }
        }
        (Repr::Function(a), Repr::Function(b)) => Arc::ptr_eq(a, b),
        (Repr::Macro(a), Repr::Macro(b)) => Arc::ptr_eq(a, b),
        (Repr::Loop(a), Repr::Loop(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Value) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Value) -> Ordering {
        let by_kind = self.kind().cmp(&other.kind());
        if by_kind != Ordering::Equal {
            return by_kind;
        }
        match (&self.0, &other.0) {
            (Repr::None, Repr::None) | (Repr::Undefined(_), Repr::Undefined(_)) => Ordering::Equal,
            (Repr::Str(a, _), Repr::Str(b, _)) => a.cmp(b),
            _ => match coerce(self, other, false) {
                Some(Coerced::Float(a, b)) => cmp_f64(a, b),
                Some(Coerced::Int(a, b)) => a.cmp(&b),
                Some(Coerced::Str(a, b)) => a.cmp(b),
                None => match (&self.0, &other.0) {
                    (Repr::Int(a), Repr::Float(b)) => cmp_f64_int(*b, *a).reverse(),
                    (Repr::Float(a), Repr::Int(b)) => cmp_f64_int(*a, *b),
                    (Repr::Seq(a, a_kind, _), Repr::Seq(b, b_kind, _)) => (*a_kind
                        == SeqKind::Tuple)
                        .cmp(&(*b_kind == SeqKind::Tuple))
                        .then_with(|| a.iter().cmp(b.iter())),
                    _ => match (self.as_map(), other.as_map()) {
                        (Some(a), Some(b)) => a.iter().cmp(b.iter()),
                        _ => self.to_string().cmp(&other.to_string()),
                    },
                },
            },
        }
    }
}

/// Compares a float with an integer that has no exact float.
fn cmp_f64_int(left: f64, right: i128) -> Ordering {
    #[allow(clippy::cast_precision_loss)]
    match cmp_f64(left, right as f64) {
        Ordering::Equal if left.is_finite() =>
        {
            #[allow(clippy::cast_precision_loss)]
            if left >= i128::MAX as f64 {
                Ordering::Greater
            } else {
                let trunc = left.trunc();
                #[allow(clippy::cast_possible_truncation)]
                (trunc as i128)
                    .cmp(&right)
                    .then_with(|| left.partial_cmp(&trunc).unwrap_or(Ordering::Equal))
            }
        }
        other => other,
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Repr::Undefined(_) => Ok(()),
            Repr::None => f.write_str("None"),
            Repr::Bool(value) => f.write_str(if *value { "True" } else { "False" }),
            Repr::Int(value) => write!(f, "{value}"),
            Repr::Float(value) => write_float(f, *value),
            Repr::Str(value, _) => f.write_str(value),
            Repr::Module(module) => f.write_str(&module.captured),
            _ => fmt::Debug::fmt(self, f),
        }
    }
}

/// A float as templates print it: `1.0`, `0.5`, `NaN`, `inf`.
fn write_float(f: &mut fmt::Formatter<'_>, value: f64) -> fmt::Result {
    if value.is_nan() {
        f.write_str("NaN")
    } else if value.is_infinite() {
        f.write_str(if value.is_sign_negative() {
            "-inf"
        } else {
            "inf"
        })
    } else {
        let mut text = value.to_string();
        if !text.contains('.') {
            text.push_str(".0");
        }
        f.write_str(&text)
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Repr::Undefined(_) => f.write_str("undefined"),
            Repr::None => f.write_str("None"),
            Repr::Bool(value) => f.write_str(if *value { "True" } else { "False" }),
            Repr::Int(value) => write!(f, "{value}"),
            Repr::Float(value) => write!(f, "{value:?}"),
            Repr::Str(value, _) => python_string_repr(value, f),
            Repr::Seq(values, SeqKind::Tuple, _) => {
                f.write_str("(")?;
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{value:?}")?;
                }
                if values.len() == 1 {
                    f.write_str(",")?;
                }
                f.write_str(")")
            }
            Repr::Seq(values, ..) => f.debug_list().entries(values.iter()).finish(),
            Repr::Map(map, _) | Repr::Kwargs(map, _) => f.debug_map().entries(map.iter()).finish(),
            Repr::Module(module) => f.debug_map().entries(module.values.iter()).finish(),
            Repr::Function(function) => write!(f, "<function {}>", function.name),
            Repr::Macro(object) => write!(f, "<macro {}>", object.name()),
            Repr::Loop(object) => object.render(f),
        }
    }
}

/// A string the way Python's `repr` writes it.
fn python_string_repr(value: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    write!(f, "{quote}")?;
    for ch in value.chars() {
        match ch {
            '\'' if quote == '\'' => f.write_str("\\'")?,
            '"' if quote == '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            ch if ch.is_control() => {
                let code = u32::from(ch);
                if code <= 0xff {
                    write!(f, "\\x{code:02x}")?;
                } else if code <= 0xffff {
                    write!(f, "\\u{code:04x}")?;
                } else {
                    write!(f, "\\U{code:08x}")?;
                }
            }
            ch => write!(f, "{ch}")?,
        }
    }
    write!(f, "{quote}")
}

impl serde::Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeSeq};
        match &self.0 {
            Repr::Undefined(_) | Repr::None => serializer.serialize_unit(),
            Repr::Bool(value) => serializer.serialize_bool(*value),
            Repr::Int(value) => match i64::try_from(*value) {
                Ok(value) => serializer.serialize_i64(value),
                Err(_) => match u64::try_from(*value) {
                    Ok(value) => serializer.serialize_u64(value),
                    Err(_) => serializer.serialize_i128(*value),
                },
            },
            Repr::Float(value) => serializer.serialize_f64(*value),
            Repr::Str(value, _) => serializer.serialize_str(value),
            Repr::Seq(values, ..) => {
                let mut seq = serializer.serialize_seq(Some(values.len()))?;
                for value in values.iter() {
                    seq.serialize_element(value)?;
                }
                seq.end()
            }
            Repr::Map(map, _) | Repr::Kwargs(map, _) => {
                let mut out = serializer.serialize_map(Some(map.len()))?;
                for (key, value) in map.iter() {
                    out.serialize_entry(key, value)?;
                }
                out.end()
            }
            Repr::Module(module) => {
                let mut out = serializer.serialize_map(Some(module.values.len()))?;
                for (key, value) in module.values.iter() {
                    out.serialize_entry(key, value)?;
                }
                out.end()
            }
            _ => serializer.serialize_str(&self.to_string()),
        }
    }
}

impl Default for Value {
    fn default() -> Value {
        Value::UNDEFINED
    }
}

/// An error for a value that cannot convert into `target`.
pub(crate) fn cannot_convert(value: &Value, target: &str) -> Error {
    Error::new(
        ErrorKind::InvalidOperation,
        format!("cannot convert {} to {target}", value.kind()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_like_templates_expect() {
        assert_eq!(Value::from(1.0).to_string(), "1.0");
        assert_eq!(Value::from(0.5).to_string(), "0.5");
        assert_eq!(Value::from(1e21).to_string(), "1000000000000000000000.0");
        assert_eq!(Value::from(f64::NAN).to_string(), "NaN");
        assert_eq!(Value::from(f64::NEG_INFINITY).to_string(), "-inf");
        assert_eq!(Value::from(true).to_string(), "True");
        assert_eq!(Value::NONE.to_string(), "None");
        assert_eq!(Value::UNDEFINED.to_string(), "");
        let list = Value::list(vec![Value::from(1), Value::from("a'b"), Value::from("x")]);
        assert_eq!(list.to_string(), r#"[1, "a'b", 'x']"#);
        assert_eq!(
            Value::tuple(vec![Value::from(1)]).to_string(),
            "(1,)".to_owned()
        );
    }

    #[test]
    fn compares_across_number_types() {
        assert_eq!(Value::from(1), Value::from(1.0));
        assert_eq!(Value::from(true), Value::from(1));
        assert_ne!(Value::from("1"), Value::from(1));
        assert!(Value::from(1) < Value::from(1.5));
        assert!(Value::from("a") < Value::from("b"));
        // Kinds order first: a number sorts before a string.
        assert!(Value::from(9) < Value::from("1"));
    }
}
