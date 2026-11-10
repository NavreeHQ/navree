//! The lexer: template text into tokens.
//!
//! Text outside tags is template data. `{{ .. }}` holds an expression,
//! `{% .. %}` a statement and `{# .. #}` a comment. A `-` just inside a
//! delimiter (`{%-`, `-%}`) removes the whitespace on that side, a `+`
//! keeps it. With `trim_blocks` the first newline after a statement or
//! comment is removed, with `lstrip_blocks` the spaces and tabs before one
//! on its line, and without `keep_trailing_newline` a single trailing
//! newline of the template.

use std::borrow::Cow;

use crate::error::{Error, ErrorKind};

/// Whitespace handling of a template (see the module documentation).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Whitespace {
    pub(crate) trim_blocks: bool,
    pub(crate) lstrip_blocks: bool,
    pub(crate) keep_trailing_newline: bool,
}

/// A token.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Token<'s> {
    TemplateData(&'s str),
    VariableStart,
    VariableEnd,
    BlockStart,
    BlockEnd,
    Ident(&'s str),
    Str(Cow<'s, str>),
    Int(i128),
    Float(f64),
    Plus,
    Minus,
    Mul,
    Div,
    FloorDiv,
    Pow,
    Mod,
    Dot,
    Comma,
    Colon,
    Tilde,
    Assign,
    Pipe,
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    BracketOpen,
    BracketClose,
    ParenOpen,
    ParenClose,
    BraceOpen,
    BraceClose,
}

impl std::fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Token::TemplateData(_) => "template-data",
            Token::VariableStart => "start of variable block",
            Token::VariableEnd => "end of variable block",
            Token::BlockStart => "start of block",
            Token::BlockEnd => "end of block",
            Token::Ident(_) => "identifier",
            Token::Str(_) => "string",
            Token::Int(_) => "integer",
            Token::Float(_) => "float",
            Token::Plus => "`+`",
            Token::Minus => "`-`",
            Token::Mul => "`*`",
            Token::Div => "`/`",
            Token::FloorDiv => "`//`",
            Token::Pow => "`**`",
            Token::Mod => "`%`",
            Token::Dot => "`.`",
            Token::Comma => "`,`",
            Token::Colon => "`:`",
            Token::Tilde => "`~`",
            Token::Assign => "`=`",
            Token::Pipe => "`|`",
            Token::Eq => "`==`",
            Token::Ne => "`!=`",
            Token::Gt => "`>`",
            Token::Gte => "`>=`",
            Token::Lt => "`<`",
            Token::Lte => "`<=`",
            Token::BracketOpen => "`[`",
            Token::BracketClose => "`]`",
            Token::ParenOpen => "`(`",
            Token::ParenClose => "`)`",
            Token::BraceOpen => "`{`",
            Token::BraceClose => "`}`",
        })
    }
}

/// Where a token starts: line (1-based) and column (1-based, in characters).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Loc {
    pub(crate) line: u32,
    pub(crate) col: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Template,
    Variable,
    Block,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Marker {
    Variable,
    Block,
    Comment,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ws {
    Default,
    Preserve,
    Remove,
}

impl Ws {
    fn from_byte(byte: Option<u8>) -> Ws {
        match byte {
            Some(b'-') => Ws::Remove,
            Some(b'+') => Ws::Preserve,
            _ => Ws::Default,
        }
    }

    fn len(self) -> usize {
        match self {
            Ws::Default => 0,
            Ws::Preserve | Ws::Remove => 1,
        }
    }
}

/// The tokenizer.
pub(crate) struct Lexer<'s> {
    source: &'s str,
    name: &'s str,
    offset: usize,
    line: u32,
    col: u32,
    stack: Vec<State>,
    trim_leading_whitespace: bool,
    pending: Option<(Marker, usize)>,
    paren_balance: i64,
    ws: Whitespace,
}

