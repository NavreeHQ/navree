//! The objects of a PDF: [`Object`], [`Dictionary`] and [`Stream`].

use std::fmt;

use bytes::Bytes;
use indexmap::IndexMap;

/// An object's number and generation, as an indirect reference names it.
pub type ObjectId = (u32, u16);

/// How a string is written: `(literal)` or `<hexadecimal>`. Reading keeps
/// the form the file used; writing honours it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StringFormat {
    /// `(...)`, with escapes where needed.
    Literal,
    /// `<...>`, two hex digits a byte.
    Hexadecimal,
}

/// A PDF object.
#[derive(Clone, PartialEq)]
pub enum Object {
    /// `null`.
    Null,
    /// `true` or `false`.
    Boolean(bool),
    /// An integer.
    Integer(i64),
    /// A real number.
    Real(f32),
    /// A name, without its slash and with `#xx` escapes decoded.
    Name(Vec<u8>),
    /// A string's bytes, with escapes decoded.
    String(Vec<u8>, StringFormat),
    /// An array.
    Array(Vec<Object>),
    /// A dictionary.
    Dictionary(Dictionary),
    /// A stream: a dictionary and the bytes after it, as stored (still
    /// filtered).
    Stream(Stream),
    /// An indirect reference (`12 0 R`).
    Reference(ObjectId),
}

impl Object {
    /// A name from its text.
    pub fn name(name: impl AsRef<[u8]>) -> Object {
        Object::Name(name.as_ref().to_vec())
    }

    /// A literal string of `text`'s bytes.
    pub fn string_literal(text: impl Into<Vec<u8>>) -> Object {
        Object::String(text.into(), StringFormat::Literal)
    }

