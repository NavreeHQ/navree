//! A `multipart/form-data` body as it arrives (RFC 2046, section 5.1.1, and
//! RFC 7578): what comes before the first delimiter, then each part's
//! delimiter line, head and value, then the closing delimiter.
//!
//! [`Parser`] is handed the body's chunks and answers with what they hold.
//! It keeps only what it has not understood yet: the end of a preamble that
//! may be the start of a delimiter, a head up to the head limit, and the end
//! of a value that may be the start of the next delimiter. A value goes to
//! the handler as it arrives, and a search for a head's end starts where the
//! last one stopped. What it finds, and when a limit applies, depends on the
//! bytes and never on how they were split.

use bytes::{Buf, Bytes, BytesMut};
use http::HeaderMap;
use http::header::{CONTENT_DISPOSITION, CONTENT_TYPE, HeaderName, HeaderValue};
use memchr::memmem::Finder;

use super::disposition::{self, Disposition, Refused};

/// The most headers a part may have.
const MAX_HEADERS: usize = 32;

/// The blank line that ends a head.
const HEAD_END: &[u8] = b"\r\n\r\n";

/// The length of a line end.
const LINE_END: usize = 2;

/// Where the parser is in the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Before the first delimiter, which starts the body or a line.
    Preamble,
    /// Just past a delimiter: `--` closes the form, anything else is padding
    /// and a line end.
    Delimited,
    /// In the spaces and tabs that may follow a delimiter, then its line end.
    Padding,
    /// In a part's headers, up to and including the blank line. A blank line
    /// at the very start does not end them: the search starts there.
    Head,
    /// In a part's value, which ends at a line end and a delimiter.
    Value,
    /// Past the closing delimiter: what follows is not read.
    Closed,
}

/// A part's head.
#[derive(Debug)]
pub(super) struct Head {
    /// Every header; of one named twice, the last (`Content-Disposition` and
    /// `Content-Type` may not be).
    pub(super) headers: HeaderMap,
    pub(super) name: Option<String>,
    pub(super) file_name: Option<String>,
    /// Its `Content-Type`, when it is a media type.
    pub(super) content_type: Option<mime::Mime>,
}

/// What comes after a part, or before the first.
// Made once per part and moved once, into the part's `Field`.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub(super) enum Next {
    Part(Head),
    /// The closing delimiter.
    Closed,
}

/// A piece of a part's value.
#[derive(Debug)]
pub(super) enum Piece {
    Bytes(Bytes),
    /// The value has ended.
    End,
}

/// The message of a form that stops, or goes wrong, outside a value.
const INCOMPLETE: &str = "incomplete multipart stream";

/// The message of a head that is not a list of header lines.
const BAD_HEAD: &str = "failed to read headers";

/// Why the body is not a form the parser reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Failure {
    /// A preamble, padding or head past the head limit.
    HeadTooLarge,
    /// Not a well-formed form, or one that stops before its end.
    Malformed(String),
}

impl Failure {
    fn malformed(message: &str) -> Self {
        Failure::Malformed(message.to_owned())
    }
}

/// Reads a form's delimiters, heads and values out of its chunks.
#[derive(Debug)]
pub(super) struct Parser {
    stage: Stage,
    // What has arrived and has not been read yet.
    buffer: BytesMut,
    // The bytes of this stage that are no longer in the buffer: a
    // preamble's, and padding.
    seen: usize,
    // Where the search for the end of a head goes on from.
    searched: usize,
    // The body has ended: nothing more will arrive.
    ended: bool,
    // The name of the part whose value is being read.
    field: Option<String>,
    // `--boundary`.
    dash_boundary: Vec<u8>,
    // A line end then `--boundary`: what ends a value.
    delimiter: Finder<'static>,
    head_end: Finder<'static>,
}

impl Parser {
    pub(super) fn new(boundary: &str) -> Self {
        Self {
            stage: Stage::Preamble,
            buffer: BytesMut::new(),
            seen: 0,
            searched: 0,
            ended: false,
            field: None,
            dash_boundary: format!("--{boundary}").into_bytes(),
            delimiter: Finder::new(format!("\r\n--{boundary}").as_bytes()).into_owned(),
            head_end: Finder::new(HEAD_END).into_owned(),
        }
    }

