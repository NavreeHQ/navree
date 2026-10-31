//! What the comparisons share.

use std::io::Read as _;

use bytes::Bytes;
use http::header::CONTENT_ENCODING;
use http::{HeaderName, HeaderValue, StatusCode};
use http_body_util::BodyExt;
use navree_http::response::Response;

/// All of a response a client or an outer layer can see: its status, its
/// headers in order, its data frames one by one and how the body ended.
#[derive(Debug, PartialEq, Eq)]
pub struct Seen {
    pub status: StatusCode,
    pub headers: Vec<(HeaderName, HeaderValue)>,
    pub frames: Vec<Bytes>,
    pub error: Option<String>,
    pub size_hint: (u64, Option<u64>),
}

/// Reads a whole response without gluing its frames together.
pub async fn seen<B>(response: http::Response<B>) -> Seen
where
    B: http_body::Body<Data = Bytes> + Send + Unpin + 'static,
    B::Error: std::fmt::Display,
{
    let (parts, mut body) = response.into_parts();
    let hint = http_body::Body::size_hint(&body);
    let mut frames = Vec::new();
    let mut error = None;
    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => frames.extend(frame.into_data().ok()),
            Err(failure) => {
                error = Some(failure.to_string());
                break;
            }
        }
    }
    Seen {
        status: parts.status,
        headers: parts
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
        frames,
        error,
        size_hint: (hint.lower(), hint.upper()),
    }
}

/// [`seen`] of a response of Navree's.
pub async fn ours(response: Response) -> Seen {
    seen(response).await
}

impl Seen {
    /// Everything [`seen`] read, on one line: the status, the headers in
    /// order (`name: value`, joined by ` / `), the data frames (escaped, one
    /// by one), the size hint, and how the body ended when it failed.
    pub fn line(&self) -> String {
        let headers: Vec<String> = self
            .headers
            .iter()
            .map(|(name, value)| format!("{name}: {}", value.as_bytes().escape_ascii()))
            .collect();
        let frames: Vec<String> = self
            .frames
            .iter()
            .map(|frame| format!("\"{}\"", frame.escape_ascii()))
            .collect();
        let (lower, upper) = self.size_hint;
        let upper = upper.map_or(String::new(), |upper| upper.to_string());
        let mut line = format!(
            "{} | {} | [{}] | {lower}..{upper}",
            self.status.as_u16(),
            headers.join(" / "),
            frames.join(", ")
        );
        if let Some(error) = &self.error {
            line.push_str(&format!(" | error: {error}"));
        }
        line
    }
}

impl Seen {
    /// The response with a gzip body read as the text it holds, in one
    /// frame: the compressor's own output is its business, what a client
    /// reads from it is not.
    pub fn gunzipped(mut self) -> Seen {
        let gzip = self
            .headers
            .iter()
            .any(|(name, value)| name == CONTENT_ENCODING && value == "gzip");
        if gzip {
            let mut text = Vec::new();
            // A body that failed part-way ends mid-stream: what came out
            // before is what a client reads.
            let read = flate2::read::GzDecoder::new(self.frames.concat().as_slice())
                .read_to_end(&mut text);
            assert!(
                read.is_ok() || self.error.is_some(),
                "a gzip body: {read:?}"
            );
            self.frames = vec![Bytes::from(text)];
        }
        self
    }
}

/// Checks `actual` against the file `golden/{name}`, which holds what the
/// responses looked like when they were last read by eye. With
/// `NAVREE_BLESS=1` the file is written instead (look at the diff).
pub fn golden(name: &str, actual: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/responses/golden")
        .join(name);
    if std::env::var_os("NAVREE_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("a directory");
        std::fs::write(&path, actual).expect("the golden file is written");
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error} (NAVREE_BLESS=1 writes it)", path.display()));
    // Git may check the file out with CRLF line ends.
    let expected = expected.replace("\r\n", "\n");
    if expected != actual {
        let differing = expected
            .lines()
            .zip(actual.lines())
            .position(|(expected, actual)| expected != actual)
            .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()));
        panic!(
            "{name} differs at line {}:\n expected: {}\n   actual: {}",
            differing + 1,
            expected.lines().nth(differing).unwrap_or("(the end)"),
            actual.lines().nth(differing).unwrap_or("(the end)"),
        );
    }
}

/// One thread's runtime for a property test.
pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .expect("a runtime")
}
