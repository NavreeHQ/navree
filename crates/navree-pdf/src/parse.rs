//! The syntax of PDF objects: white space, comments, numbers, names,
//! strings, arrays, dictionaries and references.
//!
//! Every object is charged to a [`Meter`] as it is built, so that what a
//! parse holds is counted before it grows further, and nesting is held to
//! [`MAX_DEPTH`].

use crate::error::PdfError;
use crate::object::{Dictionary, Object, StringFormat};

/// How deep arrays and dictionaries may nest (100).
pub(crate) const MAX_DEPTH: usize = 100;
/// How deep parentheses may nest inside a literal string (100).
const MAX_PARENS: usize = 100;

/// What an object holds besides its own heap bytes.
pub(crate) const OBJECT_MEMORY: usize = std::mem::size_of::<Object>();
/// What a dictionary entry holds besides its key's bytes and its value:
/// the key's vector and the map's index.
pub(crate) const ENTRY_MEMORY: usize = std::mem::size_of::<Vec<u8>>() + 16;

/// Counts the memory a parse builds.
pub(crate) trait Meter {
    /// Counts `bytes`, or fails when there is no room for them.
    fn charge(&self, bytes: usize) -> Result<(), PdfError>;
}

/// Why a parse stopped.
#[derive(Debug)]
pub(crate) enum Fail {
    /// The bytes are not the syntax expected.
    Syntax,
    /// Arrays, dictionaries or string parentheses nest deeper than allowed.
    Deep,
    /// A limit ran out.
    Over(PdfError),
}

impl From<PdfError> for Fail {
    fn from(error: PdfError) -> Fail {
        Fail::Over(error)
    }
}

pub(crate) fn is_white(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\0' | b'\x0C')
}

pub(crate) fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

