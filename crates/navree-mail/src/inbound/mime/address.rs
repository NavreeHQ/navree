//! Address lists (RFC 5322 section 3.4): `From`, `To`, `Cc`, `Reply-To`.
//! Groups are flattened, display names decoded (encoded words, quoted
//! strings), and a comment stands in for a missing display name
//! (`ada@example.com (Ada Lovelace)`).
//!
//! The list is read token by token and each mailbox built as it goes, so
//! what is held is the mailboxes themselves, at most `max` of them.

use super::words::{self, EncodedWords};

/// A mailbox: the display name when there is one, and the address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Mailbox {
    pub name: Option<String>,
    pub email: String,
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    /// An atom (`Ada`, `ada@example.com`).
    Atom(String),
    /// An encoded word, decoded.
    Encoded(String),
    /// A quoted string's text.
    Quoted(String),
    /// A comment's text, blanks collapsed.
    Comment(String),
    /// What `<...>` holds, without blanks and comments.
    Angle(String),
    Comma,
    Colon,
    Semicolon,
}

/// The mailboxes of an address list (unfolded), in order. Entries without
/// an address are left out. `None` when the list has more than `max`.
pub(super) fn list(value: &[u8], max: usize) -> Option<Vec<Mailbox>> {
    let mut out = Vec::new();
    let mut current = Building::default();
    for (token, space) in Tokens::new(value) {
        match token {
            Token::Comma | Token::Semicolon => {
                current.finish(&mut out);
                if out.len() > max {
                    return None;
                }
            }
            // `group-name:` starts a group; the name is not a mailbox.
            Token::Colon if current.angle.is_none() => current = Building::default(),
            Token::Colon => {}
            Token::Angle(address) => current.angle = Some(address),
            Token::Comment(text) => {
                if !text.is_empty() {
                    if !current.comment.is_empty() {
                        current.comment.push(' ');
                    }
                    current.comment.push_str(&text);
                }
            }
            // An address without angle brackets; before `<...>` it was part
            // of the display name after all.
            Token::Atom(atom) if atom.contains('@') && current.angle.is_none() => {
                current.address.push_str(&atom);
                current.with_addresses.push(&atom, space, false);
            }
            Token::Atom(text) | Token::Quoted(text) => {
                current.with_addresses.push(&text, space, false);
                current.name.push(&text, space, false);
            }
            Token::Encoded(text) => {
                current.with_addresses.push(&text, space, true);
                current.name.push(&text, space, true);
            }
        }
    }
    current.finish(&mut out);
    (out.len() <= max).then_some(out)
}

/// Whether `value` is one mailbox that every reader takes for the same
/// address: at most one `<...>`, and an address that is only an address, not
/// spelled by an encoded word.
pub(super) fn is_one_address(value: &[u8]) -> bool {
    let mut angles = 0;
    let mut encoded = false;
    for (token, _) in Tokens::new(value) {
        match token {
            Token::Angle(_) => angles += 1,
            Token::Encoded(_) => encoded = true,
            _ => {}
        }
    }
    if angles > 1 || (angles == 0 && encoded) {
        return false;
    }
    match list(value, 2).as_deref() {
        Some([mailbox]) => is_plain_address(&mailbox.email),
        _ => false,
    }
}

/// One `@` with something on each side, and nothing a reader splits a list or
/// an address at.
fn is_plain_address(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && !email
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || ",;:<>()\"[]\\".contains(c))
}

/// A display name being put together: words joined by a space where the
/// header has blanks, adjacent encoded words without one.
#[derive(Default)]
struct Phrase {
    text: String,
    last_encoded: bool,
}

impl Phrase {
    fn push(&mut self, word: &str, space: bool, encoded: bool) {
        if space && !self.text.is_empty() && !(encoded && self.last_encoded) {
            self.text.push(' ');
        }
        self.text.push_str(word);
        self.last_encoded = encoded;
    }
}

/// The parts of the mailbox being read.
#[derive(Default)]
struct Building {
    /// The display name without bare addresses.
    name: Phrase,
    /// The display name with them (what it is when `<...>` follows).
    with_addresses: Phrase,
    comment: String,
    angle: Option<String>,
    address: String,
}

impl Building {
    fn finish(&mut self, out: &mut Vec<Mailbox>) {
        let building = std::mem::take(self);
        let (mut email, name) = match building.angle {
            Some(angle) => (angle, building.with_addresses.text),
            None => (building.address, building.name.text),
        };
        let name = name.trim().to_owned();
        let comment = building.comment;
        let name = if email.trim().is_empty() {
            // `postmaster (The Postmaster)`: a single word with a comment
            // is a local address.
            if comment.is_empty() || name.is_empty() || name.contains(char::is_whitespace) {
                return;
            }
            email = name;
            comment
        } else {
            match (name.is_empty(), comment.is_empty()) {
                (false, false) => format!("{name} ({comment})"),
                (false, true) => name,
                (true, _) => comment,
            }
        };
        let email = email.trim();
        if email.is_empty() {
            return;
        }
        out.push(Mailbox {
            name: Some(name).filter(|name| !name.is_empty()),
            email: email.to_owned(),
        });
    }
}

