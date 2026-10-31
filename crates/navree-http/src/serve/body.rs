//! The bodies around a request: the pace a request body must keep, and the
//! response body that marks the end of a request.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, ready};
use std::time::Duration;

use bytes::Bytes;
use http_body::{Body as HttpBody, Frame, SizeHint};
use navree_kernel::BoxError;
use pin_project_lite::pin_project;
use tokio::time::{Instant, Sleep};

use super::clock::InFlight;
use super::streams::Progress;

/// The most of a response HTTP/2 is given at a time. A larger chunk would sit
/// in HTTP/2's send buffer with no sign of how far it has got: a download at
/// any speed could look stalled. This is the default size of a frame.
const PROGRESS_BYTES: usize = 16 * 1024;

/// The error a request body ends with when the client sends it too slowly:
/// `http.server.body_read_timeout_ms` and
/// `http.server.body_min_bytes_per_sec`. [`read_body`](crate::body::read_body),
/// [`Json`](crate::Json) and [`Form`](crate::Form) answer 408.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BodyReadTimeout;

impl std::fmt::Display for BodyReadTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the request body arrived too slowly")
    }
}

impl std::error::Error for BodyReadTimeout {}

/// The pace a request body must keep, as a budget of time spent waiting on
/// the client. It starts full (`timeout`), waiting spends it, and each byte
/// earns back `1 / rate` seconds, up to full again. So one stall can never
/// outlast `timeout`, and a client averaging less than `rate` runs out too,
/// however short its pauses. A body that arrives fast earns nothing beyond
/// full, so an early burst buys no long stall later.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pace {
    timeout: Duration,
    rate: Option<u64>,
}

impl Pace {
    pub(super) fn new(timeout: Duration, rate: Option<u64>) -> Self {
        Self {
            timeout,
            rate: rate.map(|rate| rate.max(1)),
        }
    }
}

/// In the extensions of a request served by [`serve`](super::serve()) with a
/// body: lifts the pace the body must keep, for a request whose body is a
/// conversation rather than an upload (a gRPC stream sits silent between
/// messages).
#[derive(Clone, Debug, Default)]
pub struct BodyReadLimit(Arc<AtomicBool>);

