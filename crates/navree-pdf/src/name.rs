//! [`Name`]: the bytes of a PDF name, kept inside the object when they are
//! short.

use std::borrow::Borrow;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;

/// The longest name kept inside the object itself (22 bytes): every key and
/// value in a real file is shorter, so a name costs an allocation only when it
/// is unusual.
pub(crate) const INLINE: usize = 22;

#[derive(Clone)]
enum Repr {
    Inline(u8, [u8; INLINE]),
    Heap(Box<[u8]>),
}

/// A name without its slash and with `#xx` escapes decoded: the key of a
/// dictionary entry, or the value [`Object::Name`](crate::Object::Name) holds.
/// It reads as its bytes (`&[u8]`).
#[derive(Clone)]
pub struct Name(Repr);

impl Name {
    /// The name of `bytes`.
    pub fn new(bytes: &[u8]) -> Name {
        if bytes.len() <= INLINE {
            let mut array = [0; INLINE];
            if let Some(room) = array.get_mut(..bytes.len()) {
                room.copy_from_slice(bytes);
            }
            Name(Repr::Inline(bytes.len() as u8, array))
        } else {
            Name(Repr::Heap(bytes.into()))
        }
    }

    /// The bytes.
    pub fn as_slice(&self) -> &[u8] {
        match &self.0 {
            Repr::Inline(len, bytes) => bytes.get(..usize::from(*len)).unwrap_or_default(),
            Repr::Heap(bytes) => bytes,
        }
    }

    /// What the name holds besides its own bytes, outside the object: the
    /// allocation of one that is not short.
    pub(crate) fn heap_len(&self) -> usize {
        match &self.0 {
            Repr::Inline(..) => 0,
            Repr::Heap(bytes) => bytes.len(),
        }
    }
}

impl Deref for Name {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsRef<[u8]> for Name {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Borrow<[u8]> for Name {
    fn borrow(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Hash for Name {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl PartialEq for Name {
    fn eq(&self, other: &Name) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for Name {}

impl PartialOrd for Name {
    fn partial_cmp(&self, other: &Name) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Name {
    fn cmp(&self, other: &Name) -> std::cmp::Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}

impl PartialEq<[u8]> for Name {
    fn eq(&self, other: &[u8]) -> bool {
        self.as_slice() == other
    }
}

impl PartialEq<&[u8]> for Name {
    fn eq(&self, other: &&[u8]) -> bool {
        self.as_slice() == *other
    }
}

impl<const N: usize> PartialEq<[u8; N]> for Name {
    fn eq(&self, other: &[u8; N]) -> bool {
        self.as_slice() == other
    }
}

impl<const N: usize> PartialEq<&[u8; N]> for Name {
    fn eq(&self, other: &&[u8; N]) -> bool {
        self.as_slice() == *other
    }
}

impl PartialEq<Vec<u8>> for Name {
    fn eq(&self, other: &Vec<u8>) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl PartialEq<&str> for Name {
    fn eq(&self, other: &&str) -> bool {
        self.as_slice() == other.as_bytes()
    }
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "/{}", String::from_utf8_lossy(self.as_slice()))
    }
}

impl From<&[u8]> for Name {
    fn from(bytes: &[u8]) -> Name {
        Name::new(bytes)
    }
}

impl<const N: usize> From<&[u8; N]> for Name {
    fn from(bytes: &[u8; N]) -> Name {
        Name::new(bytes)
    }
}

impl From<&Vec<u8>> for Name {
    fn from(bytes: &Vec<u8>) -> Name {
        Name::new(bytes)
    }
}

impl From<Vec<u8>> for Name {
    fn from(bytes: Vec<u8>) -> Name {
        if bytes.len() <= INLINE {
            Name::new(&bytes)
        } else {
            Name(Repr::Heap(bytes.into_boxed_slice()))
        }
    }
}

impl From<&str> for Name {
    fn from(name: &str) -> Name {
        Name::new(name.as_bytes())
    }
}

impl From<String> for Name {
    fn from(name: String) -> Name {
        Name::from(name.into_bytes())
    }
}

impl From<&Name> for Name {
    fn from(name: &Name) -> Name {
        name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names_are_kept_inside_and_long_ones_outside() {
        let short = Name::new(b"StructElem");
        assert_eq!(short, b"StructElem");
        assert_eq!(short.heap_len(), 0);
        let edge = Name::new(&[b'a'; INLINE]);
        assert_eq!(edge.len(), INLINE);
        assert_eq!(edge.heap_len(), 0);
        let long = Name::from(vec![b'b'; INLINE + 1]);
        assert_eq!(long.len(), INLINE + 1);
        assert_eq!(long.heap_len(), INLINE + 1);
        assert_eq!(Name::new(b""), b"");
        assert!(std::mem::size_of::<Name>() <= 24);
    }

    #[test]
    fn names_compare_and_hash_as_their_bytes() {
        use std::collections::HashMap;
        let mut map: HashMap<Name, u8> = HashMap::new();
        map.insert(Name::from("Type"), 1);
        assert_eq!(map.get(&b"Type"[..]), Some(&1));
        let (a, b) = (Name::from("A"), Name::from("B"));
        assert_eq!(a, Name::from(b"A".to_vec()));
        assert!(a < b);
        assert_eq!(format!("{:?}", Name::from("Font")), "/Font");
    }
}
