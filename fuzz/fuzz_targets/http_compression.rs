//! `navree_http::CompressionLayer`: the client's `Accept-Encoding`, the
//! response's headers, and a body handed over in any pattern of chunks,
//! waits, trailers and failures.
//!
//! Invariants: nothing panics, and the handler's body is never polled after
//! its end; every data frame is at most 4 KiB; a gzipped body decodes to
//! exactly the data the handler sent (a failed one to a prefix of it), its
//! trailers follow, and its response varies with `Accept-Encoding`; a body
//! that is not gzipped comes out as it went in; a client whose
//! `Accept-Encoding` doesn't name gzip never gets it.
#![no_main]

use std::collections::VecDeque;
use std::convert::Infallible;
use std::future::Future;
use std::io::Read;
use std::pin::{Pin, pin};
use std::task::{Context, Poll, Waker};

use arbitrary::Arbitrary;
use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Request, Response};
use http_body::{Body as HttpBody, Frame, SizeHint};
use libfuzzer_sys::fuzz_target;
use navree_http::CompressionLayer;
use navree_http::body::Body;
use navree_kernel::BoxError;
use tower::{Layer, Service, service_fn};

#[derive(Debug, Arbitrary)]
enum Step {
    Data(Vec<u8>),
    Wait,
    Trailers,
    Fail,
}

#[derive(Debug, Arbitrary)]
struct Input {
    accept: Vec<Vec<u8>>,
    content_type: Option<Vec<u8>>,
    content_length: Option<Vec<u8>>,
    vary: Vec<Vec<u8>>,
    encoded: bool,
    range: bool,
    exact: bool,
    steps: Vec<Step>,
}

/// The handler's body: `steps` one poll at a time; panics if polled after
/// its end.
struct Scripted {
    steps: VecDeque<Step>,
    exact: Option<u64>,
    ended: bool,
}

impl HttpBody for Scripted {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        assert!(!self.ended, "the handler's body is polled after its end");
        match self.steps.pop_front() {
            None => {
                self.ended = true;
                Poll::Ready(None)
            }
            Some(Step::Data(data)) => Poll::Ready(Some(Ok(Frame::data(Bytes::from(data))))),
            Some(Step::Wait) => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Some(Step::Trailers) => Poll::Ready(Some(Ok(Frame::trailers(trailers())))),
            Some(Step::Fail) => Poll::Ready(Some(Err("the handler failed".into()))),
        }
    }

    fn size_hint(&self) -> SizeHint {
        self.exact.map_or_else(SizeHint::new, SizeHint::with_exact)
    }
}

fn trailers() -> HeaderMap {
    let mut trailers = HeaderMap::new();
    trailers.insert("x-checksum", HeaderValue::from_static("abc"));
    trailers
}

fn run<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("the response is ready at once"),
    }
}

/// The data the handler sends before its trailers or its failure, whether
/// a failure comes first, and all the data it sends before a failure.
fn sent(steps: &[Step]) -> (Vec<u8>, bool, Vec<u8>) {
    let mut gzipped = None;
    let mut all = Vec::new();
    for step in steps {
        match step {
            Step::Data(bytes) => all.extend_from_slice(bytes),
            Step::Wait => {}
            Step::Trailers => {
                gzipped.get_or_insert((all.clone(), false));
            }
            Step::Fail => {
                gzipped.get_or_insert((all.clone(), true));
                break;
            }
        }
    }
    let (data, fails) = gzipped.unwrap_or_else(|| (all.clone(), false));
    (data, fails, all)
}

fuzz_target!(|input: Input| {
    let (data, fails, all) = sent(&input.steps);
    let mut request = Request::new(Body::empty());
    for value in &input.accept {
        if let Ok(value) = HeaderValue::from_bytes(value) {
            request.headers_mut().append("accept-encoding", value);
        }
    }
    let names_gzip = request
        .headers()
        .get_all("accept-encoding")
        .iter()
        .any(|value| {
            value
                .as_bytes()
                .windows(4)
                .any(|window| window.eq_ignore_ascii_case(b"gzip"))
        });

    let mut response = Response::new(Body::new(Scripted {
        exact: input.exact.then_some(data.len() as u64),
        steps: input.steps.into(),
        ended: false,
    }));
    let headers = response.headers_mut();
    let pairs = [
        ("content-type", input.content_type.as_ref()),
        ("content-length", input.content_length.as_ref()),
    ];
    for (name, value) in pairs {
        if let Some(value) = value.and_then(|value| HeaderValue::from_bytes(value).ok()) {
            headers.insert(name, value);
        }
    }
    for value in &input.vary {
        if let Ok(value) = HeaderValue::from_bytes(value) {
            headers.append("vary", value);
        }
    }
    if input.encoded {
        headers.insert("content-encoding", HeaderValue::from_static("br"));
    }
    if input.range {
        headers.insert("content-range", HeaderValue::from_static("bytes 0-9/100"));
    }

    let response = std::sync::Mutex::new(Some(response));
    let mut service = CompressionLayer::new().layer(service_fn(move |_: Request<Body>| {
        let response = response.lock().unwrap().take().unwrap();
        async move { Ok::<_, Infallible>(response) }
    }));
    let response = run(service.call(request)).unwrap();
    let gzipped = !input.encoded
        && response
            .headers()
            .get("content-encoding")
            .is_some_and(|value| value == "gzip");
    assert!(
        !gzipped || names_gzip,
        "gzip for a client that doesn't name it"
    );
    if gzipped {
        assert!(response.headers().get_all("vary").iter().any(|value| {
            value
                .as_bytes()
                .windows(15)
                .any(|window| window.eq_ignore_ascii_case(b"accept-encoding"))
        }));
        assert!(!response.headers().contains_key("content-length"));
    }

    let mut body = response.into_body();
    let mut out = Vec::new();
    let mut seen_trailers = None;
    let mut failed = false;
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        match Pin::new(&mut body).poll_frame(&mut cx) {
            Poll::Pending => {}
            Poll::Ready(None) => break,
            Poll::Ready(Some(Err(_))) => {
                failed = true;
                break;
            }
            Poll::Ready(Some(Ok(frame))) => match frame.into_data() {
                Ok(chunk) => {
                    assert!(
                        !gzipped || chunk.len() <= 4096,
                        "a frame of {}",
                        chunk.len()
                    );
                    out.extend_from_slice(&chunk);
                }
                Err(frame) => seen_trailers = frame.into_trailers().ok(),
            },
        }
    }
    if !gzipped {
        // Frame for frame, up to the first failure.
        assert_eq!(out, all);
        return;
    }
    let mut decoded = Vec::new();
    let read = flate2::read::GzDecoder::new(out.as_slice()).read_to_end(&mut decoded);
    if fails {
        assert!(failed);
        assert!(
            data.starts_with(&decoded),
            "a failed body decodes to a prefix"
        );
    } else {
        // The gzip stream is whole before the trailers, whatever the
        // handler does after them.
        assert!(read.is_ok(), "{read:?}");
        assert_eq!(decoded, data);
        if seen_trailers.is_some() {
            assert_eq!(seen_trailers, Some(trailers()));
        }
    }
});
