use std::collections::VecDeque;
use std::convert::Infallible;
use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Waker;

use bytes::Bytes;
use http::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE};
use http::{HeaderMap, Request, StatusCode};
use http_body::{Body as HttpBody, Frame, SizeHint};
use http_body_util::BodyExt;
use tower::{ServiceBuilder, ServiceExt, service_fn};

use super::*;
use crate::body::Body;

/// `text` repeated past the 32-byte floor.
fn long(text: &str) -> String {
    text.repeat(64)
}

fn request(accept: &[&str]) -> Request<Body> {
    let mut builder = Request::builder().uri("/");
    for value in accept {
        builder = builder.header(ACCEPT_ENCODING, *value);
    }
    builder.body(Body::empty()).unwrap()
}

/// Sends a request accepting `accept` through the layer, to a service that
/// answers `response`.
async fn send(accept: &[&str], response: Response<Body>) -> Response<Body> {
    let response = std::sync::Mutex::new(Some(response));
    let inner = service_fn(move |_: Request<Body>| {
        let response = response.lock().unwrap().take().expect("one call");
        async move { Ok::<_, Infallible>(response) }
    });
    ServiceBuilder::new()
        .layer(CompressionLayer::new())
        .service(inner)
        .oneshot(request(accept))
        .await
        .unwrap()
}

fn text(body: impl Into<Body>) -> Response<Body> {
    Response::builder()
        .header(CONTENT_TYPE, "text/plain")
        .body(body.into())
        .unwrap()
}

fn gzipped(response: &Response<Body>) -> bool {
    response
        .headers()
        .get(CONTENT_ENCODING)
        .map(HeaderValue::as_bytes)
        == Some(b"gzip")
}

fn gunzip(bytes: &[u8]) -> Vec<u8> {
    let mut text = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut text)
        .expect("a whole gzip stream");
    text
}

async fn collect(response: Response<Body>) -> (Vec<Bytes>, Option<HeaderMap>) {
    let mut body = response.into_body();
    let mut data = Vec::new();
    let mut trailers = None;
    while let Some(frame) = body.frame().await {
        let frame = frame.unwrap();
        match frame.into_data() {
            Ok(chunk) => data.push(chunk),
            Err(frame) => trailers = frame.into_trailers().ok(),
        }
    }
    (data, trailers)
}

#[test]
fn the_client_chooses_gzip_by_its_accept_encoding() {
    let cases: &[(&[&str], bool)] = &[
        (&[], false),
        (&["gzip"], true),
        (&["GZip"], true),
        (&["x-gzip"], true),
        (&["  gzip  "], true),
        (&["br, deflate, gzip"], true),
        (&["br", "gzip"], true),
        (&["br"], false),
        (&["*"], false),
        (&["identity"], false),
        (&["gzip;q=0"], false),
        (&["gzip;q=0.000"], false),
        (&["gzip; q=0.001"], true),
        (&["gzip;Q=1"], true),
        (&["gzip;q=1.000"], true),
        (&["gzip;q=1.001"], false),
        (&["gzip;q=0.5, identity"], false),
        (&["gzip;q=0.5, identity;q=0.5"], true),
        (&["gzip;q=0.4, gzip;q=0.6, identity;q=0.5"], true),
        (&["identity;q=0, gzip;q=0.1"], true),
        (&["gzip;q=0.1234"], false),
        (&["gzip;q=.5"], false),
        (&["gzip;q=0."], false),
        (&["gzip;q=1."], true),
        (&["gzip;level=1"], false),
        (&["gzip;q=0.5;level=1"], false),
        (&["gzip ;q=0.5"], true),
        (&["gzip;q = 0.5"], false),
        (&["gzip;q=abc", "gzip;q=0.2"], true),
        (&["gzip,"], true),
        (&[",,gzip"], true),
        (&["gzipx"], false),
        (&["deflate"], false),
    ];
    for (accept, chosen) in cases {
        let mut headers = HeaderMap::new();
        for value in *accept {
            headers.append(ACCEPT_ENCODING, HeaderValue::from_static(value));
        }
        assert_eq!(accepts_gzip(&headers), *chosen, "{accept:?}");
    }
    // A value that is not visible ASCII is skipped, not the whole list.
    let mut headers = HeaderMap::new();
    headers.append(
        ACCEPT_ENCODING,
        HeaderValue::from_bytes(b"gzip\xff").unwrap(),
    );
    assert!(!accepts_gzip(&headers));
    headers.append(ACCEPT_ENCODING, HeaderValue::from_static("gzip"));
    assert!(accepts_gzip(&headers));
}

