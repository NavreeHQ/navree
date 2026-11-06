//! What is left of an HTTP/1 request body its handler let go of before its
//! end: a 401 before the body was looked at, a 404, a 413 for a body declared
//! over its route's limit, a handler that did not need it.
//!
//! A client sends its whole body before it reads the answer, and a socket
//! closed with bytes unread resets the connection, which can throw the answer
//! away before the client has read it. So the answer goes out first, and:
//!
//! - What is left of the body is read and thrown away, so the connection can
//!   take the next request: at most the route's body limit (the
//!   [`UnreadLimit`] the route pipeline sets, else `http.body_limit_bytes`),
//!   for at most [`DRAIN_FOR`], at the body's pace. A body that declares more
//!   than that is not read at all.
//! - Any other body closes the connection after the answer, with
//!   `Connection: close` when the answer has not gone yet: one that declares
//!   more, one not read to its end in time, one that failed, and one from a
//!   client waiting on `100 Continue`, which has sent nothing and is not asked
//!   to now.
//! - That close lingers ([`Linger`]): the server ends its side, then reads what
//!   the client is still sending and throws it away, until the client closes
//!   its side too, sends nothing for [`LINGER_QUIET`], or [`LINGER_AT_MOST`]
//!   has passed.
//!
//! What has already arrived costs nothing more to throw away, so a body hyper
//! has read to its end leaves the connection open whatever its size. HTTP/2
//! needs none of this: an answer ends its stream, and the connection goes on.

use std::mem::MaybeUninit;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use bytes::Bytes;
use http::header::EXPECT;
use http::request::Parts;
use http::{HeaderMap, Version};
use http_body::{Body as HttpBody, Frame, SizeHint};
use tokio::io::{AsyncRead, ReadBuf};
use tokio::time::Instant;

use super::clock::{Clock, Timer};

/// The longest the rest of a body may take to be read after its answer.
pub(super) const DRAIN_FOR: Duration = Duration::from_secs(5);

/// How long a lingering close waits for the next bytes from the client.
pub(super) const LINGER_QUIET: Duration = Duration::from_secs(2);

/// The longest a lingering close reads.
pub(super) const LINGER_AT_MOST: Duration = Duration::from_secs(5);

/// The frames or reads taken in one poll, before the connection's other work
/// gets a turn: a body that is always ready must not hold its task.
const READS_PER_POLL: usize = 16;

/// The most a lingering close reads at a time.
const LINGER_READ_BYTES: usize = 8 * 1024;

/// In the extensions of an HTTP/1 request with a body: the most of what is
/// left of the body that is read after an early answer. The route pipeline
/// sets its route's body limit.
#[derive(Clone, Debug)]
pub(crate) struct UnreadLimit(Arc<AtomicU64>);

impl UnreadLimit {
    fn new(bytes: u64) -> Self {
        Self(Arc::new(AtomicU64::new(bytes)))
    }

    /// At most `bytes` of what is left are read.
    pub(crate) fn set(&self, bytes: usize) {
        self.0
            .store(u64::try_from(bytes).unwrap_or(u64::MAX), Ordering::Relaxed);
    }

    fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// One HTTP/1 connection's: the body a handler let go of, for its [`Drain`],
/// and whether the answer closes the connection. One request is in flight at
/// a time, and the next head is not read before the last body has ended, so
/// what one request leaves is settled before the next starts.
pub(super) struct Unread<B> {
    clock: Arc<Clock>,
    /// The limit before a route sets its own.
    limit: u64,
    /// The answer to the request in flight says `Connection: close`.
    closes: AtomicBool,
    /// `left` holds a body: the connection's task looks at no lock until then.
    deposited: AtomicBool,
    /// A body to drain, and how many more of its bytes may be read.
    left: Mutex<Option<(B, u64)>>,
}

impl<B> Unread<B> {
    pub(super) fn new(clock: Arc<Clock>, limit: usize) -> Self {
        Self {
            clock,
            limit: u64::try_from(limit).unwrap_or(u64::MAX),
            closes: AtomicBool::new(false),
            deposited: AtomicBool::new(false),
            left: Mutex::new(None),
        }
    }

    /// A request starts: the last one has left nothing behind.
    pub(super) fn next_request(&self) {
        self.closes.store(false, Ordering::Relaxed);
    }

    /// The limit for a body of the request in flight.
    pub(super) fn limit(&self) -> UnreadLimit {
        UnreadLimit::new(self.limit)
    }

