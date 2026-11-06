//! [`Gate`]: who gets a connection, and what a full server does.
//!
//! The server holds two limits at the door. `http.server.max_connections` is
//! a [`Semaphore`]: a full server stops serving new clients, which wait
//! instead of costing a task. But a client in the listen backlog is invisible,
//! and an idle keep-alive connection would keep its slot for as long as its
//! client cared to send a request per idle timeout. So a full server does
//! accept, without reading anything:
//!
//! - A client that has sent something wants a slot: the longest-idle
//!   connection closes to make room for it, as it would at its idle timeout,
//!   and the clients behind it stay in the backlog until it has one. It gets
//!   one reclaim, however long it waits, and no other happens while the last
//!   connection reclaimed is lingering, so a client that has to wait for a
//!   slow goodbye costs one connection and not one per look.
//! - A client that has sent nothing costs nobody anything, and a client that
//!   has gone is forgotten. One that says nothing does not hold up the client
//!   behind it either: the server goes on accepting, up to
//!   [`WAITERS_AT_MOST`], and past that the one silent the longest makes way.
//!   It has until `header_read`, like any new connection.
//!
//! A connection with a request in flight is never reclaimed, and one still on
//! its first request has `header_read` to finish it. One that is only sending
//! its next head counts as idle: a client must not keep its slot by starting
//! a head with each request.
//!
//! `http.server.max_connections_per_ip` limits what one address can hold. It
//! does not stop a client with many addresses. An address at its limit is
//! often many users behind one (an office, a carrier's NAT) or one browser
//! keeping connections open, so a new connection from it takes the place of
//! its own connection idle the longest, which closes as at its idle timeout,
//! by the same rules as above. Only when none of its connections is idle is
//! the new one closed: the address is only known after the accept, so it
//! cannot wait in line.
//!
//! A connection that made way still counts until its socket has closed: its
//! client can answer the goodbye with one more request, or with an HTTP/2
//! stream (a WebSocket) that keeps it open for good. So an address holds at
//! most its limit and the few connections closing for newer ones, never more
//! than [`CLOSING_AT_MOST`] of those (or the limit itself, when that is
//! smaller); with that many closing, a new connection is closed as when none
//! is idle. The count of an address is exact whatever order its sockets close
//! in. Peers named in `http.trusted_proxies` are not counted: each carries
//! many clients.

use std::collections::{HashMap, VecDeque};
use std::future::poll_fn;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

use super::Listener;
use super::clock::Clock;
use crate::ServerOptions;
use crate::extract::ProxyTrust;
use crate::network::rate_limit_key;

/// How often a limit being hit is logged.
const WARNING_EVERY: Duration = Duration::from_secs(60);

/// How often a full server with a client waiting looks again for an idle
/// connection: the ones it passed over were busy, not gone.
const RECLAIM_EVERY: Duration = Duration::from_millis(100);

/// The fewest connections tracked before the finished ones are dropped.
const TRACKED_AT_LEAST: usize = 64;

/// The most clients a full server holds accepted. Waiting costs a client a
/// socket here, and nothing in the backlog.
const WAITERS_AT_MOST: usize = 64;

/// How long a client that has sent nothing keeps its place at
/// [`WAITERS_AT_MOST`]: long enough for the request of a client that has only
/// just connected.
const SILENT_AT_LEAST: Duration = Duration::from_secs(1);

/// The most connections of one address that close for newer ones at a time,
/// over its limit: enough for a browser that opens its connections at once.
const CLOSING_AT_MOST: usize = 8;

/// How long an address at its limit with no idle connection is not looked
/// through again: a storm of connections from it costs the accept loop one
/// look, not one per connection.
const NO_IDLE_FOR: Duration = Duration::from_millis(10);

/// See the [module documentation](self).
pub(super) struct Gate<L: Listener> {
    cap: Option<Arc<Semaphore>>,
    per_ip: Option<PerIp>,
    header_read: Option<Duration>,
    /// Clients accepted by a full server, oldest first.
    waiting: VecDeque<Waiter<L>>,
    /// The connection reclaimed last.
    reclaimed: Option<Arc<Clock>>,
    /// Every connection open but `reclaimed`, plus any that ended since
    /// `prune_at` last moved. Only with a cap, the only reason to look.
    tracked: Vec<Arc<Clock>>,
    prune_at: usize,
    warned_full: Option<Instant>,
}

/// A client accepted by a full server. Nothing has been read from it.
struct Waiter<L: Listener> {
    stream: L::Stream,
    remote: L::Addr,
    ip: Option<IpSlot>,
    since: Instant,
    /// It has sent something, so it wants a slot.
    sent: bool,
    /// A connection has been reclaimed for it.
    reclaimed: bool,
}

