//! A client that wants the parser to spend, or to misread.

use std::future::poll_fn;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use http::{Request, StatusCode};
use navree_http::body::{Body, BodyError, BodyLimit};
use navree_http::extract::{FromRequest, Multipart, MultipartError, MultipartLimits};
use navree_http::serve::BodyReadTimeout;

use crate::support::*;

/// Extraction alone fails, before a byte of the body is read.
async fn refused(content_type: &str) {
    let polls = Arc::new(AtomicUsize::new(0));
    let counter = polls.clone();
    let body = futures_util::stream::poll_fn(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        Poll::Ready(None::<Result<Bytes, std::io::Error>>)
    });
    let request = Request::builder()
        .header("content-type", content_type)
        .body(Body::from_stream(body))
        .unwrap();
    let rejection = Multipart::from_request(request).await.unwrap_err();
    assert_eq!(
        rejection.status(),
        StatusCode::BAD_REQUEST,
        "{content_type}"
    );
    assert_eq!(
        rejection.body_text(),
        "Invalid `boundary` for `multipart/form-data` request",
        "{content_type}"
    );
    assert_eq!(polls.load(Ordering::SeqCst), 0, "{content_type}");
}

#[tokio::test]
async fn a_form_without_a_usable_boundary_is_refused() {
    refused("multipart/form-data").await;
    refused("multipart/form-data; charset=utf-8").await;
    refused("multipart/form-data; boundary=").await;
    refused("multipart/form-data; boundary=\"\"").await;
    refused("multipart/mixed; boundary=x").await;
    refused("application/x-www-form-urlencoded").await;
    refused("not a media type").await;
}

/// A delimiter may be 70 characters (RFC 2046), and every search for one is
/// paid for by the server.
#[tokio::test]
async fn a_boundary_longer_than_the_rfc_allows_is_refused() {
    let longest = "x".repeat(70);
    let content_type = format!("multipart/form-data; boundary={longest}");
    let body = format!(
        "--{longest}\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n--{longest}--"
    );
    let mut form = Multipart::from_request(request_of(&content_type, vec![body.into_bytes()]))
        .await
        .unwrap();
    assert_eq!(read_all(&mut form).await.unwrap()[0].bytes, b"1");

    refused(&format!("multipart/form-data; boundary={longest}x")).await;
}

/// Two parsers that pick different boundaries see different forms: one in
/// front of the app must not be fooled into checking the wrong one.
#[tokio::test]
async fn a_second_boundary_parameter_is_refused() {
    refused("multipart/form-data; boundary=a; boundary=b").await;
}

