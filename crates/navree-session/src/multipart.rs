//! The CSRF token of a `multipart/form-data` body: what a form with a file
//! field sends when no script sends it.
//!
//! Only the body's first part is read, and only from the first
//! [`TOKEN_PART_MAX`] bytes: it has to be the token field, as navree-ui's forms
//! send it (the field comes first in the form), and it has to end there. So
//! nothing of a file is held, however large: the body goes on to the handler as
//! it came, the frames read first, then the rest as it arrives.
//!
//! What two readers could take differently is refused rather than guessed at:
//! anything before the first delimiter or after it on its line, a head with a
//! bare line end, a folded line or a header name that is not a token, two
//! `Content-Disposition` headers, and a disposition that is anything but
//! `form-data; name="_csrf"`: another field, a file (`filename`), or any other
//! parameter. What is found depends on the bytes, never on how they were split
//! into frames.

use std::collections::VecDeque;
use std::future::poll_fn;
use std::ops::Range;
use std::pin::Pin;
use std::task::{Context, Poll};

use http::HeaderMap;
use http::header::CONTENT_TYPE;
use http_body::{Body as HttpBody, Frame, SizeHint};
use memchr::memmem;
use navree_http::body::{Body, Bytes};
use navree_http::csrf::CSRF_FIELD;
use navree_kernel::BoxError;

/// The most of a multipart body read for its token: the first part's
/// delimiter line, head and value, and the line end or close after it.
pub(crate) const TOKEN_PART_MAX: usize = 8 * 1024;

/// The longest boundary: RFC 2046's 70 characters.
const MAX_BOUNDARY_BYTES: usize = 70;

/// The blank line that ends a head.
const HEAD_END: &[u8] = b"\r\n\r\n";

/// Spaces and tabs.
const BLANK: [char; 2] = [' ', '\t'];

/// The boundary of the `multipart/form-data` body that `headers` announce. Two
/// `Content-Type` headers, or two `boundary` parameters, name none: whoever is
/// in front of the app may have read the other one.
pub(crate) fn form_boundary(headers: &HeaderMap) -> Option<String> {
    let mut values = headers.get_all(CONTENT_TYPE).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    let media_type = value.to_str().ok()?.parse::<mime::Mime>().ok()?;
    if media_type.type_() != mime::MULTIPART || media_type.subtype() != mime::FORM_DATA {
        return None;
    }
    let mut boundaries = media_type
        .params()
        .filter(|(name, _)| *name == mime::BOUNDARY);
    match (boundaries.next(), boundaries.next()) {
        (Some((_, boundary)), None)
            if (1..=MAX_BOUNDARY_BYTES).contains(&boundary.as_str().len()) =>
        {
            Some(boundary.as_str().to_owned())
        }
        _ => None,
    }
}

/// Reads the start of `body`, at most `limit` bytes of it, until its first part
/// ends. When that part is the token field, its value and the body as it came
/// (what was read comes again first). `None` when the first part is anything
/// else, does not end within `limit` bytes, or the body fails or ends first.
pub(crate) async fn first_part_token(
    mut body: Body,
    boundary: &str,
    limit: usize,
) -> Option<(Vec<u8>, Body)> {
    let mut reader = FirstPartReader::new(boundary);
    let mut start: Vec<u8> = Vec::new();
    let mut read = VecDeque::new();
    let mut ended = false;
    // Every frame counts, empty ones too: a body of nothing but empty frames
    // ends here as well.
    for _ in 0..=limit {
        match reader.read(&start) {
            FirstPart::Token(value) => {
                let value = start.get(value)?.to_vec();
                return Some((value, Body::new(Replay { read, rest: body })));
            }
            FirstPart::Other => return None,
            FirstPart::More if ended || start.len() >= limit => return None,
            FirstPart::More => {}
        }
        match poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
            Some(Ok(frame)) => {
                if let Some(data) = frame.data_ref() {
                    if data.is_empty() {
                        continue;
                    }
                    let room = limit.saturating_sub(start.len()).min(data.len());
                    start.extend_from_slice(data.get(..room).unwrap_or_default());
                }
                read.push_back(frame);
            }
            Some(Err(_)) => return None,
            None => ended = true,
        }
    }
    None
}