/// What a connection holds at the door, freed when its socket closes.
#[derive(Debug, Default)]
pub(super) struct Slot {
    _cap: Option<OwnedSemaphorePermit>,
    ip: Option<IpSlot>,
}

impl<L: Listener> Gate<L> {
    pub(super) fn new(options: &ServerOptions, proxies: ProxyTrust, ipv6_prefix: u8) -> Self {
        Self {
            cap: options.max_connections.map(|connections| {
                Arc::new(Semaphore::new(connections.min(Semaphore::MAX_PERMITS)))
            }),
            per_ip: options
                .max_connections_per_ip
                .map(|limit| PerIp::new(limit, proxies, ipv6_prefix)),
            header_read: options.header_read_timeout_ms.map(super::limit),
            waiting: VecDeque::new(),
            reclaimed: None,
            tracked: Vec::new(),
            prune_at: TRACKED_AT_LEAST,
            warned_full: None,
        }
    }

    /// The next connection to serve, and its slot: waits for both.
    pub(super) async fn next(&mut self, listener: &mut L) -> (L::Stream, L::Addr, Slot) {
        let Some(cap) = self.cap.clone() else {
            let (stream, remote, ip) = admitted(&mut self.per_ip, listener).await;
            let slot = Slot { _cap: None, ip };
            return (stream, remote, slot);
        };
        // A slot first: a client with none waits instead of costing a task.
        let permit = match cap.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => self.wait_for_a_slot(listener, &cap).await,
        };
        let (stream, remote, ip) = match self.next_waiter() {
            Some(waiter) => (waiter.stream, waiter.remote, waiter.ip),
            None => admitted(&mut self.per_ip, listener).await,
        };
        let slot = Slot {
            _cap: Some(permit),
            ip,
        };
        (stream, remote, slot)
    }

    /// What a full server does until a slot comes free.
    async fn wait_for_a_slot(
        &mut self,
        listener: &mut L,
        cap: &Arc<Semaphore>,
    ) -> OwnedSemaphorePermit {
        if self
            .warned_full
            .is_none_or(|at| at.elapsed() >= WARNING_EVERY)
        {
            self.warned_full = Some(Instant::now());
            tracing::warn!(target: "navree::http", "http.server.max_connections reached; idle connections are closed for clients that wait, the rest wait until one closes");
        }
        loop {
            self.forget_the_silent();
            self.reclaim();
            let accepting = self.accepting();
            let looking = !self.waiting.is_empty();
            tokio::select! {
                biased;
                permit = cap.clone().acquire_owned() => match permit {
                    Ok(permit) => return permit,
                    Err(_) => std::future::pending().await,
                },
                () = poll_fn(|cx| poll_waiting(&mut self.waiting, cx)) => {}
                accepted = admitted(&mut self.per_ip, listener), if accepting => {
                    self.wait_in_line(accepted);
                }
                () = tokio::time::sleep(RECLAIM_EVERY), if looking => {}
            }
        }
    }

    /// Whether a full server takes another client from the backlog: not once
    /// one has sent something, and only if the ones here leave room.
    fn accepting(&self) -> bool {
        let silent_long = |waiter: &Waiter<L>| waiter.since.elapsed() >= SILENT_AT_LEAST;
        !self.waiting.iter().any(|waiter| waiter.sent)
            && (self.waiting.len() < WAITERS_AT_MOST
                || self.waiting.front().is_some_and(silent_long))
    }

    /// A client joins the line, in place of the one silent the longest if
    /// there is no room: only silent clients are here while accepting.
    fn wait_in_line(&mut self, (stream, remote, ip): (L::Stream, L::Addr, Option<IpSlot>)) {
        if self.waiting.len() >= WAITERS_AT_MOST {
            self.waiting.pop_front();
        }
        self.waiting.push_back(Waiter {
            stream,
            remote,
            ip,
            since: Instant::now(),
            sent: false,
            reclaimed: false,
        });
    }

    /// Who gets a slot that came free: the first who has sent something, or
    /// else the first.
    fn next_waiter(&mut self) -> Option<Waiter<L>> {
        let first = self.waiting.iter().position(|waiter| waiter.sent);
        self.waiting.remove(first.unwrap_or(0))
    }

    /// A client that has sent nothing has `header_read`, like any other.
    fn forget_the_silent(&mut self) {
        if let Some(limit) = self.header_read {
            self.waiting
                .retain(|waiter| waiter.sent || waiter.since.elapsed() < limit);
        }
    }

    /// Follows `clock`, the connection's that holds `slot`, for
    /// [`reclaim`](Self::reclaim) and for its address's limit.
    pub(super) fn track(&mut self, clock: &Arc<Clock>, slot: &Slot) {
        if let Some(ip) = &slot.ip {
            ip.follow(clock);
        }
        if self.cap.is_none() {
            return;
        }
        if self.tracked.len() >= self.prune_at {
            prune(&mut self.tracked);
            self.prune_at = (self.tracked.len() * 2).max(TRACKED_AT_LEAST);
        }
        self.tracked.push(clock.clone());
    }

    /// Closes the connection that has been idle the longest, if any is, for
    /// the first client that has sent something and had none closed for it.
    /// One at a time: not while the last one closed is still saying goodbye.
    fn reclaim(&mut self) {
        let lingering = self
            .reclaimed
            .as_ref()
            .is_some_and(|clock| Arc::strong_count(clock) > 1 && clock.lingering());
        if lingering {
            return;
        }
        let wants_one = |waiter: &&mut Waiter<L>| waiter.sent && !waiter.reclaimed;
        let Some(waiter) = self.waiting.iter_mut().find(wants_one) else {
            return;
        };
        prune(&mut self.tracked);
        let longest_idle = self
            .tracked
            .iter()
            .enumerate()
            .filter_map(|(at, clock)| clock.reclaimable_since().map(|since| (since, at)))
            .min_by_key(|(since, _)| *since);
        let Some((_, at)) = longest_idle else {
            return;
        };
        let clock = self.tracked.swap_remove(at);
        if clock.reclaim() {
            waiter.reclaimed = true;
            self.reclaimed = Some(clock);
        } else {
            self.tracked.push(clock);
        }
    }
}

