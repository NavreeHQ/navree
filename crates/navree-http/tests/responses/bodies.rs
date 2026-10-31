//! Bodies: streaming, sizes, reading and errors.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_util::{FutureExt as _, StreamExt as _, stream};
use http::HeaderMap;
use http_body::{Body as _, Frame, SizeHint};
use http_body_util::{BodyExt, Empty, Full, StreamBody};
use navree_http::body::{Body, BodyError, read_body, to_bytes};
use navree_http::serve::BodyReadTimeout;
use proptest::prelude::*;

fn hint(hint: SizeHint) -> (u64, Option<u64>) {
    (hint.lower(), hint.upper())
}

/// A body made `From` a value: its size is known and exact, and it reads as
/// the bytes of the value.
macro_rules! from_value {
    ($value:expr, $bytes:expr) => {{
        let bytes: &[u8] = $bytes;
        let len = bytes.len() as u64;
        let body = Body::from($value);
        assert_eq!(hint(body.size_hint()), (len, Some(len)));
        assert_eq!(body.is_end_stream(), len == 0);
        assert_eq!(body.collect().await.unwrap().to_bytes(), bytes);
    }};
}

#[tokio::test]
async fn sizes_and_contents_are_exact() {
    from_value!((), b"");
    from_value!("", b"");
    from_value!("text", b"text");
    from_value!(String::from("text"), b"text");
    from_value!(vec![1_u8, 2], &[1, 2]);
    from_value!(Bytes::from_static(b"bytes"), b"bytes");
    from_value!(&b"bytes"[..], b"bytes");
    from_value!(std::borrow::Cow::<'static, str>::Borrowed("cow"), b"cow");
    from_value!(std::borrow::Cow::<'static, [u8]>::Owned(vec![1]), &[1]);

    assert_eq!(hint(Body::empty().size_hint()), (0, Some(0)));
    assert!(Body::empty().is_end_stream());
    assert!(Body::default().is_end_stream());
}

fn chunks(
    parts: &'static [&'static str],
) -> impl futures_util::Stream<Item = Result<&'static str, std::io::Error>> + Send {
    stream::iter(parts.iter().copied().map(Ok))
}

#[tokio::test]
async fn a_stream_has_no_known_size_and_keeps_its_chunks() {
    let body = Body::from_stream(chunks(&["a", "", "bc"]));
    assert_eq!(hint(body.size_hint()), (0, None));
    assert!(!body.is_end_stream());

    let mut body = body.into_data_stream();
    let mut seen = Vec::new();
    while let Some(chunk) = body.next().await {
        seen.push(chunk.unwrap());
    }
    assert_eq!(seen, ["a", "", "bc"], "chunks are neither joined nor split");
}

/// A body that says how often it was polled, and is pending after its first
/// frame until released.
struct Dripping {
    polls: Arc<AtomicUsize>,
    frames: Vec<Bytes>,
    released: Arc<std::sync::atomic::AtomicBool>,
}

impl http_body::Body for Dripping {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        let polls = self.polls.fetch_add(1, Ordering::SeqCst);
        if polls > 0 && !self.released.load(Ordering::SeqCst) {
            return Poll::Pending;
        }
        Poll::Ready(self.frames.pop().map(|data| Ok(Frame::data(data))))
    }
}

#[tokio::test]
async fn nothing_is_read_ahead_of_the_reader() {
    let polls = Arc::new(AtomicUsize::new(0));
    let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut body = Body::new(Dripping {
        polls: polls.clone(),
        frames: vec![Bytes::from_static(b"second"), Bytes::from_static(b"first")],
        released: released.clone(),
    });
    assert_eq!(polls.load(Ordering::SeqCst), 0, "boxing reads nothing");
    let first = body.frame().await.unwrap().unwrap();
    assert_eq!(first.into_data().unwrap(), "first");
    assert_eq!(polls.load(Ordering::SeqCst), 1, "one poll for one frame");
    assert!(
        body.frame().now_or_never().is_none(),
        "it waits, it does not spin"
    );
    released.store(true, Ordering::SeqCst);
    let second = body.frame().await.unwrap().unwrap();
    assert_eq!(second.into_data().unwrap(), "second");
    assert!(body.frame().await.is_none());
}

#[tokio::test]
async fn trailers_pass_through_a_body_and_not_a_data_stream() {
    let mut trailers = HeaderMap::new();
    trailers.insert("x-sum", "1".parse().unwrap());
    let frames = || {
        stream::iter([
            Ok::<_, Infallible>(Frame::data(Bytes::from_static(b"data"))),
            Ok(Frame::trailers(trailers.clone())),
        ])
    };

    let mut body = Body::new(StreamBody::new(frames()));
    let _ = body.frame().await.unwrap().unwrap();
    let last = body.frame().await.unwrap().unwrap();
    assert_eq!(last.into_trailers().unwrap()["x-sum"], "1");

    let mut data = Body::new(StreamBody::new(frames())).into_data_stream();
    assert_eq!(data.next().await.unwrap().unwrap(), "data");
    assert!(data.next().await.is_none());
}