/// What the start of a body says about its first part.
#[derive(Clone, Debug, PartialEq, Eq)]
enum FirstPart {
    /// It takes more of the body.
    More,
    /// The first part is the token field, with its value at this range of the
    /// start.
    Token(Range<usize>),
    /// The first part is something else, or the start does not read one way.
    Other,
}

/// Reads a form's first part from the start of its body as the start grows. A
/// search goes on where the last one stopped.
struct FirstPartReader {
    /// `--boundary` and its line end: how the body opens.
    opening: Vec<u8>,
    /// A line end then `--boundary`: what ends the value.
    delimiter: Vec<u8>,
    /// Where the value starts, once the head has been read.
    value_start: Option<usize>,
    /// Where the search for the end of the head, or of the value, goes on from.
    searched: usize,
}

impl FirstPartReader {
    fn new(boundary: &str) -> Self {
        Self {
            opening: format!("--{boundary}\r\n").into_bytes(),
            delimiter: format!("\r\n--{boundary}").into_bytes(),
            value_start: None,
            searched: 0,
        }
    }

    /// What `start`, the first bytes of the body, says. `start` only grows
    /// from one call to the next.
    fn read(&mut self, start: &[u8]) -> FirstPart {
        let opening = self.opening.len();
        // The body opens with the delimiter line: no preamble, no padding.
        let Some(line) = start.get(..opening) else {
            return if self.opening.starts_with(start) {
                FirstPart::More
            } else {
                FirstPart::Other
            };
        };
        if line != self.opening.as_slice() {
            return FirstPart::Other;
        }
        let value_start = match self.value_start {
            Some(at) => at,
            None => {
                let from = self.searched.max(opening);
                let Some(at) = memmem::find(start.get(from..).unwrap_or_default(), HEAD_END) else {
                    // The blank line may have begun in the last three bytes.
                    self.searched = start.len().saturating_sub(HEAD_END.len() - 1).max(opening);
                    return FirstPart::More;
                };
                let head_end = from + at;
                if !names_the_token(start.get(opening..head_end).unwrap_or_default()) {
                    return FirstPart::Other;
                }
                let value_start = head_end + HEAD_END.len();
                self.value_start = Some(value_start);
                self.searched = value_start;
                value_start
            }
        };
        let from = self.searched;
        let Some(at) = memmem::find(start.get(from..).unwrap_or_default(), &self.delimiter) else {
            // The delimiter may have begun in the last bytes.
            self.searched = start
                .len()
                .saturating_sub(self.delimiter.len() - 1)
                .max(value_start);
            return FirstPart::More;
        };
        let value_end = from + at;
        // The next part's line end, or the close of the form.
        let after = start
            .get(value_end + self.delimiter.len()..)
            .unwrap_or_default();
        match after {
            [b'\r', b'\n', ..] | [b'-', b'-', ..] => FirstPart::Token(value_start..value_end),
            [] | [b'\r'] | [b'-'] => {
                self.searched = value_end;
                FirstPart::More
            }
            _ => FirstPart::Other,
        }
    }
}

/// Whether a part's head (its header lines, without the blank line that ends
/// them) is the token field's: header lines that read one way, and one
/// `Content-Disposition`, the token field's.
fn names_the_token(head: &[u8]) -> bool {
    let Ok(head) = std::str::from_utf8(head) else {
        return false;
    };
    let mut disposition = None;
    for line in head.split("\r\n") {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        // A bare line end may end a line elsewhere; a folded line starts
        // with a blank, which no header name has.
        if line.contains(['\r', '\n']) || name.is_empty() || !name.bytes().all(is_token) {
            return false;
        }
        if name.eq_ignore_ascii_case("content-disposition") && disposition.replace(value).is_some()
        {
            return false;
        }
    }
    disposition.is_some_and(is_token_disposition)
}

