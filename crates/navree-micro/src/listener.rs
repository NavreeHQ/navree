//! The message listener (deliveries in, replies and acknowledgments out)
//! and the outbox relay.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use chrono::Utc;
use http::StatusCode;
use navree_kernel::{AppContext, BoxError};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::envelope::now_millis;
use crate::options::DeadLetterMode;
use crate::pipeline::{MessageRouter, Outcome, Registered};
use crate::runtime::MicroRuntime;
use crate::{
    DeadLetter, Delivery, DeliveryActions, Envelope, Group, PatternKind, Subscribed, Subscription,
};

/// Outbox rows younger than this are left to the after-commit hook that
/// publishes them; the relay takes the older ones.
const OUTBOX_GRACE_MS: i64 = 5_000;

/// Outbox rows one relay sweep publishes.
const OUTBOX_BATCH: u64 = 100;

/// How often handled message ids are pruned.
const INBOX_PRUNE_EVERY: Duration = Duration::from_secs(3_600);

/// Statuses that retrying cannot change: the event goes to the dead
/// letters at once (a payload that does not decode or validate, a guard
/// that refuses).
fn is_permanent(status: StatusCode) -> bool {
    matches!(status.as_u16(), 400 | 401 | 403 | 413 | 415 | 422)
}

/// The subscriptions of the app's patterns, in pattern order (a delivery's
/// subscription index is its pattern's index).
pub(crate) fn subscriptions(runtime: &MicroRuntime, router: &MessageRouter) -> Vec<Subscription> {
    let events_timeout = Duration::from_secs(runtime.options.events.timeout_secs);
    let request_timeout = Duration::from_millis(runtime.options.request_timeout_ms);
    router
        .patterns
        .iter()
        .map(|registered| Subscription {
            pattern: registered.pattern.clone(),
            kind: registered.kind,
            group: if registered.meta.broadcast {
                Group::Instance(runtime.instance.clone())
            } else {
                Group::Service(runtime.service.clone())
            },
            ordered: registered.meta.ordered,
            ack_wait: registered.meta.timeout.unwrap_or(match registered.kind {
                PatternKind::Event => events_timeout,
                PatternKind::Message => request_timeout,
            }),
        })
        .collect()
}

/// Limits per pattern, and the order per key: an event waits for the one
/// with the same key received before it.
struct Gates {
    patterns: HashMap<usize, Arc<Semaphore>>,
    keys: Mutex<HashMap<(usize, String), oneshot::Receiver<()>>>,
}

impl Gates {
    fn new(router: &MessageRouter) -> Self {
        let patterns = router
            .patterns
            .iter()
            .filter_map(|registered| {
                registered
                    .meta
                    .concurrency
                    .map(|limit| (registered.index, Arc::new(Semaphore::new(limit.max(1)))))
            })
            .collect();
        Self {
            patterns,
            keys: Mutex::default(),
        }
    }

    /// Joins the chain of `key` on pattern `index`: returns what to wait
    /// for (the previous event with that key) and what to signal when done.
    /// Called in the order deliveries arrive.
    fn chain(
        &self,
        index: usize,
        key: &str,
    ) -> (Option<oneshot::Receiver<()>>, oneshot::Sender<()>) {
        let (done, tail) = oneshot::channel();
        let mut keys = lock(&self.keys);
        // Forget chains whose last event is finished.
        if keys.len() > 1_024 {
            keys.retain(|_, tail| {
                matches!(tail.try_recv(), Err(oneshot::error::TryRecvError::Empty))
            });
        }
        let previous = keys.insert((index, key.to_owned()), tail);
        (previous, done)
    }
}

