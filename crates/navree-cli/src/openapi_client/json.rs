//! The JSON the generator reads: a document tree that keeps object keys in
//! file order, with JavaScript's rules for iterating them and for turning
//! numbers into text, which the generated types follow.
//!
//! The reader is strict (RFC 8259), caps nesting, and never indexes or
//! allocates from a number in the input.

use std::collections::HashMap;
use std::rc::Rc;

/// How deeply arrays and objects may nest.
pub(crate) const MAX_DEPTH: usize = 128;

/// A JSON value. Containers and strings are shared, so copying a value (the
/// generator copies schemas to change one key) costs a count, not the tree.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    Arr(Rc<Vec<Value>>),
    Obj(Rc<Object>),
}

/// A JSON object: its entries in file order, a repeated key keeping the place
/// of its first appearance and the value of its last.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Object {
    entries: Vec<(Rc<str>, Value)>,
}

impl Object {
    /// The value under `key`.
    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        self.entries
            .iter()
            .find(|(name, _)| &**name == key)
            .map(|(_, value)| value)
    }

    /// The value under `key`, to change.
    pub(crate) fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.entries
            .iter_mut()
            .find(|(name, _)| &**name == key)
            .map(|(_, value)| value)
    }

    /// Whether `key` is present, whatever its value.
    pub(crate) fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// How many keys there are.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Sets `key`, keeping its place when it is already there.
    pub(crate) fn set(&mut self, key: &str, value: Value) {
        match self.get_mut(key) {
            Some(slot) => *slot = value,
            None => self.entries.push((Rc::from(key), value)),
        }
    }

    /// Removes `key`.
    pub(crate) fn remove(&mut self, key: &str) {
        self.entries.retain(|(name, _)| &**name != key);
    }

    /// The entries as JavaScript's `Object.entries` lists them: keys that are
    /// array indices (`"0"`, `"200"`) first in numeric order, then the others
    /// in file order.
    pub(crate) fn js_entries(&self) -> Vec<(&str, &Value)> {
        let mut indices: Vec<(u32, &str, &Value)> = Vec::new();
        let mut rest: Vec<(&str, &Value)> = Vec::new();
        for (name, value) in &self.entries {
            match array_index(name) {
                Some(index) => indices.push((index, name, value)),
                None => rest.push((name, value)),
            }
        }
        indices.sort_by_key(|(index, _, _)| *index);
        indices
            .into_iter()
            .map(|(_, name, value)| (name, value))
            .chain(rest)
            .collect()
    }
}

/// `key` as an array index: the canonical decimal form of an integer from 0
/// to 2^32 - 2.
fn array_index(key: &str) -> Option<u32> {
    let canonical = key == "0"
        || (!key.starts_with('0') && !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()));
    if !canonical || key.len() > 10 {
        return None;
    }
    key.parse::<u32>().ok().filter(|index| *index != u32::MAX)
}