impl<'s> Lexer<'s> {
    pub(crate) fn new(source: &'s str, name: &'s str, ws: Whitespace) -> Lexer<'s> {
        let mut source = source;
        if !ws.keep_trailing_newline {
            if let Some(rest) = source.strip_suffix('\n') {
                source = rest;
            }
            if let Some(rest) = source.strip_suffix('\r') {
                source = rest;
            }
        }
        Lexer {
            source,
            name,
            offset: 0,
            line: 1,
            col: 1,
            stack: vec![State::Template],
            trim_leading_whitespace: false,
            pending: None,
            paren_balance: 0,
            ws,
        }
    }

    pub(crate) fn loc(&self) -> Loc {
        Loc {
            line: self.line,
            col: self.col,
        }
    }

    pub(crate) fn name(&self) -> &'s str {
        self.name
    }

    fn rest(&self) -> &'s str {
        self.source.get(self.offset..).unwrap_or("")
    }

    fn advance(&mut self, bytes: usize) -> &'s str {
        let rest = self.rest();
        let end = bytes.min(rest.len());
        let skipped = rest.get(..end).unwrap_or("");
        for ch in skipped.chars() {
            if ch == '\n' {
                self.line = self.line.saturating_add(1);
                self.col = 1;
            } else {
                self.col = self.col.saturating_add(1);
            }
        }
        self.offset += skipped.len();
        skipped
    }

    fn error(&self, message: &'static str) -> Error {
        let mut error = Error::new(ErrorKind::SyntaxError, message);
        error.locate(self.name, self.line as usize, self.col as usize);
        error
    }

    /// The next token and where it starts, or `None` at the end.
    pub(crate) fn next_token(&mut self) -> Result<Option<(Token<'s>, Loc)>, Error> {
        loop {
            if self.rest().is_empty() {
                return Ok(None);
            }
            let state = self.stack.last().copied();
            let produced = match state {
                Some(State::Template) => self.tokenize_root()?,
                Some(State::Block) => self.tokenize_inside(State::Block)?,
                Some(State::Variable) => self.tokenize_inside(State::Variable)?,
                None => return Err(self.error("unexpected input after end of variable block")),
            };
            if let Some(token) = produced {
                return Ok(Some(token));
            }
        }
    }

    fn tokenize_root(&mut self) -> Result<Option<(Token<'s>, Loc)>, Error> {
        if let Some((marker, skip)) = self.pending.take() {
            return self.start_marker(marker, skip);
        }
        if self.trim_leading_whitespace {
            self.trim_leading_whitespace = false;
            let skip: usize = self
                .rest()
                .chars()
                .take_while(|c| c.is_whitespace())
                .map(char::len_utf8)
                .sum();
            self.advance(skip);
        }
        let start = self.loc();
        let rest = self.rest();
        let lead = match find_marker(rest) {
            Some((at, marker, skip, ws)) => {
                self.pending = Some((marker, skip));
                let before = rest.get(..at).unwrap_or("");
                let keep = match ws {
                    Ws::Default if self.should_lstrip(marker, at) => lstrip_block(before),
                    Ws::Default | Ws::Preserve => before,
                    Ws::Remove => before.trim_end(),
                };
                let lead = self.advance(keep.len());
                self.advance(before.len() - keep.len());
                lead
            }
            None => self.advance(rest.len()),
        };
        Ok((!lead.is_empty()).then_some((Token::TemplateData(lead), start)))
    }

    /// Whether `lstrip_blocks` applies to a tag starting `at` bytes into
    /// the rest: only to statements and comments that start their line.
    fn should_lstrip(&self, marker: Marker, at: usize) -> bool {
        if !self.ws.lstrip_blocks || marker == Marker::Variable {
            return false;
        }
        let prefix = self.source.get(..self.offset + at).unwrap_or("");
        for ch in prefix.chars().rev() {
            if ch == '\n' || ch == '\r' {
                return true;
            }
            if !ch.is_whitespace() {
                return false;
            }
        }
        true
    }

    fn start_marker(
        &mut self,
        marker: Marker,
        skip: usize,
    ) -> Result<Option<(Token<'s>, Loc)>, Error> {
        match marker {
            Marker::Comment => {
                let rest = self.rest();
                let body = rest.get(skip..).unwrap_or("");
                match body.find("#}") {
                    Some(end) => {
                        let ws = Ws::from_byte(
                            rest.as_bytes().get(end.saturating_sub(1) + skip).copied(),
                        );
                        self.advance(end + skip + 2);
                        self.tail_whitespace(ws);
                        Ok(None)
                    }
                    None => {
                        self.advance(rest.len());
                        Err(self.error("unexpected end of comment"))
                    }
                }
            }
            Marker::Variable => {
                let start = self.loc();
                self.advance(skip);
                self.stack.push(State::Variable);
                Ok(Some((Token::VariableStart, start)))
            }
            Marker::Block => {
                let after = self.rest().get(skip..).unwrap_or("");
                if let Some((len, ws)) = basic_tag(after, "raw", false) {
                    self.advance(skip + len);
                    return self.raw_block(ws).map(Some);
                }
                let start = self.loc();
                self.advance(skip);
                self.stack.push(State::Block);
                Ok(Some((Token::BlockStart, start)))
            }
        }
    }

    fn raw_block(&mut self, ws_start: Ws) -> Result<(Token<'s>, Loc), Error> {
        let start = self.loc();
        let rest = self.rest();
        let mut search = 0;
        while let Some(found) = rest.get(search..).and_then(|r| r.find("{%")) {
            let tag = search + found + 2;
            if let Some((len, ws_next)) = basic_tag(rest.get(tag..).unwrap_or(""), "endraw", true) {
                let ws_end = Ws::from_byte(rest.as_bytes().get(tag).copied());
                let mut body = rest.get(..tag - 2).unwrap_or("");
                self.advance(tag - 2);
                self.advance(2 + len);
                match ws_start {
                    Ws::Default if self.ws.trim_blocks => {
                        body = body.strip_prefix('\r').unwrap_or(body);
                        body = body.strip_prefix('\n').unwrap_or(body);
                    }
                    Ws::Remove => body = body.trim_start(),
                    _ => {}
                }
                body = match ws_end {
                    Ws::Default if self.ws.lstrip_blocks => lstrip_block(body),
                    Ws::Remove => body.trim_end(),
                    _ => body,
                };
                self.tail_whitespace(ws_next);
                return Ok((Token::TemplateData(body), start));
            }
            search = tag;
        }
        self.advance(rest.len());
        Err(self.error("unexpected end of raw block"))
    }

    fn tail_whitespace(&mut self, ws: Ws) {
        match ws {
            Ws::Preserve => {}
            Ws::Default => self.skip_newline_if_trim_blocks(),
            Ws::Remove => self.trim_leading_whitespace = true,
        }
    }

    fn skip_newline_if_trim_blocks(&mut self) {
        if self.ws.trim_blocks {
            if self.rest().starts_with('\r') {
                self.advance(1);
            }
            if self.rest().starts_with('\n') {
                self.advance(1);
            }
        }
    }

    fn tokenize_inside(&mut self, state: State) -> Result<Option<(Token<'s>, Loc)>, Error> {
        let rest = self.rest();
        // Whitespace between tokens is skipped.
        match rest.bytes().position(|b| !b.is_ascii_whitespace()) {
            Some(0) => {}
            Some(offset) => {
                self.advance(offset);
                return Ok(None);
            }
            None => {
                self.advance(rest.len());
                return Ok(None);
            }
        }
        let start = self.loc();
        if self.paren_balance == 0 {
            let end = if state == State::Block { "%}" } else { "}}" };
            let token = if state == State::Block {
                Token::BlockEnd
            } else {
                Token::VariableEnd
            };
            let first = rest.as_bytes().first().copied();
            if matches!(first, Some(b'-' | b'+'))
                && rest.get(1..).is_some_and(|r| r.starts_with(end))
            {
                self.stack.pop();
                self.advance(3);
                if first == Some(b'-') {
                    self.trim_leading_whitespace = true;
                }
                return Ok(Some((token, start)));
            }
            if rest.starts_with(end) {
                self.stack.pop();
                self.advance(2);
                if state == State::Block {
                    self.skip_newline_if_trim_blocks();
                }
                return Ok(Some((token, start)));
            }
        }
        let two = match rest.as_bytes().get(..2) {
            Some(b"//") => Some(Token::FloorDiv),
            Some(b"**") => Some(Token::Pow),
            Some(b"==") => Some(Token::Eq),
            Some(b"!=") => Some(Token::Ne),
            Some(b">=") => Some(Token::Gte),
            Some(b"<=") => Some(Token::Lte),
            _ => None,
        };
        if let Some(token) = two {
            self.advance(2);
            return Ok(Some((token, start)));
        }
        let one = match rest.as_bytes().first() {
            Some(b'+') => Token::Plus,
            Some(b'-') => Token::Minus,
            Some(b'*') => Token::Mul,
            Some(b'/') => Token::Div,
            Some(b'%') => Token::Mod,
            Some(b'.') => Token::Dot,
            Some(b',') => Token::Comma,
            Some(b':') => Token::Colon,
            Some(b'~') => Token::Tilde,
            Some(b'|') => Token::Pipe,
            Some(b'=') => Token::Assign,
            Some(b'>') => Token::Gt,
            Some(b'<') => Token::Lt,
            Some(b'(') => {
                self.paren_balance += 1;
                Token::ParenOpen
            }
            Some(b')') => {
                self.paren_balance -= 1;
                Token::ParenClose
            }
            Some(b'[') => {
                self.paren_balance += 1;
                Token::BracketOpen
            }
            Some(b']') => {
                self.paren_balance -= 1;
                Token::BracketClose
            }
            Some(b'{') => {
                self.paren_balance += 1;
                Token::BraceOpen
            }
            Some(b'}') => {
                self.paren_balance -= 1;
                Token::BraceClose
            }
            Some(&quote @ (b'\'' | b'"')) => return self.string(quote, start).map(Some),
            Some(b) if b.is_ascii_digit() => return self.number(start).map(Some),
            _ => return self.identifier(start).map(Some),
        };
        self.advance(1);
        Ok(Some((one, start)))
    }

    fn identifier(&mut self, start: Loc) -> Result<(Token<'s>, Loc), Error> {
        let len = identifier_len(self.rest());
        if len == 0 {
            return Err(self.error("unexpected character"));
        }
        Ok((Token::Ident(self.advance(len)), start))
    }

    fn string(&mut self, quote: u8, start: Loc) -> Result<(Token<'s>, Loc), Error> {
        let rest = self.rest();
        let bytes = rest.as_bytes();
        let mut escaped = false;
        let mut has_escapes = false;
        let mut len = 0;
        for &b in bytes.iter().skip(1) {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
                has_escapes = true;
            } else if b == quote {
                break;
            }
            len += 1;
        }
        if escaped || bytes.get(len + 1) != Some(&quote) {
            self.advance(len + 1);
            return Err(self.error("unexpected end of string"));
        }
        let text = self.advance(len + 2);
        let inner = text.get(1..text.len() - 1).unwrap_or("");
        if has_escapes {
            let unescaped = unescape(inner).map_err(|mut error| {
                error.locate(self.name, start.line as usize, start.col as usize);
                error
            })?;
            Ok((Token::Str(Cow::Owned(unescaped)), start))
        } else {
            Ok((Token::Str(Cow::Borrowed(inner)), start))
        }
    }

    fn number(&mut self, start: Loc) -> Result<(Token<'s>, Loc), Error> {
        #[derive(Clone, Copy)]
        enum Part {
            RadixInteger,
            Integer,
            Fraction,
            Exponent,
            ExponentSign,
        }
        // A number right after a dot is an item index: `foo.1.0` is
        // `foo[1][0]`.
        let after_dot = self
            .source
            .as_bytes()
            .get(..self.offset)
            .and_then(<[u8]>::last)
            == Some(&b'.');
        let radix = match self.rest().as_bytes().get(..2) {
            Some(b"0b" | b"0B") => 2,
            Some(b"0o" | b"0O") => 8,
            Some(b"0x" | b"0X") => 16,
            _ => 10,
        };
        let mut part = if radix == 10 {
            Part::Integer
        } else {
            self.advance(2);
            Part::RadixInteger
        };
        let rest = self.rest();
        let bytes = rest.as_bytes();
        let mut len = bytes.iter().take_while(|c| c.is_ascii_digit()).count();
        let mut has_underscore = false;
        while let Some(&c) = bytes.get(len) {
            part = match (c, part) {
                (b'.', Part::Integer) if after_dot => break,
                (b'.', Part::Integer) => {
                    let is_exp = matches!(bytes.get(len + 1), Some(b'e' | b'E'))
                        && matches!(bytes.get(len + 2), Some(b'+' | b'-' | b'0'..=b'9'));
                    if !is_exp && identifier_len(rest.get(len + 1..).unwrap_or("")) > 0 {
                        break;
                    }
                    Part::Fraction
                }
                (b'E' | b'e', Part::Integer | Part::Fraction) => Part::Exponent,
                (b'+' | b'-' | b'0'..=b'9', Part::Exponent) => Part::ExponentSign,
                (b'0'..=b'9', part) => part,
                (b'a'..=b'f' | b'A'..=b'F', Part::RadixInteger) if radix == 16 => part,
                (b'_', part) => {
                    has_underscore = true;
                    part
                }
                _ => break,
            };
            len += 1;
        }
        let is_float = !matches!(part, Part::Integer | Part::RadixInteger);
        let mut text = Cow::Borrowed(self.advance(len));
        if has_underscore {
            if text.ends_with('_') {
                return Err(self.error("'_' may not occur at end of number"));
            }
            text = Cow::Owned(text.replace('_', ""));
        }
        if is_float {
            text.parse::<f64>()
                .map(|value| (Token::Float(value), start))
                .map_err(|_| self.error("invalid float"))
        } else {
            match u128::from_str_radix(&text, radix) {
                Ok(value) => i128::try_from(value)
                    .map(|value| (Token::Int(value), start))
                    .map_err(|_| self.error("invalid integer (too large)")),
                Err(_) => Err(self.error("invalid integer (too large)")),
            }
        }
    }
}