#[tokio::test]
async fn a_response_is_compressed_for_a_client_that_accepts_gzip() {
    let body = long("compressible text ");
    let response = send(
        &["gzip"],
        Response::builder()
            .header(CONTENT_TYPE, "text/plain")
            .header(CONTENT_LENGTH, body.len())
            .header(ACCEPT_RANGES, "bytes")
            .body(Body::from(body.clone()))
            .unwrap(),
    )
    .await;
    assert!(gzipped(&response));
    assert_eq!(response.headers()[VARY], "accept-encoding");
    assert!(!response.headers().contains_key(CONTENT_LENGTH));
    assert!(!response.headers().contains_key(ACCEPT_RANGES));
    assert_eq!(response.body().size_hint().exact(), None);
    assert!(!response.body().is_end_stream());
    let (data, _) = collect(response).await;
    assert_eq!(gunzip(&data.concat()), body.as_bytes());
}

#[tokio::test]
async fn a_client_that_does_not_accept_gzip_still_gets_the_vary() {
    let body = long("text ");
    let response = send(&["br"], text(body.clone())).await;
    assert!(!gzipped(&response));
    assert_eq!(response.headers()[VARY], "accept-encoding");
    assert_eq!(
        response.body().size_hint().exact(),
        Some(body.len() as u64),
        "an uncompressed body is passed on as it is"
    );
    let (data, _) = collect(response).await;
    assert_eq!(data.concat(), body.as_bytes());
}

#[tokio::test]
async fn some_responses_are_never_compressed() {
    let big = long("x");
    let cases: Vec<(&str, Response<Body>)> = vec![
        ("31 bytes", text("x".repeat(31))),
        (
            "a content-length under 32",
            Response::builder()
                .header(CONTENT_LENGTH, "31")
                .body(Body::from_stream(futures_util::stream::iter([Ok::<
                    _,
                    Infallible,
                >(
                    "x".repeat(31),
                )])))
                .unwrap(),
        ),
        (
            "already encoded",
            Response::builder()
                .header(CONTENT_ENCODING, "br")
                .body(Body::from(big.clone()))
                .unwrap(),
        ),
        (
            "a range",
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(CONTENT_RANGE, "bytes 0-99/200")
                .body(Body::from(big.clone()))
                .unwrap(),
        ),
        ("an image", typed("image/png", &big)),
        ("gRPC", typed("application/grpc", &big)),
        ("gRPC with protobuf", typed("application/grpc+proto", &big)),
        ("events", typed("text/event-stream", &big)),
        ("empty", text("")),
    ];
    for (name, response) in cases {
        let response = send(&["gzip"], response).await;
        assert!(!gzipped(&response), "{name}");
        assert!(!response.headers().contains_key(VARY), "{name}");
    }
}

fn typed(content_type: &str, body: &str) -> Response<Body> {
    Response::builder()
        .header(CONTENT_TYPE, content_type)
        .body(Body::from(body.to_owned()))
        .unwrap()
}

#[tokio::test]
async fn svg_grpc_web_and_unknown_sizes_are_compressed() {
    let big = long("x");
    let cases: Vec<(&str, Response<Body>)> = vec![
        ("32 bytes", text("x".repeat(32))),
        ("svg", typed("image/svg+xml", &big)),
        ("gRPC-Web", typed("application/grpc-web+proto", &big)),
        // Content types are compared as they are written.
        ("an upper-case image type", typed("Image/png", &big)),
        ("no content type", Response::new(Body::from(big.clone()))),
        (
            "a stream of unknown size",
            Response::new(Body::from_stream(futures_util::stream::iter([Ok::<
                _,
                Infallible,
            >(
                "x"
            )]))),
        ),
        (
            "a content-length that is not a number",
            Response::builder()
                .header(CONTENT_LENGTH, "ten")
                .body(Body::from_stream(futures_util::stream::iter([Ok::<
                    _,
                    Infallible,
                >(
                    "x"
                )])))
                .unwrap(),
        ),
    ];
    for (name, response) in cases {
        let response = send(&["gzip"], response).await;
        assert!(gzipped(&response), "{name}");
    }
}