impl Value {
    /// An object holding `entries`.
    pub(crate) fn object<'a>(entries: impl IntoIterator<Item = (&'a str, Value)>) -> Value {
        let mut object = Object::default();
        for (key, value) in entries {
            object.set(key, value);
        }
        Value::Obj(Rc::new(object))
    }

    /// A string value.
    pub(crate) fn string(text: &str) -> Value {
        Value::Str(Rc::from(text))
    }

    /// The object, when this is one.
    pub(crate) fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Obj(object) => Some(object),
            _ => None,
        }
    }

    /// The elements, when this is an array.
    pub(crate) fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Arr(items) => Some(items),
            _ => None,
        }
    }

    /// The text, when this is a string.
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(text) => Some(text),
            _ => None,
        }
    }

    /// `self[key]` as JavaScript reads it: a key of an object, or an index
    /// of an array.
    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(object) => object.get(key),
            Value::Arr(items) => array_index(key).and_then(|i| items.get(usize::try_from(i).ok()?)),
            _ => None,
        }
    }

    /// Whether this is an object with `key` (JavaScript's `key in value`).
    pub(crate) fn has(&self, key: &str) -> bool {
        self.as_object().is_some_and(|object| object.contains(key))
    }

    /// JavaScript truthiness.
    pub(crate) fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(flag) => *flag,
            Value::Num(number) => *number != 0.0 && !number.is_nan(),
            Value::Str(text) => !text.is_empty(),
            Value::Arr(_) | Value::Obj(_) => true,
        }
    }

    /// The object, to change (copied first when it is shared).
    pub(crate) fn object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Value::Obj(object) => Some(Rc::make_mut(object)),
            _ => None,
        }
    }

    /// The elements, to change (copied first when they are shared).
    pub(crate) fn array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::Arr(items) => Some(Rc::make_mut(items)),
            _ => None,
        }
    }

    /// `String(value)` for the values a description or example can be: text
    /// as it is, numbers and booleans as JavaScript prints them.
    pub(crate) fn js_string(&self) -> String {
        match self {
            Value::Null => "null".to_owned(),
            Value::Bool(flag) => flag.to_string(),
            Value::Num(number) => js_number(*number),
            Value::Str(text) => text.to_string(),
            Value::Arr(_) | Value::Obj(_) => self.stringify(),
        }
    }

    /// `JSON.stringify(value, null, 2)`.
    pub(crate) fn stringify(&self) -> String {
        let mut out = String::new();
        self.write_pretty(&mut out, 0);
        out
    }

    fn write_pretty(&self, out: &mut String, depth: usize) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
            Value::Num(number) if number.is_finite() => out.push_str(&js_number(*number)),
            Value::Num(_) => out.push_str("null"),
            Value::Str(text) => out.push_str(&json_quote(text)),
            Value::Arr(items) if items.is_empty() => out.push_str("[]"),
            Value::Arr(items) => {
                out.push('[');
                for (n, item) in items.iter().enumerate() {
                    out.push_str(if n == 0 { "\n" } else { ",\n" });
                    out.push_str(&"  ".repeat(depth + 1));
                    item.write_pretty(out, depth + 1);
                }
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
                out.push(']');
            }
            Value::Obj(object) if object.len() == 0 => out.push_str("{}"),
            Value::Obj(object) => {
                out.push('{');
                for (n, (key, value)) in object.js_entries().into_iter().enumerate() {
                    out.push_str(if n == 0 { "\n" } else { ",\n" });
                    out.push_str(&"  ".repeat(depth + 1));
                    out.push_str(&json_quote(key));
                    out.push_str(": ");
                    value.write_pretty(out, depth + 1);
                }
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
                out.push('}');
            }
        }
    }
}