    /// The next chunk of the body.
    pub(super) fn push(&mut self, chunk: &[u8]) {
        if self.stage != Stage::Closed {
            self.buffer.extend_from_slice(chunk);
        }
    }

    /// The body has ended.
    pub(super) fn end(&mut self) {
        self.ended = true;
    }

    /// The next piece of the current part's value, or [`Piece::End`] when
    /// there is no current part. `None` when it takes more of the body.
    pub(super) fn piece(&mut self) -> Result<Option<Piece>, Failure> {
        let piece = self.read_piece();
        self.settle(piece)
    }

    /// The next part's head, past what is left of the current part's value,
    /// or the close of the form. `None` when it takes more of the body.
    /// `limit` bounds a head, what comes before the first delimiter, and the
    /// padding after a delimiter.
    pub(super) fn next(&mut self, limit: usize) -> Result<Option<Next>, Failure> {
        let next = self.read_next(limit);
        self.settle(next)
    }

    /// Once the body has ended, wanting more of it is a form cut short.
    fn settle<T>(&self, read: Result<Option<T>, Failure>) -> Result<Option<T>, Failure> {
        match read {
            Ok(None) if self.ended => Err(match self.stage {
                Stage::Value => Failure::Malformed(format!(
                    "field {:?} received with incomplete data",
                    self.field.as_deref().unwrap_or("<unknown>")
                )),
                _ => Failure::malformed(INCOMPLETE),
            }),
            read => read,
        }
    }

    fn read_piece(&mut self) -> Result<Option<Piece>, Failure> {
        if self.stage != Stage::Value {
            return Ok(Some(Piece::End));
        }
        let length = self.delimiter.needle().len();
        match self.delimiter.find(&self.buffer) {
            Some(0) => {
                self.buffer.advance(length);
                self.enter(Stage::Delimited);
                Ok(Some(Piece::End))
            }
            Some(at) => Ok(Some(Piece::Bytes(self.buffer.split_to(at).freeze()))),
            None => {
                let held = held_back(&self.buffer, self.delimiter.needle());
                let ready = self.buffer.len() - held;
                if ready == 0 {
                    return Ok(None);
                }
                Ok(Some(Piece::Bytes(self.buffer.split_to(ready).freeze())))
            }
        }
    }