    /// The integer.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Object::Integer(value) => Some(*value),
            _ => None,
        }
    }

    /// An integer or a real, as a real.
    pub fn as_float(&self) -> Option<f32> {
        match self {
            Object::Integer(value) => Some(*value as f32),
            Object::Real(value) => Some(*value),
            _ => None,
        }
    }

    /// The boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Object::Boolean(value) => Some(*value),
            _ => None,
        }
    }

    /// The name's bytes.
    pub fn as_name(&self) -> Option<&[u8]> {
        match self {
            Object::Name(name) => Some(name),
            _ => None,
        }
    }

    /// The string's bytes.
    pub fn as_str(&self) -> Option<&[u8]> {
        match self {
            Object::String(bytes, _) => Some(bytes),
            _ => None,
        }
    }

    /// The array's items.
    pub fn as_array(&self) -> Option<&Vec<Object>> {
        match self {
            Object::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The array's items, to change.
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Object>> {
        match self {
            Object::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The dictionary (not a stream's: see [`as_stream`](Self::as_stream)).
    pub fn as_dict(&self) -> Option<&Dictionary> {
        match self {
            Object::Dictionary(dictionary) => Some(dictionary),
            _ => None,
        }
    }

    /// The dictionary, to change.
    pub fn as_dict_mut(&mut self) -> Option<&mut Dictionary> {
        match self {
            Object::Dictionary(dictionary) => Some(dictionary),
            _ => None,
        }
    }

    /// The stream.
    pub fn as_stream(&self) -> Option<&Stream> {
        match self {
            Object::Stream(stream) => Some(stream),
            _ => None,
        }
    }

    /// The stream, to change.
    pub fn as_stream_mut(&mut self) -> Option<&mut Stream> {
        match self {
            Object::Stream(stream) => Some(stream),
            _ => None,
        }
    }

    /// The reference.
    pub fn as_reference(&self) -> Option<ObjectId> {
        match self {
            Object::Reference(id) => Some(*id),
            _ => None,
        }
    }

    /// The `Type` of a dictionary or of a stream's dictionary.
    pub fn type_name(&self) -> Option<&[u8]> {
        match self {
            Object::Dictionary(dictionary) => dictionary.type_name(),
            Object::Stream(stream) => stream.dict.type_name(),
            _ => None,
        }
    }
}

impl fmt::Debug for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Object::Null => f.write_str("null"),
            Object::Boolean(value) => write!(f, "{value}"),
            Object::Integer(value) => write!(f, "{value}"),
            Object::Real(value) => write!(f, "{value}"),
            Object::Name(name) => write!(f, "/{}", String::from_utf8_lossy(name)),
            Object::String(bytes, _) => write!(f, "({})", String::from_utf8_lossy(bytes)),
            Object::Array(items) => f.debug_list().entries(items).finish(),
            Object::Dictionary(dictionary) => dictionary.fmt(f),
            Object::Stream(stream) => stream.fmt(f),
            Object::Reference((number, generation)) => write!(f, "{number} {generation} R"),
        }
    }
}

impl From<bool> for Object {
    fn from(value: bool) -> Object {
        Object::Boolean(value)
    }
}

impl From<i64> for Object {
    fn from(value: i64) -> Object {
        Object::Integer(value)
    }
}

impl From<i32> for Object {
    fn from(value: i32) -> Object {
        Object::Integer(i64::from(value))
    }
}

impl From<u32> for Object {
    fn from(value: u32) -> Object {
        Object::Integer(i64::from(value))
    }
}

impl From<f32> for Object {
    fn from(value: f32) -> Object {
        Object::Real(value)
    }
}

/// A name.
impl From<&str> for Object {
    fn from(name: &str) -> Object {
        Object::Name(name.as_bytes().to_vec())
    }
}

impl From<Vec<Object>> for Object {
    fn from(items: Vec<Object>) -> Object {
        Object::Array(items)
    }
}

impl From<Dictionary> for Object {
    fn from(dictionary: Dictionary) -> Object {
        Object::Dictionary(dictionary)
    }
}

impl From<Stream> for Object {
    fn from(stream: Stream) -> Object {
        Object::Stream(stream)
    }
}

impl From<ObjectId> for Object {
    fn from(id: ObjectId) -> Object {
        Object::Reference(id)
    }
}

/// A dictionary: its entries in the order they were read or set. Two
/// dictionaries are equal when they hold the same entries, in any order.
#[derive(Clone, Default, PartialEq)]
pub struct Dictionary(IndexMap<Vec<u8>, Object>);

impl Dictionary {
    /// An empty dictionary.
    pub fn new() -> Dictionary {
        Dictionary(IndexMap::new())
    }

    /// The value of `key`.
    pub fn get(&self, key: &[u8]) -> Option<&Object> {
        self.0.get(key)
    }

    /// The value of `key`, to change.
    pub fn get_mut(&mut self, key: &[u8]) -> Option<&mut Object> {
        self.0.get_mut(key)
    }

    /// Whether the dictionary has `key`.
    pub fn has(&self, key: &[u8]) -> bool {
        self.0.contains_key(key)
    }

    /// Sets `key` to `value`: a new key goes last, a key already there keeps
    /// its place.
    pub fn set(&mut self, key: impl Into<Vec<u8>>, value: impl Into<Object>) {
        self.0.insert(key.into(), value.into());
    }

    /// Removes `key`, keeping the order of the others.
    pub fn remove(&mut self, key: &[u8]) -> Option<Object> {
        self.0.shift_remove(key)
    }

    /// The entries, in order.
    pub fn iter(&self) -> impl Iterator<Item = (&Vec<u8>, &Object)> {
        self.0.iter()
    }

    /// The entries, in order, to change.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Vec<u8>, &mut Object)> {
        self.0.iter_mut()
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Lets go of the room kept for entries to come.
    pub(crate) fn shrink_to_fit(&mut self) {
        self.0.shrink_to_fit();
    }

    /// The name in `Type`.
    pub fn type_name(&self) -> Option<&[u8]> {
        self.get(b"Type").and_then(Object::as_name)
    }

    /// Whether `Type` is the name `name`.
    pub fn has_type(&self, name: &[u8]) -> bool {
        self.type_name() == Some(name)
    }
}

impl fmt::Debug for Dictionary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(
                self.0
                    .iter()
                    .map(|(key, value)| (format!("/{}", String::from_utf8_lossy(key)), value)),
            )
            .finish()
    }
}

