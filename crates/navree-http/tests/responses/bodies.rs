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

/// The same value through each side's `From`.
macro_rules! same_from {
    ($value:expr) => {{
        let a = Body::from($value);
        let b = axum::body::Body::from($value);
        assert_eq!(hint(a.size_hint()), hint(b.size_hint()));
        assert_eq!(a.is_end_stream(), b.is_end_stream());
        let a = a.collect().await.unwrap().to_bytes();
        let b = b.collect().await.unwrap().to_bytes();
        assert_eq!(a, b);
    }};
}

#[tokio::test]
async fn sizes_and_contents_are_the_same() {
    same_from!(());
    same_from!("");
    same_from!("text");
    same_from!(String::from("text"));
    same_from!(vec![1_u8, 2]);
    same_from!(Bytes::from_static(b"bytes"));
    same_from!(&b"bytes"[..]);
    same_from!(std::borrow::Cow::<'static, str>::Borrowed("cow"));
    same_from!(std::borrow::Cow::<'static, [u8]>::Owned(vec![1]));

    assert_eq!(hint(Body::empty().size_hint()), (0, Some(0)));
    assert!(Body::empty().is_end_stream());
    assert!(Body::default().is_end_stream());
    let empty = axum::body::Body::empty();
    assert_eq!(hint(empty.size_hint()), (0, Some(0)));
}

fn chunks(
    parts: &'static [&'static str],
) -> impl futures_util::Stream<Item = Result<&'static str, std::io::Error>> + Send {
    stream::iter(parts.iter().copied().map(Ok))
}

#[tokio::test]
async fn a_stream_has_no_known_size_and_keeps_its_chunks() {
    let a = Body::from_stream(chunks(&["a", "", "bc"]));
    let b = axum::body::Body::from_stream(chunks(&["a", "", "bc"]));
    assert_eq!(hint(a.size_hint()), hint(b.size_hint()));
    assert_eq!(a.is_end_stream(), b.is_end_stream());
    assert!(!a.is_end_stream());

    let frames = |body: Body| async move {
        let mut body = body.into_data_stream();
        let mut seen = Vec::new();
        while let Some(chunk) = body.next().await {
            seen.push(chunk.unwrap());
        }
        seen
    };
    let a = frames(a).await;
    assert_eq!(a, ["a", "", "bc"], "chunks are neither joined nor split");
    assert_eq!(a, frames(Body::new(b)).await);
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
async fn errors_read_the_same() {
    // Difference 1: a boxed error rather than a wrapper around one.
    let frames = |body: Body| async move {
        let mut body = body;
        let first = body.frame().await.unwrap().unwrap();
        (first.into_data().unwrap(), body.frame().await.unwrap())
    };
    let (data, failure) = frames(Body::from_stream(failing())).await;
    assert_eq!(data, "start");
    let ours = failure.unwrap_err();
    let (data, failure) = frames(Body::new(axum::body::Body::from_stream(failing()))).await;
    assert_eq!(data, "start");
    let theirs = failure.unwrap_err();
    assert_eq!(ours.to_string(), theirs.to_string());
    assert_eq!(ours.to_string(), "the disk is gone");
    assert!(ours.downcast_ref::<std::io::Error>().is_some());

    let ours = to_bytes(Body::from_stream(failing()), usize::MAX).await;
    let theirs = axum::body::to_bytes(axum::body::Body::from_stream(failing()), usize::MAX).await;
    assert_eq!(
        ours.unwrap_err().to_string(),
        theirs.unwrap_err().to_string()
    );
}

#[tokio::test]
async fn an_error_is_not_swallowed_by_the_data_stream() {
    let mut data = Body::from_stream(failing()).into_data_stream();
    assert_eq!(data.next().await.unwrap().unwrap(), "start");
    let error = data.next().await.unwrap().unwrap_err();
    assert_eq!(error.to_string(), "the disk is gone");
}

#[tokio::test]
async fn to_bytes_stops_at_the_limit_on_both_sides() {
    let long = "0123456789";
    for limit in [0, 1, 9, 10, 11] {
        let a = to_bytes(Body::from(long), limit).await;
        let b = axum::body::to_bytes(axum::body::Body::from(long), limit).await;
        assert_eq!(
            a.as_ref().map_err(ToString::to_string),
            b.as_ref().map_err(ToString::to_string),
            "{limit}"
        );

        let a = to_bytes(Body::from_stream(chunks(&["01234", "56789"])), limit).await;
        let b = axum::body::to_bytes(
            axum::body::Body::from_stream(chunks(&["01234", "56789"])),
            limit,
        )
        .await;
        assert_eq!(
            a.as_ref().map_err(ToString::to_string),
            b.as_ref().map_err(ToString::to_string),
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
        // A bridged route's body: Navree's inside the previous framework's
        // inside Navree's.
        Body::new(axum::body::Body::new(Body::from_stream(slow()))),
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
    let bridged = Body::new(axum::body::Body::new(Body::from_stream(broken())));
    assert_eq!(read_body(bridged, 64).await.unwrap_err(), error);

    let long = Body::new(axum::body::Body::from("too long"));
    assert_eq!(read_body(long, 4).await.unwrap_err(), BodyError::TooLarge);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Whatever the chunks and the limit, both sides read the same bytes and
    /// fail at the same point.
    #[test]
    fn generated_streams_read_the_same(
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
        let (a, b) = crate::support::runtime().block_on(async {
            (
                to_bytes(Body::from_stream(items()), limit).await,
                axum::body::to_bytes(axum::body::Body::from_stream(items()), limit).await,
            )
        });
        prop_assert_eq!(
            a.map_err(|error| error.to_string()),
            b.map_err(|error| error.to_string())
        );
    }
}