    fn read_next(&mut self, limit: usize) -> Result<Option<Next>, Failure> {
        loop {
            match self.stage {
                // What the handler left of a value is read and dropped.
                Stage::Value => match self.read_piece()? {
                    Some(_) => {}
                    None => return Ok(None),
                },
                Stage::Preamble => {
                    // The first delimiter starts the body or a line. Nothing
                    // has left the buffer while `seen` is 0.
                    let dash_boundary = self.dash_boundary.as_slice();
                    if self.seen == 0 {
                        if self.buffer.starts_with(dash_boundary) {
                            let length = dash_boundary.len();
                            self.buffer.advance(length);
                            self.enter(Stage::Delimited);
                            continue;
                        }
                        if dash_boundary.starts_with(&self.buffer) {
                            return Ok(None);
                        }
                    }
                    let length = self.delimiter.needle().len();
                    match self.delimiter.find(&self.buffer) {
                        Some(at) => {
                            // The line end is the preamble's last.
                            if self.seen.saturating_add(at + LINE_END) > limit {
                                return Err(Failure::HeadTooLarge);
                            }
                            self.buffer.advance(at + length);
                            self.enter(Stage::Delimited);
                        }
                        None => {
                            // Only the end may be the start of a delimiter,
                            // and only what can't be one with its line end
                            // counts yet.
                            let held = held_back(&self.buffer, self.delimiter.needle());
                            let gone = self.buffer.len() - held;
                            self.buffer.advance(gone);
                            self.seen = self.seen.saturating_add(gone);
                            let unsure = self.dash_boundary.len() - 1;
                            if self.seen.saturating_add(held).saturating_sub(unsure) > limit {
                                return Err(Failure::HeadTooLarge);
                            }
                            return Ok(None);
                        }
                    }
                }
                Stage::Delimited => {
                    let Some(next) = self.buffer.get(..2) else {
                        return Ok(None);
                    };
                    if next == b"--" {
                        self.stage = Stage::Closed;
                        self.buffer = BytesMut::new();
                    } else {
                        self.enter(Stage::Padding);
                    }
                }
                Stage::Padding => {
                    let padding = self
                        .buffer
                        .iter()
                        .position(|byte| !matches!(byte, b' ' | b'\t'))
                        .unwrap_or(self.buffer.len());
                    self.buffer.advance(padding);
                    self.seen = self.seen.saturating_add(padding);
                    if self.seen > limit {
                        return Err(Failure::HeadTooLarge);
                    }
                    let Some(line_end) = self.buffer.get(..2) else {
                        return Ok(None);
                    };
                    if line_end != b"\r\n" {
                        return Err(Failure::malformed(INCOMPLETE));
                    }
                    self.buffer.advance(2);
                    self.enter(Stage::Head);
                }
                Stage::Head => {
                    let from = self.searched;
                    let unsearched = self.buffer.get(from..).unwrap_or_default();
                    match self.head_end.find(unsearched) {
                        Some(at) => {
                            let length = from + at + HEAD_END.len();
                            if length > limit {
                                return Err(Failure::HeadTooLarge);
                            }
                            let head = read_head(self.buffer.get(..length).unwrap_or_default())?;
                            self.buffer.advance(length);
                            self.enter(Stage::Value);
                            self.field.clone_from(&head.name);
                            return Ok(Some(Next::Part(head)));
                        }
                        None => {
                            // Its blank line is still to come.
                            if self.buffer.len() >= limit {
                                return Err(Failure::HeadTooLarge);
                            }
                            self.searched = self.buffer.len().saturating_sub(HEAD_END.len() - 1);
                            return Ok(None);
                        }
                    }
                }
                Stage::Closed => return Ok(Some(Next::Closed)),
            }
        }
    }

    fn enter(&mut self, stage: Stage) {
        self.stage = stage;
        self.seen = 0;
        self.searched = 0;
    }

    /// The bytes held, waiting to be understood.
    #[cfg(test)]
    fn held(&self) -> usize {
        self.buffer.len()
    }
}

/// How many bytes at the end of `buffer` may be the start of `pattern`: the
/// longest end of it that `pattern` starts with, shorter than `pattern`.
fn held_back(buffer: &[u8], pattern: &[u8]) -> usize {
    let longest = pattern.len().saturating_sub(1).min(buffer.len());
    let from = buffer.len() - longest;
    (from..buffer.len())
        .find(|&start| {
            buffer
                .get(start..)
                .is_some_and(|end| pattern.starts_with(end))
        })
        .map_or(0, |start| buffer.len() - start)
}