impl<K: Into<Vec<u8>>, V: Into<Object>> FromIterator<(K, V)> for Dictionary {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(entries: I) -> Dictionary {
        let mut dictionary = Dictionary::new();
        for (key, value) in entries {
            dictionary.set(key, value);
        }
        dictionary
    }
}

/// A stream: its dictionary and its bytes as stored, still filtered (see
/// [`Document::decode`](crate::Document::decode)).
#[derive(Clone, PartialEq)]
pub struct Stream {
    /// The dictionary. Writing sets its `Length`.
    pub dict: Dictionary,
    /// The bytes between `stream` and `endstream`.
    pub content: Bytes,
}

impl Stream {
    /// A stream of `content` with `dict`.
    pub fn new(dict: Dictionary, content: impl Into<Bytes>) -> Stream {
        Stream {
            dict,
            content: content.into(),
        }
    }

    /// The names in `Filter`, in the order they decode.
    pub fn filters(&self) -> Vec<&[u8]> {
        match self.dict.get(b"Filter") {
            Some(Object::Name(name)) => vec![name.as_slice()],
            Some(Object::Array(names)) => names.iter().filter_map(Object::as_name).collect(),
            _ => Vec::new(),
        }
    }
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} stream({} bytes)", self.dict, self.content.len())
    }
}

/// A [`Dictionary`] from `key => value` pairs; keys are names, values
/// anything that converts into an [`Object`].
///
/// ```
/// use navree_pdf::{Object, dictionary};
///
/// let font = dictionary! { "Type" => "Font", "Subtype" => "Type1", "Size" => 12 };
/// assert_eq!(font.get(b"Size"), Some(&Object::Integer(12)));
/// ```
#[macro_export]
macro_rules! dictionary {
    () => { $crate::Dictionary::new() };
    ($($key:expr => $value:expr),+ $(,)?) => {{
        let mut dictionary = $crate::Dictionary::new();
        $(dictionary.set($key, $value);)+
        dictionary
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_return_what_the_object_holds() {
        assert_eq!(Object::Integer(3).as_float(), Some(3.0));
        assert_eq!(Object::Real(1.5).as_float(), Some(1.5));
        assert_eq!(Object::Real(1.5).as_i64(), None);
        assert_eq!(Object::from("Font").as_name(), Some(&b"Font"[..]));
        assert_eq!(Object::Reference((4, 0)).as_reference(), Some((4, 0)));
        let dict = dictionary! { "Type" => "Page", "Count" => 2 };
        assert!(dict.has_type(b"Page"));
        assert_eq!(Object::from(dict).type_name(), Some(&b"Page"[..]));
    }

    #[test]
    fn dictionaries_keep_their_order_and_compare_as_maps() {
        let mut a = dictionary! { "A" => 1, "B" => 2 };
        a.set("A", 3);
        let keys: Vec<&[u8]> = a.iter().map(|(k, _)| k.as_slice()).collect();
        assert_eq!(keys, [&b"A"[..], b"B"]);
        let b = dictionary! { "B" => 2, "A" => 3 };
        assert_eq!(a, b);
        a.remove(b"A");
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn filters_are_a_name_or_an_array_of_names() {
        let one = Stream::new(dictionary! { "Filter" => "FlateDecode" }, Vec::new());
        assert_eq!(one.filters(), [&b"FlateDecode"[..]]);
        let two = Stream::new(
            dictionary! { "Filter" => vec![Object::from("ASCII85Decode"), Object::from("FlateDecode")] },
            Vec::new(),
        );
        assert_eq!(two.filters().len(), 2);
        assert!(
            Stream::new(Dictionary::new(), Vec::new())
                .filters()
                .is_empty()
        );
    }
}
