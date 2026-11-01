//! Brotli for rendered pages and ops, at the quality that gives the most per
//! unit of CPU on the toolkit's own markup: quality 4 costs about what gzip's
//! default costs and is a third smaller on a table, because markup repeats
//! over long distances. Files are compressed once, at quality 11, ahead of time
//! (`assets`); this is for what is rendered per request. Below [`MIN_SIZE`],
//! brotli gained 2% for three times the CPU when measured, so small responses
//! stay with the HTTP pipeline's gzip unless the client takes only brotli.

use std::io::Write as _;

use bytes::Bytes;
use http::header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HeaderValue, VARY};
use http::{HeaderMap, Method};
use navree_http::body::{Body, read_body};
use navree_http::response::Response;

use crate::assets::accepts;
use crate::op::OPS_CONTENT_TYPE;

/// The quality used per request.
pub const QUALITY: u32 = 4;

/// The window, in bits: the pages are far smaller than the default's 4 MiB.
const WINDOW: u32 = 18;

/// From this size a body goes out in brotli even if the client takes gzip.
pub const MIN_SIZE: usize = 32 * 1024;

/// Below this, a body isn't worth compressing at all.
const TINY: usize = 256;

/// `bytes` in brotli.
pub fn brotli(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() / 8 + 64);
    let mut writer = ::brotli::CompressorWriter::new(&mut out, 16 * 1024, QUALITY, WINDOW);
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
/// with a body of unknown size is left to the pipeline.
pub(crate) async fn encode(method: &Method, accept: &str, response: Response) -> Response {
    if !wanted(method, accept, response.headers()) {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let Some(size) = http_body::Body::size_hint(&body).exact().and_then(|size| usize::try_from(size).ok()) else {
        return Response::from_parts(parts, body);
    };
    if size < TINY || (size < MIN_SIZE && accepts(accept, "gzip")) {
        return Response::from_parts(parts, body);
    }
    let bytes = match read_body(body, size).await {
        Ok(bytes) => bytes,
        Err(_) => return Response::from_parts(parts, Body::empty()),
    };
    parts.headers.remove(CONTENT_LENGTH);
    parts.headers.insert(CONTENT_ENCODING, HeaderValue::from_static("br"));
    parts.headers.append(VARY, HeaderValue::from_static("accept-encoding"));
    Response::from_parts(parts, Body::from(Bytes::from(brotli(&bytes))))
}

/// The value of `Accept-Encoding`, or nothing.
pub(crate) fn accept_of(headers: &HeaderMap) -> String {
    headers.get(ACCEPT_ENCODING).and_then(|value| value.to_str().ok()).unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brotli_shrinks_markup_and_reads_back() {
        let html = "<tr data-key=\"1\"><td><a href=\"/deals/1\">Acme</a></td></tr>".repeat(1000);
        let packed = brotli(html.as_bytes());
        assert!(packed.len() < html.len() / 10, "{}", packed.len());
        let mut back = Vec::new();
        ::brotli::BrotliDecompress(&mut packed.as_slice(), &mut back).unwrap();
        assert_eq!(back, html.as_bytes());
    }

    #[test]
    fn only_rendered_responses_that_the_client_takes() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
        assert!(wanted(&Method::GET, "gzip, br", &headers));
        assert!(!wanted(&Method::GET, "gzip", &headers));
        assert!(!wanted(&Method::GET, "br;q=0", &headers));
        assert!(!wanted(&Method::HEAD, "br", &headers), "a HEAD has no body to compress");
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        assert!(!wanted(&Method::GET, "br", &headers));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/html"));
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        assert!(!wanted(&Method::GET, "br", &headers));
    }
}