/// Runs the listener until `stop` is cancelled: subscribes the app's
/// patterns, handles deliveries (up to `micro.concurrency` at once), then
/// stops taking messages and finishes the ones in flight.
pub(crate) async fn run(
    runtime: Arc<MicroRuntime>,
    ctx: AppContext,
    router: Arc<MessageRouter>,
    stop: CancellationToken,
    started: oneshot::Sender<()>,
) -> Result<(), BoxError> {
    let subscriptions = subscriptions(&runtime, &router);
    if subscriptions.is_empty() {
        tracing::debug!(target: "navree::micro", "no message handlers: the listener has nothing to do");
        return Ok(());
    }
    if runtime.options.auto_setup {
        match runtime.transport.setup(&subscriptions).await {
            Ok(lines) => {
                for line in lines {
                    tracing::info!(target: "navree::micro", "{}: {line}", runtime.transport.name());
                }
            }
            Err(error) => {
                tracing::warn!(target: "navree::micro", %error, "micro.auto_setup failed");
            }
        }
    }
    let concurrency = runtime.options.concurrency.max(1);
    let (sender, mut deliveries) = mpsc::channel::<Delivery>(concurrency);
    let transport = runtime.transport.clone();
    // Boot goes on once the transport says the broker has registered the
    // subscriptions (or its listen ended without saying so).
    let (subscribed, is_subscribed) = Subscribed::channel();
    let listening = tokio::spawn(async move {
        transport
            .listen_subscribed(subscriptions, sender, subscribed)
            .await
    });
    let _ = is_subscribed.await;
    let _ = started.send(());
    tracing::info!(
        target: "navree::micro",
        service = %runtime.service,
        transport = runtime.transport.name(),
        patterns = router.patterns.len(),
        "message listener started"
    );

    let permits = Arc::new(Semaphore::new(concurrency));
    let gates = Arc::new(Gates::new(&router));
    let mut in_flight = JoinSet::new();
    loop {
        let delivery = tokio::select! {
            delivery = deliveries.recv() => delivery,
            () = stop.cancelled() => None,
        };
        let Some(delivery) = delivery else {
            break;
        };
        // Wait for room: messages over the limit stay in the broker.
        let permit = tokio::select! {
            permit = permits.clone().acquire_owned() => permit.ok(),
            () = stop.cancelled() => None,
        };
        let Some(permit) = permit else {
            drop(delivery);
            break;
        };
        // Events with a key wait for the previous one with that key; an
        // ordered handler's events wait for the previous one, whatever
        // their key.
        let chain = match router.patterns.get(delivery.subscription()) {
            Some(registered)
                if registered.kind == PatternKind::Event && registered.meta.ordered =>
            {
                Some(gates.chain(registered.index, ""))
            }
            Some(registered) if registered.kind == PatternKind::Event => delivery
                .envelope()
                .key()
                .map(|key| gates.chain(registered.index, key)),
            _ => None,
        };
        let runtime = runtime.clone();
        let ctx = ctx.clone();
        let router = router.clone();
        let gates = gates.clone();
        in_flight.spawn(async move {
            let done = match chain {
                Some((previous, done)) => {
                    if let Some(previous) = previous {
                        let _ = previous.await;
                    }
                    Some(done)
                }
                None => None,
            };
            handle(&runtime, &ctx, &router, &gates, delivery, permit).await;
            if let Some(done) = done {
                let _ = done.send(());
            }
        });
        while in_flight.try_join_next().is_some() {}
    }
    // Stop taking messages: whatever the transport still holds is left to
    // the broker's redelivery.
    drop(deliveries);
    while in_flight.join_next().await.is_some() {}
    match listening.await {
        Ok(Err(error)) => {
            tracing::warn!(target: "navree::micro", %error, "the transport stopped with an error")
        }
        Err(error) if error.is_panic() => {
            tracing::error!(target: "navree::micro", "the transport panicked")
        }
        _ => {}
    }
    tracing::info!(target: "navree::micro", "message listener stopped");
    Ok(())
}