/// Finds the next `{{`, `{%` or `{#`: its offset, kind, the length of the
/// marker with its whitespace control, and the whitespace control.
fn find_marker(text: &str) -> Option<(usize, Marker, usize, Ws)> {
    let bytes = text.as_bytes();
    let mut offset = 0;
    loop {
        let at = offset + bytes.get(offset..)?.iter().position(|&b| b == b'{')?;
        let marker = match bytes.get(at + 1) {
            Some(b'{') => Marker::Variable,
            Some(b'%') => Marker::Block,
            Some(b'#') => Marker::Comment,
            _ => {
                offset = at + 1;
                continue;
            }
        };
        let ws = Ws::from_byte(bytes.get(at + 2).copied());
        return Some((at, marker, 2 + ws.len(), ws));
    }
}

/// `text` without the spaces and tabs that end it, when they follow a
/// newline or the start (the indentation of a tag on its own line).
fn lstrip_block(text: &str) -> &str {
    let trimmed = text.trim_end_matches(|c: char| c.is_whitespace() && c != '\n' && c != '\r');
    if trimmed.is_empty() || trimmed.ends_with('\n') {
        trimmed
    } else {
        text
    }
}

/// Matches a tag holding only `name` (`raw`, `endraw`) at the start of
/// `text` (just after `{%`): its length up to and including `%}`, and the
/// whitespace control before `%}`.
fn basic_tag(text: &str, name: &str, skip_ws_control: bool) -> Option<(usize, Ws)> {
    let mut ptr = text;
    if skip_ws_control {
        ptr = ptr.strip_prefix(['-', '+']).unwrap_or(ptr);
    }
    ptr = ptr.trim_start_matches(|c: char| c.is_ascii_whitespace());
    ptr = ptr.strip_prefix(name)?;
    ptr = ptr.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let ws = if let Some(rest) = ptr.strip_prefix('-') {
        ptr = rest;
        Ws::Remove
    } else if let Some(rest) = ptr.strip_prefix('+') {
        ptr = rest;
        Ws::Preserve
    } else {
        Ws::Default
    };
    ptr.starts_with("%}")
        .then(|| (text.len() - ptr.len() + 2, ws))
}

