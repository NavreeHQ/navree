//! Where rendered text goes, and what a render may spend.
//!
//! [`Budget`] counts one render's work and allocations. [`Output`] is the only
//! way a render builds text: it charges the budget for each block of memory
//! before asking for it, so what a render's buffers hold can't pass the cap
//! however many there are or however they grow.

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use crate::error::Error;
use crate::value::{Repr, Safety, Value};

/// The work and allocations of one render.
pub(crate) struct Budget {
    max_operations: u64,
    max_allocated_bytes: u64,
    operations: Cell<u64>,
    allocated: Cell<u64>,
}

impl Budget {
    pub(crate) fn new(max_operations: u64, max_allocated_bytes: u64) -> Rc<Budget> {
        Rc::new(Budget {
            max_operations,
            max_allocated_bytes,
            operations: Cell::new(0),
            allocated: Cell::new(0),
        })
    }

    /// Charges work.
    pub(crate) fn charge(&self, operations: u64) -> Result<(), Error> {
        let total = self.operations.get().saturating_add(operations);
        self.operations.set(total);
        if total > self.max_operations {
            return Err(Error::limit(format!(
                "the render did more than {} operations",
                self.max_operations
            )));
        }
        Ok(())
    }

    /// Charges bytes about to be allocated.
    pub(crate) fn charge_allocation(&self, bytes: u64) -> Result<(), Error> {
        let total = self.allocated.get().saturating_add(bytes);
        self.allocated.set(total);
        if total > self.max_allocated_bytes {
            return Err(Error::limit(format!(
                "the render allocated more than {} bytes",
                self.max_allocated_bytes
            )));
        }
        Ok(())
    }
}

/// A buffer of rendered text, capped in size, or nowhere while discarding.
pub(crate) struct Output {
    pub(crate) buf: String,
    discard: usize,
    limit: usize,
    too_large: &'static str,
    budget: Rc<Budget>,
    /// Whether this text ends up in an HTML document.
    html: bool,
    position: HtmlPosition,
    /// Why the last `fmt::Write` call failed, which `fmt::Error` can't say.
    failure: Option<Error>,
}

impl Output {
    /// A render's output, or what a macro or a `{% set %}` block captures.
    pub(crate) fn new(limit: usize, budget: Rc<Budget>, html: bool) -> Output {
        Output {
            buf: String::new(),
            discard: 0,
            limit,
            too_large: "the output is too large",
            budget,
            html,
            position: HtmlPosition::default(),
            failure: None,
        }
    }

    pub(crate) fn discarding(limit: usize, budget: Rc<Budget>) -> Output {
        let mut out = Output::new(limit, budget, false);
        out.discard = 1;
        out
    }

    /// A buffer for a string a filter or operator builds.
    pub(crate) fn for_string(limit: usize, budget: Rc<Budget>) -> Output {
        let mut out = Output::new(limit, budget, false);
        out.too_large = "a string is too large";
        out
    }

    #[cfg(test)]
    pub(crate) fn for_tests() -> Output {
        Output::for_string(usize::MAX, Budget::new(u64::MAX, u64::MAX))
    }

    pub(crate) fn limit(&self) -> usize {
        self.limit
    }

    pub(crate) fn is_discarding(&self) -> bool {
        self.discard > 0
    }

    pub(crate) fn begin_discarding(&mut self) {
        self.discard += 1;
    }

    pub(crate) fn end_discarding(&mut self) {
        self.discard = self.discard.saturating_sub(1);
    }

    /// Whether this text ends up in an HTML document.
    pub(crate) fn is_html(&self) -> bool {
        self.html
    }

    /// From here on this text is part of an HTML document.
    pub(crate) fn mark_html(&mut self) {
        self.html = true;
    }

    /// Whether what is written next lands in the content of a `<script>`.
    pub(crate) fn in_script(&self) -> bool {
        self.position.in_script()
    }

    /// Makes room for `bytes` in all, charged now.
    pub(crate) fn reserve(&mut self, bytes: usize) -> Result<(), Error> {
        if bytes > self.limit {
            return Err(Error::limit(self.too_large));
        }
        self.grow_to(bytes)
    }

    fn grow_to(&mut self, capacity: usize) -> Result<(), Error> {
        if capacity <= self.buf.capacity() {
            return Ok(());
        }
        self.budget.charge_allocation(capacity as u64)?;
        self.buf.reserve_exact(capacity - self.buf.len());
        Ok(())
    }

    pub(crate) fn write_str(&mut self, text: &str) -> Result<(), Error> {
        if self.discard > 0 {
            return Ok(());
        }
        let needed = self.buf.len().saturating_add(text.len());
        if needed > self.limit {
            return Err(Error::limit(self.too_large));
        }
        if needed > self.buf.capacity() {
            let doubled = self.buf.capacity().saturating_mul(2).max(64);
            self.grow_to(needed.max(doubled.min(self.limit)))?;
        }
        self.budget.charge((text.len() / 64) as u64)?;
        self.position.advance(text.as_bytes());
        self.buf.push_str(text);
        Ok(())
    }

