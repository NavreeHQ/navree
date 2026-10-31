//! [`Clock`]: when a connection with no request in flight must close.
//!
//! hyper's HTTP/1 header read timeout cannot do this job. It starts as soon
//! as a connection goes idle, so it would close idle connections after a
//! header's worth of seconds; it does not cover the protocol detection, which
//! reads before hyper's HTTP/1 code runs (a client that sends nothing, or
//! only `P`, would never be timed); and HTTP/2 has no such timeout at all.
//! One clock on the connection covers every case the same way:
//!
//! - A new connection has `header_read` to start a request, whatever the
//!   protocol.
//! - Between two HTTP/1 requests it has `idle`; the first byte read then
//!   gives it `header_read` for the rest of the head, once. With no
//!   `header_read`, `idle` stands.
//! - A request without a body whose last read went past its head has left the
//!   start of the next head in hyper's buffer, where no read will see it, so
//!   `header_read` starts when the response ends. After a request with a body
//!   the two cannot be told apart and `idle` applies.
//! - Between two HTTP/2 requests it has `idle`, and nothing it sends extends
//!   that, so pings or half a header block cannot keep it open.
//! - While a request is in flight (from the service call to the end of the
//!   response body) the clock does not run: the request's own limits apply.
//!   A response body ending is not the response arriving, so the clock waits
//!   for what hyper has not written yet too, and that is what makes it the
//!   write stall limit's business. HTTP/2 holds a response's last byte back
//!   for the same reason: see [`first_part`](super::body::first_part).
//! - An HTTP/1 upgrade takes the socket out of hyper: the connection's task
//!   ends and nothing polls the clock again.
//! - An HTTP/2 upgrade (a WebSocket) is one stream of the connection, in
//!   flight until that stream ends, whatever the response said. The clock
//!   runs again then.
//!
//! The state changes on the connection's task, in the middle of polling
//! hyper, which is why [`Clock::poll_expired`] is polled right after hyper:
//! it sees every change with no wake-up, and a keep-alive request costs a
//! lock and no timer. An HTTP/2 stream ends on a task of its own and a
//! reclaim comes from the accept loop, and each wakes the connection.
//!
//! A read takes no lock unless it may start a head, and a write never does:
//! [`Watched`] keeps what it needs to itself.

use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker, ready};
use std::time::Duration;

use http::Version;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::{Instant, Sleep};

use super::frames::Frames;
use super::gate::Slot;

/// How long a connection with nothing in flight lives once it is closing:
/// enough for an HTTP/2 GOAWAY to go out, or for a head already on its way
/// to arrive.
const LINGER: Duration = Duration::from_secs(1);

/// A timer created the first time a deadline needs it.
pub(super) type Timer = Option<Pin<Box<Sleep>>>;

/// See the [module documentation](self).
#[derive(Debug)]
pub(super) struct Clock {
    header_read: Option<Duration>,
    idle: Option<Duration>,
    /// Between two HTTP/1 requests: the next byte read starts a head. Out of
    /// `state` so that the bytes of a request cost no lock.
    between_requests: AtomicBool,
    /// Whether the last read ended where a head ends.
    read_a_whole_head: AtomicBool,
    /// Nothing but upgraded traffic is left on this socket: the bytes of an
    /// HTTP/1 upgrade, or HTTP/2 streams that are all upgraded.
    only_upgraded: AtomicBool,
    /// Something written has not reached the socket yet: the end of a
    /// response body says nothing about that, and closing the connection
    /// would lose what hyper still holds. Cleared by a flush.
    behind: AtomicBool,
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    /// When the connection closes unless a request starts first.
    deadline: Option<Instant>,
    /// Requests from the service call to the end of the response body, and
    /// the tasks of HTTP/2 streams.
    in_flight: usize,
    /// How many of those are upgraded HTTP/2 streams.
    tunnels: usize,
    /// Since when the connection has had nothing in flight, for as long as a
    /// client that waits may have its slot: from the end of a request to the
    /// end of the next head, because a head that has only started has earned
    /// nothing.
    reclaimable_since: Option<Instant>,
    /// The last HTTP/1 request had no body.
    bodiless: bool,
    /// An HTTP/1 response has ended.
    answered_on_http1: bool,
    /// An HTTP/2 connection, known from its first request.
    multiplexed: bool,
    /// An HTTP/1 upgrade: the socket went with it.
    upgraded: bool,
    /// Shutting down: nothing extends the deadline any more.
    closing: bool,
    /// A client that waits took this connection's slot and it has not heard
    /// yet: a request that arrives first does not undo that.
    reclaimed: bool,
    /// The connection's task, for what happens on a task of its own.
    waker: Option<Waker>,
}

/// Whether `bytes`, a read from the socket, end where a request head ends. The
/// last bytes of a head can also arrive on their own.
fn ends_a_head(bytes: &[u8]) -> bool {
    matches!(
        bytes,
        [.., b'\n', b'\r', b'\n'] | [.., b'\n', b'\n'] | [b'\r', b'\n'] | [b'\n']
    )
}

