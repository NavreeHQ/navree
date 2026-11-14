//! Brotli for rendered pages and ops, at the quality that gives the most per
//! unit of CPU on the toolkit's own markup: quality 4 costs about what gzip's
//! default costs and is a third smaller on a table, because markup repeats
//! over long distances. Files are compressed once, at quality 11, ahead of time
//! (`assets`); this is for what is rendered per request. Below [`MIN_SIZE`],
//! brotli gained 2% for three times the CPU when measured, so small responses
//! stay with the HTTP pipeline's gzip unless the client takes only brotli.
//!
//! It follows the HTTP pipeline's rule ([`navree_http::may_compress`]): a
//! route with `no_compression` and a response with `Cache-Control:
//! no-transform` go out as they are, and with `http.compression = false`
//! nothing is compressed here either.

use std::io::Write as _;

use bytes::Bytes;
use http::header::{
    ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HeaderValue, VARY,
};
use http::{HeaderMap, Method};
use navree_http::body::{Body, read_body};
use navree_http::extract::Request;
use navree_http::response::Response;

use crate::assets::accepts;
use crate::op::OPS_CONTENT_TYPE;

/// The quality used per request. 5 makes a page of 100 rows about 8% smaller than 4 for
/// under a millisecond more, and one of 1,000 rows about 13% smaller for about 4 ms.
pub const QUALITY: u32 = 5;

/// The window, in bits: the smallest that holds the whole body, from 256 KiB to 4 MiB.
/// A window smaller than the body loses every repeat further back than it reaches (a
/// table of 1,000 rows came out 21% larger at 256 KiB), and one larger than the body only
/// costs memory.
fn window(len: usize) -> u32 {
    let bits = usize::BITS - len.saturating_add(16).saturating_sub(1).leading_zeros();
    bits.clamp(18, 22)
}

/// From this size a body goes out in brotli even if the client takes gzip.
pub const MIN_SIZE: usize = 32 * 1024;

/// Below this, a body isn't worth compressing at all.
const TINY: usize = 256;

/// Whether a body of `size` bytes goes out in brotli to a client that takes it:
/// from [`MIN_SIZE`], or from [`TINY`] when the client doesn't take gzip.
pub(crate) fn worth_brotli(size: usize, accept: &str) -> bool {
    accepts(accept, "br") && size >= TINY && (size >= MIN_SIZE || !accepts(accept, "gzip"))
}

/// `bytes` in brotli.
pub fn brotli(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() / 8 + 64);
    let mut writer =
        ::brotli::CompressorWriter::new(&mut out, 16 * 1024, QUALITY, window(bytes.len()));
    // Writing to a `Vec` can't fail.
    let _ = writer.write_all(bytes);
    drop(writer);
    out
}

/// Whether this response is one of the toolkit's, and the request takes brotli.
fn wanted(method: &Method, request: &str, headers: &HeaderMap) -> bool {
    if method == Method::HEAD || headers.contains_key(CONTENT_ENCODING) || !accepts(request, "br") {
        return false;
    }
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html") || value.starts_with(OPS_CONTENT_TYPE))
}

/// Compresses a rendered response when the client takes brotli. A response
/// with a body of unknown size is left to the pipeline, and so is one that
/// may not be compressed at all: its route opted out, or it says
/// `no-transform`.
pub(crate) async fn encode(method: &Method, accept: &str, response: Response) -> Response {
    if !navree_http::may_compress(&response) || !wanted(method, accept, response.headers()) {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let Some(size) = http_body::Body::size_hint(&body)
        .exact()
        .and_then(|size| usize::try_from(size).ok())
    else {
        return Response::from_parts(parts, body);
    };
    if !worth_brotli(size, accept) {
        return Response::from_parts(parts, body);
    }
    let bytes = match read_body(body, size).await {
        Ok(bytes) => bytes,
        Err(_) => return Response::from_parts(parts, Body::empty()),
    };
    parts.headers.remove(CONTENT_LENGTH);
    parts
        .headers
        .insert(CONTENT_ENCODING, HeaderValue::from_static("br"));
    parts
        .headers
        .append(VARY, HeaderValue::from_static("accept-encoding"));
    Response::from_parts(parts, Body::from(Bytes::from(brotli(&bytes))))
}

/// In a request's extensions when the app turned compression off
/// (`http.compression = false`): nothing the toolkit answers it is compressed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Off;

/// The request's `Accept-Encoding`, or nothing: when it has none, and when the
/// app turned compression off (`Off`), as if the client took no coding.
pub(crate) fn accept_of(request: &Request) -> String {
    if request.extensions().get::<Off>().is_some() {
        return String::new();
    }
    request
        .headers()
        .get(ACCEPT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_holds_the_whole_body_up_to_4_mib() {
        assert_eq!(window(0), 18);
        assert_eq!(window(200 * 1024), 18);
        assert_eq!(window((1 << 18) - 16), 18);
        assert_eq!(window((1 << 18) - 15), 19);
        assert_eq!(window(2_000_000), 21);
        assert_eq!(window(usize::MAX), 22);
    }

    /// A repeat further back than a small window reaches is still found: the second
    /// half of this body costs almost nothing.
    #[test]
    fn a_big_body_finds_repeats_from_its_start() {
        let mut seed: u64 = 7;
        let half: String = (0..300_000)
            .map(|_| {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                char::from(b'a' + u8::try_from((seed >> 33) % 26).unwrap_or(0))
            })
            .collect();
        let once = brotli(half.as_bytes()).len();
        let twice = brotli(format!("{half}{half}").as_bytes()).len();
        assert!(twice < once + once / 20, "{once} then {twice}");
    }

    #[test]
    fn brotli_shrinks_markup_and_reads_back() {
        let html = "<tr data-key=\"1\"><td><a href=\"/deals/1\">Agra</a></td></tr>".repeat(1000);
        let packed = brotli(html.as_bytes());
        assert!(packed.len() < html.len() / 10, "{}", packed.len());
        let mut back = Vec::new();
        ::brotli::BrotliDecompress(&mut packed.as_slice(), &mut back).unwrap();
        assert_eq!(back, html.as_bytes());
    }

    #[test]
    fn only_rendered_responses_that_the_client_takes() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
        assert!(wanted(&Method::GET, "gzip, br", &headers));
        assert!(!wanted(&Method::GET, "gzip", &headers));
        assert!(!wanted(&Method::GET, "br;q=0", &headers));
        assert!(
            !wanted(&Method::HEAD, "br", &headers),
            "a HEAD has no body to compress"
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        assert!(!wanted(&Method::GET, "br", &headers));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/html"));
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        assert!(!wanted(&Method::GET, "br", &headers));
    }
}