    /// Whether the answer to the request in flight closes the connection.
    pub(super) fn closes(&self) -> bool {
        self.closes.load(Ordering::Relaxed)
    }

    fn take(&self) -> Option<(B, u64)> {
        if !self.deposited.swap(false, Ordering::Acquire) {
            return None;
        }
        self.left
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

impl<B: HttpBody<Data = Bytes> + Unpin> Unread<B> {
    /// A handler let go of `body` before its end. One that is not `readable`
    /// (a client waiting on `100 Continue` that has not been asked for it, or
    /// a body that failed) is not read any further.
    fn let_go(&self, mut body: B, readable: bool, limit: u64) {
        // Without a runtime a body's pace cannot be timed: nothing is read.
        let rest = if readable && tokio::runtime::Handle::try_current().is_ok() {
            rest(&mut body, limit)
        } else {
            Rest::Unread
        };
        match rest {
            Rest::Ended => {}
            Rest::Drain(left) => {
                self.clock.left_unread(true);
                *self.left.lock().unwrap_or_else(PoisonError::into_inner) = Some((body, left));
                self.deposited.store(true, Ordering::Release);
                self.clock.wake();
            }
            Rest::Unread => {
                self.clock.left_unread(false);
                self.closes.store(true, Ordering::Relaxed);
            }
        }
    }
}

/// What is left of a body a handler let go of.
#[derive(Debug, PartialEq, Eq)]
enum Rest {
    /// Nothing: it has all arrived.
    Ended,
    /// Something, to be read: at most this many more bytes of it.
    Drain(u64),
    /// More than is read, or nothing more can be: the connection closes.
    Unread,
}

/// What is left of `body`, once what has already arrived is thrown away. It is
/// polled with no waker, so it gives only what hyper already holds.
fn rest<B: HttpBody<Data = Bytes> + Unpin>(body: &mut B, limit: u64) -> Rest {
    let mut cx = Context::from_waker(Waker::noop());
    let mut left = limit;
    for _ in 0..READS_PER_POLL {
        match Pin::new(&mut *body).poll_frame(&mut cx) {
            Poll::Ready(None) => return Rest::Ended,
            Poll::Ready(Some(Ok(frame))) => {
                if body.is_end_stream() {
                    return Rest::Ended;
                }
                match left.checked_sub(length(&frame)) {
                    Some(fewer) => left = fewer,
                    None => return Rest::Unread,
                }
            }
            Poll::Ready(Some(Err(_))) => return Rest::Unread,
            Poll::Pending => break,
        }
    }
    match body.size_hint().exact() {
        Some(declared) if declared > left => Rest::Unread,
        _ => Rest::Drain(left),
    }
}

fn length(frame: &Frame<Bytes>) -> u64 {
    frame
        .data_ref()
        .map_or(0, |data| u64::try_from(data.len()).unwrap_or(u64::MAX))
}

/// Whether the client of `parts` waits on `100 Continue` before it sends its
/// body, as hyper reads it: the last `Expect`, from an HTTP/1.1 client.
pub(super) fn waits_to_continue(parts: &Parts) -> bool {
    parts.version == Version::HTTP_11 && last_expect_is_continue(&parts.headers)
}

fn last_expect_is_continue(headers: &HeaderMap) -> bool {
    headers
        .get_all(EXPECT)
        .iter()
        .next_back()
        .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"100-continue"))
}

/// An HTTP/1 request body that hands what its handler did not read to the
/// connection (see the [module documentation](self)).
pub(super) struct Kept<B: HttpBody<Data = Bytes> + Unpin> {
    body: Option<B>,
    unread: Arc<Unread<B>>,
    limit: UnreadLimit,
    /// The client waits on `100 Continue`, and the body has not been asked for.
    waits: bool,
    ended: bool,
    failed: bool,
}

impl<B: HttpBody<Data = Bytes> + Unpin> Kept<B> {
    pub(super) fn new(body: B, unread: Arc<Unread<B>>, limit: UnreadLimit, waits: bool) -> Self {
        Self {
            body: Some(body),
            unread,
            limit,
            waits,
            ended: false,
            failed: false,
        }
    }
}

impl<B: HttpBody<Data = Bytes> + Unpin> HttpBody for Kept<B> {
    type Data = Bytes;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, B::Error>>> {
        let this = self.get_mut();
        let Some(body) = &mut this.body else {
            return Poll::Ready(None);
        };
        // Asking for the body is what sends `100 Continue`.
        this.waits = false;
        let polled = Pin::new(&mut *body).poll_frame(cx);
        match &polled {
            Poll::Ready(None) => this.ended = true,
            Poll::Ready(Some(Ok(_))) => this.ended = body.is_end_stream(),
            Poll::Ready(Some(Err(_))) => this.failed = true,
            Poll::Pending => {}
        }
        polled
    }