impl Clock {
    pub(super) fn new(header_read: Option<Duration>, idle: Option<Duration>) -> Clock {
        Clock {
            header_read,
            idle,
            between_requests: AtomicBool::new(false),
            read_a_whole_head: AtomicBool::new(true),
            only_upgraded: AtomicBool::new(false),
            behind: AtomicBool::new(false),
            state: Mutex::new(State {
                deadline: header_read.map(|limit| Instant::now() + limit),
                in_flight: 0,
                tunnels: 0,
                reclaimable_since: None,
                bodiless: false,
                answered_on_http1: false,
                multiplexed: false,
                upgraded: false,
                closing: false,
                reclaimed: false,
                waker: None,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The handshake is done: the time to a first request starts now.
    pub(super) fn handshaken(&self) {
        self.lock().deadline = self.header_read.map(|limit| Instant::now() + limit);
    }

    /// `bytes` arrived. Takes the lock only for the first byte of a head.
    fn read(&self, bytes: &[u8]) {
        self.read_a_whole_head
            .store(ends_a_head(bytes), Ordering::Relaxed);
        if !self.between_requests.load(Ordering::Relaxed) {
            return;
        }
        let mut state = self.lock();
        if self.between_requests.swap(false, Ordering::Relaxed)
            && !state.closing
            && let Some(limit) = self.header_read
        {
            state.deadline = Some(Instant::now() + limit);
        }
    }

    /// A request head is complete: the service is about to run.
    pub(super) fn request(self: &Arc<Self>, version: Version, bodiless: bool) -> InFlight {
        let mut state = self.lock();
        state.in_flight += 1;
        if !state.reclaimed {
            state.deadline = None;
        }
        state.reclaimable_since = None;
        state.bodiless = bodiless;
        state.multiplexed |= version == Version::HTTP_2;
        if state.tunnels > 0 {
            self.only_upgraded.store(false, Ordering::Relaxed);
        }
        self.between_requests.store(false, Ordering::Relaxed);
        InFlight {
            clock: self.clone(),
            tunnel: false,
        }
    }

    /// A task of an HTTP/2 stream started; a `tunnel` is an upgraded stream.
    /// It stays in flight until the task ends, which is when the stream does.
    pub(super) fn stream(self: &Arc<Self>, tunnel: bool) -> InFlight {
        let mut state = self.lock();
        state.in_flight += 1;
        state.tunnels += usize::from(tunnel);
        if !state.reclaimed {
            state.deadline = None;
        }
        state.reclaimable_since = None;
        InFlight {
            clock: self.clone(),
            tunnel,
        }
    }

    /// Something in flight ended: with nothing left, back to waiting for a
    /// request.
    fn finished(&self, tunnel: bool) {
        let waker = {
            let mut state = self.lock();
            state.in_flight -= 1;
            state.tunnels -= usize::from(tunnel);
            if state.upgraded || state.in_flight > state.tunnels {
                return;
            }
            if state.in_flight > 0 {
                self.only_upgraded.store(true, Ordering::Relaxed);
            } else {
                self.only_upgraded.store(false, Ordering::Relaxed);
                self.await_request(&mut state);
            }
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Sets what happens next on a connection with nothing in flight.
    fn await_request(&self, state: &mut State) {
        let now = Instant::now();
        // An HTTP/1 response ends inside hyper's write, before its flush.
        if !state.multiplexed {
            state.answered_on_http1 = true;
            self.behind.store(true, Ordering::Relaxed);
        }
        let head_started =
            !state.multiplexed && state.bodiless && !self.read_a_whole_head.load(Ordering::Relaxed);
        self.between_requests
            .store(!state.multiplexed && !head_started, Ordering::Relaxed);
        state.reclaimable_since = (!state.closing).then_some(now);
        state.deadline = if state.closing {
            Some(now + LINGER)
        } else if head_started {
            self.header_read.or(self.idle).map(|limit| now + limit)
        } else {
            self.idle.map(|limit| now + limit)
        };
    }

    /// The server is shutting down: with nothing in flight the connection
    /// closes after [`LINGER`], or sooner if its deadline says so.
    pub(super) fn close_soon(&self) {
        let mut state = self.lock();
        state.closing = true;
        state.reclaimable_since = None;
        if state.in_flight == 0 && !state.upgraded {
            let linger = Instant::now() + LINGER;
            state.deadline = Some(state.deadline.map_or(linger, |at| at.min(linger)));
        }
    }

    /// The deadline passed and the connection is saying goodbye: [`LINGER`]
    /// more for its GOAWAY to go out, after whatever a reclaim found in
    /// flight.
    pub(super) fn say_goodbye(&self) {
        let mut state = self.lock();
        state.closing = true;
        state.reclaimed = false;
        state.reclaimable_since = None;
        state.deadline = (state.in_flight == 0).then(|| Instant::now() + LINGER);
    }

    /// Since when a client that waits may have this connection's slot, if it
    /// may: it has finished a request, has nothing in flight and nothing on
    /// its way to the socket, and is not closing already. A connection that
    /// has not finished one has `header_read` to do it instead, and loses the
    /// slot then.
    pub(super) fn reclaimable_since(&self) -> Option<Instant> {
        let state = self.lock();
        let on_its_way = self.response_on_its_way(&state);
        state.reclaimable_since.filter(|_| !on_its_way)
    }

    /// A client is waiting for this connection's slot: if it can still be
    /// had, the connection closes now, as at its idle timeout.
    pub(super) fn reclaim(&self) -> bool {
        let waker = {
            let mut state = self.lock();
            if self.response_on_its_way(&state) || state.reclaimable_since.take().is_none() {
                return false;
            }
            state.closing = true;
            state.reclaimed = true;
            state.deadline = Some(Instant::now());
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
        true
    }

    /// Whether a reclaimed connection is only finishing: nothing is left in
    /// flight on it, so its slot is on its way.
    pub(super) fn lingering(&self) -> bool {
        let state = self.lock();
        state.in_flight == 0 && !state.upgraded
    }

    /// Whether a connection whose deadline passed has to say goodbye first:
    /// a GOAWAY tells an HTTP/2 client it is closing, and a request racing a
    /// reclaim still gets its answer.
    pub(super) fn says_goodbye(&self) -> bool {
        let state = self.lock();
        state.multiplexed || state.in_flight > 0
    }

    /// Whether nothing but upgraded traffic is left on the socket.
    fn only_upgraded(&self) -> bool {
        self.only_upgraded.load(Ordering::Relaxed)
    }

    /// Whether the connection is known to be HTTP/2.
    fn is_http2(&self) -> bool {
        self.lock().multiplexed
    }

    /// Whether what hyper holds of an HTTP/1 response has yet to reach the
    /// socket. Only HTTP/1: all it writes between requests is a response. On
    /// HTTP/2 a client can make the server write with a flood of frames, and
    /// must not hold off a deadline by not reading the answers, and a closing
    /// connection gets to finish its writes in its linger.
    fn response_on_its_way(&self, state: &State) -> bool {
        state.answered_on_http1 && self.behind.load(Ordering::Relaxed)
    }

    /// A write did not go through: the socket is full.
    fn fell_behind(&self) {
        self.behind.store(true, Ordering::Relaxed);
    }

    /// Everything written so far has reached the socket.
    fn flushed(&self) {
        self.behind.store(false, Ordering::Relaxed);
    }

    /// Ready when the deadline passes. Poll it right after the connection,
    /// so it sees what that poll changed (see the module documentation).
    ///
    /// The time is compared here instead of being left to the timer: tokio
    /// counts what a task does per poll, and a timer answers `Pending` once
    /// that budget is spent. hyper reading a steady flood of frames spends
    /// all of it on every poll, so a timer that has fired would never be
    /// heard.
    pub(super) fn poll_expired(&self, timer: &mut Timer, cx: &mut Context<'_>) -> Poll<()> {
        let deadline = {
            let mut state = self.lock();
            if !state
                .waker
                .as_ref()
                .is_some_and(|waker| waker.will_wake(cx.waker()))
            {
                state.waker = Some(cx.waker().clone());
            }
            // A response still on its way is not idle, and the write that is
            // waiting wakes this task. The write stall limit is what ends a
            // client that has stopped taking it.
            match state.deadline {
                Some(deadline) if state.closing || !self.response_on_its_way(&state) => deadline,
                _ => return Poll::Pending,
            }
        };
        if Instant::now() >= deadline {
            return Poll::Ready(());
        }
        let sleep = timer.get_or_insert_with(|| Box::pin(tokio::time::sleep_until(deadline)));
        // A later deadline is picked up when the timer fires: a request's
        // worth of changes does not touch the timer wheel.
        if deadline < sleep.deadline() {
            sleep.as_mut().reset(deadline);
        }
        loop {
            ready!(sleep.as_mut().poll(cx));
            if Instant::now() >= deadline {
                return Poll::Ready(());
            }
            sleep.as_mut().reset(deadline);
        }
    }
}

/// Something in flight on a [`Clock`]; dropped when it ends.
#[derive(Debug)]
pub(super) struct InFlight {
    clock: Arc<Clock>,
    tunnel: bool,
}

impl InFlight {
    /// The response hands an HTTP/1 connection to another protocol: the
    /// socket goes with it, to the upgrade or to its end, and no more HTTP
    /// runs on it. On HTTP/2 only a stream is upgraded, and only the end of
    /// that stream counts: this does nothing.
    pub(super) fn upgraded(&self) {
        let mut state = self.clock.lock();
        if !state.multiplexed {
            state.upgraded = true;
            self.clock.only_upgraded.store(true, Ordering::Relaxed);
        }
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.clock.finished(self.tunnel);
    }
}

/// One limit on a socket: nothing runs until it is armed.
#[derive(Debug)]
struct Alarm {
    limit: Duration,
    deadline: Option<Instant>,
    timer: Timer,
}

impl Alarm {
    fn new(limit: Duration) -> Self {
        Self {
            limit,
            deadline: None,
            timer: None,
        }
    }

    fn clear(&mut self) {
        self.deadline = None;
    }

    /// The limit starts over from now.
    fn restart(&mut self) {
        self.deadline = Some(Instant::now() + self.limit);
    }

    /// The error when the limit passes, counted from the first poll since it
    /// was cleared. The time is compared here, as in [`Clock::poll_expired`]:
    /// the timer only wakes the task.
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<io::Error> {
        let now = Instant::now();
        let deadline = *self.deadline.get_or_insert(now + self.limit);
        if now >= deadline {
            return Poll::Ready(io::ErrorKind::TimedOut.into());
        }
        let sleep = self
            .timer
            .get_or_insert_with(|| Box::pin(tokio::time::sleep_until(deadline)));
        if sleep.deadline() != deadline {
            sleep.as_mut().reset(deadline);
        }
        let _ = sleep.as_mut().poll(cx);
        Poll::Pending
    }
}

/// A connection's bytes on their way to hyper. It tells the [`Clock`] when
/// bytes arrive, times what only the socket can see (a write that does not
/// move, an upgrade gone quiet), and holds the connection's [`Slot`], so the
/// slot follows the socket through an upgrade and is freed when the socket
/// closes.
#[derive(Debug)]
pub(super) struct Watched<I> {
    io: I,
    clock: Arc<Clock>,
    write_stall: Option<Alarm>,
    upgraded_idle: Option<Quiet>,
    _slot: Slot,
}

/// `http.server.upgraded_idle_timeout_ms` on one connection.
#[derive(Debug)]
struct Quiet {
    alarm: Alarm,
    frames: Frames,
}

impl<I> Watched<I> {
    pub(super) fn new(
        io: I,
        clock: Arc<Clock>,
        slot: Slot,
        write_stall: Option<Duration>,
        upgraded_idle: Option<Duration>,
    ) -> Self {
        Self {
            io,
            clock,
            write_stall: write_stall.map(Alarm::new),
            upgraded_idle: upgraded_idle.map(|limit| Quiet {
                alarm: Alarm::new(limit),
                frames: Frames::new(),
            }),
            _slot: slot,
        }
    }

    /// `bytes` arrived. They are all that shows a client is there. What the
    /// server writes says nothing, or a protocol's heartbeat would keep a
    /// silent client's socket open, and on HTTP/2 neither do the frames that
    /// look after the connection. On HTTP/1 the bytes are the upgrade's own.
    fn heard(&mut self, bytes: &[u8]) {
        let Some(quiet) = &mut self.upgraded_idle else {
            return;
        };
        let data = quiet.frames.read_data(bytes);
        if !self.clock.only_upgraded() {
            quiet.alarm.clear();
        } else if data || !self.clock.is_http2() {
            quiet.alarm.restart();
        }
    }

    /// Ready with the error once an upgraded socket has gone quiet for too
    /// long. Called when a read has nothing to give.
    fn poll_quiet(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.upgraded_idle {
            Some(quiet) if self.clock.only_upgraded() => quiet.alarm.poll(cx).map(Err),
            _ => Poll::Pending,
        }
    }

    /// What a write polled to.
    fn wrote(
        &mut self,
        polled: Poll<io::Result<usize>>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<usize>> {
        match polled {
            Poll::Ready(Ok(written)) if written > 0 => {
                if let Some(stall) = &mut self.write_stall {
                    stall.clear();
                }
                Poll::Ready(Ok(written))
            }
            Poll::Pending => {
                self.clock.fell_behind();
                self.poll_stall(cx).map(Err)
            }
            other => other,
        }
    }

    /// The error once a write has not moved for too long. Each byte the
    /// socket takes starts it over, so only a client that takes nothing at
    /// all runs out.
    fn poll_stall(&mut self, cx: &mut Context<'_>) -> Poll<io::Error> {
        match &mut self.write_stall {
            Some(stall) => stall.poll(cx),
            None => Poll::Pending,
        }
    }
}

impl<I: AsyncRead + Unpin> AsyncRead for Watched<I> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        match Pin::new(&mut this.io).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                let read = &buf.filled()[before..];
                if !read.is_empty() {
                    this.clock.read(read);
                    this.heard(read);
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => this.poll_quiet(cx),
        }
    }
}

impl<I: AsyncWrite + Unpin> AsyncWrite for Watched<I> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let polled = Pin::new(&mut this.io).poll_write(cx, buf);
        this.wrote(polled, cx)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match Pin::new(&mut this.io).poll_flush(cx) {
            Poll::Ready(Ok(())) => {
                this.clock.flushed();
                if let Some(stall) = &mut this.write_stall {
                    stall.clear();
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => {
                this.clock.fell_behind();
                this.poll_stall(cx).map(Err)
            }
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match Pin::new(&mut this.io).poll_shutdown(cx) {
            Poll::Pending => this.poll_stall(cx).map(Err),
            ready => ready,
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let polled = Pin::new(&mut this.io).poll_write_vectored(cx, bufs);
        this.wrote(polled, cx)
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }
}

#[cfg(test)]
mod tests {
    use std::future::poll_fn;

    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

    use super::*;

    const HEADER: Duration = Duration::from_millis(100);
    const IDLE: Duration = Duration::from_millis(1000);
    const HEAD: &[u8] = b"GET / HTTP/1.1\r\n\r\n";

    fn clock() -> Arc<Clock> {
        Arc::new(Clock::new(Some(HEADER), Some(IDLE)))
    }

    /// Runs the clock from now until it expires; how long that took. Time
    /// is paused, so a clock that never expires fails here at once.
    async fn runs_for(clock: &Clock) -> Duration {
        let started = Instant::now();
        let mut timer = None;
        tokio::time::timeout(
            Duration::from_secs(3600),
            poll_fn(|cx| clock.poll_expired(&mut timer, cx)),
        )
        .await
        .expect("the clock expires");
        started.elapsed()
    }

    fn pending(clock: &Clock) -> bool {
        let mut timer = None;
        let mut cx = Context::from_waker(Waker::noop());
        clock.poll_expired(&mut timer, &mut cx).is_pending()
    }

    /// One whole request and its response, all of it on the socket.
    fn served(clock: &Arc<Clock>, version: Version) {
        clock.read(HEAD);
        drop(clock.request(version, true));
        clock.flushed();
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_connection_has_the_header_timeout_however_many_bytes_it_sends() {
        let clock = clock();
        clock.read(b"G");
        clock.read(b"E");
        assert_eq!(runs_for(&clock).await, HEADER);
    }

    #[tokio::test(start_paused = true)]
    async fn a_handshake_does_not_spend_the_header_timeout() {
        let clock = clock();
        tokio::time::advance(HEADER / 2).await;
        clock.handshaken();
        assert_eq!(runs_for(&clock).await, HEADER);
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_in_flight_stops_the_clock_and_its_end_starts_the_idle_timeout() {
        let clock = clock();
        clock.read(HEAD);
        let request = clock.request(Version::HTTP_11, true);
        tokio::time::advance(HEADER * 5).await;
        assert!(pending(&clock), "no deadline while a request is in flight");
        drop(request);
        clock.flushed();
        assert_eq!(runs_for(&clock).await, IDLE);
    }

    #[tokio::test(start_paused = true)]
    async fn the_first_byte_between_http1_requests_starts_the_header_timeout_once() {
        let clock = clock();
        served(&clock, Version::HTTP_11);
        tokio::time::advance(IDLE / 2).await;
        clock.read(b"G");
        tokio::time::advance(HEADER / 2).await;
        clock.read(b"E");
        assert_eq!(runs_for(&clock).await, HEADER / 2, "not extended");
    }

    /// With no header timeout there is nothing to replace it with: the idle
    /// deadline stays.
    #[tokio::test(start_paused = true)]
    async fn the_first_byte_leaves_the_idle_deadline_with_no_header_timeout() {
        let clock = Arc::new(Clock::new(None, Some(IDLE)));
        served(&clock, Version::HTTP_11);
        tokio::time::advance(IDLE / 2).await;
        clock.read(b"G");
        assert_eq!(runs_for(&clock).await, IDLE / 2);
    }

    /// hyper keeps what follows a head in its buffer, and no read will show
    /// it: the response's end starts the time for that head.
    #[tokio::test(start_paused = true)]
    async fn a_read_past_a_head_without_a_body_starts_the_header_timeout() {
        let clock = clock();
        clock.read(b"GET / HTTP/1.1\r\n\r\nGET / HT");
        drop(clock.request(Version::HTTP_11, true));
        clock.flushed();
        assert_eq!(runs_for(&clock).await, HEADER);
    }

    /// Sending the start of a head with every request must not keep a slot
    /// from a client that waits, in a read of its own or left in hyper's
    /// buffer.
    #[tokio::test(start_paused = true)]
    async fn a_head_that_has_only_started_keeps_nothing_from_a_client_that_waits() {
        let clock = clock();
        clock.read(b"GET / HTTP/1.1\r\n\r\nGET / HT");
        drop(clock.request(Version::HTTP_11, true));
        clock.flushed();
        let finished = Instant::now();
        assert_eq!(clock.reclaimable_since(), Some(finished));

        tokio::time::advance(HEADER / 2).await;
        clock.read(b"TP/1.1\r\n\r\nG");
        drop(clock.request(Version::HTTP_11, true));
        clock.flushed();
        let finished = Instant::now();
        tokio::time::advance(HEADER / 2).await;
        clock.read(b"E");
        assert_eq!(clock.reclaimable_since(), Some(finished), "oldest first");
    }

    #[tokio::test(start_paused = true)]
    async fn with_no_header_timeout_a_pipelined_head_has_the_idle_timeout() {
        let clock = Arc::new(Clock::new(None, Some(IDLE)));
        clock.read(b"GET / HTTP/1.1\r\n\r\nGET / HT");
        drop(clock.request(Version::HTTP_11, true));
        clock.flushed();
        assert_eq!(runs_for(&clock).await, IDLE);
    }

    /// A body ends anywhere, so a read that ends mid-line says nothing.
    #[tokio::test(start_paused = true)]
    async fn a_body_that_does_not_end_like_a_head_is_not_a_pipelined_head() {
        let clock = clock();
        clock.read(b"POST / HTTP/1.1\r\nContent-Length: 4\r\n\r\nbody");
        drop(clock.request(Version::HTTP_11, false));
        clock.flushed();
        assert_eq!(runs_for(&clock).await, IDLE);
    }

    #[test]
    fn heads_end_with_an_empty_line() {
        for head in [
            &b"GET / HTTP/1.1\r\n\r\n"[..],
            b"GET / HTTP/1.1\n\n",
            b"GET / HTTP/1.1\r\n\n",
            b"\n",
            b"\r\n",
        ] {
            assert!(ends_a_head(head), "{head:?}");
        }
        for bytes in [&b"GET / HT"[..], b"GET / HTTP/1.1\r\n", b"", b"\r"] {
            assert!(!ends_a_head(bytes), "{bytes:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn nothing_an_http2_client_sends_extends_the_idle_timeout() {
        let clock = clock();
        served(&clock, Version::HTTP_2);
        tokio::time::advance(IDLE / 2).await;
        clock.read(b"G");
        assert_eq!(runs_for(&clock).await, IDLE / 2);
    }

    #[tokio::test(start_paused = true)]
    async fn an_http2_stream_ending_on_its_own_task_wakes_the_connection() {
        let clock = clock();
        let request = clock.request(Version::HTTP_2, true);
        let waiting = tokio::spawn({
            let clock = clock.clone();
            async move { runs_for(&clock).await }
        });
        tokio::task::yield_now().await;
        drop(request);
        assert_eq!(waiting.await.unwrap(), IDLE);
    }

    /// The answer to a `CONNECT` says nothing: the stream's task does.
    #[tokio::test(start_paused = true)]
    async fn an_http2_upgrade_holds_the_clock_until_its_stream_ends() {
        let clock = clock();
        let request = clock.request(Version::HTTP_2, true);
        let stream = clock.stream(false);
        let tunnel = clock.stream(true);
        request.upgraded();
        drop(request);
        drop(stream);
        tokio::time::advance(IDLE * 2).await;
        assert!(pending(&clock), "the upgraded stream is still open");
        assert!(clock.only_upgraded());
        assert_eq!(clock.reclaimable_since(), None);
        drop(tunnel);
        assert!(!clock.only_upgraded());
        assert_eq!(runs_for(&clock).await, IDLE);
    }

    /// With no stream behind it, a 2xx to `CONNECT` is any other response.
    #[tokio::test(start_paused = true)]
    async fn an_http2_connect_nobody_upgrades_leaves_the_clock_running() {
        let clock = clock();
        let request = clock.request(Version::HTTP_2, true);
        request.upgraded();
        drop(request);
        assert_eq!(runs_for(&clock).await, IDLE);
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_next_to_an_upgraded_stream_is_more_than_upgraded_traffic() {
        let clock = clock();
        let tunnel = clock.stream(true);
        drop(clock.request(Version::HTTP_2, true));
        assert!(clock.only_upgraded());
        let request = clock.request(Version::HTTP_2, true);
        assert!(!clock.only_upgraded());
        drop(request);
        assert!(clock.only_upgraded());
        drop(tunnel);
    }

    #[tokio::test(start_paused = true)]
    async fn an_http1_upgrade_stops_the_clock_and_cannot_be_reclaimed() {
        let clock = clock();
        let request = clock.request(Version::HTTP_11, true);
        request.upgraded();
        drop(request);
        tokio::time::advance(IDLE * 2).await;
        assert!(pending(&clock));
        assert_eq!(clock.reclaimable_since(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn turned_off_limits_never_expire_until_a_shutdown() {
        let clock = Arc::new(Clock::new(None, None));
        assert!(pending(&clock));
        served(&clock, Version::HTTP_11);
        assert!(pending(&clock));
        clock.read(b"G");
        assert!(pending(&clock));
        clock.close_soon();
        assert_eq!(runs_for(&clock).await, LINGER);
    }

    #[tokio::test(start_paused = true)]
    async fn a_shutdown_keeps_an_earlier_deadline_and_a_request_in_flight() {
        let clock = clock();
        clock.close_soon();
        assert_eq!(runs_for(&clock).await, HEADER, "sooner than the linger");

        let clock = Arc::new(Clock::new(Some(HEADER), Some(IDLE)));
        let request = clock.request(Version::HTTP_11, true);
        clock.close_soon();
        assert!(pending(&clock), "the request finishes first");
        drop(request);
        clock.read(b"G");
        assert_eq!(runs_for(&clock).await, LINGER, "bytes extend nothing");
    }

    /// The deadline that just passed must not end the goodbye before the
    /// GOAWAY has gone out.
    #[tokio::test(start_paused = true)]
    async fn a_goodbye_gets_the_linger_after_an_expired_deadline() {
        let clock = clock();
        served(&clock, Version::HTTP_2);
        assert_eq!(runs_for(&clock).await, IDLE);
        clock.say_goodbye();
        assert_eq!(runs_for(&clock).await, LINGER);
    }

    #[tokio::test(start_paused = true)]
    async fn only_a_connection_that_has_finished_a_request_can_be_reclaimed() {
        let clock = clock();
        assert_eq!(clock.reclaimable_since(), None, "a new connection");
        clock.read(b"G");
        assert_eq!(clock.reclaimable_since(), None, "its first head");
        let request = clock.request(Version::HTTP_11, true);
        assert_eq!(clock.reclaimable_since(), None, "a request is in flight");
        drop(request);
        assert_eq!(clock.reclaimable_since(), None, "it has not been flushed");
        clock.flushed();
        assert_eq!(clock.reclaimable_since(), Some(Instant::now()));
    }

    /// The body ended and what hyper holds of it has not reached the socket.
    #[tokio::test(start_paused = true)]
    async fn a_connection_behind_on_its_writes_cannot_be_reclaimed() {
        let clock = clock();
        served(&clock, Version::HTTP_11);
        clock.fell_behind();
        assert_eq!(clock.reclaimable_since(), None);
        assert!(!clock.reclaim());
        assert!(pending(&clock), "it does not close");
        clock.flushed();
        assert!(clock.reclaimable_since().is_some());
        assert!(clock.reclaim());
    }

    #[tokio::test(start_paused = true)]
    async fn the_idle_timeout_waits_for_a_response_on_its_way() {
        let clock = clock();
        served(&clock, Version::HTTP_11);
        clock.fell_behind();
        tokio::time::advance(IDLE * 2).await;
        assert!(pending(&clock), "the response is still going out");
        clock.flushed();
        assert_eq!(runs_for(&clock).await, Duration::ZERO, "it was due");
    }

    /// Past the end of the server, a client that is not reading does not get
    /// to keep the connection.
    #[tokio::test(start_paused = true)]
    async fn a_shutdown_does_not_wait_for_a_response_on_its_way() {
        let clock = clock();
        served(&clock, Version::HTTP_11);
        clock.fell_behind();
        clock.close_soon();
        assert_eq!(runs_for(&clock).await, LINGER);
    }

    /// An HTTP/2 client can make the server write by flooding it, so a socket
    /// that is full must not hold off a deadline or a reclaim, and neither
    /// must one before the first request.
    #[tokio::test(start_paused = true)]
    async fn a_full_socket_holds_off_nothing_on_http2() {
        let clock = clock();
        clock.fell_behind();
        assert_eq!(runs_for(&clock).await, HEADER, "the first request");

        served(&clock, Version::HTTP_2);
        clock.fell_behind();
        assert!(clock.reclaimable_since().is_some());
        assert_eq!(runs_for(&clock).await, IDLE);
    }

    #[tokio::test(start_paused = true)]
    async fn a_reclaimed_connection_closes_now_and_wakes_up_to_do_it() {
        let clock = clock();
        served(&clock, Version::HTTP_11);
        let waiting = tokio::spawn({
            let clock = clock.clone();
            async move { runs_for(&clock).await }
        });
        tokio::task::yield_now().await;
        tokio::time::advance(IDLE / 4).await;
        clock.reclaim();
        assert_eq!(waiting.await.unwrap(), IDLE / 4);
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_that_started_is_not_reclaimed() {
        let clock = clock();
        served(&clock, Version::HTTP_11);
        let request = clock.request(Version::HTTP_11, true);
        assert!(!clock.reclaim());
        assert!(pending(&clock));
        drop(request);
    }

    /// The head that arrives in the same moment as a reclaim gets its answer,
    /// but the slot has been given away, and only the connection closing
    /// hands it over. Nothing the client does after that keeps it.
    #[tokio::test(start_paused = true)]
    async fn a_request_racing_a_reclaim_does_not_undo_it() {
        let clock = clock();
        served(&clock, Version::HTTP_11);
        assert!(clock.reclaim());
        let request = clock.request(Version::HTTP_11, true);
        assert!(clock.says_goodbye(), "the request is answered");
        assert_eq!(runs_for(&clock).await, Duration::ZERO, "the deadline stays");

        clock.say_goodbye();
        assert!(pending(&clock), "no deadline while it is in flight");
        drop(request);
        clock.flushed();
        assert_eq!(clock.reclaimable_since(), None, "it was reclaimed");
        assert_eq!(runs_for(&clock).await, LINGER);
    }

    fn watched<I>(
        io: I,
        clock: &Arc<Clock>,
        write_stall: Option<Duration>,
        upgraded_idle: Option<Duration>,
    ) -> Watched<I> {
        Watched::new(
            io,
            clock.clone(),
            Slot::default(),
            write_stall,
            upgraded_idle,
        )
    }

    const STALL: Duration = Duration::from_secs(5);

    /// Writes until the pipe is full, then waits for the error.
    async fn write_until_stalled<I: AsyncWrite + Unpin>(socket: &mut I) -> io::Error {
        loop {
            if let Err(error) = socket.write_all(&[0; 1024]).await {
                return error;
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_write_that_does_not_move_times_out() {
        let (mut socket, _peer) = duplex(4096);
        let started = Instant::now();
        let mut socket = watched(&mut socket, &clock(), Some(STALL), None);
        let error = write_until_stalled(&mut socket).await;
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(started.elapsed(), STALL);
    }

    #[tokio::test(start_paused = true)]
    async fn with_no_limit_a_stalled_write_waits() {
        let (socket, _peer) = duplex(4096);
        let mut socket = watched(socket, &clock(), None, None);
        let stalled = write_until_stalled(&mut socket);
        assert!(
            tokio::time::timeout(STALL * 100, stalled).await.is_err(),
            "nothing ends the write"
        );
    }

    /// A reader far slower than the limit, whose every read lets a write
    /// through in time: each byte starts the limit over.
    #[tokio::test(start_paused = true)]
    async fn a_write_that_keeps_moving_does_not_time_out() {
        let (socket, mut peer) = duplex(1024);
        let mut socket = watched(socket, &clock(), Some(STALL), None);
        let reader = tokio::spawn(async move {
            let mut chunk = [0; 256];
            let mut total = 0;
            while total < 64 * 1024 {
                tokio::time::sleep(STALL / 2).await;
                total += peer.read(&mut chunk).await.unwrap();
            }
        });
        socket.write_all(&[7; 64 * 1024]).await.unwrap();
        reader.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_quiet_upgrade_times_out_and_traffic_starts_the_limit_over() {
        let clock = clock();
        let (socket, mut peer) = duplex(4096);
        let mut socket = watched(socket, &clock, None, Some(STALL));
        clock.request(Version::HTTP_11, true).upgraded();

        let started = Instant::now();
        tokio::spawn(async move {
            tokio::time::sleep(STALL / 2).await;
            peer.write_all(b"hello").await.unwrap();
            std::future::pending::<()>().await;
        });
        let mut chunk = [0; 8];
        assert_eq!(socket.read(&mut chunk).await.unwrap(), 5);
        let error = socket.read(&mut chunk).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(started.elapsed(), STALL / 2 + STALL);
    }

    #[tokio::test(start_paused = true)]
    async fn a_connection_that_is_not_upgraded_may_be_quiet() {
        let clock = clock();
        let (socket, _peer) = duplex(4096);
        let mut socket = watched(socket, &clock, None, Some(STALL));
        let mut chunk = [0; 8];
        let read = socket.read(&mut chunk);
        assert!(tokio::time::timeout(STALL * 100, read).await.is_err());
    }

    /// Reads `socket` until it fails: the error must be the limit.
    async fn reads_until_quiet<I: AsyncRead + Unpin>(socket: &mut I) {
        let mut chunk = [0; 64];
        let error = loop {
            match socket.read(&mut chunk).await {
                Ok(_) => {}
                Err(error) => break error,
            }
        };
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    /// A heartbeat of the server's says nothing about the client.
    #[tokio::test(start_paused = true)]
    async fn what_the_server_writes_is_not_traffic() {
        let clock = clock();
        let (socket, _peer) = duplex(1024 * 1024);
        let socket = watched(socket, &clock, None, Some(STALL));
        clock.request(Version::HTTP_11, true).upgraded();
        let (mut reader, mut writer) = tokio::io::split(socket);
        let started = Instant::now();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(STALL / 4).await;
                if writer.write_all(b"ping").await.is_err() {
                    return;
                }
            }
        });
        reads_until_quiet(&mut reader).await;
        assert_eq!(started.elapsed(), STALL);
    }

    /// What is on an HTTP/1 upgrade is the upgrade's, whatever it looks like:
    /// here, an HTTP/2 PING.
    #[tokio::test(start_paused = true)]
    async fn every_byte_of_an_http1_upgrade_is_traffic() {
        let mut ping = vec![0, 0, 8, 6, 0, 0, 0, 0, 0];
        ping.extend_from_slice(&[0; 8]);
        let clock = clock();
        let (socket, mut peer) = duplex(4096);
        let mut socket = watched(socket, &clock, None, Some(STALL));
        clock.request(Version::HTTP_11, true).upgraded();
        let started = Instant::now();
        tokio::spawn(async move {
            for _ in 0..8 {
                tokio::time::sleep(STALL / 2).await;
                peer.write_all(&ping).await.unwrap();
            }
            std::future::pending::<()>().await;
        });
        reads_until_quiet(&mut socket).await;
        assert_eq!(started.elapsed(), STALL * 4 + STALL);
    }

    /// Keep-alive is a connection talking to itself: it shows nobody is on
    /// the upgraded stream, in a read of its own or in one with several.
    #[tokio::test(start_paused = true)]
    async fn only_data_is_traffic_on_an_http2_upgrade() {
        let mut ping = vec![0, 0, 8, 6, 0, 0, 0, 0, 0];
        ping.extend_from_slice(&[0; 8]);
        let mut pings = ping.clone();
        pings.extend_from_slice(&ping);
        let mut data = vec![0, 0, 1, 0, 0, 0, 0, 0, 1];
        data.push(7);

        let clock = clock();
        let (socket, mut peer) = duplex(4096);
        let mut socket = watched(socket, &clock, None, Some(STALL));
        let _tunnel = clock.stream(true);
        drop(clock.request(Version::HTTP_2, true));
        assert!(clock.only_upgraded());
        let started = Instant::now();
        tokio::spawn(async move {
            peer.write_all(&[0; 24]).await.unwrap();
            tokio::time::sleep(STALL / 2).await;
            peer.write_all(&data).await.unwrap();
            for _ in 0..8 {
                tokio::time::sleep(STALL / 4).await;
                if peer.write_all(&pings).await.is_err() {
                    return;
                }
            }
            std::future::pending::<()>().await;
        });
        reads_until_quiet(&mut socket).await;
        assert_eq!(started.elapsed(), STALL / 2 + STALL);
    }
}