#[tokio::test]
async fn an_existing_vary_on_accept_encoding_is_not_repeated() {
    for vary in [
        "Accept-Encoding",
        "origin, accept-encoding",
        "x-accept-encoding-v2",
    ] {
        let response = send(
            &["gzip"],
            Response::builder()
                .header(VARY, vary)
                .body(Body::from(long("x")))
                .unwrap(),
        )
        .await;
        let values: Vec<_> = response.headers().get_all(VARY).iter().collect();
        assert_eq!(values, [vary], "{vary}");
    }
    let response = send(
        &["gzip"],
        Response::builder()
            .header(VARY, "origin")
            .body(Body::from(long("x")))
            .unwrap(),
    )
    .await;
    let values: Vec<_> = response.headers().get_all(VARY).iter().collect();
    assert_eq!(values, ["origin", "accept-encoding"]);
}

/// A body that hands its frames over one poll at a time, with a `Pending`
/// wherever the script says so, and tells when it is dropped.
struct Scripted {
    steps: VecDeque<Step>,
    dropped: Arc<AtomicBool>,
}

enum Step {
    Data(&'static str),
    Pending,
    Trailers(HeaderMap),
    Fail(&'static str),
}

impl Scripted {
    fn new(steps: impl IntoIterator<Item = Step>) -> (Self, Arc<AtomicBool>) {
        let dropped = Arc::new(AtomicBool::new(false));
        let body = Scripted {
            steps: steps.into_iter().collect(),
            dropped: dropped.clone(),
        };
        (body, dropped)
    }
}

impl Drop for Scripted {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

impl HttpBody for Scripted {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        match self.steps.pop_front() {
            None => Poll::Ready(None),
            Some(Step::Data(data)) => Poll::Ready(Some(Ok(Frame::data(Bytes::from(data))))),
            Some(Step::Pending) => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Some(Step::Trailers(trailers)) => Poll::Ready(Some(Ok(Frame::trailers(trailers)))),
            Some(Step::Fail(message)) => Poll::Ready(Some(Err(message.into()))),
        }
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::new()
    }
}

/// Polls `body` once, with a waker that does nothing.
fn poll_once<B: HttpBody + Unpin>(body: &mut B) -> Poll<Option<Result<Frame<B::Data>, B::Error>>> {
    Pin::new(body).poll_frame(&mut Context::from_waker(Waker::noop()))
}

#[tokio::test]
async fn what_a_slow_body_sent_so_far_reaches_the_client_before_it_waits() {
    let (body, _) = Scripted::new([
        Step::Data("the first part of a slow response, "),
        Step::Pending,
        Step::Data("and the rest of it, a while later"),
    ]);
    let response = send(&["gzip"], Response::new(Body::new(body))).await;
    assert!(gzipped(&response));
    let mut body = response.into_body();
    let Poll::Ready(Some(Ok(first))) = poll_once(&mut body) else {
        panic!("the first part goes out before the body waits");
    };
    let first = first.into_data().unwrap();
    // The first frame ends on a flush: a client decodes all of it now.
    let mut decoder = flate2::read::GzDecoder::new(&first[..]);
    let mut seen = Vec::new();
    let _ = decoder.read_to_end(&mut seen);
    assert_eq!(seen, b"the first part of a slow response, ");
    let mut all = first.to_vec();
    while let Some(frame) = body.frame().await {
        all.extend_from_slice(&frame.unwrap().into_data().unwrap());
    }
    assert_eq!(
        gunzip(&all),
        b"the first part of a slow response, and the rest of it, a while later"
    );
}

#[tokio::test]
async fn trailers_follow_the_compressed_data() {
    let mut trailers = HeaderMap::new();
    trailers.insert("x-checksum", HeaderValue::from_static("abc"));
    let (body, _) = Scripted::new([
        Step::Data("some data that is long enough to be compressed"),
        Step::Trailers(trailers.clone()),
    ]);
    let response = send(&["gzip"], Response::new(Body::new(body))).await;
    let (data, seen) = collect(response).await;
    assert_eq!(
        gunzip(&data.concat()),
        b"some data that is long enough to be compressed"
    );
    assert_eq!(seen, Some(trailers));
}

#[tokio::test]
async fn an_error_from_the_body_comes_out_as_it_is() {
    let (body, _) = Scripted::new([
        Step::Data("the start of a body that fails"),
        Step::Pending,
        Step::Fail("gone"),
    ]);
    let response = send(&["gzip"], Response::new(Body::new(body))).await;
    let mut body = response.into_body();
    let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert!(!first.is_empty());
    let error = body.frame().await.unwrap().unwrap_err();
    assert_eq!(error.to_string(), "gone");
}

#[tokio::test]
async fn an_empty_stream_is_an_empty_gzip_stream() {
    let (body, _) = Scripted::new([]);
    let response = send(&["gzip"], Response::new(Body::new(body))).await;
    assert!(gzipped(&response));
    let (data, _) = collect(response).await;
    assert_eq!(gunzip(&data.concat()), b"");
}

#[tokio::test]
async fn a_client_that_goes_away_drops_the_body() {
    let (body, dropped) = Scripted::new([
        Step::Data("the start of a long download, "),
        Step::Pending,
        Step::Data("never read"),
    ]);
    let response = send(&["gzip"], Response::new(Body::new(body))).await;
    let mut body = response.into_body();
    assert!(matches!(poll_once(&mut body), Poll::Ready(Some(Ok(_)))));
    assert!(!dropped.load(Ordering::SeqCst));
    drop(body);
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn a_request_cancelled_before_its_response_drops_the_handler() {
    let dropped = Arc::new(AtomicBool::new(false));
    let seen = dropped.clone();
    let inner = service_fn(move |_: Request<Body>| {
        let guard = DropFlag(seen.clone());
        async move {
            let _guard = guard;
            std::future::pending::<()>().await;
            Ok::<_, Infallible>(Response::new(Body::empty()))
        }
    });
    let mut service = CompressionLayer::new().layer(inner);
    let future = service.call(request(&["gzip"]));
    let mut future = Box::pin(future);
    assert!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    drop(future);
    assert!(dropped.load(Ordering::SeqCst));
}

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn a_body_that_is_always_ready_comes_out_in_frames_of_at_most_4_kib() {
    // Pseudo-random bytes barely compress: a megabyte in, about as much out.
    let mut state = 0x2545_f491_u32;
    let input: Vec<u8> = (0..1 << 20)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    let response = send(&["gzip"], Response::new(Body::from(input.clone()))).await;
    let (data, _) = collect(response).await;
    assert!(data.len() > 200, "{} frames", data.len());
    assert!(data.iter().all(|frame| frame.len() <= 4096));
    assert_eq!(gunzip(&data.concat()), input);
}

/// Bytes that are text one time in three and noise otherwise.
fn noise(len: usize) -> Vec<u8> {
    let mut state = 0x2545_f491_u32;
    (0..len)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            if i % 3 == 0 {
                b"navree "[i % 7]
            } else {
                state as u8
            }
        })
        .collect()
}

/// Every frame of `body` gzipped, polled with a waker that does nothing.
fn frames(body: Body) -> Vec<Bytes> {
    let mut gzip = GzipBody::new(body);
    let mut frames = Vec::new();
    loop {
        match poll_once(&mut gzip) {
            Poll::Pending => {}
            Poll::Ready(None) => return frames,
            Poll::Ready(Some(frame)) => frames.push(frame.unwrap().into_data().unwrap()),
        }
    }
}

/// The frame sizes and the SHA-256 of all the bytes.
fn fingerprint(frames: &[Bytes]) -> (Vec<usize>, String) {
    use sha2::Digest;
    let sizes = frames.iter().map(Bytes::len).collect();
    (sizes, hex::encode(sha2::Sha256::digest(frames.concat())))
}

fn sizes(full: usize, frames: usize, rest: &[usize]) -> Vec<usize> {
    let mut sizes = vec![full; frames];
    sizes.extend_from_slice(rest);
    sizes
}

/// The exact bytes for fixed inputs, with flate2 on either deflate backend
/// it may be built with (miniz_oxide alone, or zlib-rs, which other crates
/// of the workspace turn on): framing and flushing are pinned down to the
/// byte.
#[test]
fn the_bytes_for_fixed_inputs_are_pinned_down() {
    let text = frames(Body::from("Navree compresses this line. ".repeat(200)));
    let expected = [
        (
            vec![80],
            "ee66fbfe069f81f55c7bb86b28f30599bcea86b8c3f6b86fff83322d179a8439",
        ),
        (
            vec![105],
            "ebe9575fa779f2e812b2da8669d33c64acda08067319328725a142aba2aa49a1",
        ),
    ];
    check("one chunk of text", &text, &expected);

    let (slow, _) = Scripted::new([
        Step::Data("the first part of a slow response, "),
        Step::Pending,
        Step::Data("and the rest of it, a while later"),
    ]);
    let slow = frames(Body::new(slow));
    let expected = [
        (
            vec![55, 36],
            "5816d56f9ab54101231f6925fd57978e873d75ce93c269d48f272dc921d27b40",
        ),
        (
            vec![51, 38],
            "b657e4e469931ec24a2afb49f6ee745e9650da93a669b38dd059f6c630f2a166",
        ),
    ];
    check("a slow body", &slow, &expected);
    // The first frame ends on a sync flush.
    assert!(slow[0].ends_with(&[0, 0, 0xff, 0xff]));

    let large = frames(Body::from(noise(256 * 1024)));
    let expected = [
        (
            sizes(4096, 56, &[2111]),
            "7868b86612e668ee30c8b7f8ef53fc073bcb42812ab8b619516e890af3a07c4d",
        ),
        (
            sizes(4096, 56, &[1980]),
            "cb4ec14481eacd8be65d365e5508b5506a13a56b62c9f1a56b1a92201708f775",
        ),
    ];
    check("256 KiB in one chunk", &large, &expected);

    let data = noise(150_000);
    let mut steps = Vec::new();
    for (i, chunk) in data.chunks(3000).enumerate() {
        steps.push(Bytes::copy_from_slice(chunk));
        if i % 7 == 6 {
            steps.push(Bytes::new());
        }
    }
    // An empty chunk stands for a wait here.
    let waits = Body::new(Waiting(steps.into()));
    let chunked = frames(waits);
    let mut zlib = sizes(4096, 22, &[3102, 994]);
    zlib.extend(sizes(4096, 8, &[3510, 586, 2120]));
    let expected = [
        (
            sizes(4096, 32, &[1566]),
            "144fa250f60106b77959d549b91d2a07af6790481c1ec25fba28496e574dffa5",
        ),
        (
            zlib,
            "f75bca2a33787d1671339d4058bbdf4a84217b724dbb66ddb1abddcc5824978b",
        ),
    ];
    check(
        "3000-byte chunks with a wait after every seventh",
        &chunked,
        &expected,
    );
    // Frames share 4 KiB buffers: two frames cut from one fill it.
    assert_eq!(gunzip(&chunked.concat()), data);
}

fn check(name: &str, frames: &[Bytes], expected: &[(Vec<usize>, &str)]) {
    let (sizes, digest) = fingerprint(frames);
    assert!(
        expected
            .iter()
            .any(|(want, hash)| *want == sizes && *hash == digest),
        "{name}: frames {sizes:?}, sha256 {digest}"
    );
}

/// Owned chunks, an empty one standing for a `Pending`.
struct Waiting(VecDeque<Bytes>);

impl HttpBody for Waiting {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        match self.0.pop_front() {
            None => Poll::Ready(None),
            Some(chunk) if chunk.is_empty() => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Some(chunk) => Poll::Ready(Some(Ok(Frame::data(chunk)))),
        }
    }
}