fn is_blank(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// The tokens of an address list, each with whether blanks came before it.
struct Tokens<'a> {
    value: &'a [u8],
    words: EncodedWords<'a>,
    index: usize,
}

impl<'a> Tokens<'a> {
    fn new(value: &'a [u8]) -> Self {
        Self {
            value,
            words: EncodedWords::new(value),
            index: 0,
        }
    }
}

impl Iterator for Tokens<'_> {
    type Item = (Token, bool);

    fn next(&mut self) -> Option<(Token, bool)> {
        let value = self.value;
        let mut space = false;
        while let Some(&byte) = value.get(self.index) {
            let token = match byte {
                byte if is_blank(byte) => {
                    space = true;
                    self.index += 1;
                    continue;
                }
                b'>' | b')' => {
                    self.index += 1;
                    continue;
                }
                b',' => {
                    self.index += 1;
                    Token::Comma
                }
                b':' => {
                    self.index += 1;
                    Token::Colon
                }
                b';' => {
                    self.index += 1;
                    Token::Semicolon
                }
                b'"' => {
                    let (text, end) = quoted(value, self.index + 1);
                    self.index = end;
                    Token::Quoted(words::decode_inline(&text))
                }
                b'(' => {
                    let (text, end) = comment(value, self.index + 1);
                    self.index = end;
                    Token::Comment(collapse(&words::decode_inline(&text)))
                }
                b'<' => {
                    let (text, end) = angle(value, self.index + 1);
                    self.index = end;
                    Token::Angle(String::from_utf8_lossy(&text).into_owned())
                }
                _ => match self.words.at(self.index) {
                    Some((decoded, length)) => {
                        self.index += length;
                        Token::Encoded(decoded)
                    }
                    None => {
                        let rest = value.get(self.index..).unwrap_or_default();
                        let end = rest
                            .iter()
                            .position(|&byte| is_blank(byte) || b"\",:;()<>".contains(&byte))
                            .unwrap_or(rest.len())
                            .max(1);
                        self.index += end;
                        Token::Atom(
                            String::from_utf8_lossy(rest.get(..end).unwrap_or_default())
                                .into_owned(),
                        )
                    }
                },
            };
            return Some((token, space));
        }
        None
    }
}

/// A quoted string's bytes from `start` (after its `"`), without escapes
/// and line breaks, and where it ends. An unclosed string runs to the end.
fn quoted(value: &[u8], start: usize) -> (Vec<u8>, usize) {
    let rest = value.get(start..).unwrap_or_default();
    let mut text = Vec::new();
    let mut escaped = false;
    for (offset, &byte) in rest.iter().enumerate() {
        if escaped {
            text.push(byte);
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            return (text, start + offset + 1);
        } else if byte != b'\r' && byte != b'\n' {
            text.push(byte);
        }
    }
    (text, value.len())
}

/// A comment's bytes from `start` (after its `(`), nested parentheses
/// kept, and where it ends.
fn comment(value: &[u8], start: usize) -> (Vec<u8>, usize) {
    let rest = value.get(start..).unwrap_or_default();
    let mut text = Vec::new();
    let mut depth = 1usize;
    let mut escaped = false;
    for (offset, &byte) in rest.iter().enumerate() {
        if escaped {
            text.push(byte);
            escaped = false;
            continue;
        }
        match byte {
            b'\\' => escaped = true,
            b'(' => {
                depth += 1;
                text.push(byte);
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return (text, start + offset + 1);
                }
                text.push(byte);
            }
            _ => text.push(byte),
        }
    }
    (text, value.len())
}

/// What `<...>` holds from `start` (after its `<`), without blanks and
/// comments, and where it ends.
fn angle(value: &[u8], start: usize) -> (Vec<u8>, usize) {
    let rest = value.get(start..).unwrap_or_default();
    let mut text = Vec::new();
    let mut depth = 0usize;
    for (offset, &byte) in rest.iter().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            b'>' => return (text, start + offset + 1),
            byte if is_blank(byte) => {}
            byte => text.push(byte),
        }
    }
    (text, value.len())
}