/// Forgets the connections that ended: only this list still has them.
fn prune(tracked: &mut Vec<Arc<Clock>>) {
    tracked.retain(|clock| Arc::strong_count(clock) > 1);
}

/// The next connection whose address is under its limit. Any other is dropped
/// here, which closes it.
async fn admitted<L: Listener>(
    per_ip: &mut Option<PerIp>,
    listener: &mut L,
) -> (L::Stream, L::Addr, Option<IpSlot>) {
    loop {
        let (stream, remote) = listener.accept().await;
        let Some(per_ip) = per_ip else {
            return (stream, remote, None);
        };
        if let Ok(ip) = per_ip.admit(L::peer_ip(&remote)) {
            return (stream, remote, ip);
        }
    }
}

/// Notes who has sent something and forgets who has gone. Ready when that
/// changed anything.
fn poll_waiting<L: Listener>(waiting: &mut VecDeque<Waiter<L>>, cx: &mut Context<'_>) -> Poll<()> {
    let mut changed = false;
    waiting.retain_mut(|waiter| {
        if waiter.sent {
            return true;
        }
        match L::poll_sent(&waiter.stream, cx) {
            Poll::Pending => true,
            Poll::Ready(sent) => {
                changed = true;
                waiter.sent = sent;
                sent
            }
        }
    });
    if changed {
        Poll::Ready(())
    } else {
        Poll::Pending
    }
}

/// `http.server.max_connections_per_ip`: the connections open per network.
#[derive(Debug)]
struct PerIp {
    limit: usize,
    proxies: ProxyTrust,
    ipv6_prefix: u8,
    open: Arc<Mutex<HashMap<IpAddr, Address>>>,
    warned: Option<Instant>,
}

/// The connections counted under one address.
#[derive(Debug, Default)]
struct Address {
    /// Its sockets open, the ones closing for newer ones included.
    count: usize,
    /// How many of those made way for a newer one.
    closing: usize,
    /// The connections that can still make way. Ones that cannot any more
    /// are dropped from here now and then.
    held: Vec<Held>,
    /// None of them was idle at the last look, and that stands until then.
    no_idle_until: Option<Instant>,
}

/// A connection counted under an address.
#[derive(Debug)]
struct Held {
    clock: Weak<Clock>,
    standing: Arc<Standing>,
}

/// Where a connection counted under an address stands, shared by its
/// [`IpSlot`] and its [`Held`]. Changed only under the map lock.
#[derive(Debug, Default)]
struct Standing {
    /// It made way for a newer connection, and is closing.
    made_way: AtomicBool,
    /// Its socket has closed.
    closed: AtomicBool,
}

impl Address {
    /// Whether a new connection fits under `limit`. The ones closing for
    /// newer ones are not in its way: they are bounded on their own.
    fn has_room(&self, limit: usize) -> bool {
        self.count.saturating_sub(self.closing) < limit
    }

