//! What an honest client sends.

use http::{HeaderMap, HeaderValue, StatusCode};
use navree_http::extract::{FromRequest, Multipart, MultipartLimits};

use crate::support::*;

#[tokio::test]
async fn fields_and_files_are_read_with_their_headers() {
    let mut part = head(
        "Content-Disposition: form-data; name=\"page\"; filename=\"index.html\"\r\n\
         Content-Type: text/html; charset=utf-8\r\nX-Note: hello",
    );
    part.extend_from_slice(b"<title>x</title>\r\n");
    let body = form(&[text_part("title", b"Deals"), part]);
    let mut form = Multipart::from_request(request(&body)).await.unwrap();

    let field = form.next_field().await.unwrap().unwrap();
    assert_eq!(field.name(), Some("title"));
    assert_eq!(field.file_name(), None);
    assert_eq!(field.content_type(), None);
    assert_eq!(field.text().await.unwrap(), "Deals");

    let field = form.next_field().await.unwrap().unwrap();
    assert_eq!(field.name(), Some("page"));
    assert_eq!(field.file_name(), Some("index.html"));
    assert_eq!(field.content_type(), Some("text/html; charset=utf-8"));
    assert_eq!(field.headers()["x-note"], "hello");
    assert_eq!(&field.bytes().await.unwrap()[..], b"<title>x</title>");

    assert!(form.next_field().await.unwrap().is_none());
    assert!(form.next_field().await.unwrap().is_none(), "it stays ended");
}

#[tokio::test]
async fn a_field_declares_its_charset() {
    let mut part = head(
        "Content-Disposition: form-data; name=\"name\"\r\n\
         Content-Type: text/plain; charset=iso-8859-1",
    );
    part.extend_from_slice(b"caf\xe9\r\n");
    let mut multipart = Multipart::from_request(request(&form(&[part])))
        .await
        .unwrap();
    let field = multipart.next_field().await.unwrap().unwrap();
    assert_eq!(field.text().await.unwrap(), "caf\u{e9}");
}

#[tokio::test]
async fn invalid_utf8_is_replaced_in_text_and_kept_in_bytes() {
    let body = form(&[text_part("a", b"\xff"), text_part("b", b"\xff")]);
    let mut form = Multipart::from_request(request(&body)).await.unwrap();
    let field = form.next_field().await.unwrap().unwrap();
    assert_eq!(field.text().await.unwrap(), "\u{fffd}");
    let field = form.next_field().await.unwrap().unwrap();
    assert_eq!(&field.bytes().await.unwrap()[..], b"\xff");
}

#[tokio::test]
async fn a_field_is_also_a_stream() {
    use futures_util::StreamExt;

    let body = form(&[text_part("a", b"streamed")]);
    let mut form = Multipart::from_request(request(&body)).await.unwrap();
    let mut field = form.next_field().await.unwrap().unwrap();
    let mut read = Vec::new();
    while let Some(chunk) = field.next().await {
        read.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(read, b"streamed");
}

/// A handler may skip a part it does not want.
#[tokio::test]
async fn a_part_left_unread_is_skipped() {
    let body = form(&[
        file_part("big", "b.bin", &[7; 100_000]),
        text_part("after", b"kept"),
    ]);
    let mut form = Multipart::from_request(dripped(&body)).await.unwrap();
    {
        let field = form.next_field().await.unwrap().unwrap();
        assert_eq!(field.name(), Some("big"));
    }
    let field = form.next_field().await.unwrap().unwrap();
    assert_eq!(field.name(), Some("after"));
    assert_eq!(field.text().await.unwrap(), "kept");
    assert!(form.next_field().await.unwrap().is_none());
}

#[tokio::test]
async fn a_form_with_no_parts_is_empty() {
    let body = format!("--{BOUNDARY}--\r\n");
    assert_eq!(read(request(body.as_bytes()), wide()).await.unwrap(), []);
}

#[tokio::test]
async fn transport_padding_after_a_delimiter_is_allowed() {
    let body = format!(
        "--{BOUNDARY} \t \r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n--{BOUNDARY}--"
    );
    let read = read(dripped(body.as_bytes()), wide()).await.unwrap();
    assert_eq!(read[0].bytes, b"1");
}

/// The parts of a client that sends a preamble, as RFC 2046 allows.
#[tokio::test]
async fn a_short_preamble_is_skipped() {
    let mut body = b"This is a multipart message.\r\n".to_vec();
    body.extend_from_slice(&form(&[text_part("a", b"1")]));
    assert_eq!(read(request(&body), wide()).await.unwrap()[0].bytes, b"1");
}

#[tokio::test]
async fn the_content_type_is_read_without_regard_to_case_but_the_boundary_is_not() {
    for content_type in [
        format!("Multipart/Form-Data; boundary={BOUNDARY}"),
        format!("multipart/form-data; BOUNDARY={BOUNDARY}"),
        format!("multipart/form-data; charset=utf-8; boundary=\"{BOUNDARY}\""),
    ] {
        let body = form(&[text_part("a", b"1")]);
        let form = Multipart::from_request(request_of(&content_type, vec![body]))
            .await
            .unwrap_or_else(|_| panic!("{content_type}"));
        let parts = read_all(&mut form.with_limits(wide())).await.unwrap();
        assert_eq!(parts[0].bytes, b"1", "{content_type}");
    }
    let body = form(&[text_part("a", b"1")]);
    let content_type = format!("multipart/form-data; boundary={}", BOUNDARY.to_uppercase());
    let form = Multipart::from_request(request_of(&content_type, vec![body]))
        .await
        .unwrap();
    let error = read_all(&mut form.with_limits(wide())).await.unwrap_err();
    assert_eq!(error.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn optional_multipart_is_none_only_for_another_kind_of_body() {
    let extract = |headers: HeaderMap| async move {
        let mut request = request(b"");
        *request.headers_mut() = headers;
        <Option<Multipart> as FromRequest>::from_request(request).await
    };
    assert!(extract(HeaderMap::new()).await.unwrap().is_none());

    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    assert!(extract(headers).await.unwrap().is_none());

    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        HeaderValue::from_static("multipart/form-data; boundary=x"),
    );
    assert!(extract(headers).await.unwrap().is_some());

    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        HeaderValue::from_static("multipart/form-data"),
    );
    let rejection = extract(headers).await.unwrap_err();
    assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn the_default_limits_are_the_documented_ones() {
    assert_eq!(
        MultipartLimits::default(),
        MultipartLimits {
            max_parts: 1000,
            max_field_bytes: 1024 * 1024,
            max_file_bytes: None,
            max_header_bytes: 16 * 1024,
        }
    );
}
