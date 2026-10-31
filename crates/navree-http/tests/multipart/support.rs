//! Requests with a multipart body, and reading a form to its end.

#![allow(dead_code)]

use bytes::Bytes;
use http::Request;
use navree_http::body::{Body, BodyLimit};
use navree_http::extract::{FromRequest, Multipart, MultipartError, MultipartLimits};

pub const BOUNDARY: &str = "navree-boundary";

/// Limits that stay out of the way: each test sets the one it is about.
pub fn wide() -> MultipartLimits {
    MultipartLimits {
        max_parts: usize::MAX,
        max_field_bytes: u64::MAX,
        max_file_bytes: None,
        max_header_bytes: usize::MAX,
    }
}

/// `content_type`, in one header, and `body` in `chunks`.
pub fn request_of(content_type: &str, chunks: Vec<Vec<u8>>) -> Request<Body> {
    let stream = futures_util::stream::iter(
        chunks
            .into_iter()
            .map(|chunk| Ok::<_, std::io::Error>(Bytes::from(chunk))),
    );
    let mut request = Request::builder()
        .method("POST")
        .uri("/")
        .header("content-type", content_type)
        .body(Body::from_stream(stream))
        .unwrap();
    request.extensions_mut().insert(BodyLimit(64 * 1024 * 1024));
    request
}

/// The form type of [`BOUNDARY`].
pub fn form_type() -> String {
    format!("multipart/form-data; boundary={BOUNDARY}")
}

/// `body` as one chunk.
pub fn request(body: &[u8]) -> Request<Body> {
    request_of(&form_type(), vec![body.to_vec()])
}

/// `body` one byte at a time.
pub fn dripped(body: &[u8]) -> Request<Body> {
    request_of(
        &form_type(),
        body.iter().map(|byte| vec![*byte]).collect::<Vec<_>>(),
    )
}

/// A part's head, with its delimiter.
pub fn head(headers: &str) -> Vec<u8> {
    format!("--{BOUNDARY}\r\n{headers}\r\n\r\n").into_bytes()
}

/// A text part.
pub fn text_part(name: &str, value: &[u8]) -> Vec<u8> {
    let mut part = head(&format!("Content-Disposition: form-data; name=\"{name}\""));
    part.extend_from_slice(value);
    part.extend_from_slice(b"\r\n");
    part
}

/// A file part.
pub fn file_part(name: &str, file_name: &str, value: &[u8]) -> Vec<u8> {
    let mut part = head(&format!(
        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{file_name}\"\r\n\
         Content-Type: application/octet-stream"
    ));
    part.extend_from_slice(value);
    part.extend_from_slice(b"\r\n");
    part
}

/// `parts`, closed.
pub fn form(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut body = parts.concat();
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

/// A part as a handler read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read {
    pub name: Option<String>,
    pub file_name: Option<String>,
    pub bytes: Vec<u8>,
}

/// Reads every part, draining each.
pub async fn read_all(form: &mut Multipart) -> Result<Vec<Read>, MultipartError> {
    let mut parts = Vec::new();
    while let Some(mut field) = form.next_field().await? {
        let name = field.name().map(str::to_owned);
        let file_name = field.file_name().map(str::to_owned);
        let mut bytes = Vec::new();
        while let Some(chunk) = field.chunk().await? {
            bytes.extend_from_slice(&chunk);
        }
        parts.push(Read {
            name,
            file_name,
            bytes,
        });
    }
    Ok(parts)
}

/// Reads `request` with `limits`.
pub async fn read(
    request: Request<Body>,
    limits: MultipartLimits,
) -> Result<Vec<Read>, MultipartError> {
    let form = Multipart::from_request(request)
        .await
        .expect("a multipart request");
    read_all(&mut form.with_limits(limits)).await
}
