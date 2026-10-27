//! [`MemoryTransport`]: a broker inside the process.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use navree_kernel::BoxFuture;
use tokio::sync::{mpsc, oneshot, watch};

use crate::envelope::headers;
use crate::{
    DeadLetter, Delivery, DeliveryActions, Envelope, Group, MicroError, MicroTransport,
    PatternKind, Subscription, pattern_matches,
};

/// A broker inside the process, for tests and single-process setups.
///
/// It behaves like a broker with durable consumer groups:
///
/// - **Requests** go to one instance of the group that answers the
///   pattern (round robin); with no listener the request fails at once with
///   [`MicroError::NoResponders`].
/// - **Events** reach every group subscribed to the pattern, once per
///   group: one instance of each service handles it. A group that has
///   subscribed once keeps receiving events while none of its instances
///   listens; they wait until one does. Broadcast subscriptions are per
///   instance and not kept.
/// - An event is redelivered until it is acknowledged: after
///   [`retry_in`](Delivery::retry_in), and when a delivery is dropped
///   without an answer (the instance went away).
/// - `ordered` subscriptions get one event at a time per group.
/// - Wildcards (`*`, `>`) work.
///
/// Clones share the broker. Each [`listen`](MicroTransport::listen) call is
/// one instance, subscribed as soon as `listen` is called.
#[derive(Clone, Default)]
pub struct MemoryTransport {
    broker: Arc<Broker>,
}

impl MemoryTransport {
    /// A new, empty broker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Redeliver retried events at once instead of after their delay (what
    /// `MicroFake` does, so tests never wait for a backoff).
    pub fn set_instant_retries(&self, instant: bool) {
        self.broker.instant_retries.store(instant, Ordering::SeqCst);
    }

    /// Dead letters stored with `micro.events.dead_letter = "transport"`.
    pub fn dead_letters(&self) -> Vec<DeadLetter> {
        lock(&self.broker.dead_letters).clone()
    }

    /// Whether a listening instance answers `pattern`.
    pub fn has_responder(&self, pattern: &str) -> bool {
        let state = self.broker.state();
        state.instances.values().any(|instance| {
            instance.subscriptions.iter().any(|subscription| {
                subscription.kind == PatternKind::Message
                    && pattern_matches(&subscription.pattern, pattern)
            })
        })
    }

    /// Publishes `envelope` and waits until every group that received it
    /// has acknowledged its copy (handled or dead-lettered).
    pub async fn publish_and_wait(&self, envelope: Envelope) -> Result<(), MicroError> {
        let done = self.broker.publish(envelope);
        let mut done = done;
        let _ = done.wait_for(|remaining| *remaining == 0).await;
        Ok(())
    }
}

impl MicroTransport for MemoryTransport {
    fn name(&self) -> &'static str {
        "memory"
    }

    fn request(&self, envelope: Envelope) -> BoxFuture<'_, Result<Envelope, MicroError>> {
        Box::pin(async move {
            let pattern = envelope.pattern().to_owned();
            let wait = envelope.remaining();
            let reply = self.broker.request(envelope)?;
            let outcome = match wait {
                Some(left) => match tokio::time::timeout(left, reply).await {
                    Ok(outcome) => outcome,
                    Err(_) => {
                        return Err(MicroError::Timeout {
                            pattern,
                            after: left,
                        });
                    }
                },
                None => reply.await,
            };
            outcome.map_err(|_| MicroError::Timeout {
                pattern,
                after: wait.unwrap_or_default(),
            })
        })
    }

    fn publish(&self, envelope: Envelope) -> BoxFuture<'_, Result<(), MicroError>> {
        Box::pin(async move {
            drop(self.broker.publish(envelope));
            Ok(())
        })
    }

    fn listen(
        &self,
        subscriptions: Vec<Subscription>,
        deliveries: mpsc::Sender<Delivery>,
    ) -> BoxFuture<'_, Result<(), MicroError>> {
        // Subscribed as soon as `listen` is called, before the future runs.
        let (queue, mut inbox) = mpsc::unbounded_channel();
        let id = self.broker.join(subscriptions, queue);
        // Leaves the broker even if the future is dropped unpolled.
        let member = Member {
            broker: Arc::clone(&self.broker),
            id,
        };
        Box::pin(async move {
            let _member = member;
            loop {
                tokio::select! {
                    item = inbox.recv() => match item {
                        Some(delivery) => {
                            if let Err(rejected) = deliveries.send(delivery).await {
                                // The listener stopped: the delivery goes
                                // back to the broker when dropped.
                                drop(rejected);
                                break;
                            }
                        }
                        None => break,
                    },
                    () = deliveries.closed() => break,
                }
            }
            self.broker.leave(id);
            // Whatever was queued for this instance goes back to its group.
            inbox.close();
            while let Ok(delivery) = inbox.try_recv() {
                drop(delivery);
            }
            Ok(())
        })
    }

    fn dead_letter(&self, letter: DeadLetter) -> BoxFuture<'_, Result<(), MicroError>> {
        lock(&self.broker.dead_letters).push(letter);
        Box::pin(async { Ok(()) })
    }

    fn listens_at_boot(&self) -> bool {
        true
    }

    fn instant_retries(&self) -> bool {
        self.broker.instant_retries.load(Ordering::SeqCst)
    }
}