/// A body that panics when it is polled after its end, as a stream may.
struct Strict {
    frames: VecDeque<Frame<Bytes>>,
    ended: bool,
}

impl HttpBody for Strict {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        assert!(!self.ended, "polled after its end");
        let next = self.frames.pop_front();
        self.ended = next.is_none();
        Poll::Ready(next.map(Ok))
    }
}

#[tokio::test]
async fn a_body_that_has_ended_is_not_polled_again() {
    let mut trailers = HeaderMap::new();
    trailers.insert("x-checksum", HeaderValue::from_static("abc"));
    for with_trailers in [false, true] {
        let mut frames = VecDeque::from([Frame::data(Bytes::from(long("streamed ")))]);
        if with_trailers {
            frames.push_back(Frame::trailers(trailers.clone()));
        }
        let body = Strict {
            frames,
            ended: false,
        };
        let response = send(&["gzip"], Response::new(Body::new(body))).await;
        let mut body = response.into_body();
        let mut data = Vec::new();
        while let Some(frame) = body.frame().await {
            if let Ok(chunk) = frame.unwrap().into_data() {
                data.extend_from_slice(&chunk);
            }
        }
        // Polled again after its end, it stays ended.
        assert!(body.frame().await.is_none());
        assert_eq!(gunzip(&data), long("streamed ").as_bytes());
    }
}