/// `form-data; name="_csrf"`: the type and the parameter's name in any case,
/// the field's name quoted or not, blanks around each. Any other parameter,
/// `filename` among them, makes it something else.
fn is_token_disposition(value: &str) -> bool {
    let mut pieces = value.split(';').map(|piece| piece.trim_matches(BLANK));
    let (Some(kind), Some(parameter), None) = (pieces.next(), pieces.next(), pieces.next()) else {
        return false;
    };
    let Some((key, name)) = parameter.split_once('=') else {
        return false;
    };
    let name = name.trim_start_matches(BLANK);
    let unquoted = name
        .strip_prefix('"')
        .and_then(|name| name.strip_suffix('"'))
        .unwrap_or(name);
    kind.eq_ignore_ascii_case("form-data")
        && key.trim_end_matches(BLANK).eq_ignore_ascii_case("name")
        && unquoted == CSRF_FIELD
}

/// Whether `byte` may be in a header name (RFC 9110's `tchar`).
fn is_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

/// A body whose first frames have been read: they come again, then the rest.
struct Replay {
    read: VecDeque<Frame<Bytes>>,
    rest: Body,
}

impl HttpBody for Replay {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        match self.read.pop_front() {
            Some(frame) => Poll::Ready(Some(Ok(frame))),
            None => Pin::new(&mut self.rest).poll_frame(cx),
        }
    }

    fn is_end_stream(&self) -> bool {
        self.read.is_empty() && self.rest.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        let held: u64 = self
            .read
            .iter()
            .filter_map(Frame::data_ref)
            .map(|data| u64::try_from(data.len()).unwrap_or(u64::MAX))
            .fold(0, u64::saturating_add);
        let rest = self.rest.size_hint();
        let mut hint = SizeHint::new();
        hint.set_lower(rest.lower().saturating_add(held));
        if let Some(upper) = rest.upper() {
            hint.set_upper(upper.saturating_add(held));
        }
        hint
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_util::StreamExt;
    use http::HeaderValue;
    use navree_http::body::to_bytes;

    use super::*;

    /// The token field's head, as browsers send it.
    const TOKEN_HEAD: &str = "Content-Disposition: form-data; name=\"_csrf\"";

    /// A form with boundary `b`: the parts given, each a head and a value.
    fn form(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut body = String::new();
        for (head, value) in parts {
            body.push_str(&format!("--b\r\n{head}\r\n\r\n{value}\r\n"));
        }
        body.push_str("--b--\r\n");
        body.into_bytes()
    }

    /// What the reader finds in `body`, given whole, cut in two anywhere, or
    /// a byte at a time: the same, or the test fails.
    #[track_caller]
    fn read(body: &[u8]) -> FirstPart {
        let whole = read_chunks(&[body]);
        for cut in 0..=body.len() {
            let (first, second) = body.split_at(cut);
            assert_eq!(read_chunks(&[first, second]), whole, "cut at {cut}");
        }
        let bytes: Vec<&[u8]> = body.chunks(1).collect();
        assert_eq!(read_chunks(&bytes), whole, "a byte at a time");
        whole
    }

    /// What the reader finds as `chunks` arrive; [`FirstPart::More`] when they
    /// end first.
    fn read_chunks(chunks: &[&[u8]]) -> FirstPart {
        let mut reader = FirstPartReader::new("b");
        let mut start = Vec::new();
        let mut found = reader.read(&start);
        for chunk in chunks {
            if found != FirstPart::More {
                break;
            }
            start.extend_from_slice(chunk);
            found = reader.read(&start);
        }
        found
    }

    /// The value the reader finds in `body`, if it finds the token field.
    #[track_caller]
    fn token(body: &[u8]) -> Option<String> {
        match read(body) {
            FirstPart::Token(range) => Some(String::from_utf8(body[range].to_vec()).unwrap()),
            _ => None,
        }
    }

    #[test]
    fn the_token_field_first_is_found_however_the_body_is_cut() {
        let body = form(&[
            (TOKEN_HEAD, "t0ken"),
            (
                "Content-Disposition: form-data; name=\"file\"; filename=\"a.pdf\"\r\nContent-Type: application/pdf",
                "%PDF",
            ),
        ]);
        assert_eq!(token(&body).as_deref(), Some("t0ken"));
        // The form's only part, and an empty value.
        assert_eq!(
            token(&form(&[(TOKEN_HEAD, "t0ken")])).as_deref(),
            Some("t0ken")
        );
        assert_eq!(token(&form(&[(TOKEN_HEAD, "")])).as_deref(), Some(""));
        // A value holds what is not a delimiter.
        let near_misses = "a\r\n--c\r\n-\r\n--x--b\r";
        assert_eq!(
            token(&form(&[(TOKEN_HEAD, near_misses)])).as_deref(),
            Some(near_misses)
        );
    }

    #[test]
    fn the_disposition_may_be_written_in_any_of_the_ways_it_reads_the_same() {
        for head in [
            "Content-Disposition: form-data; name=_csrf",
            "content-disposition:form-data;name=\"_csrf\"",
            "CONTENT-DISPOSITION: Form-Data ; NAME = \"_csrf\" ",
            "Content-Disposition: form-data;\tname=\"_csrf\"",
            "Content-Disposition: form-data; name=\"_csrf\"\r\nContent-Type: text/plain",
            "X-Other: 1\r\nContent-Disposition: form-data; name=\"_csrf\"",
        ] {
            assert_eq!(
                token(&form(&[(head, "t")])).as_deref(),
                Some("t"),
                "{head:?}"
            );
        }
    }

    #[test]
    fn a_first_part_that_is_not_the_token_field_is_refused() {
        for head in [
            // Another field, or a name close to the token's.
            "Content-Disposition: form-data; name=\"title\"",
            "Content-Disposition: form-data; name=\"_CSRF\"",
            "Content-Disposition: form-data; name=\"csrf\"",
            "Content-Disposition: form-data; name=\"x_csrf\"",
            "Content-Disposition: form-data; name=\"_csrf[]\"",
            "Content-Disposition: form-data; name=\"_csrf \"",
            "Content-Disposition: form-data; name=\"\"_csrf\"",
            "Content-Disposition: form-data; name='_csrf'",
            // A file of that name, or any other parameter.
            "Content-Disposition: form-data; name=\"_csrf\"; filename=\"t.txt\"",
            "Content-Disposition: form-data; filename=\"t.txt\"; name=\"_csrf\"",
            "Content-Disposition: form-data; name=\"_csrf\"; filename*=UTF-8''t.txt",
            "Content-Disposition: form-data; name=\"_csrf\"; x=1",
            "Content-Disposition: form-data; name=\"_csrf\";",
            "Content-Disposition: form-data; name=\"_csrf\"; name=\"_csrf\"",
            // Another type, no name, no parameter.
            "Content-Disposition: attachment; name=\"_csrf\"",
            "Content-Disposition: form-data",
            "Content-Disposition: form-data; filename=\"_csrf\"",
            "Content-Disposition: form-data; _csrf",
            "Content-Type: text/plain",
            // Two dispositions, whichever is the token's.
            "Content-Disposition: form-data; name=\"_csrf\"\r\nContent-Disposition: form-data; name=\"a\"",
            "Content-Disposition: form-data; name=\"a\"\r\nContent-Disposition: form-data; name=\"_csrf\"",
            "Content-Disposition: form-data; name=\"_csrf\"\r\ncontent-disposition: form-data; name=\"_csrf\"",
            // Lines another reader could end or join elsewhere.
            "Content-Disposition: form-data; name=\"_csrf\"\nX: 1",
            "X: 1\rContent-Disposition: form-data; name=\"_csrf\"",
            "X: 1\r\n Content-Disposition: form-data; name=\"_csrf\"",
            "Content-Disposition: form-data;\r\n name=\"_csrf\"",
            "Content-Disposition : form-data; name=\"_csrf\"",
            "No colon\r\nContent-Disposition: form-data; name=\"_csrf\"",
            ": x\r\nContent-Disposition: form-data; name=\"_csrf\"",
        ] {
            assert_eq!(read(&form(&[(head, "t")])), FirstPart::Other, "{head:?}");
        }
        // The token field after another field does not count.
        let body = form(&[
            ("Content-Disposition: form-data; name=\"title\"", "Agra"),
            (TOKEN_HEAD, "t"),
        ]);
        assert_eq!(read(&body), FirstPart::Other);
        // Nor after a file.
        let body = form(&[
            (
                "Content-Disposition: form-data; name=\"f\"; filename=\"a\"",
                "x",
            ),
            (TOKEN_HEAD, "t"),
        ]);
        assert_eq!(read(&body), FirstPart::Other);
    }

    #[test]
    fn a_head_that_is_not_utf8_is_refused() {
        let mut body = b"--b\r\nX-Note: \xff\r\n".to_vec();
        body.extend_from_slice(format!("{TOKEN_HEAD}\r\n\r\nt\r\n--b--").as_bytes());
        assert_eq!(read(&body), FirstPart::Other);
    }

    #[test]
    fn a_head_with_nothing_in_it_is_refused() {
        assert_eq!(read(b"--b\r\n\r\n\r\nt\r\n--b--"), FirstPart::Other);
        // A blank line first, then the token's head: the blank line ends it.
        let body = format!("--b\r\n\r\n{TOKEN_HEAD}\r\n\r\nt\r\n--b--");
        assert_eq!(read(body.as_bytes()), FirstPart::Other);
    }

    #[test]
    fn the_body_opens_with_the_delimiter_line_and_nothing_else() {
        let token_part = format!("{TOKEN_HEAD}\r\n\r\nt\r\n--b--");
        for opening in [
            "preamble\r\n--b\r\n",
            "\r\n--b\r\n",
            " --b\r\n",
            "--b \r\n",
            "--b\t\r\n",
            "--bb\r\n",
            "--b\n",
            "--b\r",
            "--c\r\n",
            "-b\r\n",
            "--B\r\n",
        ] {
            let body = format!("{opening}{token_part}");
            assert_eq!(read(body.as_bytes()), FirstPart::Other, "{opening:?}");
        }
        // A form with no part has no token.
        assert_eq!(read(b"--b--\r\n"), FirstPart::Other);
        assert_eq!(read(b""), FirstPart::More);
    }

    #[test]
    fn the_value_ends_at_a_delimiter_that_ends_well() {
        // After the delimiter: the next part's line end, or the close.
        assert_eq!(
            token(format!("--b\r\n{TOKEN_HEAD}\r\n\r\nt\r\n--b\r\nX").as_bytes()).as_deref(),
            Some("t")
        );
        assert_eq!(
            token(format!("--b\r\n{TOKEN_HEAD}\r\n\r\nt\r\n--b--").as_bytes()).as_deref(),
            Some("t")
        );
        // Padding, or anything else, is refused.
        for after in [" \r\n", "\t\r\n", "x", "-x", "\rx", "\n"] {
            let body = format!("--b\r\n{TOKEN_HEAD}\r\n\r\nt\r\n--b{after}");
            assert_eq!(read(body.as_bytes()), FirstPart::Other, "{after:?}");
        }
    }

    #[test]
    fn a_first_part_cut_short_takes_more_of_the_body() {
        let body = format!("--b\r\n{TOKEN_HEAD}\r\n\r\nt0ken\r\n--b--");
        for end in 0..body.len() - 1 {
            assert_eq!(
                read_chunks(&[&body.as_bytes()[..end]]),
                FirstPart::More,
                "cut at {end}"
            );
        }
    }

    #[test]
    fn only_a_form_with_one_boundary_names_one() {
        let boundary = |values: &[&'static str]| {
            let mut headers = HeaderMap::new();
            for value in values {
                headers.append(CONTENT_TYPE, HeaderValue::from_static(value));
            }
            form_boundary(&headers)
        };
        assert_eq!(
            boundary(&["multipart/form-data; boundary=b"]).as_deref(),
            Some("b")
        );
        assert_eq!(
            boundary(&["Multipart/Form-Data; boundary=\"----x y\""]).as_deref(),
            Some("----x y")
        );
        let longest = format!("multipart/form-data; boundary={}", "b".repeat(70));
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_str(&longest).unwrap());
        assert!(form_boundary(&headers).is_some());
        let too_long = format!("multipart/form-data; boundary={}", "b".repeat(71));
        headers.insert(CONTENT_TYPE, HeaderValue::from_str(&too_long).unwrap());
        assert!(form_boundary(&headers).is_none());
        for values in [
            &[][..],
            &["multipart/form-data"],
            &["multipart/form-data; boundary="],
            &["multipart/form-data; boundary=b; boundary=c"],
            &["multipart/mixed; boundary=b"],
            &["application/x-www-form-urlencoded"],
            &["text/plain; boundary=b"],
            &["not a type"],
            &[
                "multipart/form-data; boundary=b",
                "multipart/form-data; boundary=b",
            ],
        ] {
            assert_eq!(boundary(values), None, "{values:?}");
        }
    }

    /// A body of `chunks`, then `tail`.
    fn streamed(
        chunks: Vec<Result<Bytes, BoxError>>,
        tail: impl futures_util::Stream<Item = Result<Bytes, BoxError>> + Send + 'static,
    ) -> Body {
        Body::from_stream(futures_util::stream::iter(chunks).chain(tail))
    }

    fn ok(bytes: &[u8]) -> Result<Bytes, BoxError> {
        Ok(Bytes::copy_from_slice(bytes))
    }

    #[tokio::test]
    async fn the_body_goes_on_as_it_came() {
        let file = "x".repeat(100_000);
        let body = form(&[
            (TOKEN_HEAD, "t0ken"),
            (
                "Content-Disposition: form-data; name=\"f\"; filename=\"a.bin\"",
                file.as_str(),
            ),
        ]);
        let chunks = body.chunks(1000).map(ok).collect();
        let (value, rest) = first_part_token(
            streamed(chunks, futures_util::stream::empty()),
            "b",
            TOKEN_PART_MAX,
        )
        .await
        .expect("the token field is first");
        assert_eq!(value, b"t0ken");
        assert_eq!(&to_bytes(rest, usize::MAX).await.unwrap()[..], &body[..]);
    }

    #[tokio::test]
    async fn the_size_of_the_body_is_kept() {
        let body = form(&[
            (TOKEN_HEAD, "t0ken"),
            ("Content-Disposition: form-data; name=\"a\"", "1"),
        ]);
        let length = u64::try_from(body.len()).unwrap();
        let (_, rest) = first_part_token(Body::from(body), "b", TOKEN_PART_MAX)
            .await
            .unwrap();
        assert_eq!(rest.size_hint().exact(), Some(length));
        assert!(!rest.is_end_stream());
    }

    /// The token is read without waiting for the rest of the body: a file
    /// still on its way, or one that never ends, is not held.
    #[tokio::test]
    async fn the_rest_of_the_body_is_not_waited_for() {
        let first = format!(
            "--b\r\n{TOKEN_HEAD}\r\n\r\nt0ken\r\n--b\r\nContent-Disposition: form-data; name=\"f\"; filename=\"a\"\r\n\r\n"
        );
        let body = streamed(vec![ok(first.as_bytes())], futures_util::stream::pending());
        let found = tokio::time::timeout(
            Duration::from_secs(5),
            first_part_token(body, "b", TOKEN_PART_MAX),
        )
        .await
        .expect("the token is read from the first frame");
        let (value, mut rest) = found.expect("the token field is first");
        assert_eq!(value, b"t0ken");
        let frame = poll_fn(|cx| Pin::new(&mut rest).poll_frame(cx)).await;
        let data = frame.unwrap().unwrap().into_data().unwrap();
        assert_eq!(&data[..], first.as_bytes(), "what was read comes first");
    }

    #[tokio::test]
    async fn a_first_part_past_the_limit_is_refused_without_reading_on() {
        let value = "t".repeat(TOKEN_PART_MAX);
        let body = form(&[(TOKEN_HEAD, value.as_str())]);
        assert!(
            first_part_token(Body::from(body), "b", TOKEN_PART_MAX)
                .await
                .is_none()
        );
        // The limit is exact: the part's last byte, the close after it, at the limit.
        let part = format!("--b\r\n{TOKEN_HEAD}\r\n\r\nt0ken\r\n--b--");
        assert!(
            first_part_token(Body::from(part.clone()), "b", part.len())
                .await
                .is_some()
        );
        assert!(
            first_part_token(Body::from(part.clone()), "b", part.len() - 1)
                .await
                .is_none()
        );
        // A head that never ends is not read past the limit, nor waited on.
        let endless = futures_util::stream::repeat_with(|| ok(b"X-Pad: 0123456789\r\n"));
        let body = streamed(vec![ok(b"--b\r\n")], endless);
        let refused = tokio::time::timeout(
            Duration::from_secs(5),
            first_part_token(body, "b", TOKEN_PART_MAX),
        )
        .await;
        assert_eq!(refused.ok().map(|found| found.is_none()), Some(true));
    }

    #[tokio::test]
    async fn empty_frames_do_not_read_on_for_ever() {
        let body = streamed(
            vec![ok(b"--b\r\n")],
            futures_util::stream::repeat_with(|| Ok(Bytes::new())),
        );
        let refused = tokio::time::timeout(
            Duration::from_secs(5),
            first_part_token(body, "b", TOKEN_PART_MAX),
        )
        .await;
        assert_eq!(refused.ok().map(|found| found.is_none()), Some(true));
        // Empty frames between others are skipped, and not handed on.
        let part = format!("--b\r\n{TOKEN_HEAD}\r\n\r\nt0ken\r\n--b--");
        let (head, tail) = part.as_bytes().split_at(10);
        let body = streamed(
            vec![ok(head), Ok(Bytes::new()), ok(tail)],
            futures_util::stream::empty(),
        );
        let (value, rest) = first_part_token(body, "b", TOKEN_PART_MAX).await.unwrap();
        assert_eq!(value, b"t0ken");
        assert_eq!(
            &to_bytes(rest, usize::MAX).await.unwrap()[..],
            part.as_bytes()
        );
    }

    #[tokio::test]
    async fn a_body_that_fails_or_ends_first_is_refused() {
        let first = format!("--b\r\n{TOKEN_HEAD}\r\n\r\nt0k");
        let failed: Result<Bytes, BoxError> = Err("connection reset".into());
        let body = streamed(
            vec![ok(first.as_bytes()), failed],
            futures_util::stream::empty(),
        );
        assert!(first_part_token(body, "b", TOKEN_PART_MAX).await.is_none());
        assert!(
            first_part_token(Body::from(first), "b", TOKEN_PART_MAX)
                .await
                .is_none()
        );
        assert!(
            first_part_token(Body::empty(), "b", TOKEN_PART_MAX)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn the_boundary_is_the_one_the_headers_name() {
        let body = form(&[(TOKEN_HEAD, "t0ken")]);
        assert!(
            first_part_token(Body::from(body.clone()), "c", TOKEN_PART_MAX)
                .await
                .is_none()
        );
        assert!(
            first_part_token(Body::from(body), "b", TOKEN_PART_MAX)
                .await
                .is_some()
        );
    }
}