impl std::fmt::Debug for MemoryTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.broker.state();
        f.debug_struct("MemoryTransport")
            .field("instances", &state.instances.len())
            .field("groups", &state.groups.len())
            .finish()
    }
}

/// An instance's membership: leaving the broker when dropped.
struct Member {
    broker: Arc<Broker>,
    id: u64,
}

impl Drop for Member {
    fn drop(&mut self) {
        self.broker.leave(self.id);
    }
}

/// The shared state of a memory broker.
#[derive(Default)]
struct Broker {
    state: Mutex<State>,
    instant_retries: AtomicBool,
    next_instance: AtomicU64,
    dead_letters: Mutex<Vec<DeadLetter>>,
}

#[derive(Default)]
struct State {
    /// Listening instances by id.
    instances: BTreeMap<u64, Instance>,
    /// Event groups: (group, subscribed pattern) → backlog.
    groups: HashMap<GroupKey, GroupState>,
    /// Round-robin counters by group name.
    turns: HashMap<String, usize>,
}

struct Instance {
    subscriptions: Vec<Subscription>,
    queue: mpsc::UnboundedSender<Delivery>,
}

/// One event group: a service (or a broadcast instance) subscribed to one
/// pattern.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct GroupKey {
    group: Group,
    pattern: String,
}

#[derive(Default)]
struct GroupState {
    ordered: bool,
    /// Copies waiting for an instance (or, when ordered, for their turn).
    backlog: VecDeque<Copy>,
    /// Ordered groups: a copy is with an instance.
    busy: bool,
}

/// One group's copy of an event.
struct Copy {
    envelope: Envelope,
    attempt: u32,
    done: Done,
}

/// Counts the groups that still hold a copy of one published event.
#[derive(Clone)]
struct Done(Arc<watch::Sender<usize>>);

impl Done {
    fn new(copies: usize) -> (Done, watch::Receiver<usize>) {
        let (sender, receiver) = watch::channel(copies);
        (Done(Arc::new(sender)), receiver)
    }

    fn finish(&self) {
        self.0
            .send_modify(|remaining| *remaining = remaining.saturating_sub(1));
    }
}

impl Broker {
    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// Registers an instance; delivers what its groups have waiting.
    fn join(
        self: &Arc<Self>,
        subscriptions: Vec<Subscription>,
        queue: mpsc::UnboundedSender<Delivery>,
    ) -> u64 {
        let id = self.next_instance.fetch_add(1, Ordering::SeqCst);
        let keys: Vec<GroupKey> = subscriptions
            .iter()
            .filter(|s| s.kind == PatternKind::Event)
            .map(|s| GroupKey {
                group: s.group.clone(),
                pattern: s.pattern.clone(),
            })
            .collect();
        {
            let mut state = self.state();
            for subscription in subscriptions
                .iter()
                .filter(|s| s.kind == PatternKind::Event)
            {
                let group = state
                    .groups
                    .entry(GroupKey {
                        group: subscription.group.clone(),
                        pattern: subscription.pattern.clone(),
                    })
                    .or_default();
                group.ordered = subscription.ordered;
            }
            state.instances.insert(
                id,
                Instance {
                    subscriptions,
                    queue,
                },
            );
        }
        for key in keys {
            self.pump(&key);
        }
        id
    }