/// Handles one delivery.
async fn handle(
    runtime: &MicroRuntime,
    ctx: &AppContext,
    router: &MessageRouter,
    gates: &Gates,
    delivery: Delivery,
    _permit: OwnedSemaphorePermit,
) {
    let index = delivery.subscription();
    let Some(registered) = router.patterns.get(index).cloned() else {
        tracing::warn!(target: "navree::micro", index, "a delivery for an unknown subscription");
        let _ = delivery.ack().await;
        return;
    };
    let _pattern_permit = match gates.patterns.get(&index) {
        Some(gate) => gate.clone().acquire_owned().await.ok(),
        None => None,
    };
    let (envelope, attempt, actions) = delivery.into_parts();
    match registered.kind {
        PatternKind::Message => {
            handle_message(
                runtime,
                ctx,
                router,
                &registered,
                envelope,
                attempt,
                actions,
            )
            .await
        }
        PatternKind::Event => {
            handle_event(
                runtime,
                ctx,
                router,
                &registered,
                envelope,
                attempt,
                actions,
            )
            .await
        }
    }
}

async fn handle_message(
    runtime: &MicroRuntime,
    ctx: &AppContext,
    router: &MessageRouter,
    registered: &Registered,
    envelope: Envelope,
    attempt: u32,
    actions: Box<dyn DeliveryActions>,
) {
    if envelope.is_expired() {
        // Nobody waits for the answer.
        tracing::debug!(target: "navree::micro", pattern = envelope.pattern(), "dropping a request past its deadline");
        let _ = actions.ack().await;
        return;
    }
    let outcome = if envelope.payload().len() > runtime.options.max_message_bytes {
        too_large(runtime)
    } else {
        router
            .dispatch(ctx, registered.index, &envelope, attempt)
            .await
    };
    if envelope.is_expired() {
        let _ = actions.ack().await;
        return;
    }
    let reply = envelope.reply(outcome.status.as_u16(), &outcome.content_type, outcome.body);
    if let Err(error) = actions.reply(reply).await {
        tracing::warn!(target: "navree::micro", pattern = envelope.pattern(), %error, "the reply could not be sent");
    }
}

fn too_large(runtime: &MicroRuntime) -> Outcome {
    let error = navree_http::AppError::payload_too_large(runtime.options.max_message_bytes);
    Outcome {
        status: error.status,
        content_type: "application/json".to_owned(),
        body: serde_json::to_vec(&error.body()).unwrap_or_default().into(),
        error: Some("the payload is over micro.max_message_bytes".to_owned()),
    }
}

async fn handle_event(
    runtime: &MicroRuntime,
    ctx: &AppContext,
    router: &MessageRouter,
    registered: &Registered,
    envelope: Envelope,
    attempt: u32,
    actions: Box<dyn DeliveryActions>,
) {
    let oversized = envelope.payload().len() > runtime.options.max_message_bytes;
    if registered.meta.broadcast {
        // At most once: no retries, no dead letters.
        if !oversized {
            let outcome = router
                .dispatch(ctx, registered.index, &envelope, attempt)
                .await;
            if !outcome.status.is_success() {
                tracing::warn!(
                    target: "navree::micro",
                    pattern = envelope.pattern(),
                    status = outcome.status.as_u16(),
                    error = outcome.error.as_deref().unwrap_or_default(),
                    "a broadcast event handler failed"
                );
            }
        }
        let _ = actions.ack().await;
        return;
    }
    let max_attempts = registered
        .meta
        .max_attempts
        .unwrap_or(runtime.options.events.max_attempts)
        .max(1);
    let mut attempt = attempt.max(1);
    loop {
        let outcome = if oversized {
            too_large(runtime)
        } else {
            router
                .dispatch(ctx, registered.index, &envelope, attempt)
                .await
        };
        if outcome.status.is_success() {
            let _ = actions.ack().await;
            return;
        }
        let error = outcome
            .error
            .clone()
            .unwrap_or_else(|| outcome.status.to_string());
        let exhausted = is_permanent(outcome.status) || attempt >= max_attempts;
        if exhausted {
            let letter = dead_letter(runtime, registered, &envelope, attempt, error);
            match store_dead_letter(runtime, letter).await {
                Ok(()) => {
                    let _ = actions.ack().await;
                }
                Err(error) => {
                    tracing::error!(target: "navree::micro", %error, "storing a dead letter failed; the event will come back");
                    let delay = runtime.options.events.backoff.delay_for(attempt);
                    let _ = actions.retry_in(delay).await;
                }
            }
            return;
        }
        let delay = if runtime.transport.instant_retries() {
            Duration::ZERO
        } else {
            runtime.options.events.backoff.delay_for(attempt)
        };
        tracing::debug!(
            target: "navree::micro",
            pattern = envelope.pattern(),
            attempt,
            ?delay,
            error = %error,
            "event handler failed; retrying"
        );
        if registered.meta.ordered {
            // Hold the events behind this one until it succeeds or is
            // dead-lettered.
            tokio::time::sleep(delay).await;
            attempt += 1;
            continue;
        }
        let _ = actions.retry_in(delay).await;
        return;
    }
}

