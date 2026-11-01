//! Header blocks (RFC 5322 section 2.2): fields with folded values, up to
//! the blank line that starts the body.

/// One header field as written: its name (trimmed) and its value, from
/// after the colon to the end of its last continuation line (without the
/// line break; folds kept).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Field<'a> {
    pub name: &'a [u8],
    pub value: &'a [u8],
}

/// The fields of a header block, in order.
///
/// A line that starts with a space or a tab continues the field above it.
/// A line without a colon is skipped. The block ends at a blank line (the
/// body starts after it) or at the end of the bytes (no body).
pub(super) struct Fields<'a> {
    bytes: &'a [u8],
    at: usize,
    /// Where the body starts, once the blank line is found.
    body: Option<usize>,
    done: bool,
}

impl<'a> Fields<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            at: 0,
            body: None,
            done: false,
        }
    }

    /// Where the body starts: after the blank line that ends the block, or
    /// `None` when the block runs to the end. Meaningful once the fields
    /// have all been read.
    pub(super) fn body_start(&self) -> Option<usize> {
        self.body
    }

    /// The line at `at`: its content (without the line break) and where the
    /// next line starts.
    fn line(&self, at: usize) -> (&'a [u8], usize) {
        let rest = self.bytes.get(at..).unwrap_or_default();
        match memchr::memchr(b'\n', rest) {
            Some(end) => {
                let line = &rest[..end];
                (line.strip_suffix(b"\r").unwrap_or(line), at + end + 1)
            }
            None => (rest, self.bytes.len()),
        }
    }
}

impl<'a> Iterator for Fields<'a> {
    type Item = Field<'a>;

    fn next(&mut self) -> Option<Field<'a>> {
        while !self.done {
            if self.at >= self.bytes.len() {
                self.done = true;
                return None;
            }
            let start = self.at;
            let (line, next) = self.line(start);
            if line.iter().all(|&byte| is_space(byte)) {
                // A blank line (here, never a continuation) ends the block.
                self.done = true;
                self.body = Some(next);
                return None;
            }
            // The field's last line: continuation lines start with a blank.
            let mut end = start + line.len();
            let mut after = next;
            while let Some(&first) = self.bytes.get(after)
                && (first == b' ' || first == b'\t')
            {
                let (more, following) = self.line(after);
                end = after + more.len();
                after = following;
            }
            self.at = after;
            let Some(colon) = memchr::memchr(b':', line) else {
                continue;
            };
            let name = trim(&line[..colon]);
            if name.is_empty() {
                continue;
            }
            return Some(Field {
                name,
                value: self.bytes.get(start + colon + 1..end).unwrap_or_default(),
            });
        }
        None
    }
}

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\x0c')
}

/// The bytes without the blanks around them.
pub(super) fn trim(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|&byte| !is_space(byte) && byte != b'\n');
    let end = bytes
        .iter()
        .rposition(|&byte| !is_space(byte) && byte != b'\n');
    match (start, end) {
        (Some(start), Some(end)) => &bytes[start..=end],
        _ => &[],
    }
}

/// A folded value on one line: line breaks removed, the blanks after them
/// kept (RFC 5322 unfolding). Bytes that aren't UTF-8 become U+FFFD.
pub(crate) fn unfold(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value);
    if !text.contains(['\r', '\n']) {
        return text.into_owned();
    }
    text.chars().filter(|&c| c != '\r' && c != '\n').collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(bytes: &[u8]) -> (Vec<(String, String)>, Option<usize>) {
        let mut fields = Fields::new(bytes);
        let list = fields
            .by_ref()
            .map(|field| {
                (
                    String::from_utf8_lossy(field.name).into_owned(),
                    String::from_utf8_lossy(field.value).into_owned(),
                )
            })
            .collect();
        (list, fields.body_start())
    }

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    #[test]
    fn fields_end_at_the_blank_line() {
        let raw = b"From: a@b.test\r\nSubject: one\r\n two\r\n\tthree\r\nX-Empty:\r\n\r\nbody\r\n";
        let (list, body) = fields(raw);
        assert_eq!(
            list,
            vec![
                pair("From", " a@b.test"),
                pair("Subject", " one\r\n two\r\n\tthree"),
                pair("X-Empty", "")
            ]
        );
        assert_eq!(&raw[body.unwrap()..], b"body\r\n");
    }

    #[test]
    fn odd_lines_are_skipped_and_names_trimmed() {
        let block = b"no colon here\nSubject : hi\n: empty name\nmal formed: x\n \t \n";
        let (list, body) = fields(block);
        assert_eq!(
            list,
            vec![pair("Subject", " hi"), pair("mal formed", " x\n \t ")]
        );
        assert_eq!(body, None);
        let (_, body) = fields(b" \t \r\nbody");
        assert_eq!(body, Some(5), "a line of blanks ends the block");
    }

    #[test]
    fn a_block_without_a_blank_line_has_no_body() {
        assert_eq!(fields(b"Subject: hi"), (vec![pair("Subject", " hi")], None));
        assert_eq!(
            fields(b"Subject: hi\n"),
            (vec![pair("Subject", " hi")], None)
        );
        assert_eq!(
            fields(b"Subject: hi\n\n"),
            (vec![pair("Subject", " hi")], Some(13))
        );
        assert_eq!(fields(b"\nbody"), (vec![], Some(1)));
        assert_eq!(fields(b""), (vec![], None));
    }

    #[test]
    fn a_whitespace_line_after_a_field_continues_it() {
        let (list, body) = fields(b"Subject: a\n \nX: y\n\n");
        assert_eq!(list, vec![pair("Subject", " a\n "), pair("X", " y")]);
        assert_eq!(body, Some(19));
    }

    #[test]
    fn unfolding_keeps_the_blanks() {
        assert_eq!(unfold(b" one\r\n two\n\tthree"), " one two\tthree");
        assert_eq!(unfold(b"caf\xe9"), "caf\u{fffd}");
        assert_eq!(trim(b" \t a b \r\n"), b"a b");
    }
}
