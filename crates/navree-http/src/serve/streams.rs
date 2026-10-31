//! The tasks of an HTTP/2 connection's streams.
//!
//! hyper runs each stream on a task of its own, spawned on the connection's
//! [`Streams`] executor, and so is the send half of an upgraded stream. That
//! makes the executor the one place that sees a stream start and end, however
//! it ends:
//!
//! - An upgraded stream gives the service nothing to hold on to. The response
//!   to its `CONNECT` is dropped as soon as its head goes out, and the stream
//!   lives on inside the socket a handler got. Its task ends with the stream,
//!   which is how the [`Clock`] knows an HTTP/2 WebSocket is over.
//! - A response waiting for flow control is not polled at all, so no body can
//!   time it. Its task can: past `http.server.write_stall_timeout_ms` the task
//!   ends, dropping the stream, and h2 resets it. The connection stays. The
//!   task of an upgraded stream has no response to tell it how far it has got,
//!   and is left to `http.server.upgraded_idle_timeout_ms`.
//!
//! What counts as progress is the response body being polled for more, which
//! only happens once the client has taken what it was given.
//! [`ResponseBody`](super::body::ResponseBody) tells [`Progress`] when that
//! is.

use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use hyper::rt::Executor;
use pin_project_lite::pin_project;
use tokio::time::Instant;

use super::clock::{Clock, InFlight, Timer};

/// Not waiting: see [`Progress::since`].
const NOT_WAITING: u64 = 0;

thread_local! {
    /// The stream whose service was just called. hyper calls the service and
    /// spawns the stream's task in one go, on the connection's task, with
    /// nothing in between. A task spawned with nothing here is the send half
    /// of an upgraded stream, spawned by the stream's own task.
    static ANNOUNCED: Cell<Option<Arc<Progress>>> = const { Cell::new(None) };
}

/// How far a response is: whether it is waiting for its client to take more,
/// and since when. Written by the response's body and read by the stream's
/// task, on the same thread most of the time, and with no lock.
#[derive(Debug)]
pub(super) struct Progress {
    started: Instant,
    /// Milliseconds from `started` to when the wait began, plus one, or
    /// [`NOT_WAITING`].
    since: AtomicU64,
}

impl Progress {
    /// Tells the executor the service was called for a new HTTP/2 stream;
    /// the [`Progress`] of its response.
    pub(super) fn announce() -> Arc<Self> {
        let progress = Arc::new(Self {
            started: Instant::now(),
            since: AtomicU64::new(NOT_WAITING),
        });
        ANNOUNCED.set(Some(progress.clone()));
        progress
    }

    /// The client has been given something and has to take it.
    pub(super) fn waiting(&self) {
        let elapsed = self.started.elapsed().as_millis();
        let since = u64::try_from(elapsed).unwrap_or(u64::MAX - 1) + 1;
        self.since.store(since, Ordering::Relaxed);
    }

    /// The client took it, and the response is the one to make the next move.
    pub(super) fn busy(&self) {
        self.since.store(NOT_WAITING, Ordering::Relaxed);
    }

    /// When the wait in progress runs out of `limit`.
    fn deadline(&self, limit: Duration) -> Option<Instant> {
        match self.since.load(Ordering::Relaxed) {
            NOT_WAITING => None,
            since => Some(self.started + Duration::from_millis(since - 1) + limit),
        }
    }
}

/// The executor of one connection's streams.
#[derive(Clone, Debug)]
pub(super) struct Streams {
    clock: Arc<Clock>,
    write_stall: Option<Duration>,
}

impl Streams {
    pub(super) fn new(clock: Arc<Clock>, write_stall: Option<Duration>) -> Self {
        Self { clock, write_stall }
    }
}

impl<F> Executor<F> for Streams
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    fn execute(&self, task: F) {
        let announced = ANNOUNCED.take();
        let in_flight = self.clock.stream(announced.is_none());
        let stall = announced
            .zip(self.write_stall)
            .map(|(progress, limit)| Stall {
                progress,
                limit,
                timer: None,
            });
        tokio::spawn(StreamTask {
            task,
            stall,
            _in_flight: in_flight,
        });
    }
}