/// The dead letter of `envelope`.
pub(crate) fn dead_letter(
    runtime: &MicroRuntime,
    registered: &Registered,
    envelope: &Envelope,
    attempts: u32,
    error: String,
) -> DeadLetter {
    DeadLetter {
        id: Uuid::now_v7().to_string(),
        service: runtime.service.clone(),
        pattern: envelope.pattern().to_owned(),
        handler_pattern: registered.pattern.clone(),
        message_id: envelope.message_id().unwrap_or_default().to_owned(),
        headers: envelope.headers().clone(),
        payload: envelope.payload().clone(),
        attempts,
        error,
        failed_at: Utc::now(),
    }
}

/// Puts a dead letter where `micro.events.dead_letter` says.
pub(crate) async fn store_dead_letter(
    runtime: &MicroRuntime,
    letter: DeadLetter,
) -> Result<(), BoxError> {
    tracing::warn!(
        target: "navree::micro",
        pattern = %letter.pattern,
        message_id = %letter.message_id,
        attempts = letter.attempts,
        error = %letter.error,
        "event dead-lettered"
    );
    runtime.transport.observe_dead_letter(&letter);
    match runtime.dead_letter {
        DeadLetterMode::Database => match &runtime.store {
            Some(store) => store.dead_insert(&letter).await.map_err(Into::into),
            None => Err("micro.events.dead_letter = \"database\" needs DatabaseModule".into()),
        },
        DeadLetterMode::Transport => runtime
            .transport
            .dead_letter(letter)
            .await
            .map_err(Into::into),
        DeadLetterMode::Log => {
            tracing::error!(
                target: "navree::micro",
                pattern = %letter.pattern,
                message_id = %letter.message_id,
                error = %letter.error,
                "event dropped after {} attempts",
                letter.attempts
            );
            Ok(())
        }
    }
}

/// Runs a dead letter through this service's handler again, in this
/// process, with up to `max_attempts` attempts (pauses capped at five
/// seconds). Returns whether it was handled; when not, it is dead-lettered
/// again.
pub(crate) async fn retry_dead_letter(
    runtime: &MicroRuntime,
    ctx: &AppContext,
    router: &MessageRouter,
    letter: &DeadLetter,
) -> Result<bool, BoxError> {
    let registered = router
        .patterns
        .iter()
        .find(|r| r.kind == PatternKind::Event && r.pattern == letter.handler_pattern)
        .cloned()
        .ok_or_else(|| {
            format!(
                "no event handler for `{}` in this service any more",
                letter.handler_pattern
            )
        })?;
    let envelope = letter.envelope();
    let max_attempts = registered
        .meta
        .max_attempts
        .unwrap_or(runtime.options.events.max_attempts)
        .max(1);
    let mut attempt = 1;
    loop {
        let outcome = router
            .dispatch(ctx, registered.index, &envelope, attempt)
            .await;
        if outcome.status.is_success() {
            return Ok(true);
        }
        let error = outcome
            .error
            .clone()
            .unwrap_or_else(|| outcome.status.to_string());
        if is_permanent(outcome.status) || attempt >= max_attempts {
            store_dead_letter(
                runtime,
                dead_letter(runtime, &registered, &envelope, attempt, error),
            )
            .await?;
            return Ok(false);
        }
        let delay = runtime
            .options
            .events
            .backoff
            .delay_for(attempt)
            .min(Duration::from_secs(5));
        tokio::time::sleep(delay).await;
        attempt += 1;
    }
}

