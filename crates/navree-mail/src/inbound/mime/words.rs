//! RFC 2047 encoded words (`=?charset?Q?...?=`, `=?charset?B?...?=`), and
//! unstructured header text such as `Subject`.
//!
//! Header values are read as bytes: an encoded word decodes the bytes it
//! holds in its charset (some senders put 8-bit bytes in them), and the
//! text around encoded words is read as UTF-8, U+FFFD for what isn't.

use super::{charset, transfer};

/// The longest charset name an encoded word may carry.
const MAX_CHARSET: usize = 64;

/// Finds the encoded words of one header value, read from left to right.
///
/// The encoded text runs to the first `?=` and may hold spaces, which some
/// senders leave unencoded. Where that `?=` is gets remembered, so a value
/// full of `=?` that never ends is still read in linear time.
pub(super) struct EncodedWords<'a> {
    text: &'a [u8],
    /// The last search for `?=`: where it started and what it found.
    end: Option<(usize, Option<usize>)>,
}

impl<'a> EncodedWords<'a> {
    pub(super) fn new(text: &'a [u8]) -> Self {
        Self { text, end: None }
    }

    /// The encoded word that starts at byte `index`: its decoded text and
    /// its length. `None` when no word that decodes starts there.
    ///
    /// The charset may carry an RFC 2231 language (`utf-8*en`).
    pub(super) fn at(&mut self, index: usize) -> Option<(String, usize)> {
        let rest = self.text.get(index..)?.strip_prefix(b"=?")?;
        let charset_end = rest
            .iter()
            .take(MAX_CHARSET + 1)
            .position(|&byte| byte == b'?')?;
        let charset = rest.get(..charset_end)?;
        if charset.len() < 2 || !charset.iter().all(|byte| byte.is_ascii_graphic()) {
            return None;
        }
        let base64 = match rest.get(charset_end + 1..charset_end + 3) {
            Some([b'q' | b'Q', b'?']) => false,
            Some([b'b' | b'B', b'?']) => true,
            _ => return None,
        };
        let data_start = index + 2 + charset_end + 3;
        let data_end = self.terminator(data_start)?;
        let encoded = self.text.get(data_start..data_end)?;
        let bytes = if base64 {
            transfer::base64_word(encoded)?
        } else {
            transfer::q_word(encoded)?
        };
        let charset = String::from_utf8_lossy(charset);
        let charset = charset.split('*').next().unwrap_or_default();
        Some((charset::decode(&bytes, Some(charset)), data_end + 2 - index))
    }

    /// The first `?=` at or after `from`.
    fn terminator(&mut self, from: usize) -> Option<usize> {
        if let Some((start, found)) = self.end
            && start <= from
            && found.is_none_or(|found| found >= from)
        {
            return found;
        }
        let found = self
            .text
            .get(from..)
            .and_then(|rest| rest.windows(2).position(|pair| pair == b"?="))
            .map(|at| from + at);
        self.end = Some((from, found));
        found
    }

    /// Whether an encoded word that decodes starts at `index`.
    fn starts_at(&mut self, index: usize) -> bool {
        self.text.get(index..index + 2) == Some(b"=?") && self.at(index).is_some()
    }
}

/// An unstructured header value (folded as written), decoded: the text of
/// each line keeps its inner blanks and loses those at its ends, lines are
/// joined by one space, an encoded word is set off from the text around it
/// by one space, and adjacent encoded words are joined without one.
pub(super) fn unstructured(value: &[u8]) -> String {
    let mut words = EncodedWords::new(value);
    let mut out = String::with_capacity(value.len());
    // Whether the last piece written was an encoded word (true at the
    // start, so the first piece gets no space before it).
    let mut after_encoded = true;
    let mut index = 0;
    while let Some(start) = value
        .get(index..)
        .and_then(|rest| rest.iter().position(|&byte| !is_blank(byte)))
    {
        index += start;
        if let Some((decoded, length)) = words.at(index) {
            if !after_encoded {
                out.push(' ');
            }
            out.push_str(&decoded);
            after_encoded = true;
            index += length;
            continue;
        }
        // Plain text, up to the end of the line or an encoded word.
        let mut end = index + 1;
        while let Some(&byte) = value.get(end) {
            if byte == b'\n' || (byte == b'=' && words.starts_at(end)) {
                break;
            }
            end += 1;
        }
        let text = trim_end_blanks(value.get(index..end).unwrap_or_default());
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&String::from_utf8_lossy(text));
        after_encoded = false;
        index = end;
    }
    out
}

/// Text with encoded words anywhere in it (a quoted string, a comment,
/// a parameter value): each decoded, the blanks between two of them
/// dropped, everything else kept.
pub(super) fn decode_inline(raw: &[u8]) -> String {
    if !raw.windows(2).any(|pair| pair == b"=?") {
        return String::from_utf8_lossy(raw).into_owned();
    }
    let mut words = EncodedWords::new(raw);
    let mut out = String::with_capacity(raw.len());
    // The plain bytes not written yet.
    let mut plain = 0;
    // Where the blanks after the last encoded word started: blanks
    // between two encoded words are dropped.
    let mut blanks_after_word: Option<usize> = None;
    let mut index = 0;
    while let Some(&byte) = raw.get(index) {
        if byte == b'='
            && let Some((decoded, length)) = words.at(index)
        {
            out.push_str(&String::from_utf8_lossy(
                raw.get(plain..index).unwrap_or_default(),
            ));
            if let Some(start) = blanks_after_word.take() {
                out.truncate(start);
            }
            out.push_str(&decoded);
            index += length;
            plain = index;
            blanks_after_word = Some(out.len());
            continue;
        }
        if !(byte == b' ' || byte == b'\t') {
            blanks_after_word = None;
        }
        index += 1;
    }
    out.push_str(&String::from_utf8_lossy(
        raw.get(plain..).unwrap_or_default(),
    ));
    out
}

