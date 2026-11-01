//! Content streams: the operations a page or form draws, read one at a
//! time, and written.

use std::cell::Cell;

use crate::error::{PdfError, size};
use crate::object::Object;
use crate::parse::{Fail, Lexer, MAX_DEPTH, Meter, is_white};
use crate::write::write_object;

/// What the operands of one operation may hold in memory (16 MiB). Real
/// operations hold a few numbers, or a line of text for `TJ`.
pub const MAX_OPERANDS_MEMORY: usize = 16 * 1024 * 1024;

/// One operation: its operands, then its operator.
#[derive(Clone, Debug, PartialEq)]
pub struct Operation {
    /// `Tj`, `cm`, `BT`.
    pub operator: String,
    /// The operands, in order.
    pub operands: Vec<Object>,
}

impl Operation {
    /// `operator` with `operands`.
    pub fn new(operator: &str, operands: Vec<Object>) -> Operation {
        Operation {
            operator: operator.to_owned(),
            operands,
        }
    }
}

/// `operations` as a content stream, one a line.
pub fn encode(operations: &[Operation]) -> Vec<u8> {
    let mut out = Vec::new();
    for (index, operation) in operations.iter().enumerate() {
        if index > 0 {
            out.push(b'\n');
        }
        for operand in &operation.operands {
            write_object(&mut out, operand);
            out.push(b' ');
        }
        out.extend_from_slice(operation.operator.as_bytes());
    }
    out
}

/// The memory one operation's operands hold.
struct OperandMeter(Cell<usize>);

impl Meter for OperandMeter {
    fn charge(&self, bytes: usize) -> Result<(), PdfError> {
        let used = self.0.get().saturating_add(bytes);
        self.0.set(used);
        if used > MAX_OPERANDS_MEMORY {
            return Err(PdfError::TooLarge(format!(
                "an operation of a PDF content stream has operands of more than {}",
                size(MAX_OPERANDS_MEMORY)
            )));
        }
        Ok(())
    }
}

/// The operations of `content`, one at a time, so a stream of millions is
/// never held decoded at once. Inline images (`BI` ... `ID` ... `EI`) are
/// skipped. A closing `]`, `>` or brace with nothing to close, and a number
/// with a doubled minus or too large for an integer, are read past as other
/// readers do. Bytes that cannot be made sense of end the stream; operands
/// nested deeper than 100 levels, or one operation's operands over
/// [`MAX_OPERANDS_MEMORY`], are an error.
pub fn operations(content: &[u8]) -> Operations<'_> {
    Operations {
        lexer: Lexer::content(content),
        done: false,
        meter: OperandMeter(Cell::new(0)),
    }
}

/// The iterator of [`operations`].
pub struct Operations<'a> {
    lexer: Lexer<'a>,
    done: bool,
    /// What the operands of the operation read last hold.
    meter: OperandMeter,
}

impl Operations<'_> {
    /// What the operands of the operation read last hold in memory, as they
    /// are counted against [`MAX_OPERANDS_MEMORY`].
    pub fn operands_memory(&self) -> usize {
        self.meter.0.get()
    }

    /// Moves past an inline image's data, from just after `ID`: to the
    /// `EI` that follows white space and is followed by white space or the
    /// end.
    fn skip_image(&mut self) -> bool {
        // One white-space byte separates `ID` from the data.
        if self.lexer.peek().is_some_and(is_white) {
            self.lexer.pos += 1;
        }
        let rest = self.lexer.rest();
        let mut at = 0;
        while let Some(found) = crate::xref::find(rest, b"EI", at) {
            let before = found.checked_sub(1).and_then(|i| rest.get(i)).copied();
            let after = rest.get(found + 2).copied();
            if before.is_some_and(is_white) && after.is_none_or(is_white) {
                self.lexer.pos += found + 2;
                return true;
            }
            at = found + 1;
        }
        false
    }
}