    /// Closes the connection idle the longest, if one is and fewer than
    /// `closing_at_most` are closing already, for a new one from the same
    /// address. It counts until its socket has closed.
    fn make_way(&mut self, closing_at_most: usize) -> bool {
        if self.closing >= closing_at_most {
            return false;
        }
        let now = Instant::now();
        if self.no_idle_until.is_some_and(|until| now < until) {
            return false;
        }
        self.held.retain(Held::can_make_way);
        let mut idle: Vec<_> = self
            .held
            .iter()
            .enumerate()
            .filter_map(|(at, held)| {
                let since = held.clock.upgrade()?.reclaimable_since()?;
                Some((since, at))
            })
            .collect();
        idle.sort_unstable();
        for (_, at) in idle {
            // One that took a request a moment ago keeps its slot.
            if self.held[at]
                .clock
                .upgrade()
                .is_some_and(|clock| clock.reclaim())
            {
                let held = self.held.swap_remove(at);
                held.standing.made_way.store(true, Ordering::Relaxed);
                self.closing += 1;
                return true;
            }
        }
        self.no_idle_until = Some(now + NO_IDLE_FOR);
        false
    }
}

impl Held {
    fn can_make_way(&self) -> bool {
        !self.standing.made_way.load(Ordering::Relaxed)
            && !self.standing.closed.load(Ordering::Relaxed)
    }
}

/// A connection counted under an address until its socket closes, also once
/// it has made way for another (see [`Address::make_way`]).
#[derive(Debug)]
struct IpSlot {
    open: Arc<Mutex<HashMap<IpAddr, Address>>>,
    key: IpAddr,
    standing: Arc<Standing>,
}

impl IpSlot {
    fn new(open: &Arc<Mutex<HashMap<IpAddr, Address>>>, key: IpAddr) -> Self {
        Self {
            open: open.clone(),
            key,
            standing: Arc::default(),
        }
    }

    /// `clock` is this connection's: it can make way for another.
    fn follow(&self, clock: &Arc<Clock>) {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(address) = open.get_mut(&self.key) else {
            return;
        };
        if self.standing.made_way.load(Ordering::Relaxed) {
            return;
        }
        if address.held.len() >= address.count.max(4) * 2 {
            address.held.retain(Held::can_make_way);
        }
        address.held.push(Held {
            clock: Arc::downgrade(clock),
            standing: self.standing.clone(),
        });
    }
}

impl Drop for IpSlot {
    fn drop(&mut self) {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        if self.standing.closed.swap(true, Ordering::Relaxed) {
            return;
        }
        if let Some(address) = open.get_mut(&self.key) {
            address.count = address.count.saturating_sub(1);
            if self.standing.made_way.load(Ordering::Relaxed) {
                address.closing = address.closing.saturating_sub(1);
            }
            if address.count == 0 {
                open.remove(&self.key);
            }
        }
    }
}

/// A connection over its address's limit.
#[derive(Debug)]
struct OverLimit;

impl PerIp {
    fn new(limit: usize, proxies: ProxyTrust, ipv6_prefix: u8) -> Self {
        Self {
            limit,
            proxies,
            ipv6_prefix,
            open: Arc::default(),
            warned: None,
        }
    }

    /// How many of an address's connections may close for newer ones at once.
    fn closing_at_most(&self) -> usize {
        self.limit.min(CLOSING_AT_MOST)
    }

