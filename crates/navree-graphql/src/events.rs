//! [`EventStream`]: the app's events as a subscription stream.

use std::fmt;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};

use async_graphql::futures_util::stream::Stream;
use async_graphql::{Context, Error};
use navree_events::{Event, EventBus};
use navree_http::AppError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use crate::context::operation;

/// Every `E` emitted on the app's event bus from the moment it was created,
/// as a stream for a `#[subscription]` field.
///
/// ```ignore
/// #[subscription]
/// #[can(Read, Deal)]
/// async fn deal_updated(&self, ctx: &Context<'_>) -> Result<impl Stream<Item = DealUpdated>> {
///     Ok(EventStream::<DealUpdated>::new(ctx)?)
/// }
/// ```
///
/// It sees the events of this instance's bus (and those a transport puts
/// on it). A subscriber that falls more than 256 events behind skips the
/// oldest, like any subscriber of the bus; an event that does not
/// deserialize as `E` is skipped with a warning.
pub struct EventStream<E> {
    inner: BroadcastStream<serde_json::Value>,
    _event: PhantomData<fn() -> E>,
}

impl<E: Event> EventStream<E> {
    /// The stream of `E` on the bus of the app `ctx` belongs to; an error
    /// without `EventsModule`.
    pub fn new(ctx: &Context<'_>) -> Result<Self, Error> {
        let op = operation(ctx)?;
        let bus = op
            .shared
            .events
            .as_ref()
            .ok_or_else(|| AppError::internal("EventStream needs EventsModule in the app"))?;
        Ok(Self::from_bus(bus))
    }

    /// The stream of `E` on `bus`.
    pub fn from_bus(bus: &EventBus) -> Self {
        Self {
            inner: BroadcastStream::new(bus.subscribe_json(E::NAME)),
            _event: PhantomData,
        }
    }
}

impl<E: Event> Stream for EventStream<E> {
    type Item = E;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<E>> {
        loop {
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Ready(Some(Ok(payload))) => match serde_json::from_value::<E>(payload) {
                    Ok(event) => return Poll::Ready(Some(event)),
                    Err(error) => {
                        tracing::warn!(
                            target: "navree::graphql",
                            event = E::NAME,
                            %error,
                            "skipping an event that does not deserialize"
                        );
                    }
                },
                Poll::Ready(Some(Err(BroadcastStreamRecvError::Lagged(skipped)))) => {
                    tracing::warn!(
                        target: "navree::graphql",
                        event = E::NAME,
                        skipped,
                        "a subscriber fell behind and skipped events"
                    );
                }
            }
        }
    }
}

impl<E> fmt::Debug for EventStream<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventStream")
            .field("event", &std::any::type_name::<E>())
            .finish_non_exhaustive()
    }
}
