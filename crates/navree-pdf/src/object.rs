//! The objects of a PDF: [`Object`], [`Dictionary`] and [`Stream`].

use std::fmt;

use bytes::Bytes;
use indexmap::IndexMap;

pub use crate::name::Name;

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
    Name(Name),
    /// A string's bytes, with escapes decoded.
    String(Vec<u8>, StringFormat),
    /// An array.
    Array(Vec<Object>),
    /// A dictionary.
    Dictionary(Dictionary),
    /// A stream: a dictionary and the bytes after it, as stored (still
    /// filtered). Boxed, so that the many objects that are not streams do
    /// not each hold room for one.
    Stream(Box<Stream>),
    /// An indirect reference (`12 0 R`).
    Reference(ObjectId),
}

impl Object {
    /// A name from its text.
    pub fn name(name: impl AsRef<[u8]>) -> Object {
        Object::Name(Name::new(name.as_ref()))
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
            Object::Name(name) => Some(name.as_slice()),
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
        Object::Name(Name::from(name))
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
        Object::Stream(Box::new(stream))
    }
}

impl From<ObjectId> for Object {
    fn from(id: ObjectId) -> Object {
        Object::Reference(id)
    }
}

/// The entries a dictionary keeps in a list (16): real dictionaries have a
/// handful, and a list searched from its start is smaller and quicker than a
/// map for so few. One with more is kept in a map.
pub(crate) const LIST_MOST: usize = 16;

#[derive(Clone)]
enum Entries {
    List(Vec<(Name, Object)>),
    Map(Box<IndexMap<Name, Object>>),
}

/// A dictionary: its entries in the order they were read or set. Two
/// dictionaries are equal when they hold the same entries, in any order.
#[derive(Clone)]
pub struct Dictionary(Entries);

impl Default for Dictionary {
    fn default() -> Dictionary {
        Dictionary::new()
    }
}

impl PartialEq for Dictionary {
    fn eq(&self, other: &Dictionary) -> bool {
        self.len() == other.len()
            && self
                .iter()
                .all(|(key, value)| other.get(key).is_some_and(|found| found == value))
    }
}

/// The entries of a [`Dictionary`], in order.
pub struct Iter<'a>(IterRepr<'a>);

enum IterRepr<'a> {
    List(std::slice::Iter<'a, (Name, Object)>),
    Map(indexmap::map::Iter<'a, Name, Object>),
}

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a Name, &'a Object);

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            IterRepr::List(entries) => entries.next().map(|(key, value)| (key, value)),
            IterRepr::Map(entries) => entries.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.0 {
            IterRepr::List(entries) => entries.size_hint(),
            IterRepr::Map(entries) => entries.size_hint(),
        }
    }
}

/// The entries of a [`Dictionary`], in order, to change.
pub struct IterMut<'a>(IterMutRepr<'a>);

enum IterMutRepr<'a> {
    List(std::slice::IterMut<'a, (Name, Object)>),
    Map(indexmap::map::IterMut<'a, Name, Object>),
}

impl<'a> Iterator for IterMut<'a> {
    type Item = (&'a Name, &'a mut Object);

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            IterMutRepr::List(entries) => entries.next().map(|(key, value)| (&*key, value)),
            IterMutRepr::Map(entries) => entries.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.0 {
            IterMutRepr::List(entries) => entries.size_hint(),
            IterMutRepr::Map(entries) => entries.size_hint(),
        }
    }
}

impl Dictionary {
    /// An empty dictionary.
    pub fn new() -> Dictionary {
        Dictionary(Entries::List(Vec::new()))
    }

    /// The value of `key`.
    pub fn get(&self, key: &[u8]) -> Option<&Object> {
        match &self.0 {
            Entries::List(entries) => entries
                .iter()
                .find(|(name, _)| name.as_slice() == key)
                .map(|(_, value)| value),
            Entries::Map(entries) => entries.get(key),
        }
    }

    /// The value of `key`, to change.
    pub fn get_mut(&mut self, key: &[u8]) -> Option<&mut Object> {
        match &mut self.0 {
            Entries::List(entries) => entries
                .iter_mut()
                .find(|(name, _)| name.as_slice() == key)
                .map(|(_, value)| value),
            Entries::Map(entries) => entries.get_mut(key),
        }
    }

    /// Whether the dictionary has `key`.
    pub fn has(&self, key: &[u8]) -> bool {
        self.get(key).is_some()
    }

    /// Sets `key` to `value`: a new key goes last, a key already there keeps
    /// its place.
    pub fn set(&mut self, key: impl Into<Name>, value: impl Into<Object>) {
        self.set_name(key.into(), value.into());
    }