pin_project! {
    /// A stream's task. It ends when the task does, or when the response has
    /// waited on its client for too long.
    struct StreamTask<F> {
        #[pin]
        task: F,
        stall: Option<Stall>,
        _in_flight: InFlight,
    }
}

impl<F: Future> Future for StreamTask<F> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.project();
        if this.task.poll(cx).is_ready() {
            return Poll::Ready(());
        }
        match this.stall {
            Some(stall) => stall.poll(cx),
            None => Poll::Pending,
        }
    }
}

/// The write stall limit of one response.
#[derive(Debug)]
struct Stall {
    progress: Arc<Progress>,
    limit: Duration,
    timer: Timer,
}

impl Stall {
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let Some(deadline) = self.progress.deadline(self.limit) else {
            return Poll::Pending;
        };
        if Instant::now() >= deadline {
            return Poll::Ready(());
        }
        let sleep = self
            .timer
            .get_or_insert_with(|| Box::pin(tokio::time::sleep_until(deadline)));
        // A later deadline is picked up when the timer fires: a steady
        // response does not touch the timer wheel.
        if sleep.as_mut().poll(cx).is_ready() {
            sleep.as_mut().reset(deadline);
            let _ = sleep.as_mut().poll(cx);
        }
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use http::Version;

    use super::*;

    const LIMIT: Duration = Duration::from_secs(5);

    fn stall(progress: &Arc<Progress>) -> Stall {
        Stall {
            progress: progress.clone(),
            limit: LIMIT,
            timer: None,
        }
    }

    fn progress() -> Arc<Progress> {
        let progress = Progress::announce();
        ANNOUNCED.take();
        progress
    }

    #[tokio::test(start_paused = true)]
    async fn a_response_that_waits_too_long_on_its_client_runs_out() {
        let progress = progress();
        let mut stall = stall(&progress);
        progress.waiting();
        let started = Instant::now();
        std::future::poll_fn(|cx| stall.poll(cx)).await;
        assert_eq!(started.elapsed(), LIMIT);
    }

    #[tokio::test(start_paused = true)]
    async fn progress_starts_the_limit_over() {
        let progress = progress();
        let mut stall = stall(&progress);
        progress.waiting();
        let waiting = tokio::spawn({
            let progress = progress.clone();
            async move {
                tokio::time::sleep(LIMIT / 2).await;
                progress.waiting();
            }
        });
        let started = Instant::now();
        std::future::poll_fn(|cx| stall.poll(cx)).await;
        waiting.await.unwrap();
        assert_eq!(started.elapsed(), LIMIT / 2 + LIMIT);
    }

    /// A stream with nothing to send yet is the handler's business.
    #[tokio::test(start_paused = true)]
    async fn a_response_with_nothing_to_send_never_runs_out() {
        let progress = progress();
        let mut stall = stall(&progress);
        progress.waiting();
        progress.busy();
        let polled = tokio::time::timeout(LIMIT * 100, std::future::poll_fn(|cx| stall.poll(cx)));
        assert!(polled.await.is_err());
    }

    /// One task of each: the executor tells them apart, and an upgraded
    /// stream is counted until its task ends.
    #[tokio::test(start_paused = true)]
    async fn the_executor_knows_an_upgraded_stream_by_its_missing_announcement() {
        let clock = Arc::new(Clock::new(None, Some(LIMIT)));
        let streams = Streams::new(clock.clone(), None);
        let ended = Arc::new(Mutex::new(None));
        let (end, ending) = tokio::sync::oneshot::channel::<()>();
        drop(clock.request(Version::HTTP_2, true));
        assert!(clock.reclaimable_since().is_some());

        Progress::announce();
        streams.execute(async {});
        tokio::task::yield_now().await;
        assert!(clock.reclaimable_since().is_some(), "that stream ended");

        let done = ended.clone();
        streams.execute(async move {
            let _ = ending.await;
            *done.lock().unwrap() = Some(());
        });
        tokio::task::yield_now().await;
        assert_eq!(
            clock.reclaimable_since(),
            None,
            "the upgraded stream is open"
        );

        end.send(()).unwrap();
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        assert!(ended.lock().unwrap().is_some());
        assert!(clock.reclaimable_since().is_some());
    }
}