#[tokio::test]
async fn a_body_boxed_twice_is_boxed_once() {
    // What stays the same for a reader: nothing is lost going through
    // `Body::new` however many times, and hints survive.
    let body = Body::new(Body::new(Body::from("once")));
    assert_eq!(hint(body.size_hint()), (4, Some(4)));
    assert_eq!(body.collect().await.unwrap().to_bytes(), "once");
    let body = Body::new(Empty::<Bytes>::new());
    assert!(body.is_end_stream());
    let body = Body::new(Full::new(Bytes::from_static(b"x")));
    assert_eq!(hint(body.size_hint()), (1, Some(1)));
}

fn failing() -> impl futures_util::Stream<Item = Result<&'static str, std::io::Error>> + Send {
    stream::iter([Ok("start"), Err(std::io::Error::other("the disk is gone"))])
}

#[tokio::test]
async fn errors_come_out_as_they_went_in() {
    // A boxed error rather than a wrapper around one.
    let mut body = Body::from_stream(failing());
    let first = body.frame().await.unwrap().unwrap();
    assert_eq!(first.into_data().unwrap(), "start");
    let error = body.frame().await.unwrap().unwrap_err();
    assert_eq!(error.to_string(), "the disk is gone");
    assert!(error.downcast_ref::<std::io::Error>().is_some());

    let error = to_bytes(Body::from_stream(failing()), usize::MAX)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "the disk is gone");
}

#[tokio::test]
async fn an_error_is_not_swallowed_by_the_data_stream() {
    let mut data = Body::from_stream(failing()).into_data_stream();
    assert_eq!(data.next().await.unwrap().unwrap(), "start");
    let error = data.next().await.unwrap().unwrap_err();
    assert_eq!(error.to_string(), "the disk is gone");
}

#[tokio::test]
async fn to_bytes_stops_at_the_limit() {
    let long = "0123456789";
    for (limit, fits) in [(0, false), (1, false), (9, false), (10, true), (11, true)] {
        let expected = if fits {
            Ok(long)
        } else {
            Err("length limit exceeded")
        };

        let body = to_bytes(Body::from(long), limit).await;
        assert_eq!(
            body.as_ref()
                .map(|bytes| &bytes[..])
                .map_err(ToString::to_string),
            expected.map(str::as_bytes).map_err(str::to_owned),
            "{limit}"
        );

        let body = to_bytes(Body::from_stream(chunks(&["01234", "56789"])), limit).await;
        assert_eq!(
            body.as_ref()
                .map(|bytes| &bytes[..])
                .map_err(ToString::to_string),
            expected.map(str::as_bytes).map_err(str::to_owned),
            "stream, {limit}"
        );
    }
}

#[tokio::test]
async fn a_slow_body_is_told_from_a_broken_one_through_every_wrapper() {
    let slow = || stream::iter([Err::<Bytes, _>(BodyReadTimeout)]);
    let wrapped = [
        Body::from_stream(slow()),
        Body::new(Body::from_stream(slow())),
        Body::new(Body::new(Body::from_stream(slow()))),
    ];
    for body in wrapped {
        assert_eq!(read_body(body, 64).await.unwrap_err(), BodyError::TimedOut);
    }

    let broken = || stream::iter([Err::<Bytes, _>(std::io::Error::other("reset"))]);
    let error = read_body(Body::from_stream(broken()), 64)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Failed to buffer the request body: reset"
    );
    let wrapped = Body::new(Body::new(Body::from_stream(broken())));
    assert_eq!(read_body(wrapped, 64).await.unwrap_err(), error);

    let long = Body::new(Body::from("too long"));
    assert_eq!(read_body(long, 4).await.unwrap_err(), BodyError::TooLarge);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Whatever the chunks and the limit, the bytes come out in order until
    /// the limit is passed (then it fails, whatever follows) or the stream
    /// fails (then it fails there).
    #[test]
    fn generated_streams_read_up_to_the_limit(
        parts in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..32), 0..8),
        limit in 0_usize..128,
        fail in any::<bool>(),
    ) {
        let items = || {
            let mut items: Vec<Result<Bytes, std::io::Error>> =
                parts.iter().map(|part| Ok(Bytes::from(part.clone()))).collect();
            if fail {
                items.push(Err(std::io::Error::other("failed")));
            }
            stream::iter(items)
        };
        let read = crate::support::runtime().block_on(to_bytes(Body::from_stream(items()), limit));

        let mut expected = Vec::new();
        let mut outcome = None;
        for part in &parts {
            if expected.len() + part.len() > limit {
                outcome = Some("length limit exceeded");
                break;
            }
            expected.extend_from_slice(part);
        }
        if outcome.is_none() && fail {
            outcome = Some("failed");
        }
        prop_assert_eq!(
            read.map(|bytes| bytes.to_vec()).map_err(|error| error.to_string()),
            match outcome {
                Some(error) => Err(error.to_owned()),
                None => Ok(expected),
            }
        );
    }
}