#[tokio::test]
async fn two_content_types_name_no_boundary() {
    let request = Request::builder()
        .header("content-type", "multipart/form-data; boundary=a")
        .header("content-type", "multipart/form-data; boundary=b")
        .body(Body::empty())
        .unwrap();
    let rejection = Multipart::from_request(request).await.unwrap_err();
    assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_request_with_no_content_type_is_refused() {
    let request = Request::new(Body::empty());
    let rejection = Multipart::from_request(request).await.unwrap_err();
    assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn too_many_parts_are_refused_at_the_first_past_the_limit() {
    let parts: Vec<_> = (0..4).map(|_| text_part("a", b"1")).collect();
    let limits = MultipartLimits {
        max_parts: 4,
        ..wide()
    };
    assert_eq!(read(request(&form(&parts)), limits).await.unwrap().len(), 4);

    let limits = MultipartLimits {
        max_parts: 3,
        ..wide()
    };
    let error = read(request(&form(&parts)), limits).await.unwrap_err();
    assert_eq!(error, MultipartError::TooManyParts { limit: 3 });
    assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

/// Parts a handler skips still count: the loop is the cost.
#[tokio::test]
async fn skipped_parts_count_towards_the_limit() {
    let parts: Vec<_> = (0..4).map(|_| text_part("a", b"1")).collect();
    let form_body = form(&parts);
    let multipart = Multipart::from_request(request(&form_body)).await.unwrap();
    let mut multipart = multipart.with_limits(MultipartLimits {
        max_parts: 2,
        ..wide()
    });
    let mut seen = 0;
    let error = loop {
        match multipart.next_field().await {
            Ok(Some(_)) => seen += 1,
            Ok(None) => panic!("the limit never applied"),
            Err(error) => break error,
        }
    };
    assert_eq!(seen, 2);
    assert_eq!(error, MultipartError::TooManyParts { limit: 2 });
}

#[tokio::test]
async fn the_default_limit_on_parts_holds() {
    let parts: Vec<_> = (0..1001).map(|_| text_part("a", b"")).collect();
    let body = form(&parts);
    let error = read(request(&body), MultipartLimits::default())
        .await
        .unwrap_err();
    assert_eq!(error, MultipartError::TooManyParts { limit: 1000 });
}

#[tokio::test]
async fn an_oversized_field_is_refused_however_it_is_read() {
    let limits = MultipartLimits {
        max_field_bytes: 8,
        ..wide()
    };
    let fits = form(&[text_part("a", b"12345678")]);
    assert_eq!(
        read(request(&fits), limits).await.unwrap()[0].bytes.len(),
        8
    );
    assert_eq!(
        read(dripped(&fits), limits).await.unwrap()[0].bytes.len(),
        8
    );

    let over = form(&[text_part("a", b"123456789")]);
    let expected = MultipartError::FieldTooLarge { limit: 8 };
    assert_eq!(read(request(&over), limits).await.unwrap_err(), expected);
    assert_eq!(read(dripped(&over), limits).await.unwrap_err(), expected);
    assert_eq!(expected.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let start = || async {
        Multipart::from_request(request(&over))
            .await
            .unwrap()
            .with_limits(limits)
    };
    let mut form = start().await;
    let field = form.next_field().await.unwrap().unwrap();
    assert_eq!(field.bytes().await.unwrap_err(), expected);

    let mut form = start().await;
    let field = form.next_field().await.unwrap().unwrap();
    assert_eq!(field.text().await.unwrap_err(), expected);

    let mut form = start().await;
    let mut field = form.next_field().await.unwrap().unwrap();
    // A loop that does not stop at an error still ends.
    let mut failed = None;
    let mut items = 0;
    while let Some(chunk) = field.next().await {
        items += 1;
        assert!(items < 16, "the stream goes on after its failure");
        if let Err(error) = chunk {
            failed = Some(error);
        }
    }
    assert_eq!(failed, Some(expected));
}

#[tokio::test]
async fn the_field_limit_is_a_mebibyte_by_default_and_files_answer_to_the_body_limit() {
    let megabyte = vec![b'x'; 1024 * 1024];
    let mut over = megabyte.clone();
    over.push(b'x');
    let limits = MultipartLimits::default();
    assert!(
        read(request(&form(&[text_part("a", &megabyte)])), limits)
            .await
            .is_ok()
    );
    assert_eq!(
        read(request(&form(&[text_part("a", &over)])), limits)
            .await
            .unwrap_err(),
        MultipartError::FieldTooLarge { limit: 1024 * 1024 }
    );
    assert!(
        read(request(&form(&[file_part("a", "a.bin", &over)])), limits)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn an_oversized_file_is_refused() {
    let limits = MultipartLimits {
        max_file_bytes: Some(8),
        ..wide()
    };
    let fits = form(&[file_part("a", "a.bin", b"12345678")]);
    assert_eq!(
        read(dripped(&fits), limits).await.unwrap()[0].bytes.len(),
        8
    );

    let over = form(&[file_part("a", "a.bin", b"123456789")]);
    let error = read(request(&over), limits).await.unwrap_err();
    assert_eq!(error, MultipartError::FileTooLarge { limit: 8 });
    assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

/// The two limits are chosen by the file name a client sends, so `text`,
/// which holds the whole value in memory, keeps to the field limit whatever
/// the part calls itself.
#[tokio::test]
async fn a_file_name_does_not_lift_the_field_limit_from_text() {
    let limits = MultipartLimits {
        max_field_bytes: 8,
        ..wide()
    };
    let body = form(&[file_part("a", "a.txt", b"123456789")]);
    let mut form = Multipart::from_request(request(&body))
        .await
        .unwrap()
        .with_limits(limits);
    let field = form.next_field().await.unwrap().unwrap();
    assert_eq!(
        field.text().await.unwrap_err(),
        MultipartError::FieldTooLarge { limit: 8 }
    );
}

fn with_head_of(bytes: usize) -> Vec<u8> {
    let prefix = "Content-Disposition: form-data; name=\"a\"\r\nX-Pad: ";
    let padding = bytes - prefix.len() - "\r\n\r\n".len();
    let headers = format!("{prefix}{}", "p".repeat(padding));
    let mut part = head(&headers);
    part.extend_from_slice(b"1\r\n");
    form(&[part])
}

#[tokio::test]
async fn a_head_is_refused_one_byte_past_the_limit_however_the_body_is_split() {
    let limits = MultipartLimits {
        max_header_bytes: 512,
        ..wide()
    };
    let fits = with_head_of(512);
    assert_eq!(read(request(&fits), limits).await.unwrap()[0].bytes, b"1");
    assert_eq!(read(dripped(&fits), limits).await.unwrap()[0].bytes, b"1");

    let over = with_head_of(513);
    let expected = MultipartError::HeadTooLarge { limit: 512 };
    assert_eq!(read(request(&over), limits).await.unwrap_err(), expected);
    assert_eq!(read(dripped(&over), limits).await.unwrap_err(), expected);
    assert_eq!(expected.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

/// The attack the limit is for: a head that never ends is kept in memory and
/// searched again from its start with every chunk, so its cost grows with
/// the square of its length. The body stops arriving as soon as the limit
/// passes.
#[tokio::test]
async fn a_head_that_never_ends_stops_the_reading_at_the_limit() {
    let served = Arc::new(AtomicUsize::new(0));
    let counter = served.clone();
    let endless = std::iter::repeat_with(|| b"X-Pad: 12345678901234567890\r\n".to_vec());
    // Long enough to show the limit, and short enough to end if it breaks.
    let lines = std::iter::once(head("X-First: 1")).chain(endless.take(300_000));
    let chunks = futures_util::stream::iter(lines).map(move |mut chunk| {
        // The first chunk's blank line would end the head.
        if chunk.ends_with(b"\r\n\r\n") {
            chunk.truncate(chunk.len() - 2);
        }
        counter.fetch_add(chunk.len(), Ordering::SeqCst);
        Ok::<_, std::io::Error>(Bytes::from(chunk))
    });
    let request = Request::builder()
        .header("content-type", form_type())
        .body(Body::from_stream(chunks))
        .unwrap();
    let error = read(request, MultipartLimits::default()).await.unwrap_err();
    assert_eq!(error, MultipartError::HeadTooLarge { limit: 16 * 1024 });
    assert!(served.load(Ordering::SeqCst) < 17 * 1024, "{served:?}");
}

#[tokio::test]
async fn a_preamble_answers_to_the_head_limit_and_a_delimiter_may_end_exactly_on_it() {
    let limits = MultipartLimits {
        max_header_bytes: 64,
        ..wide()
    };
    let with_preamble = |bytes: usize| {
        // The line end that lets the delimiter start a line is part of it.
        let mut body = vec![b'.'; bytes - 2];
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(&form(&[text_part("a", b"1")]));
        body
    };
    let fits = with_preamble(64);
    assert_eq!(read(request(&fits), limits).await.unwrap()[0].bytes, b"1");
    assert_eq!(read(dripped(&fits), limits).await.unwrap()[0].bytes, b"1");

    let over = with_preamble(65);
    let expected = MultipartError::HeadTooLarge { limit: 64 };
    assert_eq!(read(request(&over), limits).await.unwrap_err(), expected);
    assert_eq!(read(dripped(&over), limits).await.unwrap_err(), expected);
}

#[tokio::test]
async fn a_body_that_is_all_preamble_stops_at_the_limit() {
    let served = Arc::new(AtomicUsize::new(0));
    let counter = served.clone();
    let chunks = futures_util::stream::repeat_with(move || {
        counter.fetch_add(1024, Ordering::SeqCst);
        Ok::<_, std::io::Error>(Bytes::from_static(&[b'.'; 1024]))
    })
    .take(8192);
    let request = Request::builder()
        .header("content-type", form_type())
        .body(Body::from_stream(chunks))
        .unwrap();
    let error = read(request, MultipartLimits::default()).await.unwrap_err();
    assert_eq!(error, MultipartError::HeadTooLarge { limit: 16 * 1024 });
    assert!(served.load(Ordering::SeqCst) < 18 * 1024, "{served:?}");
}

#[tokio::test]
async fn transport_padding_answers_to_the_head_limit() {
    let limits = MultipartLimits {
        max_header_bytes: 16,
        ..wide()
    };
    let body = format!(
        "--{BOUNDARY}{}\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n--{BOUNDARY}--",
        " ".repeat(64)
    );
    let expected = MultipartError::HeadTooLarge { limit: 16 };
    let error = read(request(body.as_bytes()), limits).await.unwrap_err();
    assert_eq!(error, expected);
    let error = read(dripped(body.as_bytes()), limits).await.unwrap_err();
    assert_eq!(error, expected);
}

/// A blank line directly after the delimiter line is not the end of the
/// head, for the parser or for the limit: `--b\r\n\r\n` then junk with no
/// blank line must not pass as a head with no headers.
#[tokio::test]
async fn a_blank_line_at_the_start_of_a_head_does_not_end_it() {
    let limits = MultipartLimits {
        max_header_bytes: 64,
        ..wide()
    };
    let mut body = format!("--{BOUNDARY}\r\n").into_bytes();
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(&[b'j'; 4096]);
    let error = read(request(&body), limits).await.unwrap_err();
    assert_eq!(error, MultipartError::HeadTooLarge { limit: 64 });
}

#[tokio::test]
async fn too_many_headers_in_a_part_are_refused() {
    let headers = (0..33)
        .map(|index| format!("X-Header-{index}: 1"))
        .collect::<Vec<_>>()
        .join("\r\n");
    let error = read(request(&form(&[head(&headers)])), wide())
        .await
        .unwrap_err();
    assert_eq!(error.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_header_without_a_name_is_refused() {
    for line in [
        ": form-data; name=\"a\"",
        " : value",
        "Content-Disposition",
        "Bad Name: value",
        "\u{e9}: value",
    ] {
        let mut part = head(line);
        part.extend_from_slice(b"1\r\n");
        let error = read(request(&form(&[part])), wide()).await.unwrap_err();
        assert_eq!(error.status(), StatusCode::BAD_REQUEST, "{line}");
        assert!(matches!(error, MultipartError::Malformed(_)), "{line}");
    }
}

/// RFC 7578 wants a `name`. A part without one is the handler's to refuse:
/// it reads as `None`, never as a name made up by the parser.
#[tokio::test]
async fn a_part_without_a_field_name_has_no_name() {
    for headers in [
        "Content-Disposition: form-data",
        "Content-Disposition: form-data; filename=\"a.txt\"",
        "Content-Type: text/plain",
    ] {
        let mut part = head(headers);
        part.extend_from_slice(b"1\r\n");
        let parts = read(request(&form(&[part])), wide()).await.unwrap();
        assert_eq!(parts[0].name, None, "{headers}");
    }
}

/// A body cut off anywhere is an error, never a shorter form: every prefix
/// of a form, and however it was split.
#[tokio::test]
async fn a_body_cut_off_anywhere_is_refused() {
    let body = form(&[text_part("a", b"12345"), file_part("b", "b.txt", b"67890")]);
    let closing_at = body.len() - "--\r\n".len();
    for end in 0..closing_at {
        let error = read(request(&body[..end]), wide())
            .await
            .expect_err(&format!("cut at {end}"));
        assert_eq!(error.status(), StatusCode::BAD_REQUEST, "cut at {end}");
        let error = read(dripped(&body[..end]), wide())
            .await
            .expect_err(&format!("cut at {end}, dripped"));
        assert_eq!(error.status(), StatusCode::BAD_REQUEST, "cut at {end}");
    }
}

/// A file name is whatever the client typed: it comes back as it was sent
/// and nothing in the framework reads it as a path.
#[tokio::test]
async fn a_file_name_with_a_path_comes_back_as_sent() {
    for file_name in [
        "../../etc/passwd",
        "..\\..\\windows\\system32\\cmd.exe",
        "/etc/passwd",
        "C:\\fakepath\\a.png",
        "%2e%2e%2fsecret",
        "a/../../b",
        ".",
        "..",
        "dir/",
    ] {
        let body = form(&[file_part("a", file_name, b"1")]);
        let parts = read(request(&body), wide()).await.unwrap();
        assert_eq!(parts[0].file_name.as_deref(), Some(file_name));
        assert_eq!(parts[0].bytes, b"1");
    }
}

#[tokio::test]
async fn a_file_name_that_is_not_text_reads_as_none() {
    let mut part =
        format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"a\"; filename=\"")
            .into_bytes();
    part.extend_from_slice(b"\xff\xfe\"\r\n\r\n1\r\n");
    let parts = read(request(&form(&[part])), wide()).await.unwrap();
    assert_eq!(parts[0].file_name, None);
}

#[tokio::test]
async fn the_route_body_limit_holds_without_a_content_length() {
    let body = form(&[file_part("a", "a.bin", &[7; 4096])]);
    let chunks: Vec<_> = body.chunks(256).map(<[u8]>::to_vec).collect();
    let mut request = request_of(&form_type(), chunks);
    request.extensions_mut().insert(BodyLimit(1024));
    let error = read(request, wide()).await.unwrap_err();
    assert_eq!(error, MultipartError::Body(BodyError::TooLarge));
    assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(error.body_text(), "Request payload is too large");

    let mut request = request_of(&form_type(), vec![body.clone()]);
    request.extensions_mut().insert(BodyLimit(body.len()));
    assert_eq!(read(request, wide()).await.unwrap()[0].bytes.len(), 4096);
}

/// A route outside the pipeline has no `BodyLimit` in its extensions: the
/// default still bounds it.
#[tokio::test]
async fn without_a_body_limit_the_default_applies() {
    let body = form(&[file_part("a", "a.bin", &vec![7; 2 * 1024 * 1024])]);
    let request = Request::builder()
        .header("content-type", form_type())
        .body(Body::from(body))
        .unwrap();
    let error = read(request, wide()).await.unwrap_err();
    assert_eq!(error, MultipartError::Body(BodyError::TooLarge));
}

/// The body limit stops an unread part too: skipping it reads it.
#[tokio::test]
async fn skipping_a_part_stays_inside_the_body_limit() {
    let body = form(&[file_part("a", "a.bin", &[7; 4096]), text_part("b", b"1")]);
    let chunks: Vec<_> = body.chunks(256).map(<[u8]>::to_vec).collect();
    let mut request = request_of(&form_type(), chunks);
    request.extensions_mut().insert(BodyLimit(1024));
    let mut form = Multipart::from_request(request).await.unwrap();
    let error = loop {
        match form.next_field().await {
            Ok(Some(_)) => {}
            Ok(None) => panic!("the whole body was read"),
            Err(error) => break error,
        }
    };
    assert_eq!(error, MultipartError::Body(BodyError::TooLarge));
}

/// The server's pace on a body arrives as an error of the body.
#[tokio::test]
async fn a_body_too_slow_for_the_server_is_a_408() {
    let start = form(&[text_part("a", b"1")]);
    let chunks = futures_util::stream::iter([
        Ok::<_, BodyReadTimeout>(Bytes::copy_from_slice(&start[..20])),
        Err(BodyReadTimeout),
    ]);
    let request = Request::builder()
        .header("content-type", form_type())
        .body(Body::from_stream(chunks))
        .unwrap();
    let error = read(request, wide()).await.unwrap_err();
    assert_eq!(error, MultipartError::Body(BodyError::TimedOut));
    assert_eq!(error.status(), StatusCode::REQUEST_TIMEOUT);
}

#[tokio::test]
async fn a_body_that_fails_is_a_400() {
    let chunks = futures_util::stream::iter([Err::<Bytes, _>(std::io::Error::other("reset"))]);
    let request = Request::builder()
        .header("content-type", form_type())
        .body(Body::from_stream(chunks))
        .unwrap();
    let error = read(request, wide()).await.unwrap_err();
    assert!(
        matches!(error, MultipartError::Body(BodyError::Read(_))),
        "{error:?}"
    );
    assert_eq!(error.status(), StatusCode::BAD_REQUEST);
}

/// A failure is final: a handler that calls again gets the failure, never
/// the end of a form that was not finished, and a later part never appears.
#[tokio::test]
async fn a_failure_stays() {
    let body = form(&[text_part("a", b"123456789"), text_part("b", b"1")]);
    let mut form = Multipart::from_request(request(&body))
        .await
        .unwrap()
        .with_limits(MultipartLimits {
            max_field_bytes: 8,
            ..wide()
        });
    let expected = MultipartError::FieldTooLarge { limit: 8 };
    let mut field = form.next_field().await.unwrap().unwrap();
    assert_eq!(field.chunk().await.unwrap_err(), expected);
    assert_eq!(field.chunk().await.unwrap_err(), expected);
    assert_eq!(form.next_field().await.unwrap_err(), expected);
    assert_eq!(form.next_field().await.unwrap_err(), expected);

    let cut = text_part("a", b"1");
    let mut form = Multipart::from_request(request(&cut)).await.unwrap();
    let field = form.next_field().await.unwrap().unwrap();
    let error = field.bytes().await.unwrap_err();
    assert_eq!(form.next_field().await.unwrap_err(), error);
}

/// A stream that is not ready on its first poll, then has the whole body and
/// its end at once: the shape of a small form on a loaded server.
struct Late {
    polls: usize,
    body: Option<Bytes>,
}

impl Stream for Late {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.polls += 1;
        if self.polls == 1 {
            context.waker().wake_by_ref();
            return Poll::Pending;
        }
        Poll::Ready(self.body.take().map(Ok))
    }
}

#[tokio::test]
async fn a_whole_body_that_arrives_with_its_end_is_read() {
    let body = form(&[text_part("a", b"1")]);
    let request = Request::builder()
        .header("content-type", form_type())
        .body(Body::from_stream(Late {
            polls: 0,
            body: Some(Bytes::from(body)),
        }))
        .unwrap();
    assert_eq!(read(request, wide()).await.unwrap()[0].bytes, b"1");
}

/// The limits take the handler's word for it before the first part only in
/// the sense that matters: the ones set last are the ones that hold.
#[tokio::test]
async fn limits_set_by_a_handler_replace_the_defaults() {
    let body = form(&[text_part("a", &[7; 32])]);
    let mut form = Multipart::from_request(request(&body))
        .await
        .unwrap()
        .with_limits(MultipartLimits {
            max_field_bytes: 16,
            ..MultipartLimits::default()
        });
    assert_eq!(form.limits().max_field_bytes, 16);
    let field = form.next_field().await.unwrap().unwrap();
    assert_eq!(
        field.bytes().await.unwrap_err(),
        MultipartError::FieldTooLarge { limit: 16 }
    );
}

/// Nothing is parsed before the handler asks.
#[tokio::test]
async fn extraction_reads_nothing() {
    let polled = Arc::new(AtomicUsize::new(0));
    let counter = polled.clone();
    let body = futures_util::stream::poll_fn(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        Poll::Ready(None::<Result<Bytes, std::io::Error>>)
    });
    let request = Request::builder()
        .header("content-type", form_type())
        .body(Body::from_stream(body))
        .unwrap();
    let form = Multipart::from_request(request).await.unwrap();
    poll_fn(|_| Poll::Ready(())).await;
    assert_eq!(polled.load(Ordering::SeqCst), 0);
    drop(form);
}

#[tokio::test]
async fn rejections_render_in_the_error_format() {
    let error = MultipartError::TooManyParts { limit: 3 }.to_app_error();
    assert_eq!(error.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        error.body(),
        serde_json::json!({
            "statusCode": 413,
            "message": "A form may have at most 3 parts",
            "error": "Payload Too Large",
            "code": "payload_too_large",
        })
    );
    let error = MultipartError::Malformed("incomplete multipart stream".to_owned()).to_app_error();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.body()["message"], "incomplete multipart stream");
}

/// Two readers of a part that each take a different `Content-Disposition`,
/// `Content-Type` or parameter see different forms: a check in front of the
/// app must not be fooled into reading the wrong one.
#[tokio::test]
async fn a_part_that_says_the_same_thing_twice_is_refused() {
    for headers in [
        "Content-Disposition: form-data; name=\"a\"\r\nContent-Disposition: form-data; name=\"b\"",
        "Content-Disposition: form-data; name=\"a\"\r\ncontent-disposition: form-data; name=\"a\"",
        "Content-Disposition: form-data; name=\"a\"\r\nContent-Type: text/plain\r\nContent-Type: text/html",
        "Content-Disposition: form-data; name=\"a\"; name=\"b\"",
        "Content-Disposition: form-data; name=\"a\"; filename=\"a.txt\"; FILENAME=\"b.php\"",
        "Content-Disposition: form-data; name=\"a\"; filename*=UTF-8''a; filename*=UTF-8''b",
    ] {
        let mut part = head(headers);
        part.extend_from_slice(b"1\r\n");
        let error = read(request(&form(&[part])), wide()).await.unwrap_err();
        assert!(matches!(error, MultipartError::Malformed(_)), "{headers}");
        assert_eq!(error.status(), StatusCode::BAD_REQUEST, "{headers}");
    }
}

/// A parameter is read where a parameter starts, never inside the value of
/// another: a file name that holds `name=` does not rename the field, and a
/// field name that holds `filename=` does not make it a file.
#[tokio::test]
async fn a_parameter_inside_a_value_is_not_read() {
    for (disposition, name, file_name) in [
        (
            "form-data; filename=\"x name=admin\"",
            None,
            Some("x name=admin"),
        ),
        (
            "form-data; name=\"a; filename=evil.php\"",
            Some("a; filename=evil.php"),
            None,
        ),
        (
            "form-data; name=\"a\"; x=\"; filename=evil.php\"",
            Some("a"),
            None,
        ),
        ("form-data; username=\"admin\"; name=\"a\"", Some("a"), None),
    ] {
        let mut part = head(&format!("Content-Disposition: {disposition}"));
        part.extend_from_slice(b"1\r\n");
        let parts = read(request(&form(&[part])), wide()).await.unwrap();
        assert_eq!(parts[0].name.as_deref(), name, "{disposition}");
        assert_eq!(parts[0].file_name.as_deref(), file_name, "{disposition}");
    }
}

/// RFC 2046: a delimiter starts a line, or the body. A reader in front of the
/// app that keeps to this sees `real` only, so the parser must too.
#[tokio::test]
async fn a_delimiter_in_the_middle_of_a_line_does_not_start_a_part() {
    let smuggled = |before: &str| {
        let mut body = before.as_bytes().to_vec();
        body.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"smuggled\"\r\n\r\nevil\r\n--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"real\"\r\n\r\nok\r\n--{BOUNDARY}--")
                .as_bytes(),
        );
        body
    };
    let names = |parts: Vec<Read>| {
        parts
            .into_iter()
            .map(|part| part.name.unwrap())
            .collect::<Vec<_>>()
    };
    for before in ["junk", "a line\r\njunk", "junk\n", "junk\r"] {
        let body = smuggled(before);
        assert_eq!(
            names(read(request(&body), wide()).await.unwrap()),
            ["real"],
            "{before:?}"
        );
        assert_eq!(
            names(read(dripped(&body), wide()).await.unwrap()),
            ["real"],
            "{before:?}"
        );
    }
    for before in ["", "a line\r\n", "\r\n"] {
        let body = smuggled(before);
        assert_eq!(
            names(read(dripped(&body), wide()).await.unwrap()),
            ["smuggled", "real"],
            "{before:?}"
        );
    }
}

/// What httparse leaves of a head would be lost, so the part is refused: two
/// `Content-Disposition` a blank line of bare LFs apart, and a head-less part
/// whose value looks like a head.
#[tokio::test]
async fn a_head_is_read_to_its_end_or_refused() {
    for head in [
        "Content-Disposition: form-data; name=\"public\"\n\nContent-Disposition: form-data; name=\"admin\"; filename=\"evil.php\"\r\n\r\n",
        "\r\nContent-Disposition: form-data; name=\"admin\"\r\n\r\n",
        "\nContent-Disposition: form-data; name=\"admin\"\r\n\r\n",
        // A line a stricter reader takes as part of the header before it.
        "X-A: 1\nContent-Disposition: form-data; name=\"admin\"\r\n\r\n",
        "Content-Disposition: form-data; name=\"a\"\nX-A: 1\r\n\r\n",
    ] {
        let body = format!("--{BOUNDARY}\r\n{head}v\r\n--{BOUNDARY}--");
        let error = read(request(body.as_bytes()), wide()).await;
        assert!(
            matches!(error, Err(MultipartError::Malformed(_))),
            "{head:?}: {error:?}"
        );
    }
}

/// A file a reader in front of the app sees and the handler would not.
#[tokio::test]
async fn a_disposition_that_can_be_read_two_ways_is_refused() {
    for disposition in [
        // An escaped backslash closes the string for one reader, not another.
        "form-data; name=\"a\\\\\"; filename=\"x.php\"",
        // A backslash is kept by one reader, and escapes the quote for another.
        "form-data; name=\"a\\\"; filename=\"x.php\"",
        // Something that is not a parameter, then a file.
        "form-data; name=\"a\" junk; filename=\"x.php\"",
        "form-data; name=\"a\" filename=\"evil.php\"",
        "form-data; name=a junk; filename=x.php",
        "form-data; name=a\tfilename=x.php",
        "form-data; name=\"a\"; junk filename=\"x.php\"",
        // A quote that never closes holds the rest of the header.
        r#"form-data; name="a"; filename="C:\temp\""#,
        "form-data; name=\"a; filename=x.php",
    ] {
        let mut part = head(&format!("Content-Disposition: {disposition}"));
        part.extend_from_slice(b"1\r\n");
        let error = read(request(&form(&[part])), wide()).await;
        assert!(
            matches!(&error, Err(MultipartError::Malformed(message)) if message.contains("Content-Disposition")),
            "{disposition}: {error:?}"
        );
    }
}

/// A file goes to the handler as it arrives: reading its first piece reads
/// a chunk or so of the body, not everything the client has already sent.
#[tokio::test]
async fn a_file_is_streamed_not_buffered() {
    let served = Arc::new(AtomicUsize::new(0));
    let counter = served.clone();
    let start = head("Content-Disposition: form-data; name=\"a\"; filename=\"a.bin\"");
    let head_length = start.len();
    let chunks = std::iter::once(start)
        .chain(std::iter::repeat_n(vec![7; 1024], 1024))
        .chain(std::iter::once(format!("\r\n--{BOUNDARY}--").into_bytes()));
    let stream = futures_util::stream::iter(chunks).map(move |chunk| {
        counter.fetch_add(chunk.len(), Ordering::SeqCst);
        Ok::<_, std::io::Error>(Bytes::from(chunk))
    });
    let request = Request::builder()
        .header("content-type", form_type())
        .body(Body::from_stream(stream))
        .unwrap();
    let mut form = Multipart::from_request(request).await.unwrap();
    let mut field = form.next_field().await.unwrap().unwrap();
    let first = field.chunk().await.unwrap().unwrap();
    assert_eq!(first.len(), 1024);
    let read = served.load(Ordering::SeqCst);
    assert!(read <= head_length + 2 * 1024, "{read} bytes read");

    let mut total = first.len();
    while let Some(chunk) = field.chunk().await.unwrap() {
        total += chunk.len();
    }
    assert_eq!(total, 1024 * 1024);
}