    /// Removes an instance; its broadcast groups go with it.
    fn leave(&self, id: u64) {
        let mut state = self.state();
        if let Some(instance) = state.instances.remove(&id) {
            for subscription in &instance.subscriptions {
                if subscription.is_broadcast() {
                    let key = GroupKey {
                        group: subscription.group.clone(),
                        pattern: subscription.pattern.clone(),
                    };
                    if let Some(group) = state.groups.remove(&key) {
                        for copy in group.backlog {
                            copy.done.finish();
                        }
                    }
                }
            }
        }
    }

    /// Hands a request to one instance of the group that answers it.
    fn request(
        self: &Arc<Self>,
        envelope: Envelope,
    ) -> Result<oneshot::Receiver<Envelope>, MicroError> {
        let pattern = envelope.pattern().to_owned();
        let mut state = self.state();
        let candidates: Vec<(u64, usize, String)> = state
            .instances
            .iter()
            .filter_map(|(id, instance)| {
                instance
                    .subscriptions
                    .iter()
                    .position(|s| {
                        s.kind == PatternKind::Message && pattern_matches(&s.pattern, &pattern)
                    })
                    .map(|index| {
                        (
                            *id,
                            index,
                            instance.subscriptions[index].group.name().to_owned(),
                        )
                    })
            })
            .collect();
        let Some(group) = candidates.first().map(|(_, _, group)| group.clone()) else {
            return Err(MicroError::NoResponders { pattern });
        };
        let members: Vec<&(u64, usize, String)> =
            candidates.iter().filter(|(_, _, g)| *g == group).collect();
        let turn = state.turns.entry(format!("request:{group}")).or_default();
        let (id, index, _) = *members[*turn % members.len()];
        *turn = turn.wrapping_add(1);
        let (reply, receiver) = oneshot::channel();
        let delivery = Delivery::new(envelope, index, 1, ReplyActions { reply: Some(reply) });
        let queue = state.instances[&id].queue.clone();
        drop(state);
        queue
            .send(delivery)
            .map_err(|_| MicroError::NoResponders { pattern })?;
        Ok(receiver)
    }

    /// Gives every subscribed group a copy of `envelope`; returns a
    /// receiver that reaches 0 once every copy is acknowledged.
    fn publish(self: &Arc<Self>, envelope: Envelope) -> watch::Receiver<usize> {
        let pattern = envelope.pattern().to_owned();
        let keys: Vec<GroupKey> = {
            let state = self.state();
            state
                .groups
                .keys()
                .filter(|key| pattern_matches(&key.pattern, &pattern))
                .cloned()
                .collect()
        };
        let (done, receiver) = Done::new(keys.len());
        {
            let mut state = self.state();
            for key in &keys {
                if let Some(group) = state.groups.get_mut(key) {
                    group.backlog.push_back(Copy {
                        envelope: envelope.clone(),
                        attempt: 1,
                        done: done.clone(),
                    });
                }
            }
        }
        for key in &keys {
            self.pump(key);
        }
        receiver
    }