/// The length of the identifier `text` starts with (ASCII letters, digits
/// and `_`, not starting with a digit).
fn identifier_len(text: &str) -> usize {
    text.bytes()
        .enumerate()
        .take_while(|&(index, c)| {
            c == b'_'
                || if index == 0 {
                    c.is_ascii_alphabetic()
                } else {
                    c.is_ascii_alphanumeric()
                }
        })
        .count()
}

/// Resolves the escapes of a string literal: `\n`, `\t`, `\r`, `\b`, `\f`,
/// `\\`, `\"`, `\'`, `\/`, `\uXXXX` (with surrogate pairs), `\xHH` and
/// octal `\NNN`. Other backslashes are kept.
fn unescape(text: &str) -> Result<String, Error> {
    let bad = || Error::from(ErrorKind::BadEscape);
    let mut out = String::with_capacity(text.len());
    let mut pending_surrogate: Option<u16> = None;
    let mut chars = text.chars().peekable();
    let push = |out: &mut String, ch: char, pending: &Option<u16>| -> Result<(), Error> {
        if pending.is_some() {
            return Err(bad());
        }
        out.push(ch);
        Ok(())
    };
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            push(&mut out, ch, &pending_surrogate)?;
            continue;
        }
        let Some(escape) = chars.next() else {
            return Err(bad());
        };
        match escape {
            '"' | '\\' | '/' | '\'' => push(&mut out, escape, &pending_surrogate)?,
            'b' => push(&mut out, '\x08', &pending_surrogate)?,
            'f' => push(&mut out, '\x0c', &pending_surrogate)?,
            'n' => push(&mut out, '\n', &pending_surrogate)?,
            'r' => push(&mut out, '\r', &pending_surrogate)?,
            't' => push(&mut out, '\t', &pending_surrogate)?,
            'u' => {
                let mut hex = String::new();
                for _ in 0..4 {
                    hex.push(chars.next().unwrap_or('\0'));
                }
                let unit = u16::from_str_radix(&hex, 16).map_err(|_| bad())?;
                match (pending_surrogate.take(), unit) {
                    (None, 0xd800..=0xdbff) => pending_surrogate = Some(unit),
                    (None, 0xdc00..=0xdfff) => return Err(bad()),
                    (None, unit) => out.push(char::from_u32(u32::from(unit)).ok_or_else(bad)?),
                    (Some(high), 0xdc00..=0xdfff) => {
                        let code = 0x10000
                            + ((u32::from(high) - 0xd800) << 10)
                            + (u32::from(unit) - 0xdc00);
                        out.push(char::from_u32(code).ok_or_else(bad)?);
                    }
                    (Some(_), _) => return Err(bad()),
                }
            }
            'x' => {
                let hex: String = chars.by_ref().take(2).collect();
                if hex.len() != 2 {
                    return Err(bad());
                }
                let byte = u8::from_str_radix(&hex, 16).map_err(|_| bad())?;
                push(&mut out, char::from(byte), &pending_surrogate)?;
            }
            '0'..='7' => {
                let mut octal = String::from(escape);
                for _ in 0..2 {
                    match chars.peek() {
                        Some(&digit @ '0'..='7') => {
                            octal.push(digit);
                            chars.next();
                        }
                        _ => break,
                    }
                }
                let byte = u8::from_str_radix(&octal, 8).map_err(|_| bad())?;
                push(&mut out, char::from(byte), &pending_surrogate)?;
            }
            other => {
                push(&mut out, '\\', &pending_surrogate)?;
                push(&mut out, other, &pending_surrogate)?;
            }
        }
    }
    if pending_surrogate.is_some() {
        return Err(bad());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(source: &str, ws: Whitespace) -> Vec<Token<'_>> {
        let mut lexer = Lexer::new(source, "t", ws);
        let mut out = Vec::new();
        while let Some((token, _)) = lexer.next_token().expect("tokens") {
            out.push(token);
        }
        out
    }

    #[test]
    fn splits_data_expressions_and_statements() {
        assert_eq!(
            tokens("a {{ b|c(1) }} {%- if x -%}  d", Whitespace::default()),
            vec![
                Token::TemplateData("a "),
                Token::VariableStart,
                Token::Ident("b"),
                Token::Pipe,
                Token::Ident("c"),
                Token::ParenOpen,
                Token::Int(1),
                Token::ParenClose,
                Token::VariableEnd,
                Token::BlockStart,
                Token::Ident("if"),
                Token::Ident("x"),
                Token::BlockEnd,
                Token::TemplateData("d"),
            ]
        );
    }

    #[test]
    fn trims_blocks_and_strips_their_indentation() {
        let ws = Whitespace {
            trim_blocks: true,
            lstrip_blocks: true,
            keep_trailing_newline: true,
        };
        assert_eq!(
            tokens("a\n  {% if x %}\nb\n", ws),
            vec![
                Token::TemplateData("a\n"),
                Token::BlockStart,
                Token::Ident("if"),
                Token::Ident("x"),
                Token::BlockEnd,
                Token::TemplateData("b\n"),
            ]
        );
    }

    #[test]
    fn raw_blocks_are_data() {
        assert_eq!(
            tokens("{% raw %}{{ x }}{% endraw %}", Whitespace::default()),
            vec![Token::TemplateData("{{ x }}")]
        );
    }

    #[test]
    fn unescapes_strings() {
        assert_eq!(unescape(r"a\nbé\x41\101\q").expect("ok"), "a\nbéAA\\q");
        assert_eq!(unescape(r"😀").expect("ok"), "😀");
        assert!(unescape(r"\ud83d").is_err());
        assert!(unescape(r"\x4").is_err());
    }

    #[test]
    fn numbers() {
        assert_eq!(
            tokens("{{ 1_000 0x1f 1.5e3 2.0 }}", Whitespace::default()),
            vec![
                Token::VariableStart,
                Token::Int(1000),
                Token::Int(31),
                Token::Float(1500.0),
                Token::Float(2.0),
                Token::VariableEnd,
            ]
        );
    }
}