impl Iterator for Operations<'_> {
    type Item = Result<Operation, PdfError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        self.meter.0.set(0);
        let meter = &self.meter;
        let mut operands = Vec::new();
        loop {
            self.lexer.skip_space();
            let Some(byte) = self.lexer.peek() else {
                self.done = true;
                return None;
            };
            // A closing bracket with nothing open: one byte, read past.
            if matches!(byte, b']' | b'>' | b'{' | b'}') {
                self.lexer.pos += 1;
                continue;
            }
            let operator_byte =
                |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'*' | b'\'' | b'"');
            if byte.is_ascii_alphabetic() || matches!(byte, b'*' | b'\'' | b'"') {
                let start = self.lexer.pos;
                let mut end = start;
                while self.lexer.buf.get(end).copied().is_some_and(operator_byte) {
                    end += 1;
                }
                let word = self.lexer.buf.get(start..end).unwrap_or_default();
                if matches!(word, b"true" | b"false" | b"null") {
                    match self.lexer.object(MAX_DEPTH, meter) {
                        Ok(object) => operands.push(object),
                        Err(Fail::Over(error)) => return Some(Err(error)),
                        Err(_) => {
                            self.done = true;
                            return None;
                        }
                    }
                    continue;
                }
                self.lexer.pos = end;
                let operator = String::from_utf8_lossy(word).into_owned();
                if operator == "BI" {
                    // The image's dictionary entries, up to `ID`.
                    loop {
                        self.lexer.skip_space();
                        if self.lexer.starts_with(b"ID") {
                            self.lexer.pos += 2;
                            break;
                        }
                        if self.lexer.peek().is_none() {
                            self.done = true;
                            return None;
                        }
                        match self.lexer.object(MAX_DEPTH, meter) {
                            Ok(_) => {}
                            Err(Fail::Over(error)) => return Some(Err(error)),
                            Err(_) => {
                                self.done = true;
                                return None;
                            }
                        }
                    }
                    if !self.skip_image() {
                        self.done = true;
                        return Some(Err(PdfError::damaged("an inline image has no end (EI)")));
                    }
                    return Some(Ok(Operation::new("BI", Vec::new())));
                }
                return Some(Ok(Operation { operator, operands }));
            }
            match self.lexer.object(MAX_DEPTH, meter) {
                Ok(object) => operands.push(object),
                Err(Fail::Over(error)) => {
                    self.done = true;
                    return Some(Err(error));
                }
                Err(Fail::Deep) => {
                    self.done = true;
                    return Some(Err(PdfError::damaged(
                        "a content stream nests arrays or dictionaries too deep",
                    )));
                }
                // Not an operand or an operator: the rest is not read.
                Err(Fail::Syntax) => {
                    self.done = true;
                    return None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(content: &str) -> Vec<Operation> {
        operations(content.as_bytes())
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn operations_come_with_their_operands() {
        let ops = decoded(
            "BT /F1 12 Tf 1 0 0 1 50 700 Tm (Hi) Tj [(A) -20 (B)] TJ ET q 1 0 0 1 0 0 cm Q",
        );
        let names: Vec<&str> = ops.iter().map(|op| op.operator.as_str()).collect();
        assert_eq!(names, ["BT", "Tf", "Tm", "Tj", "TJ", "ET", "q", "cm", "Q"]);
        assert_eq!(ops[1].operands, [Object::name("F1"), Object::Integer(12)]);
        assert_eq!(ops[3].operands, [Object::string_literal("Hi")]);
        // No references in a content stream: these are colors.
        let colors = decoded("0 0 0 RG 1 0 0 rg");
        assert_eq!(colors[0].operator, "RG");
        assert_eq!(
            colors[0].operands,
            [Object::Integer(0), Object::Integer(0), Object::Integer(0)]
        );
        assert_eq!(colors[1].operands.len(), 3);
        let quoted = decoded("1 2 (x) \" (y) ' T* d0 d1");
        let names: Vec<&str> = quoted.iter().map(|op| op.operator.as_str()).collect();
        assert_eq!(names, ["\"", "'", "T*", "d0", "d1"]);
        assert_eq!(
            decoded("/P << /MCID 0 >> BDC true false null EMC")[1]
                .operands
                .len(),
            3
        );
    }

    #[test]
    fn inline_images_and_comments_are_skipped() {
        let content = b"q % a comment\nBI /W 2 /H 1 /CS /G /BPC 8 ID \x00\xffEI Q\nEI Q";
        let ops: Vec<Operation> = operations(content).collect::<Result<_, _>>().unwrap();
        let names: Vec<&str> = ops.iter().map(|op| op.operator.as_str()).collect();
        assert_eq!(names, ["q", "BI", "Q"]);
        assert!(operations(b"BI /W 1 ID nothing").any(|op| op.is_err()));
    }

    #[test]
    fn junk_ends_the_stream_and_deep_nesting_is_an_error() {
        let ops = decoded("BT (a) Tj ) ET");
        assert_eq!(ops.len(), 2);
        let deep = format!("{} Tj", "[".repeat(200));
        assert!(operations(deep.as_bytes()).any(|op| op.is_err()));
    }

    #[test]
    fn operations_encode_and_decode_again() {
        let ops = vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec![Object::name("F1"), 11.into()]),
            Operation::new("Tj", vec![Object::string_literal("a (b)")]),
            Operation::new(
                "Tm",
                vec![
                    1.5f32.into(),
                    0.into(),
                    0.into(),
                    1.into(),
                    2.into(),
                    3.into(),
                ],
            ),
            Operation::new("ET", vec![]),
        ];
        let bytes = encode(&ops);
        assert_eq!(decoded(std::str::from_utf8(&bytes).unwrap()), ops);
    }
}