    /// Counts a connection from `peer`: `None` means it is not counted, for
    /// a listed proxy or a listener whose peers have no address. At the
    /// limit, the address's connection idle the longest makes way for it.
    fn admit(&mut self, peer: Option<IpAddr>) -> Result<Option<IpSlot>, OverLimit> {
        let Some(ip) = peer.filter(|ip| !self.proxies.trusts(Some(*ip))) else {
            return Ok(None);
        };
        let key = rate_limit_key(ip, self.ipv6_prefix);
        let closing_at_most = self.closing_at_most();
        let made_way = {
            let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
            let address = open.entry(key).or_default();
            if address.has_room(self.limit) {
                address.count += 1;
                return Ok(Some(IpSlot::new(&self.open, key)));
            }
            let made_way = address.make_way(closing_at_most);
            address.count += usize::from(made_way);
            made_way
        };
        if self.warned.is_none_or(|at| at.elapsed() >= WARNING_EVERY) {
            self.warned = Some(Instant::now());
            if made_way {
                tracing::warn!(target: "navree::http", client = %key, "http.server.max_connections_per_ip reached; closing this address's idle connections for its new ones (behind a reverse proxy, list it in http.trusted_proxies)");
            } else {
                tracing::warn!(target: "navree::http", client = %key, "http.server.max_connections_per_ip reached; closing new connections from this address while none of its own is idle (behind a reverse proxy, list it in http.trusted_proxies)");
            }
        }
        if made_way {
            Ok(Some(IpSlot::new(&self.open, key)))
        } else {
            Err(OverLimit)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::io;
    use std::task::Waker;

    use http::Version;

    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn per_ip(limit: usize, proxies: ProxyTrust) -> PerIp {
        PerIp::new(limit, proxies, 64)
    }

    #[test]
    fn an_address_is_held_to_its_limit_and_gets_it_back() {
        let mut limit = per_ip(2, ProxyTrust::none());
        let client = Some(ip("192.0.2.1"));
        let first = limit.admit(client).unwrap();
        let second = limit.admit(client).unwrap();
        assert!(first.is_some() && second.is_some());
        assert!(limit.admit(client).is_err());
        assert!(limit.admit(Some(ip("192.0.2.2"))).is_ok(), "another client");
        drop(first);
        assert!(limit.admit(client).is_ok());
    }

    #[test]
    fn the_count_of_an_address_goes_with_its_last_connection() {
        let mut limit = per_ip(2, ProxyTrust::none());
        let slot = limit.admit(Some(ip("192.0.2.1"))).unwrap();
        assert_eq!(limit.open.lock().unwrap().len(), 1);
        drop(slot);
        assert!(
            limit.open.lock().unwrap().is_empty(),
            "no memory per client"
        );
    }

    /// A network that owns a /64 has billions of addresses to spend.
    #[test]
    fn the_addresses_of_one_ipv6_network_share_a_limit() {
        let mut limit = per_ip(1, ProxyTrust::none());
        let _held = limit.admit(Some(ip("2001:db8:1:2::1"))).unwrap();
        assert!(limit.admit(Some(ip("2001:db8:1:2::2"))).is_err());
        assert!(limit.admit(Some(ip("2001:db8:1:3::1"))).is_ok());
    }

    #[test]
    fn an_ipv4_client_counts_once_on_a_dual_stack_socket() {
        let mut limit = per_ip(1, ProxyTrust::none());
        let _held = limit.admit(Some(ip("192.0.2.1"))).unwrap();
        assert!(limit.admit(Some(ip("::ffff:192.0.2.1"))).is_err());
    }

    /// Every client of a proxy has its address: the limit would be on them
    /// all together.
    #[test]
    fn a_trusted_proxy_is_not_counted() {
        let proxies = ProxyTrust::new(false, &["10.0.0.0/8".to_owned()]);
        let mut limit = per_ip(1, proxies);
        for _ in 0..3 {
            assert!(limit.admit(Some(ip("10.1.2.3"))).unwrap().is_none());
        }
        let _held = limit.admit(Some(ip("192.0.2.1"))).unwrap();
        assert!(
            limit.admit(Some(ip("192.0.2.1"))).is_err(),
            "others still are"
        );
    }

    #[test]
    fn with_trust_proxy_every_peer_is_a_proxy() {
        let mut limit = per_ip(1, ProxyTrust::new(true, &[]));
        let first = limit.admit(Some(ip("192.0.2.1"))).unwrap();
        assert!(first.is_none());
        assert!(limit.admit(Some(ip("192.0.2.1"))).is_ok());
    }

    #[test]
    fn a_listener_with_no_addresses_is_not_counted() {
        let mut limit = per_ip(1, ProxyTrust::none());
        assert!(limit.admit(None).unwrap().is_none());
    }

    /// A connection admitted under `limit` from `client`, whose first request
    /// has finished: idle since now.
    fn idle_from(limit: &mut PerIp, client: &str) -> (Arc<Clock>, IpSlot) {
        let (clock, slot) = busy_from(limit, client);
        drop(clock.request(Version::HTTP_2, true));
        (clock, slot)
    }

    /// A connection admitted under `limit` from `client`, on its first request.
    fn busy_from(limit: &mut PerIp, client: &str) -> (Arc<Clock>, IpSlot) {
        let slot = limit.admit(Some(ip(client))).unwrap().unwrap();
        let clock = Arc::new(Clock::new(None, None));
        slot.follow(&clock);
        (clock, slot)
    }

    fn count(limit: &PerIp, client: &str) -> usize {
        limit
            .open
            .lock()
            .unwrap()
            .get(&ip(client))
            .map_or(0, |address| address.count)
    }

    /// Many users behind one address: its idle connections make way for its
    /// new ones, the one idle the longest first, and only its own.
    #[tokio::test(start_paused = true)]
    async fn an_address_at_its_limit_trades_its_longest_idle_connection_for_a_new_one() {
        let mut limit = per_ip(2, ProxyTrust::none());
        let (other, _other_slot) = idle_from(&mut limit, "192.0.2.9");
        tokio::time::advance(Duration::from_secs(1)).await;
        let (older, older_slot) = idle_from(&mut limit, "192.0.2.1");
        tokio::time::advance(Duration::from_secs(1)).await;
        let (newer, _newer_slot) = idle_from(&mut limit, "192.0.2.1");

        let (_third, _third_slot) = busy_from(&mut limit, "192.0.2.1");
        assert!(older.reclaimable_since().is_none(), "it made way");
        assert!(newer.reclaimable_since().is_some(), "it stays");
        assert!(
            other.reclaimable_since().is_some(),
            "another address's is left alone, older as it is"
        );
        assert_eq!(
            count(&limit, "192.0.2.1"),
            3,
            "it counts until its socket has closed"
        );
        drop(older_slot);
        assert_eq!(count(&limit, "192.0.2.1"), 2);

        let (_fourth, _fourth_slot) = busy_from(&mut limit, "192.0.2.1");
        assert!(newer.reclaimable_since().is_none(), "the next one made way");
        assert!(
            limit.admit(Some(ip("192.0.2.1"))).is_err(),
            "the rest are busy"
        );
    }

    /// A client must not take over the connections still on their first
    /// request, which have `header_read` to finish it.
    #[tokio::test(start_paused = true)]
    async fn an_address_with_no_idle_connection_is_refused() {
        let mut limit = per_ip(2, ProxyTrust::none());
        let (first, _first_slot) = busy_from(&mut limit, "192.0.2.1");
        let (second, _second_slot) = idle_from(&mut limit, "192.0.2.1");
        let _request = second.request(Version::HTTP_2, true);
        assert!(limit.admit(Some(ip("192.0.2.1"))).is_err());
        assert!(first.reclaimable_since().is_none());
        assert_eq!(count(&limit, "192.0.2.1"), 2);
    }

    /// However the sockets close, the count is the sockets open, it ends at
    /// nothing, and nothing is kept for the address.
    #[tokio::test(start_paused = true)]
    async fn the_count_is_exact_through_every_trade() {
        let mut limit = per_ip(2, ProxyTrust::none());
        let mut open: Vec<(Arc<Clock>, IpSlot)> = Vec::new();
        for _ in 0..20 {
            open.push(idle_from(&mut limit, "192.0.2.1"));
            assert_eq!(count(&limit, "192.0.2.1"), open.len(), "every socket open");
            // Of the two closing for newer ones, the older has closed.
            let closing: Vec<usize> = (0..open.len())
                .filter(|&at| open[at].0.reclaimable_since().is_none())
                .collect();
            if let [first, _] = closing.as_slice() {
                open.remove(*first);
            }
            tokio::time::advance(Duration::from_millis(10)).await;
        }
        assert!(
            limit.open.lock().unwrap()[&ip("192.0.2.1")].held.len() <= 8,
            "the ones that made way are not kept"
        );
        // The ones still closing close last.
        open.sort_by_key(|(clock, _)| clock.reclaimable_since().is_none());
        drop(open);
        assert!(limit.open.lock().unwrap().is_empty());
    }

    /// A connection that made way and then takes a stream all the same (an
    /// HTTP/2 client that answers the GOAWAY with a WebSocket, or an HTTP/1
    /// head that raced the reclaim) still counts: whatever its clients do, an
    /// address holds its limit and at most the few closing for newer ones.
    #[tokio::test(start_paused = true)]
    async fn a_connection_that_made_way_counts_until_its_socket_closes() {
        for most in [1, 4, 12] {
            let mut limit = per_ip(most, ProxyTrust::none());
            let closing_at_most = most.min(CLOSING_AT_MOST);
            let mut idle: Vec<(Arc<Clock>, IpSlot)> = Vec::new();
            let mut closing = Vec::new();
            let mut refused = 0;
            for _ in 0..100 {
                tokio::time::advance(NO_IDLE_FOR).await;
                match limit.admit(Some(ip("192.0.2.1"))) {
                    Ok(slot) => {
                        let slot = slot.unwrap();
                        let clock = Arc::new(Clock::new(None, None));
                        slot.follow(&clock);
                        drop(clock.request(Version::HTTP_2, true));
                        idle.push((clock, slot));
                    }
                    Err(OverLimit) => refused += 1,
                }
                let (made_way, still_idle): (Vec<_>, Vec<_>) = idle
                    .into_iter()
                    .partition(|(clock, _)| clock.reclaimable_since().is_none());
                idle = still_idle;
                // Its client opens a stream that never ends.
                for (clock, slot) in made_way {
                    clock.say_goodbye();
                    closing.push((slot, clock.stream(true)));
                }
                let sockets = idle.len() + closing.len();
                assert_eq!(count(&limit, "192.0.2.1"), sockets, "every socket open");
                assert!(
                    sockets <= most + closing_at_most,
                    "{sockets} sockets open for a limit of {most}"
                );
            }
            assert_eq!(closing.len(), closing_at_most, "a limit of {most}");
            assert!(refused > 0, "past that, new connections are closed");

            // One of them closes: room for one more.
            drop(closing.pop());
            tokio::time::advance(NO_IDLE_FOR).await;
            assert!(limit.admit(Some(ip("192.0.2.1"))).is_ok());
        }
    }

    /// An address at its limit with every connection busy is looked through
    /// once in a while, not for each connection of a storm from it.
    #[tokio::test(start_paused = true)]
    async fn a_busy_address_is_not_looked_through_for_every_connection() {
        let mut limit = per_ip(1, ProxyTrust::none());
        let (busy, _slot) = busy_from(&mut limit, "192.0.2.1");
        assert!(limit.admit(Some(ip("192.0.2.1"))).is_err());
        drop(busy.request(Version::HTTP_2, true));
        assert!(busy.reclaimable_since().is_some(), "idle now");
        assert!(
            limit.admit(Some(ip("192.0.2.1"))).is_err(),
            "not looked at again so soon"
        );
        tokio::time::advance(NO_IDLE_FOR).await;
        assert!(
            limit.admit(Some(ip("192.0.2.1"))).is_ok(),
            "then it makes way"
        );
        assert!(busy.reclaimable_since().is_none());
    }

    /// A listener whose connections say what they say: a stream is what
    /// [`Listener::poll_sent`] answers for it.
    struct Door;

    impl Listener for Door {
        type Stream = Poll<bool>;
        type Io = tokio::io::DuplexStream;
        type Addr = ();

        async fn accept(&mut self) -> (Poll<bool>, ()) {
            std::future::pending().await
        }

        fn poll_sent(stream: &Poll<bool>, _cx: &mut Context<'_>) -> Poll<bool> {
            *stream
        }

        fn handshake(
            &self,
            _stream: Poll<bool>,
        ) -> impl Future<Output = io::Result<tokio::io::DuplexStream>> + Send + 'static {
            std::future::ready(Err(io::ErrorKind::Unsupported.into()))
        }
    }

    const SENT: Poll<bool> = Poll::Ready(true);
    const GONE: Poll<bool> = Poll::Ready(false);
    const SILENT: Poll<bool> = Poll::Pending;

    fn gate(header_read: Option<u64>) -> Gate<Door> {
        let options = ServerOptions {
            max_connections: Some(4),
            header_read_timeout_ms: header_read,
            ..ServerOptions::default()
        };
        Gate::new(&options, ProxyTrust::none(), 64)
    }

    /// `streams` join the line, and the gate sees what they have done.
    fn line_up(gate: &mut Gate<Door>, streams: &[Poll<bool>]) {
        for stream in streams {
            gate.wait_in_line((*stream, (), None));
        }
        look(gate);
    }

    fn look(gate: &mut Gate<Door>) {
        let _ = poll_waiting(&mut gate.waiting, &mut Context::from_waker(Waker::noop()));
    }

    /// A connection `gate` follows, idle since now.
    fn idle(gate: &mut Gate<Door>) -> Arc<Clock> {
        let clock = Arc::new(Clock::new(None, None));
        gate.track(&clock, &Slot::default());
        drop(clock.request(Version::HTTP_2, true));
        clock
    }

    #[tokio::test(start_paused = true)]
    async fn the_longest_idle_connection_goes_first() {
        let mut gate = gate(None);
        let busy = idle(&mut gate);
        let _request = busy.request(Version::HTTP_11, true);
        let older = idle(&mut gate);
        tokio::time::advance(Duration::from_secs(1)).await;
        let newer = idle(&mut gate);
        tokio::time::advance(Duration::from_secs(1)).await;

        line_up(&mut gate, &[SENT, SENT]);
        gate.reclaim();
        assert!(older.reclaimable_since().is_none(), "reclaimed");
        assert!(newer.reclaimable_since().is_some(), "still idle");
        drop(older);
        gate.reclaim();
        assert!(newer.reclaimable_since().is_none(), "the next one");
    }

    /// However many times the gate looks, and however long the goodbye takes.
    #[tokio::test(start_paused = true)]
    async fn one_client_that_waits_costs_one_connection() {
        let mut gate = gate(None);
        let first = idle(&mut gate);
        let second = idle(&mut gate);
        line_up(&mut gate, &[SENT]);
        gate.reclaim();
        assert!(first.reclaimable_since().is_none());
        drop(first);
        gate.reclaim();
        gate.reclaim();
        assert!(second.reclaimable_since().is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn nothing_is_reclaimed_while_the_last_connection_reclaimed_lingers() {
        let mut gate = gate(None);
        let first = idle(&mut gate);
        let second = idle(&mut gate);
        line_up(&mut gate, &[SENT, SENT]);
        gate.reclaim();
        gate.reclaim();
        assert!(second.reclaimable_since().is_some(), "the first lingers");
        drop(first);
        gate.reclaim();
        assert!(second.reclaimable_since().is_none(), "it has gone");
    }

    /// A reclaimed connection that took a request is not lingering: its
    /// client must not be able to stop every other reclaim with a long one.
    #[tokio::test(start_paused = true)]
    async fn a_request_on_a_reclaimed_connection_holds_up_nobody() {
        let mut gate = gate(None);
        let first = idle(&mut gate);
        let second = idle(&mut gate);
        line_up(&mut gate, &[SENT, SENT]);
        gate.reclaim();
        let _request = first.request(Version::HTTP_2, true);
        gate.reclaim();
        assert!(second.reclaimable_since().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn a_client_that_has_sent_nothing_costs_nobody_anything() {
        let mut gate = gate(None);
        let held = idle(&mut gate);
        line_up(&mut gate, &[SILENT]);
        gate.reclaim();
        assert!(held.reclaimable_since().is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn a_client_that_has_gone_is_forgotten() {
        let mut gate = gate(None);
        let held = idle(&mut gate);
        line_up(&mut gate, &[GONE]);
        assert!(gate.waiting.is_empty());
        gate.reclaim();
        assert!(held.reclaimable_since().is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn a_client_that_says_nothing_has_the_header_timeout() {
        let mut gate = gate(Some(1000));
        line_up(&mut gate, &[SILENT, SENT]);
        tokio::time::advance(Duration::from_millis(999)).await;
        gate.forget_the_silent();
        assert_eq!(gate.waiting.len(), 2);
        tokio::time::advance(Duration::from_millis(1)).await;
        gate.forget_the_silent();
        assert_eq!(gate.waiting.len(), 1, "a client that has sent stays");
        assert!(gate.waiting[0].sent);
    }

    /// Silent clients must not hold up the one behind them, nor a burst lose
    /// a client that has only just connected.
    #[tokio::test(start_paused = true)]
    async fn a_silent_client_gives_way_once_it_has_had_its_time() {
        let mut gate = gate(None);
        line_up(&mut gate, &[SILENT]);
        assert!(gate.accepting());
        for _ in 1..WAITERS_AT_MOST {
            gate.wait_in_line((SILENT, (), None));
        }
        assert!(!gate.accepting(), "they have only just connected");
        tokio::time::advance(SILENT_AT_LEAST).await;
        assert!(gate.accepting());
        gate.wait_in_line((SENT, (), None));
        assert_eq!(gate.waiting.len(), WAITERS_AT_MOST);
        assert_eq!(gate.waiting.back().unwrap().stream, SENT);
    }

    #[tokio::test(start_paused = true)]
    async fn the_backlog_waits_behind_a_client_that_has_sent_something() {
        let mut gate = gate(None);
        line_up(&mut gate, &[SILENT, SENT]);
        assert!(!gate.accepting());
    }

    #[tokio::test(start_paused = true)]
    async fn a_free_slot_goes_to_the_first_client_that_has_sent_something() {
        let mut gate = gate(None);
        line_up(&mut gate, &[SILENT, SENT, SENT]);
        assert_eq!(gate.next_waiter().unwrap().stream, SENT);
        assert_eq!(gate.next_waiter().unwrap().stream, SENT);
        assert_eq!(gate.next_waiter().unwrap().stream, SILENT);
        assert!(gate.next_waiter().is_none());
    }

    #[tokio::test]
    async fn finished_connections_are_forgotten() {
        let mut gate = gate(None);
        for _ in 0..10 * TRACKED_AT_LEAST {
            let clock = Arc::new(Clock::new(None, None));
            gate.track(&clock, &Slot::default());
        }
        assert!(
            gate.tracked.len() <= TRACKED_AT_LEAST + 1,
            "{}",
            gate.tracked.len()
        );
    }

    #[test]
    fn with_no_cap_nothing_is_tracked() {
        let options = ServerOptions {
            max_connections: None,
            ..ServerOptions::default()
        };
        let mut gate = Gate::<Door>::new(&options, ProxyTrust::none(), 64);
        gate.track(&Arc::new(Clock::new(None, None)), &Slot::default());
        assert!(gate.tracked.is_empty());
    }
}