    fn is_end_stream(&self) -> bool {
        self.ended || self.body.as_ref().is_none_or(HttpBody::is_end_stream)
    }

    fn size_hint(&self) -> SizeHint {
        self.body
            .as_ref()
            .map_or_else(|| SizeHint::with_exact(0), HttpBody::size_hint)
    }
}

impl<B: HttpBody<Data = Bytes> + Unpin> Drop for Kept<B> {
    fn drop(&mut self) {
        let Some(body) = self.body.take() else {
            return;
        };
        if self.ended || body.is_end_stream() {
            return;
        }
        // A body that failed (too slow, say) is not read any further.
        let readable = !self.waits && !self.failed;
        self.unread.let_go(body, readable, self.limit.get());
    }
}

/// Reads the bodies a connection's handlers let go of to their end, after
/// their answers, so the connection can take the next request.
pub(super) struct Drain<B> {
    unread: Arc<Unread<B>>,
    current: Option<Draining<B>>,
    timer: Timer,
}

struct Draining<B> {
    body: B,
    /// How many more bytes may be read.
    left: u64,
    until: Instant,
}

impl<B: HttpBody<Data = Bytes> + Unpin> Drain<B> {
    pub(super) fn new(unread: Arc<Unread<B>>) -> Self {
        Self {
            unread,
            current: None,
            timer: None,
        }
    }

    /// Reads what it has been left. Never ready: it is polled next to the
    /// connection, for as long as the connection runs.
    pub(super) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        loop {
            if self.current.is_none() {
                let Some((body, left)) = self.unread.take() else {
                    return Poll::Pending;
                };
                self.current = Some(Draining {
                    body,
                    left,
                    until: Instant::now() + DRAIN_FOR,
                });
            }
            let Some(draining) = &mut self.current else {
                return Poll::Pending;
            };
            let to_its_end = std::task::ready!(draining.poll(cx, &mut self.timer));
            self.current = None;
            self.unread.clock.drained(to_its_end);
        }
    }

    /// The connection is closing: no request follows, so nothing more is read
    /// for one, and the close lingers instead.
    pub(super) fn give_up(&mut self) {
        let gone = self.current.take().map(|draining| draining.body);
        let left = self.unread.take().map(|(body, _)| body);
        if gone.is_some() || left.is_some() {
            self.unread.clock.drained(false);
        }
    }
}

