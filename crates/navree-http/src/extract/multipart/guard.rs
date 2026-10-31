//! What stands between a request body and the parser.
//!
//! The parser keeps what it has not understood yet, and searches all of it
//! again each time more arrives, so a head or a preamble that never ends is
//! memory, and time that grows with the square of its length. It has no limit
//! on either. [`Scanner`] reads the body as it goes past, finding what the
//! parser will find in the same places, and stops the body at the limit,
//! before the parser buffers more of it. It only ever refuses: the bytes it
//! lets through are the bytes that came in.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, ready};

use bytes::Bytes;
use futures_util::Stream;
use http_body_util::{BodyDataStream, BodyExt, LengthLimitError, Limited};
use memchr::memmem::Finder;
use navree_kernel::BoxError;
use pin_project_lite::pin_project;

use crate::body::{Body, BodyError};
use crate::extract::rejection::MultipartError;
use crate::serve::BodyReadTimeout;

pin_project! {
    /// A request body inside its limit, watched by a [`Scanner`]. Its errors
    /// are [`MultipartError`]s, and the first one stays.
    pub(super) struct Guarded {
        #[pin]
        body: BodyDataStream<Limited<Body>>,
        scanner: Scanner,
        head_limit: Arc<AtomicUsize>,
        failed: Option<MultipartError>,
        ended: bool,
    }
}

impl Guarded {
    pub(super) fn new(
        body: Body,
        body_limit: usize,
        boundary: &str,
        head_limit: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            body: Limited::new(body, body_limit).into_data_stream(),
            scanner: Scanner::new(boundary),
            head_limit,
            failed: None,
            ended: false,
        }
    }
}

impl Stream for Guarded {
    type Item = Result<Bytes, MultipartError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        if let Some(error) = this.failed {
            return Poll::Ready(Some(Err(error.clone())));
        }
        if *this.ended {
            return Poll::Ready(None);
        }
        let failure = match ready!(this.body.poll_next(cx)) {
            Some(Ok(chunk)) => {
                let limit = this.head_limit.load(Ordering::Relaxed);
                if this.scanner.scan(&chunk, limit) {
                    return Poll::Ready(Some(Ok(chunk)));
                }
                MultipartError::HeadTooLarge { limit }
            }
            Some(Err(error)) => MultipartError::Body(body_error(&error)),
            None => {
                // The parser polls twice in one go while it looks for the
                // first delimiter, and takes the end of the body from the
                // second poll to mean the first found none, without looking
                // at what that same poll delivered. So the end comes on a
                // later wake, once it has looked.
                *this.ended = true;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
        };
        *this.failed = Some(failure.clone());
        Poll::Ready(Some(Err(failure)))
    }
}

