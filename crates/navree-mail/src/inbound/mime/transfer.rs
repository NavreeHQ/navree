//! Content transfer encodings: base64 and quoted-printable, for bodies and
//! for RFC 2047 encoded words. Every decoder reads its input once and
//! writes at most as many bytes as it reads.

/// The value of a base64 character, or `None` for anything else.
fn base64_value(byte: u8) -> Option<u32> {
    match byte {
        b'A'..=b'Z' => Some(u32::from(byte - b'A')),
        b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// The value of a hexadecimal digit (either case).
pub(super) fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Collects base64 groups of four characters into bytes.
#[derive(Default)]
struct Base64 {
    group: u32,
    count: u8,
}

impl Base64 {
    fn push(&mut self, value: u32, out: &mut Vec<u8>) {
        self.group = (self.group << 6) | value;
        self.count += 1;
        if self.count == 4 {
            let [_, a, b, c] = self.group.to_be_bytes();
            out.extend_from_slice(&[a, b, c]);
            self.group = 0;
            self.count = 0;
        }
    }

    /// Ends a group early (`=` padding, or the end of the data): the whole
    /// bytes its characters hold. A lone character holds none.
    fn flush(&mut self, out: &mut Vec<u8>) {
        match self.count {
            2 => out.push((self.group >> 4) as u8),
            3 => {
                let group = self.group >> 2;
                out.extend_from_slice(&[(group >> 8) as u8, group as u8]);
            }
            _ => {}
        }
        self.group = 0;
        self.count = 0;
    }
}

/// Decodes a base64 body. Line breaks, spaces and characters outside the
/// alphabet are skipped; padding ends a group, and decoding goes on after
/// it (some senders join encoded blocks).
pub(super) fn base64(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() / 4 * 3 + 3);
    let mut state = Base64::default();
    for &byte in input {
        match base64_value(byte) {
            Some(value) => state.push(value, &mut out),
            None if byte == b'=' => state.flush(&mut out),
            None => {}
        }
    }
    state.flush(&mut out);
    out
}

/// Decodes the text of a `B` encoded word: `None` when it holds anything
/// but base64 characters and padding.
pub(super) fn base64_word(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() / 4 * 3 + 3);
    let mut state = Base64::default();
    for &byte in input {
        match base64_value(byte) {
            Some(value) => state.push(value, &mut out),
            None if byte == b'=' => state.flush(&mut out),
            None if matches!(byte, b' ' | b'\t' | b'\r' | b'\n') => {}
            None => return None,
        }
    }
    state.flush(&mut out);
    Some(out)
}

/// Decodes a quoted-printable body (RFC 2045): `=XY` escapes, soft line
/// breaks (`=` at the end of a line, transport padding allowed), and the
/// spaces and tabs a line ends with dropped. Each line keeps its line
/// break as written. An `=` that starts no escape is kept as it is.
pub(super) fn quoted_printable(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut rest = input;
    while !rest.is_empty() {
        let (line, ending, next) = match memchr::memchr(b'\n', rest) {
            Some(end) => {
                let (line, ending) = match end.checked_sub(1) {
                    Some(cr) if rest.get(cr) == Some(&b'\r') => (&rest[..cr], &b"\r\n"[..]),
                    _ => (&rest[..end], &b"\n"[..]),
                };
                (line, ending, &rest[end + 1..])
            }
            None => (rest, &b""[..], &b""[..]),
        };
        let line = trim_end_blanks(line);
        let soft = line.last() == Some(&b'=');
        let line = if soft { &line[..line.len() - 1] } else { line };
        let mut index = 0;
        while let Some(&byte) = line.get(index) {
            if byte == b'='
                && let (Some(high), Some(low)) = (
                    line.get(index + 1).copied().and_then(hex_value),
                    line.get(index + 2).copied().and_then(hex_value),
                )
            {
                out.push((high << 4) | low);
                index += 3;
                continue;
            }
            out.push(byte);
            index += 1;
        }
        if !soft {
            out.extend_from_slice(ending);
        }
        rest = next;
    }
    out
}

/// Decodes the text of a `Q` encoded word: `_` is a space. `None` when an
/// `=` starts no escape.
pub(super) fn q_word(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len());
    let mut index = 0;
    while let Some(&byte) = input.get(index) {
        match byte {
            b'=' => {
                let high = input.get(index + 1).copied().and_then(hex_value)?;
                let low = input.get(index + 2).copied().and_then(hex_value)?;
                out.push((high << 4) | low);
                index += 3;
                continue;
            }
            b'_' => out.push(b' '),
            b'\r' => {}
            // A word folded across lines: the fold is not part of it.
            b'\n' => {
                while matches!(input.get(index + 1), Some(b' ' | b'\t')) {
                    index += 1;
                }
            }
            _ => out.push(byte),
        }
        index += 1;
    }
    Some(out)
}

/// The line without the spaces and tabs it ends with.
fn trim_end_blanks(line: &[u8]) -> &[u8] {
    let end = line
        .iter()
        .rposition(|&byte| !matches!(byte, b' ' | b'\t'))
        .map_or(0, |last| last + 1);
    &line[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_skips_line_breaks_and_junk() {
        assert_eq!(base64(b"SGVs\r\nbG8s\n IHdv cmxk"), b"Hello, world");
        assert_eq!(base64(b"SGVsbG8=\r\n"), b"Hello");
        assert_eq!(base64(b"SGk=SGk="), b"HiHi");
        assert_eq!(base64(b"SG*Vs#bG8"), b"Hello");
        assert_eq!(
            base64(b"QUI"),
            b"AB",
            "unpadded groups keep their whole bytes"
        );
        assert_eq!(base64(b"Q"), b"");
        assert_eq!(base64(b""), b"");
    }

    #[test]
    fn base64_words_refuse_junk() {
        assert_eq!(base64_word(b"SGVsbG8=").as_deref(), Some(&b"Hello"[..]));
        assert_eq!(base64_word(b"SGVs bG8=").as_deref(), Some(&b"Hello"[..]));
        assert_eq!(base64_word(b"SGV*bG8="), None);
    }

    #[test]
    fn quoted_printable_decodes_escapes_and_soft_breaks() {
        assert_eq!(
            quoted_printable(b"Sch=F6ne Gr=FC=DFe!"),
            b"Sch\xf6ne Gr\xfc\xdfe!"
        );
        assert_eq!(quoted_printable(b"a=\r\nb\r\nc"), b"ab\r\nc");
        assert_eq!(quoted_printable(b"a= \t\nb\n"), b"ab\n");
        assert_eq!(quoted_printable(b"trailing   \nnext"), b"trailing\nnext");
        assert_eq!(quoted_printable(b"kept=20\n"), b"kept \n");
        assert_eq!(quoted_printable(b"a=zz=4"), b"a=zz=4");
        assert_eq!(quoted_printable(b"lower=e9"), b"lower\xe9");
        assert_eq!(quoted_printable(b"=\n="), b"");
    }

    #[test]
    fn q_words_turn_underscores_into_spaces() {
        assert_eq!(q_word(b"Keith_Moore").as_deref(), Some(&b"Keith Moore"[..]));
        assert_eq!(q_word(b"a=3Fb").as_deref(), Some(&b"a?b"[..]));
        assert_eq!(q_word(b"a=3"), None);
        assert_eq!(q_word(b"a=G0"), None);
        assert_eq!(q_word(b"a\r\n  b").as_deref(), Some(&b"ab"[..]));
    }
}
