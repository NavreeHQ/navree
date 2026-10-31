//! `navree_http`'s `Multipart` on untrusted input: boundaries, delimiters,
//! part heads and values, and the limits in front of the parser. The input
//! is a `Content-Type` value, a NUL, then the body. The first byte of the
//! body sets the size of its chunks, so a target run covers every way a
//! pattern can be cut.
//!
//! Invariants:
//! - nothing panics, and every failure is a 4xx (a client's input never makes
//!   a 500);
//! - whether a form is accepted, and what it holds, does not depend on how
//!   the body was split into chunks;
//! - with limits that cannot apply, the extractor reads what `multer` reads
//!   alone;
//! - accepted values keep to their limits, and no more parts come back than
//!   `max_parts`.
#![no_main]

use bytes::Bytes;
use http::Request;
use libfuzzer_sys::fuzz_target;
use navree_fuzz::{block_on, split_nul};
use navree_http::body::{Body, BodyLimit};
use navree_http::extract::{FromRequest, Multipart, MultipartError, MultipartLimits};

const WIDE: MultipartLimits = MultipartLimits {
    max_parts: usize::MAX,
    max_field_bytes: u64::MAX,
    max_file_bytes: None,
    max_header_bytes: usize::MAX,
};

const TIGHT: MultipartLimits = MultipartLimits {
    max_parts: 4,
    max_field_bytes: 32,
    max_file_bytes: Some(64),
    max_header_bytes: 128,
};

#[derive(Debug, PartialEq)]
struct Part {
    name: Option<String>,
    file_name: Option<String>,
    value: Vec<u8>,
}

fn request(content_type: &str, body: &[u8], chunk: usize) -> Option<Request<Body>> {
    let chunks: Vec<Result<Bytes, std::io::Error>> = body
        .chunks(chunk.max(1))
        .map(|piece| Ok(Bytes::copy_from_slice(piece)))
        .collect();
    let mut request = Request::builder()
        .method("POST")
        .uri("/")
        .header("content-type", content_type)
        .body(Body::from_stream(futures::stream::iter(chunks)))
        .ok()?;
    request.extensions_mut().insert(BodyLimit(1024 * 1024));
    Some(request)
}

async fn drain(form: &mut Multipart) -> Result<Vec<Part>, MultipartError> {
    let mut parts = Vec::new();
    while let Some(mut field) = form.next_field().await? {
        let name = field.name().map(str::to_owned);
        let file_name = field.file_name().map(str::to_owned);
        let mut value = Vec::new();
        while let Some(chunk) = field.chunk().await? {
            value.extend_from_slice(&chunk);
        }
        parts.push(Part {
            name,
            file_name,
            value,
        });
    }
    Ok(parts)
}

/// Reads a body with `limits`; `None` when the request is refused outright.
fn read(
    content_type: &str,
    body: &[u8],
    chunk: usize,
    limits: MultipartLimits,
) -> Option<Result<Vec<Part>, MultipartError>> {
    let request = request(content_type, body, chunk)?;
    block_on(async {
        match Multipart::from_request(request).await {
            Ok(form) => Some(drain(&mut form.with_limits(limits)).await),
            Err(rejection) => {
                assert_eq!(rejection.status(), http::StatusCode::BAD_REQUEST);
                None
            }
        }
    })
}

/// `multer` on its own, with the first boundary of `content_type`.
fn plain(boundary: &str, body: &[u8]) -> Option<Vec<Part>> {
    let stream = futures::stream::iter([Ok::<_, std::io::Error>(Bytes::copy_from_slice(body))]);
    let mut form = multer::Multipart::new(stream, boundary);
    block_on(async {
        let mut parts = Vec::new();
        while let Some(mut field) = form.next_field().await.ok()? {
            let name = field.name().map(str::to_owned);
            let file_name = field.file_name().map(str::to_owned);
            let mut value = Vec::new();
            while let Some(chunk) = field.chunk().await.ok()? {
                value.extend_from_slice(&chunk);
            }
            parts.push(Part {
                name,
                file_name,
                value,
            });
        }
        Some(parts)
    })
}

fn accepted(read: Option<Result<Vec<Part>, MultipartError>>) -> Option<Option<Vec<Part>>> {
    let read = read?;
    if let Err(error) = &read {
        assert!(
            error.status().is_client_error(),
            "a failure on client input is a 4xx, got {error:?}"
        );
    }
    Some(read.ok())
}

fuzz_target!(|data: &[u8]| {
    let (content_type, body) = split_nul(data);
    let content_type = String::from_utf8_lossy(content_type);
    let (chunk, body) = match body.split_first() {
        Some((chunk, body)) => (usize::from(*chunk), body),
        None => (1, body),
    };

    for limits in [WIDE, TIGHT] {
        let whole = accepted(read(&content_type, body, body.len(), limits));
        let split = accepted(read(&content_type, body, chunk, limits));
        assert_eq!(whole, split, "the chunks changed the outcome");
        if let Some(Some(parts)) = &whole
            && limits == TIGHT
        {
            assert!(parts.len() <= limits.max_parts);
            for part in parts {
                let limit = match part.file_name {
                    Some(_) => 64,
                    None => 32,
                };
                assert!(part.value.len() <= limit);
            }
        }
    }

    // The form's boundary is whatever the extractor took.
    if let Some(Some(parts)) = accepted(read(&content_type, body, body.len(), WIDE))
        && let Some(boundary) = content_type
            .parse::<mime::Mime>()
            .ok()
            .and_then(|media_type| {
                media_type
                    .get_param(mime::BOUNDARY)
                    .map(|name| name.as_str().to_owned())
            })
    {
        assert_eq!(Some(parts), plain(&boundary, body));
    }
});