    /// Keeps why a formatted write is about to fail.
    pub(crate) fn set_failure(&mut self, error: Error) {
        self.failure = Some(error);
    }

    /// The reason the last formatted write failed.
    pub(crate) fn take_failure(&mut self) -> Error {
        self.failure
            .take()
            .unwrap_or_else(|| Error::limit(self.too_large))
    }

    /// The text as a value marked `safety`. A value's string is a copy of its
    /// own, charged here.
    pub(crate) fn finish(self, safety: Safety) -> Result<Value, Error> {
        self.budget.charge_allocation(self.buf.len() as u64)?;
        self.budget.charge((self.buf.len() / 64) as u64)?;
        Ok(Value(Repr::Str(Arc::from(self.buf), safety)))
    }

    /// The text as it is.
    pub(crate) fn into_string(self) -> String {
        self.buf
    }
}

impl fmt::Write for Output {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        Output::write_str(self, text).map_err(|error| {
            self.failure = Some(error);
            fmt::Error
        })
    }
}

/// The elements whose content is text up to their own end tag.
const RAW_TEXT: [&[u8]; 8] = [
    b"script",
    b"style",
    b"textarea",
    b"title",
    b"xmp",
    b"iframe",
    b"noembed",
    b"noframes",
];

/// The longest of [`RAW_TEXT`].
const MAX_RAW_NAME: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Scan {
    #[default]
    Data,
    /// After `<`.
    TagOpen,
    /// In a tag's name.
    Name,
    /// In a tag, between attributes.
    Tag,
    Unquoted,
    DoubleQuoted,
    SingleQuoted,
    /// After `<!`.
    Markup,
    /// In `<!-- -->`.
    Comment,
    /// In `<!x>` or `<?x>`.
    Bogus,
    /// In the content of a [`RAW_TEXT`] element.
    RawText,
    /// In that content, `matched` bytes into what may be its end tag.
    RawEnd,
}

/// Follows HTML's tokenizing far enough to tell where the next byte lands.
/// It errs on the side of "not in a script": a `tojson` value printed there
/// escapes its quotes, which can't be unsafe.
#[derive(Default)]
struct HtmlPosition {
    scan: Scan,
    name: [u8; MAX_RAW_NAME],
    name_len: usize,
    closing: bool,
    /// The [`RAW_TEXT`] element a tag being read opens, or the one it is in.
    raw: Option<usize>,
    self_closing: bool,
    after_equals: bool,
    /// Dashes in a row in a comment, or bytes of an end tag matched.
    matched: usize,
}

impl HtmlPosition {
    fn in_script(&self) -> bool {
        matches!(self.scan, Scan::RawText | Scan::RawEnd) && self.raw == Some(0)
    }

    fn advance(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        while let Some((&byte, tail)) = rest.split_first() {
            // Text is most of what goes through here.
            if matches!(self.scan, Scan::Data | Scan::RawText) && byte != b'<' {
                let skip = tail.iter().position(|&b| b == b'<').unwrap_or(tail.len());
                rest = tail.get(skip..).unwrap_or(&[]);
                continue;
            }
            self.step(byte);
            rest = tail;
        }
    }

    fn step(&mut self, byte: u8) {
        match self.scan {
            Scan::Data => {
                if byte == b'<' {
                    self.scan = Scan::TagOpen;
                }
            }
            Scan::TagOpen => self.tag_open(byte),
            Scan::Name => self.name_byte(byte),
            Scan::Tag => self.in_tag(byte),
            Scan::Unquoted => match byte {
                b'>' => self.close_tag(),
                b' ' | b'\t' | b'\n' | b'\r' | 0x0c => {
                    self.scan = Scan::Tag;
                    self.after_equals = false;
                }
                _ => {}
            },
            Scan::DoubleQuoted | Scan::SingleQuoted => {
                let quote = if self.scan == Scan::DoubleQuoted {
                    b'"'
                } else {
                    b'\''
                };
                if byte == quote {
                    self.scan = Scan::Tag;
                    self.after_equals = false;
                }
            }
            Scan::Markup => match byte {
                b'-' if self.matched == 1 => {
                    self.scan = Scan::Comment;
                    self.matched = 0;
                }
                b'-' => self.matched = 1,
                b'>' => self.scan = Scan::Data,
                _ => self.scan = Scan::Bogus,
            },
            Scan::Comment => match byte {
                b'-' => self.matched += 1,
                b'>' if self.matched >= 2 => self.scan = Scan::Data,
                _ => self.matched = 0,
            },
            Scan::Bogus => {
                if byte == b'>' {
                    self.scan = Scan::Data;
                }
            }
            Scan::RawText => {
                if byte == b'<' {
                    self.scan = Scan::RawEnd;
                    self.matched = 0;
                }
            }
            Scan::RawEnd => self.raw_end(byte),
        }
    }

    fn tag_open(&mut self, byte: u8) {
        self.closing = false;
        self.self_closing = false;
        self.after_equals = false;
        self.raw = None;
        self.name_len = 0;
        self.matched = 0;
        match byte {
            b'/' => {
                self.closing = true;
                self.scan = Scan::Name;
            }
            b'!' => self.scan = Scan::Markup,
            b'?' => self.scan = Scan::Bogus,
            b'<' => {}
            _ if byte.is_ascii_alphabetic() => {
                self.scan = Scan::Name;
                self.name_byte(byte);
            }
            _ => self.scan = Scan::Data,
        }
    }

