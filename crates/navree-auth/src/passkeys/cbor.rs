//! The CBOR WebAuthn needs (RFC 8949): definite-length integers, byte and
//! text strings, arrays, maps and the simple values, decoded with a depth
//! limit, and encoded for the virtual authenticator.

/// A CBOR data item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Cbor {
    /// An integer (major types 0 and 1).
    Int(i128),
    /// A byte string.
    Bytes(Vec<u8>),
    /// A text string.
    Text(String),
    /// An array.
    Array(Vec<Cbor>),
    /// A map, in encoded order.
    Map(Vec<(Cbor, Cbor)>),
    /// `true` or `false`.
    Bool(bool),
    /// `null` or `undefined`.
    Null,
}

/// The deepest nesting accepted.
const MAX_DEPTH: usize = 16;

impl Cbor {
    /// The item at the start of `input` and the bytes after it.
    pub(crate) fn decode(input: &[u8]) -> Option<(Cbor, &[u8])> {
        decode(input, 0)
    }

    /// The value of map key `key` (a text key or an integer key).
    pub(crate) fn get(&self, key: &Cbor) -> Option<&Cbor> {
        match self {
            Cbor::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The value of text key `key`.
    pub(crate) fn get_text(&self, key: &str) -> Option<&Cbor> {
        self.get(&Cbor::Text(key.to_owned()))
    }

    /// The value of integer key `key`.
    pub(crate) fn get_int(&self, key: i128) -> Option<&Cbor> {
        self.get(&Cbor::Int(key))
    }

    pub(crate) fn as_int(&self) -> Option<i128> {
        match self {
            Cbor::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Cbor::Bytes(bytes) => Some(bytes),
            _ => None,
        }
    }

    pub(crate) fn as_text(&self) -> Option<&str> {
        match self {
            Cbor::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The encoding of the item (shortest heads, as the canonical form).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_into(&mut out);
        out
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Cbor::Int(value) if *value >= 0 => head(out, 0, u64::try_from(*value).unwrap_or(0)),
            Cbor::Int(value) => head(out, 1, u64::try_from(-1 - *value).unwrap_or(0)),
            Cbor::Bytes(bytes) => {
                head(out, 2, bytes.len() as u64);
                out.extend_from_slice(bytes);
            }
            Cbor::Text(text) => {
                head(out, 3, text.len() as u64);
                out.extend_from_slice(text.as_bytes());
            }
            Cbor::Array(items) => {
                head(out, 4, items.len() as u64);
                for item in items {
                    item.encode_into(out);
                }
            }
            Cbor::Map(entries) => {
                head(out, 5, entries.len() as u64);
                for (key, value) in entries {
                    key.encode_into(out);
                    value.encode_into(out);
                }
            }
            Cbor::Bool(false) => out.push(0xF4),
            Cbor::Bool(true) => out.push(0xF5),
            Cbor::Null => out.push(0xF6),
        }
    }
}

fn head(out: &mut Vec<u8>, major: u8, value: u64) {
    let major = major << 5;
    match value {
        0..=23 => out.push(major | value as u8),
        24..=0xFF => out.extend_from_slice(&[major | 24, value as u8]),
        0x100..=0xFFFF => {
            out.push(major | 25);
            out.extend_from_slice(&(value as u16).to_be_bytes());
        }
        0x1_0000..=0xFFFF_FFFF => {
            out.push(major | 26);
            out.extend_from_slice(&(value as u32).to_be_bytes());
        }
        _ => {
            out.push(major | 27);
            out.extend_from_slice(&value.to_be_bytes());
        }
    }
}

/// The argument of a head with additional information `info`.
fn argument(info: u8, input: &[u8]) -> Option<(u64, &[u8])> {
    let take = |n: usize| -> Option<(u64, &[u8])> {
        if input.len() < n {
            return None;
        }
        let (bytes, rest) = input.split_at(n);
        Some((
            bytes.iter().fold(0_u64, |v, b| (v << 8) | u64::from(*b)),
            rest,
        ))
    };
    match info {
        0..=23 => Some((u64::from(info), input)),
        24 => take(1),
        25 => take(2),
        26 => take(4),
        27 => take(8),
        // Indefinite lengths and reserved values: not in WebAuthn's CBOR.
        _ => None,
    }
}

fn decode(input: &[u8], depth: usize) -> Option<(Cbor, &[u8])> {
    if depth > MAX_DEPTH {
        return None;
    }
    let (&first, rest) = input.split_first()?;
    let (major, info) = (first >> 5, first & 0x1F);
    if major == 7 {
        return match info {
            20 => Some((Cbor::Bool(false), rest)),
            21 => Some((Cbor::Bool(true), rest)),
            22 | 23 => Some((Cbor::Null, rest)),
            _ => None,
        };
    }
    let (value, rest) = argument(info, rest)?;
    match major {
        0 => Some((Cbor::Int(i128::from(value)), rest)),
        1 => Some((Cbor::Int(-1 - i128::from(value)), rest)),
        2 | 3 => {
            let len = usize::try_from(value).ok()?;
            if rest.len() < len {
                return None;
            }
            let (bytes, rest) = rest.split_at(len);
            if major == 2 {
                Some((Cbor::Bytes(bytes.to_vec()), rest))
            } else {
                Some((Cbor::Text(String::from_utf8(bytes.to_vec()).ok()?), rest))
            }
        }
        4 => {
            let len = usize::try_from(value).ok()?;
            // Every item takes at least one byte.
            if len > rest.len() {
                return None;
            }
            let mut items = Vec::with_capacity(len);
            let mut rest = rest;
            for _ in 0..len {
                let (item, after) = decode(rest, depth + 1)?;
                items.push(item);
                rest = after;
            }
            Some((Cbor::Array(items), rest))
        }
        5 => {
            let len = usize::try_from(value).ok()?;
            if len > rest.len() / 2 {
                return None;
            }
            let mut entries = Vec::with_capacity(len);
            let mut rest = rest;
            for _ in 0..len {
                let (key, after) = decode(rest, depth + 1)?;
                let (value, after) = decode(after, depth + 1)?;
                entries.push((key, value));
                rest = after;
            }
            Some((Cbor::Map(entries), rest))
        }
        // Tags (6) are not used by WebAuthn.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(bytes: &[u8]) -> Cbor {
        let (item, rest) = Cbor::decode(bytes).expect("decodes");
        assert!(rest.is_empty(), "trailing bytes");
        item
    }

    #[test]
    fn rfc_8949_examples_decode() {
        assert_eq!(decoded(&[0x00]), Cbor::Int(0));
        assert_eq!(decoded(&[0x17]), Cbor::Int(23));
        assert_eq!(decoded(&[0x18, 0x18]), Cbor::Int(24));
        assert_eq!(decoded(&[0x19, 0x03, 0xE8]), Cbor::Int(1000));
        assert_eq!(decoded(&[0x20]), Cbor::Int(-1));
        assert_eq!(decoded(&[0x38, 0x63]), Cbor::Int(-100));
        assert_eq!(decoded(&[0x43, 1, 2, 3]), Cbor::Bytes(vec![1, 2, 3]));
        assert_eq!(
            decoded(&[0x64, b'I', b'E', b'T', b'F']),
            Cbor::Text("IETF".into())
        );
        assert_eq!(
            decoded(&[0x83, 0x01, 0x02, 0x03]),
            Cbor::Array(vec![Cbor::Int(1), Cbor::Int(2), Cbor::Int(3)])
        );
        assert_eq!(
            decoded(&[0xA2, 0x61, b'a', 0x01, 0x61, b'b', 0x82, 0x02, 0x03]),
            Cbor::Map(vec![
                (Cbor::Text("a".into()), Cbor::Int(1)),
                (
                    Cbor::Text("b".into()),
                    Cbor::Array(vec![Cbor::Int(2), Cbor::Int(3)])
                ),
            ])
        );
        assert_eq!(decoded(&[0xF4]), Cbor::Bool(false));
        assert_eq!(decoded(&[0xF5]), Cbor::Bool(true));
        assert_eq!(decoded(&[0xF6]), Cbor::Null);
    }

    #[test]
    fn items_round_trip_and_leave_what_follows() {
        let item = Cbor::Map(vec![
            (Cbor::Int(1), Cbor::Int(2)),
            (Cbor::Int(3), Cbor::Int(-7)),
            (Cbor::Int(-2), Cbor::Bytes(vec![7; 32])),
            (Cbor::Text("fmt".into()), Cbor::Text("none".into())),
            (Cbor::Int(70_000), Cbor::Int(-5_000_000_000)),
        ]);
        let mut bytes = item.encode();
        bytes.extend_from_slice(&[0xAA, 0xBB]);
        let (decoded, rest) = Cbor::decode(&bytes).unwrap();
        assert_eq!(decoded, item);
        assert_eq!(rest, [0xAA, 0xBB]);
        assert_eq!(decoded.get_int(3).and_then(Cbor::as_int), Some(-7));
        assert_eq!(
            decoded.get_text("fmt").and_then(Cbor::as_text),
            Some("none")
        );
        assert_eq!(
            decoded.get_int(-2).and_then(Cbor::as_bytes),
            Some(&[7; 32][..])
        );
    }

    #[test]
    fn malformed_input_is_refused() {
        for bytes in [
            &[][..],
            &[0x18],
            &[0x43, 1, 2],
            &[0x5F, 0x41, 0x00, 0xFF],
            &[0x9F, 0x01, 0xFF],
            &[0xC1, 0x00],
            &[0x62, 0xFF, 0xFE],
            &[0x9B, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
            &[0xBB, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
            &[0xF9, 0x3C, 0x00],
        ] {
            assert!(Cbor::decode(bytes).is_none(), "{bytes:?}");
        }
        // Nesting deeper than the limit.
        let deep: Vec<u8> = std::iter::repeat_n(0x81, 40).chain([0x00]).collect();
        assert!(Cbor::decode(&deep).is_none());
    }
}