fn is_blank(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// The bytes without the blanks they end with.
fn trim_end_blanks(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|&byte| !is_blank(byte))
        .map_or(0, |last| last + 1);
    bytes.get(..end).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str) -> Option<(String, usize)> {
        EncodedWords::new(text.as_bytes()).at(0)
    }

    #[test]
    fn encoded_words_decode_in_their_charset() {
        for (input, text) in [
            (
                "=?iso-8859-1?q?this=20is=20some=20text?=",
                "this is some text",
            ),
            ("=?iso-8859-1?q?this is some text?=", "this is some text"),
            ("=?US-ASCII?Q?Keith_Moore?=", "Keith Moore"),
            (
                "=?iso_8859-1:1987?Q?Keld_J=F8rn_Simonsen?=",
                "Keld Jørn Simonsen",
            ),
            (
                "=?ISO-8859-1?B?SWYgeW91IGNhbiByZWFkIHRoaXMgeW8=?=",
                "If you can read this yo",
            ),
            ("=?iso-8859-1?Q?=805.4bn?=", "€5.4bn"),
            ("=?ISO-8859-1*?Q?a?=", "a"),
            ("=?utf-8*en?Q?a_b?=", "a b"),
            ("=?UTF-8?B?SsO8cmdlbiBNw7xsbGVy?=", "Jürgen Müller"),
        ] {
            assert_eq!(
                word(input).map(|(text, _)| text).as_deref(),
                Some(text),
                "{input}"
            );
        }
        for input in [
            "=?utf-8?x?abc?=",
            "=??q?abc?=",
            "=?a?q?abc?=",
            "=?utf-8?q?abc",
            "=?utf-8?q?a=zz?=",
            "=?utf-8?é?abc?=",
            "=?utf-8?q",
        ] {
            assert_eq!(word(input), None, "{input}");
        }
        assert_eq!(word("=?utf-8?q?a?= rest").map(|(_, n)| n), Some(13));
        // 8-bit bytes in an encoded word are read in its charset.
        assert_eq!(
            EncodedWords::new(b"=?iso-8859-1?Q?J\xfcrgen?=")
                .at(0)
                .map(|(text, _)| text)
                .as_deref(),
            Some("Jürgen")
        );
    }

    #[test]
    fn unstructured_text_joins_words_and_encoded_words() {
        for (input, text) in [
            (" Re: [#42] Printer on fire", "Re: [#42] Printer on fire"),
            ("Re:  spaced \r\n\t out ", "Re:  spaced out"),
            ("Tab\tinside", "Tab\tinside"),
            ("folded\r\n    twice\n\tover", "folded twice over"),
            (
                "=?ISO-8859-1?Q?Gr=FC=DFe_aus_M=FCnchen?=",
                "Grüße aus München",
            ),
            ("=?utf-8?q?a?= =?utf-8?q?b?=", "ab"),
            ("=?utf-8?q?a?=\r\n =?utf-8?q?b?=", "ab"),
            ("Hello =?utf-8?q?w=C3=B6rld?= again", "Hello wörld again"),
            ("x=?utf-8?q?y?=", "x y"),
            ("a =?bad?= b", "a =?bad?= b"),
            ("Why not both? =?utf-8?b?4pi6?=", "Why not both? ☺"),
            ("ünï =?utf-8?q?x?=", "ünï x"),
            ("", ""),
            ("   ", ""),
        ] {
            assert_eq!(unstructured(input.as_bytes()), text, "{input:?}");
        }
        assert_eq!(
            unstructured(b"caf\xe9 =?iso-8859-1?q?caf=E9?="),
            "caf\u{fffd} café"
        );
    }

    #[test]
    fn inline_words_are_decoded_where_they_are() {
        assert_eq!(decode_inline(b"plain"), "plain");
        assert_eq!(decode_inline(b"=?utf-8?q?a?= =?utf-8?q?b?=.pdf"), "ab.pdf");
        assert_eq!(decode_inline(b"x =?utf-8?q?=C3=A9?= y"), "x é y");
        assert_eq!(decode_inline("ü =?utf-8?q?x?=ü".as_bytes()), "ü xü");
    }

    #[test]
    fn a_value_full_of_unended_words_reads_in_linear_time() {
        let value = "=?utf-8?q?a ".repeat(200_000);
        let started = std::time::Instant::now();
        assert_eq!(unstructured(value.as_bytes()).len(), value.len() - 1);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));

        // One long word with encoded words inside it, and one without.
        let value = "x=?utf-8?q?y?=".repeat(100_000);
        let started = std::time::Instant::now();
        assert_eq!(
            unstructured(value.as_bytes()),
            "x y ".repeat(100_000).trim_end()
        );
        let value = "x".repeat(2_000_000);
        assert_eq!(unstructured(value.as_bytes()), value);
        assert_eq!(
            decode_inline("=?a?q?".repeat(200_000).as_bytes()).len(),
            1_200_000
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }
}