impl BodyReadLimit {
    /// This request's body may take as long as it takes.
    pub fn lift(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    fn lifted(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

pin_project! {
    /// A request body held to a [`Pace`]. Only time spent waiting for the
    /// client counts: the clock runs while `poll_frame` is pending, so a
    /// handler that is slow to read costs the client nothing.
    pub(super) struct PacedBody<B> {
        #[pin]
        inner: B,
        pace: Pace,
        limit: BodyReadLimit,
        // What is left of the budget.
        left: Duration,
        // Since when the handler has been waiting for the client.
        waiting_since: Option<Instant>,
        // Fires when `left` runs out, armed while waiting.
        timer: Option<Pin<Box<Sleep>>>,
        spent: bool,
    }
}

impl<B> PacedBody<B> {
    pub(super) fn new(inner: B, pace: Pace, limit: BodyReadLimit) -> Self {
        Self {
            inner,
            pace,
            limit,
            left: pace.timeout,
            waiting_since: None,
            timer: None,
            spent: false,
        }
    }
}

impl<B> HttpBody for PacedBody<B>
where
    B: HttpBody<Data = Bytes>,
    B::Error: Into<BoxError>,
{
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let this = self.project();
        if *this.spent {
            return Poll::Ready(Some(Err(BodyReadTimeout.into())));
        }
        match this.inner.poll_frame(cx) {
            Poll::Ready(frame) => {
                if let Some(since) = this.waiting_since.take() {
                    *this.left = this.left.saturating_sub(since.elapsed());
                }
                if let Some(Ok(frame)) = &frame
                    && let Some(data) = frame.data_ref()
                {
                    *this.left = match this.pace.rate {
                        Some(rate) => this
                            .left
                            .saturating_add(Duration::from_secs_f64(
                                data.len() as f64 / rate as f64,
                            ))
                            .min(this.pace.timeout),
                        None => this.pace.timeout,
                    };
                }
                Poll::Ready(frame.map(|frame| frame.map_err(Into::into)))
            }
            Poll::Pending if this.limit.lifted() => Poll::Pending,
            Poll::Pending => {
                let now = Instant::now();
                let deadline = match *this.waiting_since {
                    Some(since) => since + *this.left,
                    None => {
                        *this.waiting_since = Some(now);
                        now + *this.left
                    }
                };
                let timer = this
                    .timer
                    .get_or_insert_with(|| Box::pin(tokio::time::sleep_until(deadline)));
                if timer.deadline() != deadline {
                    timer.as_mut().reset(deadline);
                }
                // The time decides: a task out of budget hears nothing from
                // its timer.
                if now < deadline {
                    ready!(timer.as_mut().poll(cx));
                }
                *this.spent = true;
                Poll::Ready(Some(Err(BodyReadTimeout.into())))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

pin_project! {
    /// A response body that keeps its request [in flight](InFlight) until
    /// it ends, so a long stream is never mistaken for an idle connection.
    /// On HTTP/2 it reports its [`Progress`]: hyper asks for more only once
    /// the client has taken what it has.
    pub(super) struct ResponseBody<B> {
        // `None` for a response the server answers itself, with no body.
        #[pin]
        inner: Option<B>,
        in_flight: InFlight,
        progress: Option<Arc<Progress>>,
        // What is left of a chunk: see `first_part`.
        rest: Bytes,
    }
}

impl<B> ResponseBody<B> {
    pub(super) fn new(
        inner: Option<B>,
        in_flight: InFlight,
        progress: Option<Arc<Progress>>,
    ) -> Self {
        if let Some(progress) = &progress {
            progress.waiting();
        }
        Self {
            inner,
            in_flight,
            progress,
            rest: Bytes::new(),
        }
    }
}

/// What goes to hyper next of `data`, leaving the rest in `rest`: up to
/// [`PROGRESS_BYTES`], and the last byte of a chunk on its own.
///
/// hyper hands a chunk to h2 as soon as the client's window has room for any
/// of it, and the rest queues there, out of sight. If that was the last chunk
/// the stream's task ends with the response still behind flow control, and
/// the connection would look idle and could close under it. But hyper hands
/// over the next chunk only when the window has room beyond what is queued.
/// So when a last byte has gone, whatever comes next, nothing of the response
/// is left waiting for the client.
fn first_part(mut data: Bytes, rest: &mut Bytes) -> Bytes {
    if data.len() > 1 {
        *rest = data.split_off(PROGRESS_BYTES.min(data.len() - 1));
    }
    data
}

impl<B: HttpBody<Data = Bytes>> HttpBody for ResponseBody<B> {
    type Data = Bytes;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, B::Error>>> {
        let this = self.project();
        let Some(inner) = this.inner.as_pin_mut() else {
            return Poll::Ready(None);
        };
        let Some(progress) = this.progress else {
            return inner.poll_frame(cx);
        };
        if !this.rest.is_empty() {
            progress.waiting();
            let rest = std::mem::take(this.rest);
            return Poll::Ready(Some(Ok(Frame::data(first_part(rest, this.rest)))));
        }
        match inner.poll_frame(cx) {
            Poll::Pending => {
                progress.busy();
                Poll::Pending
            }
            Poll::Ready(Some(Ok(frame))) => {
                progress.waiting();
                let frame = match frame.into_data() {
                    Ok(data) => Frame::data(first_part(data, this.rest)),
                    Err(frame) => frame,
                };
                Poll::Ready(Some(Ok(frame)))
            }
            ended => ended,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.rest.is_empty() && self.inner.as_ref().is_none_or(HttpBody::is_end_stream)
    }

    fn size_hint(&self) -> SizeHint {
        let mut hint = self
            .inner
            .as_ref()
            .map_or_else(|| SizeHint::with_exact(0), HttpBody::size_hint);
        let rest = self.rest.len() as u64;
        hint.set_lower(hint.lower().saturating_add(rest));
        if let Some(upper) = hint.upper() {
            hint.set_upper(upper.saturating_add(rest));
        }
        hint
    }
}

#[cfg(test)]
mod tests {
    use futures_util::Stream;
    use http_body_util::{BodyExt, StreamBody};
    use tokio::sync::mpsc;

    use super::*;

    /// A body fed by hand, one chunk at a time.
    struct Chunks(mpsc::UnboundedReceiver<&'static str>);

    impl Stream for Chunks {
        type Item = Result<Frame<Bytes>, std::convert::Infallible>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            self.0
                .poll_recv(cx)
                .map(|chunk| chunk.map(|text| Ok(Frame::data(Bytes::from_static(text.as_bytes())))))
        }
    }

    const TIMEOUT: Duration = Duration::from_secs(10);

    fn body(
        rate: Option<u64>,
    ) -> (
        mpsc::UnboundedSender<&'static str>,
        PacedBody<StreamBody<Chunks>>,
        BodyReadLimit,
    ) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let limit = BodyReadLimit::default();
        let body = PacedBody::new(
            StreamBody::new(Chunks(receiver)),
            Pace::new(TIMEOUT, rate),
            limit.clone(),
        );
        (sender, body, limit)
    }

    /// The next chunk. Time is paused, so a body that never ends and never
    /// times out fails here at once.
    async fn next(body: &mut PacedBody<StreamBody<Chunks>>) -> Result<Bytes, BoxError> {
        let mut body = Pin::new(body);
        let frame = tokio::time::timeout(Duration::from_secs(100_000), body.frame())
            .await
            .expect("the body moves")
            .expect("a frame")?;
        Ok(frame.into_data().expect("data"))
    }

    fn timed_out(error: &BoxError) -> bool {
        error.is::<BodyReadTimeout>()
    }

    /// The lengths `first_part` hands to hyper for a chunk of `length`.
    fn parts(length: usize) -> Vec<usize> {
        let mut rest = Bytes::from(vec![0_u8; length]);
        let mut parts = Vec::new();
        loop {
            let next = std::mem::take(&mut rest);
            parts.push(first_part(next, &mut rest).len());
            if rest.is_empty() {
                return parts;
            }
        }
    }

    #[test]
    fn the_last_byte_of_a_chunk_goes_on_its_own() {
        assert_eq!(parts(0), [0]);
        assert_eq!(parts(1), [1]);
        assert_eq!(parts(2), [1, 1]);
        assert_eq!(parts(8192), [8191, 1]);
        assert_eq!(parts(PROGRESS_BYTES), [PROGRESS_BYTES - 1, 1]);
        assert_eq!(parts(PROGRESS_BYTES + 1), [PROGRESS_BYTES, 1]);
        assert_eq!(
            parts(PROGRESS_BYTES * 2 + 5),
            [PROGRESS_BYTES, PROGRESS_BYTES, 4, 1]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_stall_longer_than_the_timeout_ends_the_body() {
        let (_sender, mut body, _) = body(None);
        let started = Instant::now();
        let error = next(&mut body).await.unwrap_err();
        assert!(timed_out(&error), "{error}");
        assert_eq!(started.elapsed(), TIMEOUT);
        let again = next(&mut body).await.unwrap_err();
        assert!(timed_out(&again), "it stays ended");
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_rate_any_byte_restores_the_whole_timeout() {
        let (sender, mut body, _) = body(None);
        for _ in 0..5 {
            let feed = sender.clone();
            tokio::spawn(async move {
                tokio::time::sleep(TIMEOUT - Duration::from_secs(1)).await;
                feed.send("x").unwrap();
            });
            assert_eq!(&next(&mut body).await.unwrap()[..], b"x");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_client_dripping_bytes_runs_out_though_it_never_stalls() {
        let (sender, mut body, _) = body(Some(1000));
        let started = Instant::now();
        let mut error = None;
        for _ in 0..1000 {
            let feed = sender.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let _ = feed.send("x");
            });
            if let Err(failed) = next(&mut body).await {
                error = Some(failed);
                break;
            }
        }
        let error = error.expect("the drip runs out of time");
        assert!(timed_out(&error), "{error}");
        // One byte a second earns 1 ms of each second spent.
        assert!(started.elapsed() <= TIMEOUT + Duration::from_millis(50));
        assert!(started.elapsed() >= TIMEOUT);
    }

    #[tokio::test(start_paused = true)]
    async fn a_body_at_the_rate_keeps_going_and_an_early_burst_buys_no_long_stall() {
        let (sender, mut body, _) = body(Some(1000));
        // 2,000 bytes a second: double the rate, for much longer than the
        // timeout.
        let chunk: &'static str = Box::leak("y".repeat(2000).into_boxed_str());
        for _ in 0..60 {
            let feed = sender.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(1)).await;
                feed.send(chunk).unwrap();
            });
            assert_eq!(next(&mut body).await.unwrap().len(), 2000);
        }
        let started = Instant::now();
        let error = next(&mut body).await.unwrap_err();
        assert!(timed_out(&error), "{error}");
        assert_eq!(started.elapsed(), TIMEOUT);
    }

    #[tokio::test(start_paused = true)]
    async fn only_waiting_on_the_client_counts() {
        let (sender, mut body, _) = body(None);
        sender.send("x").unwrap();
        next(&mut body).await.unwrap();
        // The handler is busy for an hour, then comes back for more.
        tokio::time::advance(Duration::from_secs(3600)).await;
        sender.send("y").unwrap();
        assert_eq!(&next(&mut body).await.unwrap()[..], b"y");
    }

    #[tokio::test(start_paused = true)]
    async fn a_lifted_limit_never_fires() {
        let (sender, mut body, limit) = body(Some(1000));
        limit.lift();
        let feed = tokio::spawn(async move {
            tokio::time::sleep(TIMEOUT * 100).await;
            sender.send("late").unwrap();
        });
        assert_eq!(&next(&mut body).await.unwrap()[..], b"late");
        feed.await.unwrap();
    }
}