/// A part's head, its blank line included.
fn read_head(bytes: &[u8]) -> Result<Head, Failure> {
    // Two readers may end a line in different places.
    let bare_line_feed = bytes
        .iter()
        .enumerate()
        .any(|(at, byte)| *byte == b'\n' && (at == 0 || bytes.get(at - 1) != Some(&b'\r')));
    if bare_line_feed {
        return Err(Failure::malformed(BAD_HEAD));
    }
    let mut slots = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let raw = match httparse::parse_headers(bytes, &mut slots) {
        // What httparse leaves would be lost, unless it is only the second
        // blank line of a part with no headers.
        Ok(httparse::Status::Complete((read, raw))) if read == bytes.len() || bytes == HEAD_END => {
            raw
        }
        Ok(httparse::Status::Complete(_)) => return Err(Failure::malformed(BAD_HEAD)),
        Ok(httparse::Status::Partial) => {
            return Err(Failure::malformed("failed to read field complete headers"));
        }
        // Not header lines, or more of them than `MAX_HEADERS`.
        Err(_) => return Err(Failure::malformed(BAD_HEAD)),
    };
    let mut headers = HeaderMap::with_capacity(raw.len());
    for header in raw {
        let Ok(name) = HeaderName::from_bytes(header.name.as_bytes()) else {
            return Err(Failure::Malformed(format!(
                "failed to decode field's raw header name: {:?}",
                header.name
            )));
        };
        let Ok(value) = HeaderValue::from_bytes(header.value) else {
            return Err(Failure::malformed(
                "failed to decode field's raw header value",
            ));
        };
        // Two readers may each take a different one.
        if name == CONTENT_DISPOSITION && headers.contains_key(&name) {
            return Err(Failure::malformed(
                "A form part may have one Content-Disposition header",
            ));
        }
        if name == CONTENT_TYPE && headers.contains_key(&name) {
            return Err(Failure::malformed(
                "A form part may have one Content-Type header",
            ));
        }
        headers.insert(name, value);
    }
    let Disposition { name, file_name } = match headers.get(CONTENT_DISPOSITION) {
        Some(value) => disposition::parse(value.as_bytes()).map_err(|refused| {
            Failure::malformed(match refused {
                Refused::Repeated => {
                    "A form part may name each parameter of its Content-Disposition once"
                }
                Refused::Ambiguous => "A form part's Content-Disposition may be read one way only",
                Refused::NotUtf8 => "A form part's file name must be UTF-8",
                Refused::NotFormData => "A form part's Content-Disposition must be form-data",
            })
        })?,
        None => Disposition::default(),
    };
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<mime::Mime>().ok());
    Ok(Head {
        headers,
        name,
        file_name,
        content_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a reader of the form saw, in order.
    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Seen {
        Part(Option<String>, Option<String>),
        Value(Vec<u8>),
        Closed,
        Failed(Failure),
    }

    /// Reads `chunks` as a handler reading every value would.
    fn run(chunks: &[&[u8]], limit: usize) -> Vec<Seen> {
        let mut parser = Parser::new("b");
        let mut chunks = chunks.iter();
        let mut seen = Vec::new();
        // The value being read, once a head has been.
        let mut value: Option<Vec<u8>> = None;
        loop {
            let read = match value.take() {
                Some(mut bytes) => parser.piece().map(|piece| match piece {
                    Some(Piece::Bytes(piece)) => {
                        bytes.extend_from_slice(&piece);
                        value = Some(bytes);
                        true
                    }
                    Some(Piece::End) => {
                        seen.push(Seen::Value(bytes));
                        true
                    }
                    None => {
                        value = Some(bytes);
                        false
                    }
                }),
                None => parser.next(limit).map(|next| match next {
                    Some(Next::Part(head)) => {
                        seen.push(Seen::Part(head.name, head.file_name));
                        value = Some(Vec::new());
                        true
                    }
                    Some(Next::Closed) => {
                        seen.push(Seen::Closed);
                        false
                    }
                    None => false,
                }),
            };
            if seen.last() == Some(&Seen::Closed) {
                return seen;
            }
            match read {
                Ok(true) => {}
                Ok(false) => match chunks.next() {
                    Some(chunk) => parser.push(chunk),
                    None => parser.end(),
                },
                Err(failure) => {
                    seen.push(Seen::Failed(failure));
                    return seen;
                }
            }
        }
    }

    /// What `body` reads as, the same whole, cut in two anywhere, or a byte
    /// at a time.
    #[track_caller]
    fn read(body: &[u8], limit: usize) -> Vec<Seen> {
        let whole = run(&[body], limit);
        for cut in 0..=body.len() {
            let (first, second) = body.split_at(cut);
            assert_eq!(run(&[first, second], limit), whole, "cut at {cut}");
        }
        let bytes: Vec<&[u8]> = body.chunks(1).collect();
        assert_eq!(run(&bytes, limit), whole, "a byte at a time");
        whole
    }

    fn part(name: &str) -> Seen {
        Seen::Part(Some(name.to_owned()), None)
    }

    fn value(bytes: &[u8]) -> Seen {
        Seen::Value(bytes.to_vec())
    }

    fn malformed(body: &[u8]) -> bool {
        matches!(
            read(body, usize::MAX).last(),
            Some(Seen::Failed(Failure::Malformed(_)))
        )
    }

    const A: &str = "Content-Disposition: form-data; name=\"a\"";

    #[test]
    fn a_form_reads_however_it_is_cut() {
        let body = format!(
            "--b\r\n{A}\r\n\r\n1\r\n--b\r\n{A}; filename=\"f\"\r\n\r\n\r\n--x\r\n\r\n--b--\r\n"
        );
        assert_eq!(
            read(body.as_bytes(), usize::MAX),
            [
                part("a"),
                value(b"1"),
                Seen::Part(Some("a".into()), Some("f".into())),
                value(b"\r\n--x\r\n"),
                Seen::Closed
            ]
        );
    }

    #[test]
    fn a_preamble_and_an_epilogue_are_skipped() {
        let body = format!("preamble --x\r\n--b\r\n{A}\r\n\r\n1\r\n--b--epilogue, not read");
        assert_eq!(
            read(body.as_bytes(), usize::MAX),
            [part("a"), value(b"1"), Seen::Closed]
        );
        // A delimiter in the middle of a line is not one: the part is preamble.
        let body = format!("x--b\r\n{A}\r\n\r\n1\r\n--b--");
        assert_eq!(read(body.as_bytes(), usize::MAX), [Seen::Closed]);
        assert!(malformed(b"x--b--"));
        // The body's start, or a line's, is where it may be.
        let body = format!("x\r\n--b\r\n{A}\r\n\r\n1\r\n--b--");
        assert_eq!(read(body.as_bytes(), usize::MAX)[1], value(b"1"));
    }

    #[test]
    fn padding_may_follow_a_delimiter() {
        let body = format!("--b \t\r\n{A}\r\n\r\n1\r\n--b\t\r\n{A}\r\n\r\n2\r\n--b--");
        assert_eq!(
            read(body.as_bytes(), usize::MAX),
            [part("a"), value(b"1"), part("a"), value(b"2"), Seen::Closed]
        );
    }

    #[test]
    fn an_empty_form_and_empty_values_read() {
        assert_eq!(read(b"--b--", usize::MAX), [Seen::Closed]);
        let body = format!("--b\r\n{A}\r\n\r\n\r\n--b--");
        assert_eq!(
            read(body.as_bytes(), usize::MAX),
            [part("a"), value(b""), Seen::Closed]
        );
    }

    #[test]
    fn a_blank_line_at_the_start_of_a_head_does_not_end_it() {
        assert_eq!(
            read(b"--b\r\n\r\n\r\nv\r\n--b--", usize::MAX),
            [Seen::Part(None, None), value(b"v"), Seen::Closed]
        );
        assert!(malformed(b"--b\r\n\r\nv\r\n--b--"));
    }

    #[test]
    fn a_form_cut_short_anywhere_is_malformed() {
        let body = format!("--b\r\n{A}\r\n\r\n12345\r\n--b--");
        for end in 0..body.len() - 1 {
            assert!(malformed(&body.as_bytes()[..end]), "cut at {end}");
        }
        assert!(!malformed(body.as_bytes()));
    }

    #[test]
    fn what_is_not_a_delimiter_line_is_malformed() {
        for body in [
            "--bx",
            "--b-x",
            "--b\rx",
            "--b x",
            "--b\n",
            "--b\r\nA: 1\r\n\r\nv\r\n--bx",
            "--b\r\nA: 1\n\nv\n--b--",
        ] {
            assert!(malformed(body.as_bytes()), "{body:?}");
        }
    }

    #[test]
    fn malformed_heads_are_refused() {
        for head in [
            ": no name",
            "No colon",
            "Bad Name: 1",
            "Content-Disposition: form-data; name=\"a\"\r\nContent-Disposition: form-data; name=\"b\"",
            "Content-Type: text/plain\r\ncontent-type: text/html",
            "Content-Disposition: form-data; name=\"a\"; name=\"b\"",
        ] {
            let body = format!("--b\r\n{head}\r\n\r\nv\r\n--b--");
            assert!(malformed(body.as_bytes()), "{head:?}");
        }
        let many: Vec<String> = (0..33).map(|at| format!("X-{at}: 1")).collect();
        let body = format!("--b\r\n{}\r\n\r\nv\r\n--b--", many.join("\r\n"));
        assert_eq!(
            run(&[body.as_bytes()], usize::MAX).last(),
            Some(&Seen::Failed(Failure::malformed(BAD_HEAD)))
        );
        let enough: Vec<String> = (0..32).map(|at| format!("X-{at}: 1")).collect();
        let body = format!("--b\r\n{}\r\n\r\nv\r\n--b--", enough.join("\r\n"));
        assert!(!malformed(body.as_bytes()));
    }

    #[test]
    fn a_file_name_that_is_not_utf8_is_refused() {
        let body = b"--b\r\nContent-Disposition: form-data; name=\"a\"; filename=\"\xfe.txt\"\r\n\r\nv\r\n--b--";
        assert_eq!(
            run(&[&body[..]], usize::MAX).last(),
            Some(&Seen::Failed(Failure::malformed(
                "A form part's file name must be UTF-8"
            )))
        );
        let body = b"--b\r\nContent-Disposition: form-data; name=\"\xff\"\r\n\r\nv\r\n--b--";
        assert!(!malformed(body));
    }

    #[test]
    fn of_other_headers_named_twice_the_last_counts() {
        let mut parser = Parser::new("b");
        parser.push(b"--b\r\nX-A: 1\r\nX-A: 2\r\n\r\n");
        let Ok(Some(Next::Part(head))) = parser.next(usize::MAX) else {
            panic!("no head");
        };
        assert_eq!(head.headers["x-a"], "2");
        assert_eq!(head.headers.len(), 1);
    }

    #[test]
    fn a_content_type_is_read_as_a_media_type() {
        let mut parser = Parser::new("b");
        parser.push(b"--b\r\nContent-Type: text/plain; charset=latin1\r\n\r\n");
        let Ok(Some(Next::Part(head))) = parser.next(usize::MAX) else {
            panic!("no head");
        };
        let content_type = head.content_type.unwrap();
        assert_eq!(content_type.get_param(mime::CHARSET).unwrap(), "latin1");

        let mut parser = Parser::new("b");
        parser.push(b"--b\r\nContent-Type: not a type\r\n\r\n");
        let Ok(Some(Next::Part(head))) = parser.next(usize::MAX) else {
            panic!("no head");
        };
        assert!(head.content_type.is_none());
    }

    #[test]
    fn the_head_limit_is_exact() {
        // A head of 21 bytes: `X-Pad: 0123456789` and its blank line.
        let body = b"--b\r\nX-Pad: 0123456789\r\n\r\nv\r\n--b--";
        assert_eq!(read(body, 22).last(), Some(&Seen::Closed));
        assert_eq!(read(body, 21).last(), Some(&Seen::Closed));
        assert_eq!(
            read(body, 20).last(),
            Some(&Seen::Failed(Failure::HeadTooLarge))
        );
    }

    #[test]
    fn the_preamble_limit_is_exact() {
        // Ten bytes before the delimiter, the line end it starts with included.
        let body = b"01234567\r\n--b--";
        assert_eq!(read(body, 10), [Seen::Closed]);
        assert_eq!(read(body, 9), [Seen::Failed(Failure::HeadTooLarge)]);
        // Without a delimiter, what can no longer be the start of one counts:
        // all but the last two bytes.
        assert_eq!(
            read(b"0123456789ab", 9),
            [Seen::Failed(Failure::HeadTooLarge)]
        );
        assert!(matches!(
            read(b"0123456789ab", 10)[..],
            [Seen::Failed(Failure::Malformed(_))]
        ));
    }

    #[test]
    fn the_padding_limit_is_exact() {
        // Padding, then a head of 4 bytes: its blank line alone.
        let padded = |spaces: usize| format!("--b{}\r\n\r\n\r\nv\r\n--b--", " ".repeat(spaces));
        assert_eq!(read(padded(4).as_bytes(), 4).last(), Some(&Seen::Closed));
        assert_eq!(
            read(padded(5).as_bytes(), 4).last(),
            Some(&Seen::Failed(Failure::HeadTooLarge))
        );
    }

    #[test]
    fn a_value_holds_near_misses_of_the_delimiter() {
        let body = b"--b\r\nX: 1\r\n\r\n\r\n-\r\n--\r\n--c\r\r\n--b--";
        assert_eq!(
            read(body, usize::MAX),
            [
                Seen::Part(None, None),
                value(b"\r\n-\r\n--\r\n--c\r"),
                Seen::Closed
            ]
        );
    }

    #[test]
    fn a_value_goes_on_as_it_arrives_and_only_a_delimiter_start_is_held() {
        let mut parser = Parser::new("boundary");
        parser.push(b"--boundary\r\nX: 1\r\n\r\n");
        assert!(matches!(parser.next(64), Ok(Some(Next::Part(_)))));
        for _ in 0..1000 {
            parser.push(&[b'v'; 1024]);
            let Ok(Some(Piece::Bytes(piece))) = parser.piece() else {
                panic!("no piece");
            };
            assert_eq!(piece.len(), 1024);
            assert_eq!(parser.held(), 0);
        }
        parser.push(b"v\r\n--bound");
        assert!(matches!(parser.piece(), Ok(Some(Piece::Bytes(piece))) if &piece[..] == b"v"));
        assert_eq!(parser.held(), "\r\n--bound".len());
        assert!(matches!(parser.piece(), Ok(None)));
        parser.push(b"ary--");
        assert!(matches!(parser.piece(), Ok(Some(Piece::End))));
        assert!(matches!(parser.next(64), Ok(Some(Next::Closed))));
    }

    /// A preamble with no end holds at most a delimiter's length, however
    /// much of it arrives.
    #[test]
    fn a_preamble_is_not_kept() {
        let mut parser = Parser::new("boundary");
        for _ in 0..1000 {
            parser.push(&[b'.'; 1024]);
            assert!(matches!(parser.next(usize::MAX), Ok(None)));
            assert!(parser.held() < "--boundary".len());
        }
    }

    /// A skipped value is dropped as it arrives.
    #[test]
    fn a_skipped_value_is_not_kept() {
        let mut parser = Parser::new("boundary");
        parser.push(b"--boundary\r\nX: 1\r\n\r\n");
        assert!(matches!(parser.next(64), Ok(Some(Next::Part(_)))));
        for _ in 0..1000 {
            parser.push(&[b'v'; 1024]);
            assert!(matches!(parser.next(64), Ok(None)));
            assert!(parser.held() < "\r\n--boundary".len());
        }
    }

    #[test]
    fn after_the_close_nothing_is_kept_or_read() {
        let mut parser = Parser::new("b");
        parser.push(b"--b--");
        assert!(matches!(parser.next(64), Ok(Some(Next::Closed))));
        parser.push(&[0; 4096]);
        assert_eq!(parser.held(), 0);
        parser.end();
        assert!(matches!(parser.next(64), Ok(Some(Next::Closed))));
        assert!(matches!(parser.piece(), Ok(Some(Piece::End))));
    }

    #[test]
    fn what_may_start_a_delimiter_is_held_back() {
        assert_eq!(held_back(b"abc", b"\r\n--b"), 0);
        assert_eq!(held_back(b"abc\r", b"\r\n--b"), 1);
        assert_eq!(held_back(b"abc\r\n-", b"\r\n--b"), 3);
        assert_eq!(held_back(b"abc\r\n--", b"\r\n--b"), 4);
        assert_eq!(held_back(b"\r\n-x", b"\r\n--b"), 0);
        assert_eq!(held_back(b"", b"\r\n--b"), 0);
        assert_eq!(held_back(b"\r", b"\r\n--b"), 1);
    }
}