/// `JSON.stringify` of a string.
pub(crate) fn json_quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A number as JavaScript's `String(number)` prints it.
pub(crate) fn js_number(number: f64) -> String {
    if number.is_nan() {
        return "NaN".to_owned();
    }
    if number == 0.0 {
        return "0".to_owned();
    }
    if number.is_infinite() {
        return if number < 0.0 {
            "-Infinity"
        } else {
            "Infinity"
        }
        .to_owned();
    }
    let sign = if number < 0.0 { "-" } else { "" };
    // The shortest digits that read back as the same number, and where the
    // decimal point goes: `1.2345e2` is digits "12345", point after 3.
    let scientific = format!("{:e}", number.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let count = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    let point = exponent + 1;
    let body = if count <= point && point <= 21 {
        format!(
            "{digits}{}",
            "0".repeat(usize::try_from(point - count).unwrap_or(0))
        )
    } else if 0 < point && point <= 21 {
        let at = usize::try_from(point).unwrap_or(0);
        format!("{}.{}", &digits[..at], &digits[at..])
    } else if -6 < point && point <= 0 {
        format!(
            "0.{}{digits}",
            "0".repeat(usize::try_from(-point).unwrap_or(0))
        )
    } else {
        let sign_of_exponent = if point - 1 < 0 { '-' } else { '+' };
        let magnitude = (point - 1).abs();
        match digits.split_at(1) {
            (first, "") => format!("{first}e{sign_of_exponent}{magnitude}"),
            (first, rest) => format!("{first}.{rest}e{sign_of_exponent}{magnitude}"),
        }
    };
    format!("{sign}{body}")
}

/// Reads one JSON document.
pub(crate) fn parse(text: &str) -> Result<Value, String> {
    let mut reader = Reader {
        text,
        bytes: text.as_bytes(),
        at: 0,
    };
    if reader.bytes.starts_with(b"\xEF\xBB\xBF") {
        reader.at = 3;
    }
    let value = reader.value(0)?;
    reader.skip_space();
    if reader.at < reader.bytes.len() {
        return Err(reader.fail("unexpected data after the document"));
    }
    Ok(value)
}

struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn fail(&self, what: &str) -> String {
        let before = self.bytes.get(..self.at).unwrap_or(self.bytes);
        let line = before.iter().filter(|b| **b == b'\n').count() + 1;
        let column = before.iter().rev().take_while(|b| **b != b'\n').count() + 1;
        format!("invalid JSON at line {line}, column {column}: {what}")
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn word(&mut self, word: &str, value: Value) -> Result<Value, String> {
        if self
            .bytes
            .get(self.at..)
            .is_some_and(|rest| rest.starts_with(word.as_bytes()))
        {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.fail("unexpected character"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        self.skip_space();
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Value::Str(Rc::from(self.string()?))),
            Some(b't') => self.word("true", Value::Bool(true)),
            Some(b'f') => self.word("false", Value::Bool(false)),
            Some(b'n') => self.word("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.fail("unexpected character")),
            None => Err(self.fail("unexpected end of the document")),
        }
    }

    fn nested(&self, depth: usize) -> Result<usize, String> {
        if depth >= MAX_DEPTH {
            return Err(self.fail("nested too deeply"));
        }
        Ok(depth + 1)
    }

    fn object(&mut self, depth: usize) -> Result<Value, String> {
        let depth = self.nested(depth)?;
        self.at += 1;
        let mut object = Object::default();
        let mut places: HashMap<Rc<str>, usize> = HashMap::new();
        self.skip_space();
        if self.eat(b'}') {
            return Ok(Value::Obj(Rc::new(object)));
        }
        loop {
            self.skip_space();
            if self.peek() != Some(b'"') {
                return Err(self.fail("expected a key"));
            }
            let key: Rc<str> = Rc::from(self.string()?);
            self.skip_space();
            if !self.eat(b':') {
                return Err(self.fail("expected ':'"));
            }
            let value = self.value(depth)?;
            match places.get(&key) {
                Some(place) => {
                    if let Some(entry) = object.entries.get_mut(*place) {
                        entry.1 = value;
                    }
                }
                None => {
                    places.insert(Rc::clone(&key), object.entries.len());
                    object.entries.push((key, value));
                }
            }
            self.skip_space();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b'}') {
                return Ok(Value::Obj(Rc::new(object)));
            }
            return Err(self.fail("expected ',' or '}'"));
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, String> {
        let depth = self.nested(depth)?;
        self.at += 1;
        let mut items = Vec::new();
        self.skip_space();
        if self.eat(b']') {
            return Ok(Value::Arr(Rc::new(items)));
        }
        loop {
            items.push(self.value(depth)?);
            self.skip_space();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b']') {
                return Ok(Value::Arr(Rc::new(items)));
            }
            return Err(self.fail("expected ',' or ']'"));
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.at;
        self.eat(b'-');
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(self.fail("invalid number")),
        }
        if self.eat(b'.') {
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.fail("invalid number"));
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.fail("invalid number"));
            }
            self.digits();
        }
        let token = self.text.get(start..self.at).unwrap_or("0");
        token
            .parse::<f64>()
            .map(Value::Num)
            .map_err(|_| self.fail("invalid number"))
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
    }

    /// A string literal, escapes resolved (a lone surrogate becomes U+FFFD).
    fn string(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let run_start = self.at;
            while let Some(byte) = self.peek() {
                if byte == b'"' || byte == b'\\' || byte < 0x20 {
                    break;
                }
                self.at += 1;
            }
            out.push_str(self.text.get(run_start..self.at).unwrap_or(""));
            match self.peek() {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    self.escape(&mut out)?;
                }
                Some(_) => return Err(self.fail("a control character in a string")),
                None => return Err(self.fail("unterminated string")),
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), String> {
        let Some(byte) = self.peek() else {
            return Err(self.fail("unterminated string"));
        };
        self.at += 1;
        match byte {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{8}'),
            b'f' => out.push('\u{c}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let first = self.hex4()?;
                if (0xD800..0xDC00).contains(&first) {
                    let pair = self
                        .bytes
                        .get(self.at..self.at + 2)
                        .is_some_and(|next| next == b"\\u");
                    if pair {
                        let save = self.at;
                        self.at += 2;
                        let second = self.hex4()?;
                        if (0xDC00..0xE000).contains(&second) {
                            let scalar = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                            out.push(char::from_u32(scalar).unwrap_or('\u{FFFD}'));
                            return Ok(());
                        }
                        self.at = save;
                    }
                    out.push('\u{FFFD}');
                } else {
                    out.push(char::from_u32(first).unwrap_or('\u{FFFD}'));
                }
            }
            _ => return Err(self.fail("invalid escape")),
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .text
            .get(self.at..self.at + 4)
            .ok_or_else(|| self.fail("invalid \\u escape"))?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(self.fail("invalid \\u escape"));
        }
        self.at += 4;
        u32::from_str_radix(digits, 16).map_err(|_| self.fail("invalid \\u escape"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_keys_in_file_order_and_repeats_in_their_first_place() {
        let Value::Obj(object) = parse(r#"{"b":1,"a":2,"b":3}"#).unwrap() else {
            panic!("an object")
        };
        let keys: Vec<_> = object
            .js_entries()
            .iter()
            .map(|(k, v)| (*k, (*v).clone()))
            .collect();
        assert_eq!(keys, [("b", Value::Num(3.0)), ("a", Value::Num(2.0))]);
    }

    #[test]
    fn lists_array_index_keys_first_in_numeric_order() {
        let Value::Obj(object) =
            parse(r#"{"default":0,"404":0,"x":0,"200":0,"01":0,"2":0}"#).unwrap()
        else {
            panic!("an object")
        };
        let keys: Vec<_> = object.js_entries().iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, ["2", "200", "404", "default", "x", "01"]);
    }

    #[test]
    fn refuses_what_is_not_json() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\":}",
            "01",
            "1.",
            "\"\\x\"",
            "tru",
            "{} {}",
            "[\"a\nb\"]",
            "{\"a\" 1}",
            "-",
            "1e",
            "\"\\u12\"",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn caps_nesting_instead_of_recursing_without_end() {
        let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
        assert!(parse(&deep).unwrap_err().contains("nested too deeply"));
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(parse(&ok).is_ok());
    }

    #[test]
    fn reads_escapes_and_surrogate_pairs() {
        assert_eq!(
            parse(r#""a\n\u00e9\ud83d\ude00\ud800x""#).unwrap(),
            Value::string("a\n\u{e9}\u{1F600}\u{FFFD}x")
        );
    }

    #[test]
    fn prints_numbers_as_javascript_does() {
        for (number, text) in [
            (0.0, "0"),
            (-0.0, "0"),
            (5.0, "5"),
            (-5.5, "-5.5"),
            (0.1, "0.1"),
            (123_456_789.0, "123456789"),
            (1e21, "1e+21"),
            (1.5e21, "1.5e+21"),
            (1e20, "100000000000000000000"),
            (0.000001, "0.000001"),
            (0.0000001, "1e-7"),
            (1.2345e-7, "1.2345e-7"),
            (9_007_199_254_740_993.0, "9007199254740992"),
            (f64::INFINITY, "Infinity"),
        ] {
            assert_eq!(js_number(number), text);
        }
    }

    #[test]
    fn stringifies_with_two_space_indent() {
        let value = parse(r#"{"a":[1,{"b":null},[]],"2":{},"c":"x\"y"}"#).unwrap();
        assert_eq!(
            value.stringify(),
            "{\n  \"2\": {},\n  \"a\": [\n    1,\n    {\n      \"b\": null\n    },\n    []\n  ],\n  \"c\": \"x\\\"y\"\n}"
        );
    }
}