/// The outbox relay and the inbox pruning, until `stop`: publishes events
/// a crashed process left in the outbox (every `micro.outbox_poll_ms`) and
/// deletes handled message ids older than `micro.inbox_retention_hours`
/// (every hour).
pub(crate) async fn relay(runtime: Arc<MicroRuntime>, stop: CancellationToken) {
    let Some(store) = runtime.store.clone() else {
        return;
    };
    let poll = Duration::from_millis(runtime.options.outbox_poll_ms.max(1));
    let mut last_prune: Option<tokio::time::Instant> = None;
    loop {
        if runtime.options.outbox
            && let Err(error) = sweep_outbox(&runtime, &store).await
        {
            tracing::warn!(target: "navree::micro", %error, "the outbox relay failed");
        }
        if runtime.options.inbox && last_prune.is_none_or(|at| at.elapsed() >= INBOX_PRUNE_EVERY) {
            let retention = i64::try_from(runtime.options.inbox_retention_hours)
                .unwrap_or(i64::MAX / 3_600_000)
                .saturating_mul(3_600_000);
            match store
                .inbox_prune(now_millis().saturating_sub(retention))
                .await
            {
                Ok(pruned) if pruned > 0 => {
                    tracing::debug!(target: "navree::micro", pruned, "pruned handled message ids");
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(target: "navree::micro", %error, "pruning the inbox failed")
                }
            }
            last_prune = Some(tokio::time::Instant::now());
        }
        tokio::select! {
            () = tokio::time::sleep(poll) => {}
            () = stop.cancelled() => return,
        }
    }
}

/// One relay sweep: claims the due rows in a transaction (`FOR UPDATE SKIP
/// LOCKED` on PostgreSQL), publishes and deletes them. A crash before the
/// commit leaves them for the next sweep.
pub(crate) async fn sweep_outbox(
    runtime: &MicroRuntime,
    store: &crate::store::Store,
) -> Result<usize, BoxError> {
    let before = now_millis().saturating_sub(OUTBOX_GRACE_MS);
    let transport = runtime.transport.clone();
    let store_in = store.clone();
    let published = store
        .db()
        .transaction(|uow| async move {
            let rows = store_in.outbox_due(&uow, before, OUTBOX_BATCH).await?;
            let mut published = 0;
            for row in rows {
                match transport.publish(row.envelope).await {
                    Ok(()) => {
                        store_in.outbox_delete_on(&uow, &row.id).await?;
                        published += 1;
                    }
                    Err(error) => {
                        tracing::warn!(target: "navree::micro", %error, outbox_id = %row.id, "publishing from the outbox failed");
                        break;
                    }
                }
            }
            Ok(published)
        })
        .await?;
    if published > 0 {
        tracing::info!(target: "navree::micro", published, "the outbox relay published events left behind");
    }
    Ok(published)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_validation_and_guard_failures_are_permanent() {
        for status in [400, 401, 403, 413, 415, 422] {
            assert!(
                is_permanent(StatusCode::from_u16(status).unwrap()),
                "{status}"
            );
        }
        for status in [404, 409, 500, 503] {
            assert!(
                !is_permanent(StatusCode::from_u16(status).unwrap()),
                "{status}"
            );
        }
    }
}