    fn name_byte(&mut self, byte: u8) {
        match byte {
            b' ' | b'\t' | b'\n' | b'\r' | 0x0c => {
                self.end_name();
                self.scan = Scan::Tag;
            }
            b'/' => {
                self.end_name();
                self.scan = Scan::Tag;
                self.self_closing = true;
            }
            b'>' => {
                self.end_name();
                self.close_tag();
            }
            _ => {
                if let Some(slot) = self.name.get_mut(self.name_len) {
                    *slot = byte.to_ascii_lowercase();
                }
                self.name_len = self.name_len.saturating_add(1);
            }
        }
    }

    fn end_name(&mut self) {
        let name = self.name.get(..self.name_len).unwrap_or(&[]);
        self.raw = if self.closing {
            None
        } else {
            RAW_TEXT.iter().position(|raw| *raw == name)
        };
    }

    fn in_tag(&mut self, byte: u8) {
        match byte {
            b'>' => self.close_tag(),
            b'=' => {
                self.after_equals = true;
                self.self_closing = false;
            }
            b' ' | b'\t' | b'\n' | b'\r' | 0x0c => self.self_closing = false,
            b'/' => {
                self.self_closing = true;
                self.after_equals = false;
            }
            b'"' if self.after_equals => self.scan = Scan::DoubleQuoted,
            b'\'' if self.after_equals => self.scan = Scan::SingleQuoted,
            _ => {
                self.self_closing = false;
                if self.after_equals {
                    self.scan = Scan::Unquoted;
                }
            }
        }
    }

    fn close_tag(&mut self) {
        // `<script/>` opens a script in HTML and ends at once in SVG: taken
        // as not a script, which is the safe way to be wrong.
        self.scan = if self.raw.is_some() && !self.self_closing {
            Scan::RawText
        } else {
            self.raw = None;
            Scan::Data
        };
    }

    /// `matched` counts the bytes after `<`: the `/`, then the name.
    fn raw_end(&mut self, byte: u8) {
        let name = self
            .raw
            .and_then(|raw| RAW_TEXT.get(raw))
            .copied()
            .unwrap_or(&[]);
        if self.matched == 0 {
            if byte == b'/' {
                self.matched = 1;
            } else {
                self.scan = Scan::RawText;
                self.step(byte);
            }
            return;
        }
        match name.get(self.matched - 1) {
            Some(expected) if byte.to_ascii_lowercase() == *expected => self.matched += 1,
            None if matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c | b'/' | b'>') => {
                self.raw = None;
                self.closing = true;
                self.self_closing = false;
                self.after_equals = false;
                self.scan = Scan::Tag;
                self.in_tag(byte);
            }
            _ => {
                self.scan = Scan::RawText;
                self.step(byte);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_script_after(pieces: &[&str]) -> bool {
        let mut position = HtmlPosition::default();
        for piece in pieces {
            position.advance(piece.as_bytes());
        }
        position.in_script()
    }

    #[test]
    fn knows_a_script_from_what_looks_like_one() {
        for pieces in [
            &["<script>"][..],
            &["<SCRIPT>"],
            &["<script type=\"a>b\">"],
            &["<script type='a'>var a = 1 < 2;"],
            &["<sc", "ript", ">"],
            &["<script>", "</scrip"],
            &["<script></style>"],
            &["<script>a</scriptx>"],
            &["</script><script>"],
            &["<a href=x><script>"],
            &["<script x=y>"],
        ] {
            assert!(in_script_after(pieces), "{pieces:?}");
        }
        for pieces in [
            &[""][..],
            &["plain"],
            &["<scripty>"],
            &["<script"],
            &["<script a=\">"],
            &["<a title=\"<script>\">"],
            &["<a title='<script>'>"],
            &["<a title=<script>>"],
            &["<!-- <script> -->"],
            &["<!-- <script>"],
            &["<!doctype <script>"],
            &["<? <script> ?>"],
            &["<script></script>"],
            &["<script></SCRIPT >"],
            &["<script></scr", "ipt>"],
            &["<style><script></style>"],
            &["<textarea><script></textarea>"],
            &["<title><script></title>"],
            &["<script/>"],
            &["<a b\"c d=\"<script>"],
            &["< script>"],
        ] {
            assert!(!in_script_after(pieces), "{pieces:?}");
        }
    }

    #[test]
    fn growth_is_charged_before_it_happens() {
        let budget = Budget::new(u64::MAX, 100);
        let mut out = Output::for_string(1 << 20, budget);
        out.write_str(&"x".repeat(50)).expect("within the budget");
        let error = out
            .write_str(&"x".repeat(100))
            .expect_err("over the budget");
        assert!(error.to_string().contains("allocated"), "{error}");
        assert!(out.buf.capacity() <= 64, "{}", out.buf.capacity());
    }
}