pub(crate) fn is_regular(byte: u8) -> bool {
    !is_white(byte) && !is_delimiter(byte)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// A position in a buffer, moving forward as it reads.
pub(crate) struct Lexer<'a> {
    pub(crate) buf: &'a [u8],
    pub(crate) pos: usize,
    /// Whether `12 0 R` is a reference (in a file) or three operands (in a
    /// content stream, which has no references: `0 0 0 RG`).
    references: bool,
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(buf: &'a [u8], pos: usize) -> Lexer<'a> {
        Lexer {
            buf,
            pos,
            references: true,
        }
    }

    /// A lexer for a content stream, where there are no references.
    pub(crate) fn content(buf: &'a [u8]) -> Lexer<'a> {
        Lexer {
            buf,
            pos: 0,
            references: false,
        }
    }

    pub(crate) fn peek(&self) -> Option<u8> {
        self.buf.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.buf.get(self.pos.checked_add(offset)?).copied()
    }

    pub(crate) fn rest(&self) -> &'a [u8] {
        self.buf.get(self.pos..).unwrap_or_default()
    }

    pub(crate) fn starts_with(&self, prefix: &[u8]) -> bool {
        self.rest().starts_with(prefix)
    }

    /// Moves past `prefix` when the bytes start with it.
    pub(crate) fn eat(&mut self, prefix: &[u8]) -> bool {
        if self.starts_with(prefix) {
            self.pos += prefix.len();
            true
        } else {
            false
        }
    }

    /// Moves past white space.
    pub(crate) fn skip_white(&mut self) {
        while self.peek().is_some_and(is_white) {
            self.pos += 1;
        }
    }

    /// Moves past white space and comments.
    pub(crate) fn skip_space(&mut self) {
        loop {
            self.skip_white();
            if self.peek() != Some(b'%') {
                return;
            }
            while self
                .peek()
                .is_some_and(|byte| byte != b'\r' && byte != b'\n')
            {
                self.pos += 1;
            }
        }
    }

    /// Moves past one end of line (`\r\n`, `\n` or `\r`), if there is one.
    pub(crate) fn eat_eol(&mut self) -> bool {
        self.eat(b"\r\n") || self.eat(b"\n") || self.eat(b"\r")
    }

    /// The run of regular characters here (a keyword, an operator).
    pub(crate) fn word(&mut self) -> &'a [u8] {
        let start = self.pos;
        while self.peek().is_some_and(is_regular) {
            self.pos += 1;
        }
        self.buf.get(start..self.pos).unwrap_or_default()
    }

    /// An unsigned integer: a run of digits, if it fits in a `u64`.
    pub(crate) fn unsigned(&mut self) -> Option<u64> {
        let start = self.pos;
        let mut value = 0u64;
        while let Some(byte) = self.peek().filter(u8::is_ascii_digit) {
            value = value
                .checked_mul(10)
                .and_then(|value| value.checked_add(u64::from(byte - b'0')))
                .or_else(|| {
                    self.pos = start;
                    None
                })?;
            self.pos += 1;
        }
        (self.pos > start).then_some(value)
    }

    /// A direct object, and the white space and comments after it. A
    /// reference (`12 0 R`) is read whole.
    pub(crate) fn object(&mut self, depth: usize, meter: &dyn Meter) -> Result<Object, Fail> {
        if depth == 0 {
            return Err(Fail::Deep);
        }
        meter.charge(OBJECT_MEMORY)?;
        let object = match self.peek().ok_or(Fail::Syntax)? {
            b'n' if self.eat(b"null") => Object::Null,
            b't' if self.eat(b"true") => Object::Boolean(true),
            b'f' if self.eat(b"false") => Object::Boolean(false),
            b'0'..=b'9' if self.references => match self.reference() {
                Some(id) => Object::Reference(id),
                None => self.number()?,
            },
            b'0'..=b'9' | b'+' | b'-' | b'.' => self.number()?,
            b'/' => {
                let name = self.name().ok_or(Fail::Syntax)?;
                meter.charge(name.len())?;
                Object::Name(name)
            }
            b'(' => {
                let string = self.literal_string(meter)?;
                Object::String(string, StringFormat::Literal)
            }
            b'<' if self.peek_at(1) == Some(b'<') => {
                Object::Dictionary(self.dictionary(depth - 1, meter)?)
            }
            b'<' => Object::String(self.hex_string(meter)?, StringFormat::Hexadecimal),
            b'[' => Object::Array(self.array(depth - 1, meter)?),
            _ => return Err(Fail::Syntax),
        };
        self.skip_space();
        Ok(object)
    }

    /// `number generation R`, when the bytes here are one.
    fn reference(&mut self) -> Option<(u32, u16)> {
        let start = self.pos;
        let id = (|| {
            let number = u32::try_from(self.unsigned()?).ok()?;
            self.skip_space();
            let generation = u16::try_from(self.unsigned()?).ok()?;
            self.skip_space();
            // `R` ends the reference: `RG` is not one.
            (self.eat(b"R") && !self.peek().is_some_and(is_regular)).then_some((number, generation))
        })();
        if id.is_none() {
            self.pos = start;
        }
        id
    }

    /// An integer or a real. A real is `[sign] digits . [digits]` or
    /// `[sign] . digits`; a minus sign some writers put inside the fraction
    /// (`0.00-4142502`) is skipped.
    pub(crate) fn number(&mut self) -> Result<Object, Fail> {
        let start = self.pos;
        let negative = match self.peek() {
            Some(b'+') => {
                self.pos += 1;
                false
            }
            Some(b'-') => {
                self.pos += 1;
                true
            }
            _ => false,
        };
        let digits_start = self.pos;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.pos += 1;
        }
        let whole = self.pos - digits_start;
        if self.peek() == Some(b'.')
            && (whole > 0 || self.peek_at(1).is_some_and(|b| b.is_ascii_digit()))
        {
            self.pos += 1;
            let mut text = String::with_capacity(self.pos - start + 8);
            text.push(if negative { '-' } else { '+' });
            text.push_str(std::str::from_utf8(&self.buf[digits_start..self.pos]).unwrap_or("0."));
            while let Some(byte) = self.peek().filter(|b| b.is_ascii_digit() || *b == b'-') {
                if byte != b'-' {
                    text.push(byte as char);
                }
                self.pos += 1;
            }
            return match text.parse::<f32>() {
                Ok(value) => Ok(Object::Real(value)),
                Err(_) => {
                    self.pos = start;
                    Err(Fail::Syntax)
                }
            };
        }
        if whole == 0 {
            self.pos = start;
            return Err(Fail::Syntax);
        }
        let mut magnitude = 0i64;
        for &byte in &self.buf[digits_start..self.pos] {
            let digit = i64::from(byte - b'0');
            magnitude = match magnitude.checked_mul(10).and_then(|value| {
                if negative {
                    value.checked_sub(digit)
                } else {
                    value.checked_add(digit)
                }
            }) {
                Some(value) => value,
                None => {
                    self.pos = start;
                    return Err(Fail::Syntax);
                }
            };
        }
        Ok(Object::Integer(magnitude))
    }

    /// A name, without its slash: `#` and two hex digits stand for a byte;
    /// another `#` ends the name.
    pub(crate) fn name(&mut self) -> Option<Vec<u8>> {
        if !self.eat(b"/") {
            return None;
        }
        let mut name = Vec::new();
        loop {
            let start = self.pos;
            while self
                .peek()
                .is_some_and(|byte| byte != b'#' && is_regular(byte))
            {
                self.pos += 1;
            }
            name.extend_from_slice(&self.buf[start..self.pos]);
            if self.peek() != Some(b'#') {
                break;
            }
            match (
                self.peek_at(1).and_then(hex_value),
                self.peek_at(2).and_then(hex_value),
            ) {
                (Some(high), Some(low)) => {
                    name.push(high << 4 | low);
                    self.pos += 3;
                }
                _ => break,
            }
        }
        name.shrink_to_fit();
        Some(name)
    }

    /// A literal string, escapes decoded, ends of line as `\n`.
    pub(crate) fn literal_string(&mut self, meter: &dyn Meter) -> Result<Vec<u8>, Fail> {
        let start = self.pos;
        if !self.eat(b"(") {
            return Err(Fail::Syntax);
        }
        let mut out = Vec::new();
        let mut depth = 0usize;
        loop {
            let Some(byte) = self.peek() else {
                self.pos = start;
                return Err(Fail::Syntax);
            };
            self.pos += 1;
            match byte {
                b'(' => {
                    depth += 1;
                    if depth > MAX_PARENS {
                        return Err(Fail::Deep);
                    }
                    out.push(b'(');
                }
                b')' if depth == 0 => break,
                b')' => {
                    depth -= 1;
                    out.push(b')');
                }
                b'\r' => {
                    self.eat(b"\n");
                    out.push(b'\n');
                }
                b'\\' => {
                    let Some(next) = self.peek() else {
                        continue;
                    };
                    match next {
                        b'0'..=b'7' => {
                            let mut value = 0u16;
                            for _ in 0..3 {
                                match self.peek() {
                                    Some(digit @ b'0'..=b'7') => {
                                        value = value * 8 + u16::from(digit - b'0');
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(value as u8);
                        }
                        b'\r' | b'\n' => {
                            self.eat_eol();
                        }
                        _ => {
                            self.pos += 1;
                            out.push(match next {
                                b'n' => b'\n',
                                b'r' => b'\r',
                                b't' => b'\t',
                                b'b' => 0x08,
                                b'f' => 0x0C,
                                other => other,
                            });
                        }
                    }
                }
                other => out.push(other),
            }
        }
        out.shrink_to_fit();
        meter.charge(out.len())?;
        Ok(out)
    }

    /// A hexadecimal string; white space between digits is skipped, and an
    /// odd last digit is the high half of its byte.
    pub(crate) fn hex_string(&mut self, meter: &dyn Meter) -> Result<Vec<u8>, Fail> {
        let start = self.pos;
        if !self.eat(b"<") {
            return Err(Fail::Syntax);
        }
        let mut out = Vec::new();
        let mut high: Option<u8> = None;
        loop {
            match self.peek() {
                Some(b'>') => {
                    self.pos += 1;
                    break;
                }
                Some(byte) if is_white(byte) => self.pos += 1,
                Some(byte) => match hex_value(byte) {
                    Some(value) => {
                        self.pos += 1;
                        match high.take() {
                            Some(high) => out.push(high << 4 | value),
                            None => high = Some(value),
                        }
                    }
                    None => {
                        self.pos = start;
                        return Err(Fail::Syntax);
                    }
                },
                None => {
                    self.pos = start;
                    return Err(Fail::Syntax);
                }
            }
        }
        if let Some(high) = high {
            out.push(high << 4);
        }
        out.shrink_to_fit();
        meter.charge(out.len())?;
        Ok(out)
    }

    fn array(&mut self, depth: usize, meter: &dyn Meter) -> Result<Vec<Object>, Fail> {
        self.pos += 1;
        self.skip_space();
        let mut items = Vec::new();
        loop {
            if self.eat(b"]") {
                break;
            }
            items.push(self.object(depth, meter)?);
        }
        items.shrink_to_fit();
        Ok(items)
    }

    /// `<< /Key value ... >>`.
    pub(crate) fn dictionary(
        &mut self,
        depth: usize,
        meter: &dyn Meter,
    ) -> Result<Dictionary, Fail> {
        if !self.eat(b"<<") {
            return Err(Fail::Syntax);
        }
        self.skip_space();
        let mut dictionary = Dictionary::new();
        loop {
            if self.eat(b">>") {
                break;
            }
            let key = self.name().ok_or(Fail::Syntax)?;
            meter.charge(ENTRY_MEMORY + key.len())?;
            self.skip_space();
            let value = self.object(depth, meter)?;
            dictionary.set(key, value);
        }
        Ok(dictionary)
    }
}

/// A meter that counts nothing.
pub(crate) struct Unmetered;

impl Meter for Unmetered {
    fn charge(&self, _: usize) -> Result<(), PdfError> {
        Ok(())
    }
}

/// The object `text` holds, for tests and small inputs.
#[cfg(test)]
pub(crate) fn object(text: &[u8]) -> Option<Object> {
    let mut lexer = Lexer::new(text, 0);
    lexer.skip_space();
    lexer.object(MAX_DEPTH, &Unmetered).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary;
    use std::cell::Cell;

    fn parse(text: &str) -> Object {
        object(text.as_bytes()).unwrap_or_else(|| panic!("{text:?} does not parse"))
    }

    #[test]
    fn scalars() {
        assert_eq!(parse("null"), Object::Null);
        assert_eq!(parse("true"), Object::Boolean(true));
        assert_eq!(parse("false"), Object::Boolean(false));
        assert_eq!(parse("42"), Object::Integer(42));
        assert_eq!(parse("-17"), Object::Integer(-17));
        assert_eq!(parse("+5"), Object::Integer(5));
        assert_eq!(parse("3.5"), Object::Real(3.5));
        assert_eq!(parse("-.25"), Object::Real(-0.25));
        assert_eq!(parse("4."), Object::Real(4.0));
        assert_eq!(parse("0.00-5"), Object::Real(0.005));
        assert_eq!(parse("-9223372036854775808"), Object::Integer(i64::MIN));
        assert!(object(b"9223372036854775808").is_none());
        assert!(object(b"-").is_none());
        assert!(object(b".").is_none());
    }

    #[test]
    fn names_decode_their_escapes() {
        assert_eq!(parse("/Type"), Object::name("Type"));
        assert_eq!(parse("/A#20B"), Object::name("A B"));
        assert_eq!(parse("/"), Object::name(""));
        let mut lexer = Lexer::new(b"/A#zz", 0);
        assert_eq!(lexer.name(), Some(b"A".to_vec()));
        assert_eq!(lexer.rest(), b"#zz");
    }

    #[test]
    fn strings_decode_their_escapes() {
        let s = |text: &str| parse(text).as_str().unwrap().to_vec();
        assert_eq!(s("(a (nested) one)"), b"a (nested) one");
        assert_eq!(s(r"(\n\r\t\b\f\(\)\\)"), b"\n\r\t\x08\x0C()\\");
        assert_eq!(s(r"(\101\1012\0)"), b"AA2\0");
        assert_eq!(s("(\\\r\nsplit)"), b"split");
        assert_eq!(s("(a\r\nb\rc)"), b"a\nb\nc");
        assert_eq!(s(r"(\q)"), b"q");
        assert_eq!(s("<48 65 6C6C6F>"), b"Hello");
        assert_eq!(s("<414>"), b"A@");
        assert!(object(b"(open").is_none());
        assert!(object(b"<4G>").is_none());
        let deep = format!("{}{}", "(".repeat(102), ")".repeat(102));
        assert!(object(deep.as_bytes()).is_none());
    }

    #[test]
    fn containers_and_references() {
        assert_eq!(
            parse("[1 0 R 2 /N (s) [3]]"),
            Object::Array(vec![
                Object::Reference((1, 0)),
                Object::Integer(2),
                Object::name("N"),
                Object::string_literal("s"),
                Object::Array(vec![Object::Integer(3)]),
            ])
        );
        assert_eq!(
            parse("<</Type/Page/Kids[4 0 R]/Count 1 % comment\n>>"),
            Object::Dictionary(dictionary! {
                "Type" => "Page",
                "Kids" => vec![Object::Reference((4, 0))],
                "Count" => 1,
            })
        );
        // Not a reference: the numbers stay numbers.
        assert_eq!(
            parse("[1 2 3]"),
            Object::Array(vec![1.into(), 2.into(), 3.into()])
        );
        assert!(object(b"<< 1 2 >>").is_none());
        assert!(object(b"[1 2").is_none());
    }

    #[test]
    fn nesting_past_the_limit_is_refused() {
        let ok = format!("{}{}", "[".repeat(100), "]".repeat(100));
        assert!(object(ok.as_bytes()).is_some());
        let deep = format!("{}1{}", "[".repeat(101), "]".repeat(101));
        let mut lexer = Lexer::new(deep.as_bytes(), 0);
        assert!(matches!(
            lexer.object(MAX_DEPTH, &Unmetered),
            Err(Fail::Deep)
        ));
    }

    struct Limit(Cell<usize>);

    impl Meter for Limit {
        fn charge(&self, bytes: usize) -> Result<(), PdfError> {
            let left = self.0.get().checked_sub(bytes);
            self.0.set(left.unwrap_or(0));
            left.map(|_| ()).ok_or(PdfError::TooLarge("over".into()))
        }
    }

    #[test]
    fn parses_are_charged_as_they_build() {
        // An array of 1,000 zeros: 2,000 bytes of text, about 1,000 objects.
        let text = format!("[{}]", "0 ".repeat(1000));
        let mut lexer = Lexer::new(text.as_bytes(), 0);
        let small = Limit(Cell::new(100 * OBJECT_MEMORY));
        assert!(matches!(
            lexer.object(MAX_DEPTH, &small),
            Err(Fail::Over(_))
        ));
        let mut lexer = Lexer::new(text.as_bytes(), 0);
        let room = Limit(Cell::new(1001 * OBJECT_MEMORY));
        assert!(lexer.object(MAX_DEPTH, &room).is_ok());
    }
}