/// Blanks collapsed to one space, and trimmed.
fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text
        .split([' ', '\t', '\r', '\n'])
        .filter(|word| !word.is_empty())
    {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mailbox(name: Option<&str>, email: &str) -> Mailbox {
        Mailbox {
            name: name.map(str::to_owned),
            email: email.to_owned(),
        }
    }

    fn all(value: &str) -> Vec<Mailbox> {
        list(value.as_bytes(), usize::MAX).unwrap()
    }

    #[test]
    fn mailboxes_with_and_without_names() {
        for (value, expected) in [
            ("ada@example.com", vec![mailbox(None, "ada@example.com")]),
            (
                " Ada Lovelace <ada@example.com>",
                vec![mailbox(Some("Ada Lovelace"), "ada@example.com")],
            ),
            (
                "\"Lovelace, Ada\" <ada@example.com>",
                vec![mailbox(Some("Lovelace, Ada"), "ada@example.com")],
            ),
            ("<ada@example.com>", vec![mailbox(None, "ada@example.com")]),
            (
                "ada@example.com (Ada Lovelace)",
                vec![mailbox(Some("Ada Lovelace"), "ada@example.com")],
            ),
            (
                "Ada (Work) <ada@example.com>",
                vec![mailbox(Some("Ada (Work)"), "ada@example.com")],
            ),
            (
                "\"Ada \\\"The Countess\\\" L\" <ada@example.com>",
                vec![mailbox(Some("Ada \"The Countess\" L"), "ada@example.com")],
            ),
            (
                "Ada\r\n Lovelace <ada@example.com>",
                vec![mailbox(Some("Ada Lovelace"), "ada@example.com")],
            ),
            (
                "< ada @ example.com >",
                vec![mailbox(None, "ada@example.com")],
            ),
            (
                "ada@example.com <ada@example.com>",
                vec![mailbox(Some("ada@example.com"), "ada@example.com")],
            ),
            (
                "Ada Lovelace ada@example.com",
                vec![mailbox(Some("Ada Lovelace"), "ada@example.com")],
            ),
            (
                "postmaster (The Postmaster)",
                vec![mailbox(Some("The Postmaster"), "postmaster")],
            ),
            ("undisclosed", vec![]),
            ("<>", vec![]),
            ("", vec![]),
        ] {
            assert_eq!(all(value), expected, "{value:?}");
        }
    }

    #[test]
    fn lists_and_groups_are_flattened() {
        assert_eq!(
            all("support@acme.test, \"Billing\" <billing@acme.test>,, c@d.test"),
            vec![
                mailbox(None, "support@acme.test"),
                mailbox(Some("Billing"), "billing@acme.test"),
                mailbox(None, "c@d.test")
            ]
        );
        assert_eq!(
            all("Team: a@x.test, Bee <b@y.test>; c@z.test"),
            vec![
                mailbox(None, "a@x.test"),
                mailbox(Some("Bee"), "b@y.test"),
                mailbox(None, "c@z.test")
            ]
        );
        assert_eq!(all("undisclosed-recipients:;"), vec![]);
    }

    #[test]
    fn encoded_words_are_decoded_in_names_and_quotes() {
        for (value, name) in [
            (
                "=?UTF-8?B?SsO8cmdlbiBNw7xsbGVy?= <j@example.de>",
                "Jürgen Müller",
            ),
            (
                "=?utf-8?q?J=C3=BCrgen?= =?utf-8?q?_M=C3=BCller?= <j@example.de>",
                "Jürgen Müller",
            ),
            (
                "\"=?utf-8?q?J=C3=BCrgen_M=C3=BCller?=\" <j@example.de>",
                "Jürgen Müller",
            ),
            ("Dr. =?utf-8?q?J=C3=BCrgen?= <j@example.de>", "Dr. Jürgen"),
            ("j@example.de (=?utf-8?q?J=C3=BCrgen?=)", "Jürgen"),
        ] {
            assert_eq!(
                all(value),
                vec![mailbox(Some(name), "j@example.de")],
                "{value:?}"
            );
        }
    }

    #[test]
    fn eight_bit_bytes_are_read_in_the_charset_of_their_encoded_word() {
        assert_eq!(
            list(
                b"=?iso-8859-1?Q?J\xfcrgen?= <j@example.de>, M\xfcller <m@x.de>",
                2
            ),
            Some(vec![
                mailbox(Some("Jürgen"), "j@example.de"),
                mailbox(Some("M\u{fffd}ller"), "m@x.de")
            ])
        );
    }

    #[test]
    fn a_list_longer_than_the_limit_is_refused_as_it_is_read() {
        let value = "a@b.test, ".repeat(10);
        assert_eq!(list(value.as_bytes(), 10).map(|list| list.len()), Some(10));
        assert_eq!(list(value.as_bytes(), 9), None);
        // Separators alone make no mailboxes.
        assert_eq!(list(",".repeat(100_000).as_bytes(), 1), Some(vec![]));
    }
}