impl<B: HttpBody<Data = Bytes> + Unpin> Draining<B> {
    /// Ready once the body has ended (`true`) or is given up.
    fn poll(&mut self, cx: &mut Context<'_>, timer: &mut Timer) -> Poll<bool> {
        for _ in 0..READS_PER_POLL {
            match Pin::new(&mut self.body).poll_frame(cx) {
                Poll::Ready(None) => return Poll::Ready(true),
                Poll::Ready(Some(Ok(frame))) => {
                    if self.body.is_end_stream() {
                        return Poll::Ready(true);
                    }
                    match self.left.checked_sub(length(&frame)) {
                        Some(fewer) => self.left = fewer,
                        None => return Poll::Ready(false),
                    }
                }
                Poll::Ready(Some(Err(_))) => return Poll::Ready(false),
                Poll::Pending => return expired(self.until, timer, cx).map(|()| false),
            }
        }
        if Instant::now() >= self.until {
            return Poll::Ready(false);
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// The close of a connection whose client may still be sending a body, once
/// the server has ended its side (see the [module documentation](self)).
#[derive(Debug)]
pub(super) struct Linger {
    until: Instant,
    quiet_until: Instant,
    timer: Timer,
    done: bool,
}

impl Linger {
    pub(super) fn new() -> Self {
        let now = Instant::now();
        Self {
            until: now + LINGER_AT_MOST,
            quiet_until: now + LINGER_QUIET,
            timer: None,
            done: false,
        }
    }

    /// Reads what arrives on `io` and throws it away. Ready once the client
    /// has closed its side, has gone quiet or the time is up.
    pub(super) fn poll_read<I: AsyncRead + Unpin>(
        &mut self,
        io: &mut I,
        cx: &mut Context<'_>,
    ) -> Poll<()> {
        let mut scratch = [MaybeUninit::<u8>::uninit(); LINGER_READ_BYTES];
        for _ in 0..READS_PER_POLL {
            if self.done {
                return Poll::Ready(());
            }
            let mut buffer = ReadBuf::uninit(&mut scratch);
            match Pin::new(&mut *io).poll_read(cx, &mut buffer) {
                Poll::Ready(Ok(())) if buffer.filled().is_empty() => self.done = true,
                Poll::Ready(Ok(())) => {
                    let now = Instant::now();
                    self.quiet_until = now + LINGER_QUIET;
                    self.done = now >= self.until;
                }
                Poll::Ready(Err(_)) => self.done = true,
                Poll::Pending => {
                    let deadline = self.until.min(self.quiet_until);
                    if expired(deadline, &mut self.timer, cx).is_pending() {
                        return Poll::Pending;
                    }
                    self.done = true;
                }
            }
        }
        if self.done {
            return Poll::Ready(());
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// Ready once `deadline` has passed. The time decides and the timer only
/// wakes the task, as in [`Clock::poll_expired`].
fn expired(deadline: Instant, timer: &mut Timer, cx: &mut Context<'_>) -> Poll<()> {
    if Instant::now() >= deadline {
        return Poll::Ready(());
    }
    let sleep = timer.get_or_insert_with(|| Box::pin(tokio::time::sleep_until(deadline)));
    if sleep.deadline() != deadline {
        sleep.as_mut().reset(deadline);
    }
    loop {
        std::task::ready!(sleep.as_mut().poll(cx));
        if Instant::now() >= deadline {
            return Poll::Ready(());
        }
        sleep.as_mut().reset(deadline);
    }
}

#[cfg(test)]
mod tests {
    use std::future::poll_fn;

    use futures_util::Stream;
    use http_body_util::{BodyExt, StreamBody};
    use tokio::io::{AsyncWriteExt, duplex};
    use tokio::sync::mpsc;

    use super::*;

    /// A body fed by hand, with the length it declares, if any.
    struct Fed {
        chunks: mpsc::UnboundedReceiver<Result<Bytes, &'static str>>,
        declared: Option<u64>,
        polled: Arc<AtomicBool>,
    }

    impl Stream for Fed {
        type Item = Result<Frame<Bytes>, &'static str>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            self.polled.store(true, Ordering::SeqCst);
            let polled = self.chunks.poll_recv(cx);
            if let Poll::Ready(Some(Ok(chunk))) = &polled
                && let Some(declared) = &mut self.declared
            {
                *declared = declared.saturating_sub(chunk.len() as u64);
            }
            polled.map(|chunk| chunk.map(|chunk| chunk.map(Frame::data)))
        }
    }

    /// `Fed` as a body: its size is what it still declares.
    struct FedBody(StreamBody<Fed>);

    impl HttpBody for FedBody {
        type Data = Bytes;
        type Error = &'static str;

        fn poll_frame(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, &'static str>>> {
            Pin::new(&mut self.0).poll_frame(cx)
        }

        fn is_end_stream(&self) -> bool {
            self.0.get_ref().declared == Some(0)
        }

        fn size_hint(&self) -> SizeHint {
            self.0
                .get_ref()
                .declared
                .map_or_else(SizeHint::default, SizeHint::with_exact)
        }
    }

    struct Feed {
        sender: mpsc::UnboundedSender<Result<Bytes, &'static str>>,
        polled: Arc<AtomicBool>,
    }

    impl Feed {
        fn send(&self, bytes: usize) {
            self.sender.send(Ok(Bytes::from(vec![b'x'; bytes]))).unwrap();
        }
    }

    fn fed(declared: Option<u64>) -> (Feed, FedBody) {
        let (sender, chunks) = mpsc::unbounded_channel();
        let polled = Arc::new(AtomicBool::new(false));
        let body = FedBody(StreamBody::new(Fed {
            chunks,
            declared,
            polled: polled.clone(),
        }));
        (Feed { sender, polled }, body)
    }

    fn connection() -> (Arc<Clock>, Arc<Unread<FedBody>>) {
        let clock = Arc::new(Clock::new(None, None));
        let unread = Arc::new(Unread::new(clock.clone(), 1024));
        (clock, unread)
    }

    /// `body` as a handler on `unread`'s connection gets it.
    fn kept(unread: &Arc<Unread<FedBody>>, body: FedBody, waits: bool) -> Kept<FedBody> {
        unread.next_request();
        Kept::new(body, unread.clone(), unread.limit(), waits)
    }

    #[test]
    fn what_has_arrived_is_thrown_away_and_the_rest_is_weighed_against_the_limit() {
        let (feed, mut body) = fed(Some(2048));
        feed.send(2048);
        assert_eq!(rest(&mut body, 10), Rest::Ended, "it has all arrived");

        let (feed, mut body) = fed(Some(2048));
        feed.send(1024);
        assert_eq!(rest(&mut body, 1024), Rest::Unread, "1,024 more to come");
        let (feed, mut body) = fed(Some(2048));
        feed.send(1024);
        assert_eq!(rest(&mut body, 2048), Rest::Drain(1024), "exactly the limit");

        let (_feed, mut body) = fed(Some(2049));
        assert_eq!(rest(&mut body, 2048), Rest::Unread, "one byte over");
        let (_feed, mut body) = fed(None);
        assert_eq!(rest(&mut body, 2048), Rest::Drain(2048), "a length unknown");

        let (feed, mut body) = fed(None);
        feed.send(4096);
        assert_eq!(rest(&mut body, 2048), Rest::Unread, "more than the limit came");
        let (feed, mut body) = fed(None);
        feed.sender.send(Err("broken")).unwrap();
        assert_eq!(rest(&mut body, 2048), Rest::Unread, "a body that failed");
    }

    #[test]
    fn the_last_expect_counts_and_only_from_http_1_1() {
        let parts = |version: Version, expects: &[&str]| {
            let mut request = http::Request::builder().version(version);
            for expect in expects {
                request = request.header(EXPECT, *expect);
            }
            request.body(()).unwrap().into_parts().0
        };
        assert!(waits_to_continue(&parts(Version::HTTP_11, &["100-continue"])));
        assert!(waits_to_continue(&parts(
            Version::HTTP_11,
            &["something-else", "100-Continue"]
        )));
        assert!(!waits_to_continue(&parts(
            Version::HTTP_11,
            &["100-continue", "something-else"]
        )));
        assert!(!waits_to_continue(&parts(Version::HTTP_10, &["100-continue"])));
        assert!(!waits_to_continue(&parts(Version::HTTP_11, &[])));
    }

    #[tokio::test]
    async fn a_body_read_to_its_end_leaves_nothing() {
        let (clock, unread) = connection();
        let (feed, body) = fed(Some(10));
        let mut body = kept(&unread, body, false);
        feed.send(10);
        assert_eq!(body.frame().await.unwrap().unwrap().into_data().unwrap().len(), 10);
        drop(body);
        assert!(!unread.closes());
        assert!(unread.take().is_none());
        assert!(!clock.lingers());
    }

    #[tokio::test]
    async fn a_body_within_the_limit_is_left_to_the_connection() {
        let (clock, unread) = connection();
        let (feed, body) = fed(Some(1000));
        feed.send(10);
        drop(kept(&unread, body, false));
        assert!(!unread.closes(), "the connection can go on");
        assert!(clock.lingers(), "until the rest has been read");

        let mut drain = Drain::new(unread.clone());
        let mut cx = Context::from_waker(Waker::noop());
        assert!(drain.poll(&mut cx).is_pending());
        feed.send(990);
        assert!(drain.poll(&mut cx).is_pending());
        assert!(drain.current.is_none(), "read to its end");
        assert!(!clock.lingers());
    }

    #[tokio::test]
    async fn a_body_over_the_limit_closes_the_connection_and_its_close_lingers() {
        let (clock, unread) = connection();
        let (_feed, body) = fed(Some(1025));
        drop(kept(&unread, body, false));
        assert!(unread.closes());
        assert!(unread.take().is_none(), "nothing of it is read");
        assert!(clock.lingers());

        unread.next_request();
        assert!(!unread.closes(), "a new request starts afresh");
    }

    #[tokio::test]
    async fn a_route_sets_the_limit() {
        let (_clock, unread) = connection();
        let (_feed, body) = fed(Some(4096));
        let body = kept(&unread, body, false);
        body.limit.set(4096);
        drop(body);
        assert!(!unread.closes());
        assert_eq!(unread.take().map(|(_, left)| left), Some(4096));
    }

    #[tokio::test]
    async fn a_client_waiting_on_100_continue_is_not_asked_for_its_body() {
        let (clock, unread) = connection();
        let (feed, body) = fed(Some(10));
        drop(kept(&unread, body, true));
        assert!(!feed.polled.load(Ordering::SeqCst), "no 100 Continue");
        assert!(unread.closes());
        assert!(clock.lingers(), "it may send the body all the same");

        // Once the handler has asked for it, the client has been told to send.
        let (feed, body) = fed(Some(10));
        let mut body = kept(&unread, body, true);
        let mut cx = Context::from_waker(Waker::noop());
        assert!(Pin::new(&mut body).poll_frame(&mut cx).is_pending());
        drop(body);
        assert!(feed.polled.load(Ordering::SeqCst));
        assert!(!unread.closes());
        assert!(unread.take().is_some());
    }

    #[tokio::test]
    async fn a_body_that_failed_is_not_read_any_further() {
        let (_clock, unread) = connection();
        let (feed, body) = fed(Some(100));
        let mut body = kept(&unread, body, false);
        feed.sender.send(Err("too slow")).unwrap();
        assert!(body.frame().await.unwrap().is_err());
        drop(body);
        assert!(unread.closes());
        assert!(unread.take().is_none());
    }

    #[test]
    fn without_a_runtime_nothing_is_read() {
        let (_clock, unread) = connection();
        let (feed, body) = fed(Some(10));
        drop(kept(&unread, body, false));
        assert!(!feed.polled.load(Ordering::SeqCst));
        assert!(unread.closes());
    }

    /// Drains what `unread` was left, in paused time: how long that took.
    async fn drained(unread: &Arc<Unread<FedBody>>) -> Duration {
        let mut drain = Drain::new(unread.clone());
        let started = Instant::now();
        poll_fn(|cx| {
            let _ = drain.poll(cx);
            if drain.current.is_none() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        started.elapsed()
    }

    #[tokio::test(start_paused = true)]
    async fn a_stalled_body_is_given_up_after_its_time_and_the_close_lingers() {
        let (clock, unread) = connection();
        let (feed, body) = fed(Some(100));
        feed.send(10);
        drop(kept(&unread, body, false));
        assert_eq!(drained(&unread).await, DRAIN_FOR);
        assert!(clock.lingers());
    }

    #[tokio::test(start_paused = true)]
    async fn a_body_that_goes_on_past_its_length_unknown_is_given_up_at_the_limit() {
        let (clock, unread) = connection();
        let (feed, body) = fed(None);
        drop(kept(&unread, body, false));
        for _ in 0..3 {
            feed.send(500);
        }
        assert_eq!(drained(&unread).await, Duration::ZERO, "1,500 bytes of 1,024");
        assert!(clock.lingers());
    }

    #[tokio::test(start_paused = true)]
    async fn a_closing_connection_reads_nothing_more() {
        let (clock, unread) = connection();
        let (_feed, body) = fed(Some(100));
        drop(kept(&unread, body, false));
        let mut drain = Drain::new(unread.clone());
        drain.give_up();
        assert!(unread.take().is_none());
        assert!(clock.lingers());
    }

    #[tokio::test(start_paused = true)]
    async fn a_lingering_close_reads_until_the_client_closes() {
        let (mut socket, mut peer) = duplex(64);
        let mut linger = Linger::new();
        let client = tokio::spawn(async move {
            for _ in 0..10 {
                tokio::time::sleep(LINGER_QUIET / 2).await;
                if peer.write_all(&[b'x'; 1024]).await.is_err() {
                    return false;
                }
            }
            true
        });
        let started = Instant::now();
        poll_fn(|cx| linger.poll_read(&mut socket, cx)).await;
        assert_eq!(started.elapsed(), LINGER_AT_MOST, "however much it sends");
        drop(socket);
        assert!(!client.await.unwrap(), "the client sees the close in the end");

        let (mut socket, peer) = duplex(64);
        let mut linger = Linger::new();
        drop(peer);
        poll_fn(|cx| linger.poll_read(&mut socket, cx)).await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_lingering_close_ends_when_the_client_goes_quiet() {
        let (mut socket, mut peer) = duplex(64);
        let mut linger = Linger::new();
        peer.write_all(b"the rest").await.unwrap();
        let started = Instant::now();
        poll_fn(|cx| linger.poll_read(&mut socket, cx)).await;
        assert_eq!(started.elapsed(), LINGER_QUIET);
    }
}