/// How a body failed.
fn body_error(error: &BoxError) -> BodyError {
    let chain = || {
        std::iter::successors(
            Some(&**error as &(dyn std::error::Error + 'static)),
            |error| error.source(),
        )
    };
    if chain().any(|error| error.is::<LengthLimitError>()) {
        BodyError::TooLarge
    } else if chain().any(|error| error.is::<BodyReadTimeout>()) {
        BodyError::TimedOut
    } else {
        BodyError::Read(error.to_string())
    }
}

/// Where the parser is in the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Before the first delimiter, which may be anywhere.
    Preamble,
    /// Just past a delimiter: `--` closes the form, anything else is padding
    /// and a line end.
    Delimited,
    /// Past one `-`.
    Dash,
    /// In the spaces and tabs that may follow a delimiter.
    Padding,
    /// Past the `\r` of the delimiter line.
    LineEnd,
    /// In the headers of a part. A blank line at the very start does not end
    /// them: the line end before it was the delimiter line's, and the parser
    /// looks for `\r\n\r\n` from here.
    Head,
    /// In the value of a part, which ends at `\r\n` and a delimiter.
    Value,
    /// The form closed, or stopped being one: the parser has nothing more to
    /// read, or refuses what is next.
    Over,
}

/// Follows a body through its delimiters and heads, whatever its chunks.
#[derive(Debug)]
pub(super) struct Scanner {
    stage: Stage,
    delimiter: Finder<'static>,
    value_end: Finder<'static>,
    head_end: Finder<'static>,
    // The last bytes of this stage: what a pattern may have started with.
    tail: Vec<u8>,
    // The bytes of this stage so far.
    seen: usize,
}

impl Scanner {
    pub(super) fn new(boundary: &str) -> Self {
        Self {
            stage: Stage::Preamble,
            delimiter: Finder::new(format!("--{boundary}").as_bytes()).into_owned(),
            value_end: Finder::new(format!("\r\n--{boundary}").as_bytes()).into_owned(),
            head_end: Finder::new(b"\r\n\r\n").into_owned(),
            tail: Vec::new(),
            seen: 0,
        }
    }

    /// Reads the next chunk. `false` when a preamble, padding or head has
    /// passed `limit`: exactly when the bytes so far prove it, so the verdict
    /// does not depend on where the chunks were cut.
    pub(super) fn scan(&mut self, mut chunk: &[u8], limit: usize) -> bool {
        while let Some(&first) = chunk.first() {
            match self.stage {
                Stage::Preamble => {
                    let length = self.delimiter.needle().len();
                    match find(&self.delimiter, &self.tail, chunk) {
                        Some(end) => {
                            if (self.seen + end).saturating_sub(length) > limit {
                                return false;
                            }
                            self.enter(Stage::Delimited);
                            chunk = &chunk[end..];
                        }
                        None => {
                            self.seen = self.seen.saturating_add(chunk.len());
                            if self.seen.saturating_sub(length - 1) > limit {
                                return false;
                            }
                            keep_end(&mut self.tail, chunk, length - 1);
                            return true;
                        }
                    }
                }
                Stage::Delimited | Stage::Dash | Stage::LineEnd => {
                    self.stage = match (self.stage, first) {
                        (Stage::Delimited, b'-') => Stage::Dash,
                        (Stage::Delimited, b' ' | b'\t') => Stage::Padding,
                        (Stage::Delimited, b'\r') => Stage::LineEnd,
                        (Stage::LineEnd, b'\n') => Stage::Head,
                        _ => Stage::Over,
                    };
                    chunk = &chunk[1..];
                }
                Stage::Padding => {
                    let padding = chunk
                        .iter()
                        .position(|byte| !matches!(byte, b' ' | b'\t'))
                        .unwrap_or(chunk.len());
                    self.seen = self.seen.saturating_add(padding);
                    if self.seen > limit {
                        return false;
                    }
                    chunk = &chunk[padding..];
                    if let Some(next) = chunk.first() {
                        self.stage = if *next == b'\r' {
                            Stage::LineEnd
                        } else {
                            Stage::Over
                        };
                        self.seen = 0;
                        chunk = &chunk[1..];
                    }
                }
                Stage::Head => match find(&self.head_end, &self.tail, chunk) {
                    Some(end) => {
                        if self.seen.saturating_add(end) > limit {
                            return false;
                        }
                        self.enter(Stage::Value);
                        chunk = &chunk[end..];
                    }
                    None => {
                        self.seen = self.seen.saturating_add(chunk.len());
                        // Its blank line is still to come.
                        if self.seen >= limit {
                            return false;
                        }
                        keep_end(&mut self.tail, chunk, self.head_end.needle().len() - 1);
                        return true;
                    }
                },
                Stage::Value => match find(&self.value_end, &self.tail, chunk) {
                    Some(end) => {
                        self.enter(Stage::Delimited);
                        chunk = &chunk[end..];
                    }
                    None => {
                        keep_end(&mut self.tail, chunk, self.value_end.needle().len() - 1);
                        return true;
                    }
                },
                Stage::Over => return true,
            }
        }
        true
    }

    fn enter(&mut self, stage: Stage) {
        self.stage = stage;
        self.tail.clear();
        self.seen = 0;
    }
}

/// Where `finder`'s pattern ends in `chunk`, after `tail`, the end of what
/// came before it (shorter than the pattern). Finds the first match.
fn find(finder: &Finder<'_>, tail: &[u8], chunk: &[u8]) -> Option<usize> {
    let length = finder.needle().len();
    if !tail.is_empty() {
        let start = &chunk[..chunk.len().min(length - 1)];
        let mut joined = Vec::with_capacity(tail.len() + start.len());
        joined.extend_from_slice(tail);
        joined.extend_from_slice(start);
        if let Some(at) = finder.find(&joined) {
            return Some(at + length - tail.len());
        }
    }
    finder.find(chunk).map(|at| at + length)
}

/// Keeps the last `length` bytes of what `tail` and then `chunk` hold.
fn keep_end(tail: &mut Vec<u8>, chunk: &[u8], length: usize) {
    if chunk.len() >= length {
        tail.clear();
        tail.extend_from_slice(&chunk[chunk.len() - length..]);
    } else {
        let excess = (tail.len() + chunk.len()).saturating_sub(length);
        tail.drain(..excess);
        tail.extend_from_slice(chunk);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scans `body` split at `cut`, and reports the stage it ends in.
    fn scanned(body: &[u8], cut: usize, limit: usize) -> Option<Stage> {
        let mut scanner = Scanner::new("b");
        let (first, second) = body.split_at(cut);
        (scanner.scan(first, limit) && scanner.scan(second, limit)).then_some(scanner.stage)
    }

    /// Every way of cutting `body` in two ends in `stage`.
    #[track_caller]
    fn ends_in(body: &[u8], stage: Stage) {
        for cut in 0..=body.len() {
            assert_eq!(
                scanned(body, cut, 64),
                Some(stage),
                "{:?} cut at {cut}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn it_follows_a_form_across_any_cut() {
        ends_in(b"", Stage::Preamble);
        ends_in(b"preamble-", Stage::Preamble);
        ends_in(b"--b", Stage::Delimited);
        ends_in(b"x--b", Stage::Delimited);
        ends_in(b"--b-", Stage::Dash);
        ends_in(b"--b--", Stage::Over);
        ends_in(b"--b \t", Stage::Padding);
        ends_in(b"--b\r", Stage::LineEnd);
        ends_in(b"--b \r", Stage::LineEnd);
        ends_in(b"--b\r\n", Stage::Head);
        ends_in(b"--b\r\nA: 1", Stage::Head);
        ends_in(b"--b\r\nA: 1\r\n\r", Stage::Head);
        ends_in(b"--b\r\nA: 1\r\n\r\n", Stage::Value);
        ends_in(b"--b\r\nA: 1\r\n\r\n--b", Stage::Value);
        ends_in(b"--b\r\nA: 1\r\n\r\nvalue\r\n--", Stage::Value);
        ends_in(b"--b\r\nA: 1\r\n\r\nvalue\r\n--b", Stage::Delimited);
        ends_in(b"--b\r\nA: 1\r\n\r\n\r\n--b\r\n", Stage::Head);
        ends_in(b"--b\r\nA: 1\r\n\r\nvalue\r\n--b--", Stage::Over);
    }

    #[test]
    fn what_is_not_a_form_stops_being_watched() {
        ends_in(b"--bx", Stage::Over);
        ends_in(b"--b-x", Stage::Over);
        ends_in(b"--b\rx", Stage::Over);
        ends_in(b"--b x", Stage::Over);
        ends_in(b"--b--anything at all, of any length", Stage::Over);
    }

    #[test]
    fn a_blank_line_after_the_delimiter_line_does_not_end_the_head() {
        ends_in(b"--b\r\n\r\n", Stage::Head);
        ends_in(b"--b\r\n\r\n\r\n", Stage::Value);
    }

    #[test]
    fn a_pattern_straddling_many_chunks_is_found() {
        let mut scanner = Scanner::new("boundary");
        for byte in b"--boundary\r\nA: 1\r\n\r\nv\r\n--boundary--" {
            assert!(scanner.scan(&[*byte], 64));
        }
        assert_eq!(scanner.stage, Stage::Over);
    }

    #[test]
    fn the_first_match_counts() {
        let mut scanner = Scanner::new("b");
        assert!(scanner.scan(b"--b\r\nA: 1\r\n\r\nv\r\n--b\r\nB: 2\r\n\r\nw", 64));
        assert_eq!(scanner.stage, Stage::Value);
    }

    #[test]
    fn keeping_the_end_keeps_only_that() {
        let mut tail = b"abc".to_vec();
        keep_end(&mut tail, b"d", 3);
        assert_eq!(tail, b"bcd");
        keep_end(&mut tail, b"efghi", 3);
        assert_eq!(tail, b"ghi");
        keep_end(&mut tail, b"", 3);
        assert_eq!(tail, b"ghi");
    }
}