    /// Moves copies of group `key` from its backlog to its instances
    /// (round robin), one at a time for ordered groups.
    fn pump(self: &Arc<Self>, key: &GroupKey) {
        let mut sends = Vec::new();
        {
            let mut state = self.state();
            let members: Vec<(u64, usize)> = state
                .instances
                .iter()
                .filter_map(|(id, instance)| {
                    instance
                        .subscriptions
                        .iter()
                        .position(|s| {
                            s.kind == PatternKind::Event
                                && s.group == key.group
                                && s.pattern == key.pattern
                        })
                        .map(|index| (*id, index))
                })
                .collect();
            if members.is_empty() {
                return;
            }
            let turn_key = format!("event:{}:{}", key.group.name(), key.pattern);
            let mut turn = state.turns.get(&turn_key).copied().unwrap_or_default();
            let Some(group) = state.groups.get_mut(key) else {
                return;
            };
            while !(group.ordered && group.busy) {
                let Some(copy) = group.backlog.pop_front() else {
                    break;
                };
                if group.ordered {
                    group.busy = true;
                }
                let (id, index) = members[turn % members.len()];
                turn = turn.wrapping_add(1);
                sends.push((id, index, copy));
            }
            state.turns.insert(turn_key, turn);
            let mut ready = Vec::new();
            for (id, index, copy) in sends.drain(..) {
                let queue = state.instances[&id].queue.clone();
                ready.push((id, queue, index, copy));
            }
            drop(state);
            for (id, queue, index, copy) in ready {
                let attempt = copy.attempt;
                let mut envelope = copy.envelope.clone();
                envelope.set_header(headers::ATTEMPT, attempt.to_string());
                let actions = EventActions {
                    broker: Arc::clone(self),
                    key: key.clone(),
                    copy: Some(copy),
                };
                if let Err(unsent) = queue.send(Delivery::new(envelope, index, attempt, actions)) {
                    // The instance is gone: forget it first, so dropping the
                    // delivery requeues it for the others.
                    self.state().instances.remove(&id);
                    drop(unsent);
                }
            }
        }
    }

    /// Puts a copy back (after a retry or a lost delivery).
    fn requeue(self: &Arc<Self>, key: &GroupKey, copy: Copy, front: bool) {
        {
            let mut state = self.state();
            match state.groups.get_mut(key) {
                Some(group) => {
                    group.busy = false;
                    if front {
                        group.backlog.push_front(copy);
                    } else {
                        group.backlog.push_back(copy);
                    }
                }
                // A broadcast group whose instance left: the copy is gone.
                None => copy.done.finish(),
            }
        }
        self.pump(key);
    }

    /// A copy was acknowledged.
    fn finish(self: &Arc<Self>, key: &GroupKey, copy: Copy) {
        copy.done.finish();
        let ordered = {
            let mut state = self.state();
            match state.groups.get_mut(key) {
                Some(group) if group.ordered => {
                    group.busy = false;
                    true
                }
                _ => false,
            }
        };
        if ordered {
            self.pump(key);
        }
    }
}

/// The callbacks of a request delivery.
struct ReplyActions {
    reply: Option<oneshot::Sender<Envelope>>,
}

impl DeliveryActions for ReplyActions {
    fn ack(self: Box<Self>) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async { Ok(()) })
    }

    fn retry_in(self: Box<Self>, _delay: Duration) -> BoxFuture<'static, Result<(), MicroError>> {
        Box::pin(async { Ok(()) })
    }

    fn reply(mut self: Box<Self>, reply: Envelope) -> BoxFuture<'static, Result<(), MicroError>> {
        if let Some(sender) = self.reply.take() {
            // The requester may have stopped waiting.
            let _ = sender.send(reply);
        }
        Box::pin(async { Ok(()) })
    }
}

/// The callbacks of an event delivery. Dropped unanswered, the copy goes
/// back to its group as a new attempt.
struct EventActions {
    broker: Arc<Broker>,
    key: GroupKey,
    copy: Option<Copy>,
}

impl DeliveryActions for EventActions {
    fn ack(mut self: Box<Self>) -> BoxFuture<'static, Result<(), MicroError>> {
        if let Some(copy) = self.copy.take() {
            self.broker.finish(&self.key, copy);
        }
        Box::pin(async { Ok(()) })
    }

    fn retry_in(
        mut self: Box<Self>,
        delay: Duration,
    ) -> BoxFuture<'static, Result<(), MicroError>> {
        if let Some(mut copy) = self.copy.take() {
            copy.attempt += 1;
            let broker = Arc::clone(&self.broker);
            let key = self.key.clone();
            if broker.instant_retries.load(Ordering::SeqCst) || delay.is_zero() {
                broker.requeue(&key, copy, false);
            } else {
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    broker.requeue(&key, copy, false);
                });
            }
        }
        Box::pin(async { Ok(()) })
    }

    fn reply(self: Box<Self>, _reply: Envelope) -> BoxFuture<'static, Result<(), MicroError>> {
        self.ack()
    }
}

impl Drop for EventActions {
    fn drop(&mut self) {
        if let Some(mut copy) = self.copy.take() {
            copy.attempt += 1;
            self.broker.requeue(&self.key, copy, true);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