    pub(crate) fn set_name(&mut self, key: Name, value: Object) {
        match &mut self.0 {
            Entries::Map(entries) => {
                entries.insert(key, value);
            }
            Entries::List(entries) => {
                if let Some(entry) = entries.iter_mut().find(|(name, _)| *name == key) {
                    entry.1 = value;
                } else if entries.len() < LIST_MOST {
                    entries.push((key, value));
                } else {
                    let mut map: IndexMap<Name, Object> =
                        IndexMap::with_capacity(entries.len() + 1);
                    map.extend(std::mem::take(entries));
                    map.insert(key, value);
                    self.0 = Entries::Map(Box::new(map));
                }
            }
        }
    }

    /// Removes `key`, keeping the order of the others.
    pub fn remove(&mut self, key: &[u8]) -> Option<Object> {
        match &mut self.0 {
            Entries::List(entries) => {
                let at = entries
                    .iter()
                    .position(|(name, _)| name.as_slice() == key)?;
                Some(entries.remove(at).1)
            }
            Entries::Map(entries) => entries.shift_remove(key),
        }
    }

    /// The entries, in order.
    pub fn iter(&self) -> Iter<'_> {
        Iter(match &self.0 {
            Entries::List(entries) => IterRepr::List(entries.iter()),
            Entries::Map(entries) => IterRepr::Map(entries.iter()),
        })
    }

    /// The entries, in order, to change.
    pub fn iter_mut(&mut self) -> IterMut<'_> {
        IterMut(match &mut self.0 {
            Entries::List(entries) => IterMutRepr::List(entries.iter_mut()),
            Entries::Map(entries) => IterMutRepr::Map(entries.iter_mut()),
        })
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        match &self.0 {
            Entries::List(entries) => entries.len(),
            Entries::Map(entries) => entries.len(),
        }
    }

    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the entries are kept in a map: more than [`LIST_MOST`] of them.
    pub(crate) fn is_map(&self) -> bool {
        matches!(self.0, Entries::Map(_))
    }

    /// Lets go of the room kept for entries to come.
    pub(crate) fn shrink_to_fit(&mut self) {
        match &mut self.0 {
            Entries::List(entries) => entries.shrink_to_fit(),
            Entries::Map(entries) => entries.shrink_to_fit(),
        }
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
                self.iter()
                    .map(|(key, value)| (format!("/{}", String::from_utf8_lossy(key)), value)),
            )
            .finish()
    }
}

impl<K: Into<Name>, V: Into<Object>> FromIterator<(K, V)> for Dictionary {
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
    fn an_object_is_small() {
        // The table of a document's objects holds one of these for each; a
        // dictionary or a stream inside it is a few words.
        assert_eq!(std::mem::size_of::<Object>(), 32);
        assert_eq!(std::mem::size_of::<Dictionary>(), 24);
    }

    #[test]
    fn a_dictionary_keeps_its_order_when_it_moves_to_a_map() {
        let mut dict = Dictionary::new();
        for i in 0..40 {
            dict.set(format!("K{i}"), i);
            assert_eq!(dict.is_map(), i >= LIST_MOST as i32, "{i}");
        }
        let keys: Vec<String> = dict
            .iter()
            .map(|(key, _)| String::from_utf8_lossy(key).into_owned())
            .collect();
        let expected: Vec<String> = (0..40).map(|i| format!("K{i}")).collect();
        assert_eq!(keys, expected);
        // A key set again keeps its place, and one removed leaves the rest in order.
        dict.set("K3", "Three");
        assert_eq!(
            dict.iter().nth(3).map(|(_, value)| value),
            Some(&Object::name("Three"))
        );
        assert_eq!(dict.remove(b"K1"), Some(Object::Integer(1)));
        assert_eq!(
            dict.iter().nth(1).map(|(key, _)| key.as_slice()),
            Some(&b"K2"[..])
        );
        assert_eq!(dict.len(), 39);
        for (_, value) in dict.iter_mut() {
            if let Object::Integer(number) = value {
                *number += 1;
            }
        }
        assert_eq!(dict.get(b"K39"), Some(&Object::Integer(40)));
        // The same entries in another order are the same dictionary, in a
        // list or in a map.
        let small = dictionary! { "A" => 1, "B" => 2 };
        let reversed = dictionary! { "B" => 2, "A" => 1 };
        assert_eq!(small, reversed);
        let mut big = dict.clone();
        let mut other = Dictionary::new();
        for (key, value) in big.iter().collect::<Vec<_>>().into_iter().rev() {
            other.set(key.clone(), value.clone());
        }
        assert_eq!(big, other);
        big.set("K0", 99);
        assert_ne!(big, other);
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
