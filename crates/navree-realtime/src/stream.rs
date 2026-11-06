//! Server-sent event streams: how a page hears that what it shows changed
//! (`GET /broadcasting/stream`).
//!
//! A stream listens to topics (scoped to the reader: see [`crate::live`]) and
//! to channels (authorized like a WebSocket subscription). Every broadcast to
//! one of them becomes a nudge on the stream: the topic's or channel's name as
//! the event, the broadcast's event name as the data, and an id. The
//! broadcast's data never goes out.
//!
//! - **Resume.** Ids are the sender's clock in milliseconds and a sequence
//!   (`1730000000000-7`); a broadcast keeps its id on every process. Once a
//!   process has served a stream it keeps its last broadcasts
//!   ([`StreamOptions::replay`](crate::StreamOptions::replay)). A stream opened
//!   with `Last-Event-ID` (or `last_event_id` in the query, which is what a
//!   page's runtime sends) is told what it missed: when its last id is still
//!   kept, everything that came after it, and what came in the five seconds
//!   before it (another process's clock may run behind, or its broadcast may
//!   arrive late); when it is not, everything from five seconds before it on,
//!   if all of that is still kept. It then starts with the event `ui:resumed`.
//!   Otherwise it starts with `ui:live`, and the page fetches everything it
//!   shows once. A nudge heard twice costs a fetch; one never heard would cost
//!   the page its news.
//! - **Backpressure.** A broadcast never waits for a reader. Each stream keeps
//!   at most one unread nudge per name: a nudge for a name already waiting
//!   replaces it, and moves to the end. A stream whose nudges stay unread for a
//!   minute while more come is let go of.
//! - **Caps.** Each stream names at most `streams.names` topics and channels,
//!   each user holds at most `streams.per_user` streams, and each stream ends
//!   after `streams.lifetime_secs` (give or take a tenth), so the reader comes
//!   back, resumes, and is authorized again.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::convert::Infallible;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use http::header::{CONTENT_TYPE, HOST, ORIGIN};
use http::request::Parts;
use http::{HeaderValue, StatusCode};
use navree_contracts::{AuthState, Principal, ZoneLevel, ZoneRequest, ZoneSource};
use navree_http::AppError;
use navree_http::response::sse::{Event, KeepAlive, Sse};
use navree_http::response::{IntoResponse, Response};
use navree_kernel::AppContext;
use serde::Deserialize;
use tokio::sync::Notify;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::channel::{ChannelKind, is_valid_name};
use crate::live::{self, Audience, is_reserved, is_topic_name};
use crate::realtime::ListenDecision;
use crate::{Realtime, StreamOptions};

/// The path of the stream route.
pub const STREAM_PATH: &str = "/broadcasting/stream";

/// The event a stream starts with when it could not be resumed: the page
/// fetches everything once.
pub const FRESH_EVENT: &str = "ui:live";

/// The event a stream starts with when it was resumed: what was missed
/// follows as nudges.
pub const RESUMED_EVENT: &str = "ui:resumed";

/// How far before the last id a resumed stream looks, for broadcasts whose
/// sender's clock runs behind.
const OVERLAP_MS: u64 = 5_000;

/// How long a stream's nudges may stay unread while more come.
const STALL: Duration = Duration::from_secs(60);

/// How often a quiet stream sends a comment, so proxies keep it open.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// The header a page's runtime asks with what a stream that failed answers.
const PROBE_HEADER: &str = "x-ui-probe";

/// A nudge's id: the sender's clock in milliseconds, and a sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct EventId {
    pub(crate) ms: u64,
    pub(crate) seq: u64,
}

impl EventId {
    /// `<ms>-<seq>`; `None` for anything else.
    pub(crate) fn parse(text: &str) -> Option<EventId> {
        let (ms, seq) = text.trim().split_once('-')?;
        let digits = |part: &str| {
            !part.is_empty() && part.len() <= 20 && part.bytes().all(|b| b.is_ascii_digit())
        };
        if !digits(ms) || !digits(seq) {
            return None;
        }
        Some(EventId {
            ms: ms.parse().ok()?,
            seq: seq.parse().ok()?,
        })
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.ms, self.seq)
    }
}

/// The current unix time in milliseconds.
fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

/// One broadcast, as a stream that comes back may need it.
#[derive(Clone, Debug)]
struct Recorded {
    id: EventId,
    channels: Vec<String>,
    event: String,
}

/// The broadcasts kept for streams that come back.
#[derive(Debug)]
pub(crate) struct Recent {
    entries: VecDeque<Recorded>,
    capacity: usize,
    /// Every broadcast at or after this (sender's clock) is still kept.
    complete_since: u64,
    /// Nothing is kept until a stream has been served here.
    recording: bool,
}

impl Recent {
    fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            capacity,
            complete_since: u64::MAX,
            recording: false,
        }
    }

    /// Starts keeping broadcasts (the first stream), from `now`.
    fn start(&mut self, now: u64) {
        if !self.recording && self.capacity > 0 {
            self.recording = true;
            self.complete_since = now;
        }
    }

    fn record(&mut self, id: EventId, channels: &[String], event: &str) {
        if !self.recording {
            return;
        }
        self.entries.push_back(Recorded {
            id,
            channels: channels.to_vec(),
            event: event.to_owned(),
        });
        while self.entries.len() > self.capacity {
            if let Some(gone) = self.entries.pop_front() {
                self.complete_since = self.complete_since.max(gone.id.ms.saturating_add(1));
            }
        }
    }

    /// The newest id kept.
    fn head(&self) -> Option<EventId> {
        self.entries.back().map(|entry| entry.id)
    }

    /// Keeps a marker with no channels: the id a fresh stream starts from,
    /// so a stream that comes back from it is found.
    fn mark_start(&mut self, id: EventId) {
        self.record(id, &[], "");
    }

    /// What a stream last told `last` missed, as nudges for the names
    /// `name_of` gives its channels (one per name, the newest, oldest first);
    /// `None` when some of it may no longer be kept.
    ///
    /// When `last` is still kept, everything that came here after it was
    /// missed, and so may be what came in the five seconds before it (a
    /// stream that was on another process saw another order). When it is no
    /// longer kept, everything from five seconds before it on, if all of that
    /// still is.
    fn missed(
        &self,
        last: EventId,
        name_of: impl Fn(&str) -> Option<String>,
    ) -> Option<Vec<Nudge>> {
        if !self.recording {
            return None;
        }
        let floor = last.ms.saturating_sub(OVERLAP_MS);
        let found = self.entries.iter().position(|entry| entry.id == last);
        if found.is_none() && floor < self.complete_since {
            return None;
        }
        let mut nudges: Vec<Nudge> = Vec::new();
        let missed = self.entries.iter().enumerate().filter(|(at, entry)| {
            entry.id != last
                && match found {
                    Some(position) => *at > position || entry.id.ms >= floor,
                    None => entry.id.ms >= floor,
                }
        });
        for (_, entry) in missed {
            for channel in &entry.channels {
                if let Some(name) = name_of(channel) {
                    push(
                        &mut nudges,
                        Nudge {
                            name,
                            id: entry.id,
                            event: entry.event.clone(),
                        },
                    );
                }
            }
        }
        Some(nudges)
    }
}

/// Adds `nudge`, replacing one of the same name (which moves to the end).
fn push(queue: &mut Vec<Nudge>, nudge: Nudge) {
    queue.retain(|waiting| waiting.name != nudge.name);
    queue.push(nudge);
}

/// What a stream sends for a broadcast.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Nudge {
    /// The topic's or the channel's name: the event a page listens for.
    pub(crate) name: String,
    pub(crate) id: EventId,
    /// The broadcast's event name: the data.
    pub(crate) event: String,
}

impl Nudge {
    fn into_event(self) -> Event {
        Event::default()
            .id(self.id.to_string())
            .event(&self.name)
            .data(&self.event)
    }
}

/// One open stream.
pub(crate) struct Listener {
    pub(crate) id: u64,
    /// The signed-in user, if any.
    pub(crate) user: Option<String>,
    /// Each channel it listens on, and the name its nudges go out under.
    pub(crate) channels: Vec<(String, String)>,
    pending: Mutex<Pending>,
    wake: Notify,
    closing: CancellationToken,
}

#[derive(Default)]
struct Pending {
    queue: Vec<Nudge>,
    /// Since when the oldest unread nudge has waited.
    unread_since: Option<Instant>,
}

impl Listener {
    fn pending(&self) -> std::sync::MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues a nudge for `channel`: one per name, the newest. A stream whose
    /// nudges stayed unread for [`STALL`] while more come is closed.
    pub(crate) fn mark(&self, channel: &str, id: EventId, event: &str) {
        let Some((_, name)) = self.channels.iter().find(|(own, _)| own == channel) else {
            return;
        };
        let stalled = {
            let mut pending = self.pending();
            let now = Instant::now();
            let since = *pending.unread_since.get_or_insert(now);
            push(
                &mut pending.queue,
                Nudge {
                    name: name.clone(),
                    id,
                    event: event.to_owned(),
                },
            );
            now.duration_since(since) > STALL
        };
        if stalled {
            tracing::debug!(target: "navree::realtime", stream = self.id, "closing a stream that stopped reading");
            self.close();
        } else {
            self.wake.notify_one();
        }
    }

    /// The oldest unread nudge.
    fn take(&self) -> Option<Nudge> {
        let mut pending = self.pending();
        if pending.queue.is_empty() {
            return None;
        }
        let nudge = pending.queue.remove(0);
        pending.unread_since = (!pending.queue.is_empty()).then(Instant::now);
        Some(nudge)
    }

    /// The number of unread nudges.
    #[cfg(test)]
    pub(crate) fn unread(&self) -> usize {
        self.pending().queue.len()
    }

    /// Ends the stream (its reader reconnects).
    pub(crate) fn close(&self) {
        self.closing.cancel();
        self.wake.notify_one();
    }
}

impl fmt::Debug for Listener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Listener")
            .field("id", &self.id)
            .field("user", &self.user)
            .field("channels", &self.channels.len())
            .finish_non_exhaustive()
    }
}

/// Every stream of this process, the broadcasts kept for those that come
/// back, and the clock their ids come from.
pub(crate) struct Streams {
    options: StreamOptions,
    by_channel: DashMap<String, HashMap<u64, Arc<Listener>>>,
    by_id: DashMap<u64, Arc<Listener>>,
    per_user: DashMap<String, usize>,
    recent: Mutex<Recent>,
    /// Whether a stream was ever served here (until then nothing is kept).
    served: AtomicBool,
    last_ms: AtomicU64,
    seq: AtomicU64,
    next_stream: AtomicU64,
}

impl Streams {
    pub(crate) fn new(options: StreamOptions) -> Self {
        let replay = options.replay;
        Self {
            options,
            by_channel: DashMap::new(),
            by_id: DashMap::new(),
            per_user: DashMap::new(),
            recent: Mutex::new(Recent::new(replay)),
            served: AtomicBool::new(false),
            last_ms: AtomicU64::new(0),
            seq: AtomicU64::new(0),
            next_stream: AtomicU64::new(1),
        }
    }

    fn recent(&self) -> std::sync::MutexGuard<'_, Recent> {
        self.recent.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The id of the next broadcast sent from this process: this process's
    /// clock, which never goes back, and a sequence.
    pub(crate) fn next_id(&self) -> EventId {
        let now = unix_ms();
        let ms = self.last_ms.fetch_max(now, Ordering::SeqCst).max(now);
        EventId {
            ms,
            seq: self.seq.fetch_add(1, Ordering::SeqCst),
        }
    }

    /// The number of open streams.
    pub(crate) fn len(&self) -> usize {
        self.by_id.len()
    }

    /// The streams listening on `channel`.
    pub(crate) fn listeners(&self, channel: &str) -> usize {
        self.by_channel
            .get(channel)
            .map_or(0, |listeners| listeners.len())
    }

    /// Keeps the broadcast for streams that come back, then queues a nudge
    /// on every stream listening to one of `channels`. Never waits.
    pub(crate) fn deliver(&self, id: EventId, channels: &[String], event: &str) {
        if !self.served.load(Ordering::Acquire) {
            return;
        }
        self.recent().record(id, channels, event);
        for channel in channels {
            let Some(listeners) = self.by_channel.get(channel) else {
                continue;
            };
            for listener in listeners.values() {
                listener.mark(channel, id, event);
            }
        }
    }

    /// Closes the streams of user `user_id`; returns how many.
    pub(crate) fn close_user(&self, user_id: &str) -> usize {
        let mut closed = 0;
        for entry in &self.by_id {
            if entry.value().user.as_deref() == Some(user_id) {
                entry.value().close();
                closed += 1;
            }
        }
        closed
    }

    /// Takes a place for one more stream of `user`: `false` when they hold
    /// as many as they may.
    fn admit(&self, user: Option<&str>) -> bool {
        let Some(user) = user else {
            return true;
        };
        let mut count = self.per_user.entry(user.to_owned()).or_insert(0);
        if *count >= self.options.per_user {
            return false;
        }
        *count += 1;
        true
    }

    /// Gives back the place [`admit`](Self::admit) took.
    fn release(&self, user: Option<&str>) {
        let Some(user) = user else {
            return;
        };
        let empty = match self.per_user.get_mut(user) {
            Some(mut count) => {
                *count = count.saturating_sub(1);
                *count == 0
            }
            None => false,
        };
        if empty {
            self.per_user.remove_if(user, |_, count| *count == 0);
        }
    }

    /// Registers a stream on `channels` and works out how it starts: the
    /// nudges it missed since `last` (when it can be resumed) are queued
    /// first. Returns the listener and its first event.
    fn open(
        &self,
        user: Option<String>,
        channels: Vec<(String, String)>,
        last: Option<EventId>,
    ) -> (Arc<Listener>, Event) {
        let listener = Arc::new(Listener {
            id: self.next_stream.fetch_add(1, Ordering::SeqCst),
            user,
            channels,
            pending: Mutex::new(Pending::default()),
            wake: Notify::new(),
            closing: CancellationToken::new(),
        });
        // Under the lock of what is kept: a broadcast is either kept before
        // this, and so among what was missed, or delivered after, to this
        // stream. At worst one is both, and costs one fetch more.
        let (missed, head) = {
            let mut recent = self.recent();
            recent.start(unix_ms());
            self.served.store(true, Ordering::Release);
            self.by_id.insert(listener.id, listener.clone());
            for (channel, _) in &listener.channels {
                self.by_channel
                    .entry(channel.clone())
                    .or_default()
                    .insert(listener.id, listener.clone());
            }
            let missed = last.and_then(|last| {
                recent.missed(last, |channel| {
                    listener
                        .channels
                        .iter()
                        .find(|(own, _)| own == channel)
                        .map(|(_, name)| name.clone())
                })
            });
            // A fresh stream starts from the newest id kept, or from one kept
            // now, so it can come back from there.
            let head = match recent.head() {
                Some(head) => head,
                None => {
                    let id = self.next_id();
                    recent.mark_start(id);
                    id
                }
            };
            (missed, head)
        };
        let first = match (missed, last) {
            (Some(missed), Some(last)) => {
                {
                    let mut pending = listener.pending();
                    // What was missed is older than anything delivered since.
                    let live = std::mem::take(&mut pending.queue);
                    pending.queue = missed;
                    for nudge in live {
                        push(&mut pending.queue, nudge);
                    }
                    if !pending.queue.is_empty() {
                        pending.unread_since = Some(Instant::now());
                    }
                }
                // The reader's own last id: until the missed nudges are read,
                // a stream that drops comes back from the same place.
                Event::default()
                    .id(last.to_string())
                    .event(RESUMED_EVENT)
                    .data("resumed")
            }
            _ => Event::default()
                .id(head.to_string())
                .event(FRESH_EVENT)
                .data("live"),
        };
        (listener, first)
    }

    /// Forgets a stream that ended.
    pub(crate) fn remove(&self, listener: &Listener) {
        if self.by_id.remove(&listener.id).is_none() {
            return;
        }
        for (channel, _) in &listener.channels {
            self.by_channel.remove_if_mut(channel, |_, listeners| {
                listeners.remove(&listener.id);
                listeners.is_empty()
            });
        }
        self.release(listener.user.as_deref());
    }
}

impl fmt::Debug for Streams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Streams")
            .field("open", &self.by_id.len())
            .field("channels", &self.by_channel.len())
            .finish_non_exhaustive()
    }
}

/// The query of `GET /broadcasting/stream`.
#[derive(Debug, Default, Deserialize)]
struct StreamQuery {
    topics: Option<String>,
    channels: Option<String>,
    last_event_id: Option<String>,
}

/// The names of a comma-separated list: trimmed, empty ones dropped, each
/// once, in order.
fn names(list: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in list.unwrap_or_default().split(',') {
        let name = name.trim();
        if !name.is_empty() && !out.iter().any(|seen| seen == name) {
            out.push(name.to_owned());
        }
    }
    out
}

/// Whether a channel name can go out as the event a page listens for: a
/// valid channel name of the characters an event name may have.
fn is_listenable_channel(channel: &str) -> bool {
    is_valid_name(channel)
        && channel.len() <= 64
        && channel
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// The workspace the request is in, as the app's workspace module resolves
/// it (its resolvers, then the membership check): the workspace-level
/// `dyn ZoneSource`, which answers with the owner the request belongs to.
/// `None` without a workspace module, for a request that names none, and for
/// one that names a workspace the user may not enter.
async fn reader_workspace(
    ctx: &AppContext,
    parts: &mut Parts,
    principal: &Principal,
) -> Result<Option<Uuid>, AppError> {
    let Ok(sources) = ctx.get_all::<dyn ZoneSource>().await else {
        return Ok(None);
    };
    for source in sources
        .iter()
        .filter(|source| source.level() == ZoneLevel::Workspace)
    {
        let mut request = ZoneRequest::new(Some(principal), parts);
        match source.zone_of_request(&mut request).await {
            Ok(Some(owner)) => return Ok(Some(owner.id)),
            Ok(None) => {}
            Err(error) => {
                return Err(AppError::internal(format!(
                    "the request's workspace could not be resolved for a live stream: {error}"
                )));
            }
        }
    }
    Ok(None)
}

/// `GET /broadcasting/stream?topics=…&channels=…`: see the module.
pub(crate) async fn serve(realtime: Arc<Realtime>, ctx: AppContext, mut parts: Parts) -> Response {
    match open(&realtime, &ctx, &mut parts).await {
        Ok(Opened::Probe) => ([(CONTENT_TYPE, "text/event-stream")], "").into_response(),
        Ok(Opened::Nothing) => StatusCode::NO_CONTENT.into_response(),
        Ok(Opened::Stream(listener, first)) => respond(realtime, listener, first),
        Err(error) => error.into_response(),
    }
}

/// What a stream request comes to.
enum Opened {
    /// It names nothing to listen to (204: the runtime does not try again).
    Nothing,
    /// The runtime asked what a stream that failed answers: it would open.
    Probe,
    /// A stream, and its first event.
    Stream(Arc<Listener>, Event),
}

/// The text of header `name`, if it has one.
fn header<K: http::header::AsHeaderName>(parts: &Parts, name: K) -> Option<String> {
    parts
        .headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

async fn open(
    realtime: &Realtime,
    ctx: &AppContext,
    parts: &mut Parts,
) -> Result<Opened, AppError> {
    let origin = header(parts, ORIGIN);
    if !realtime.origin_allowed(origin.as_deref(), header(parts, HOST).as_deref()) {
        tracing::warn!(
            target: "navree::security",
            origin = origin.as_deref().unwrap_or_default(),
            "live stream from a disallowed origin"
        );
        return Err(AppError::forbidden().with_detail("Origin not allowed"));
    }
    let query: StreamQuery = serde_urlencoded::from_str(parts.uri.query().unwrap_or_default())
        .map_err(|_| AppError::bad_request("Name topics and channels as comma-separated lists"))?;
    let topics = names(query.topics.as_deref());
    let channels = names(query.channels.as_deref());
    if topics.is_empty() && channels.is_empty() {
        return Ok(Opened::Nothing);
    }
    let options = &realtime.options().streams;
    if topics.len() + channels.len() > options.names {
        return Err(AppError::bad_request(format!(
            "A stream may name at most {} topics and channels",
            options.names
        )));
    }
    if channels.iter().any(|channel| is_reserved(channel)) {
        // Topics are reached through `topics` only, which scopes them to the
        // reader: no page may name another user's or workspace's channel.
        return Err(AppError::forbidden().with_detail("Name topics with `topics`, not as channels"));
    }
    let mut errors = BTreeMap::new();
    let bad_topics: Vec<String> = topics
        .iter()
        .filter(|topic| !is_topic_name(topic))
        .map(|topic| {
            format!("`{topic}` is not a topic: use 1 to 64 letters, digits, `_`, `.` and `-`")
        })
        .collect();
    if !bad_topics.is_empty() {
        errors.insert("topics".to_owned(), bad_topics);
    }
    let bad_channels: Vec<String> = channels
        .iter()
        .filter(|channel| !is_listenable_channel(channel))
        .map(|channel| format!("`{channel}` is not a channel a page can listen to: use 1 to 64 letters, digits, `_`, `.` and `-`"))
        .collect();
    if !bad_channels.is_empty() {
        errors.insert("channels".to_owned(), bad_channels);
    }
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }

    let principal = parts
        .extensions
        .get::<AuthState>()
        .and_then(|state| state.0.clone());
    let mut listening: Vec<(String, String)> = Vec::new();
    for channel in &channels {
        if ChannelKind::of(channel) != ChannelKind::Public {
            let Some(principal) = &principal else {
                return Err(AppError::unauthenticated());
            };
            match realtime.may_listen(principal, channel, ctx).await {
                ListenDecision::Allow => {}
                ListenDecision::Deny => return Err(AppError::forbidden()),
                ListenDecision::Failed(source) => return Err(AppError::internal(source)),
            }
        }
        listening.push((channel.clone(), channel.clone()));
    }
    if !topics.is_empty() {
        let workspace = match &principal {
            Some(principal) => reader_workspace(ctx, parts, principal).await?,
            None => None,
        };
        let mut audiences = vec![Audience::Public];
        if let Some(principal) = &principal {
            audiences.push(Audience::User(principal.user_id));
        }
        if let Some(workspace) = workspace {
            audiences.push(Audience::Workspace(workspace));
        }
        for topic in &topics {
            for audience in &audiences {
                listening.push((live::channel_of(*audience, topic), topic.clone()));
            }
        }
    }

    let user = principal
        .as_ref()
        .map(|principal| principal.user_id.to_string());
    if parts.headers.contains_key(PROBE_HEADER) {
        return Ok(Opened::Probe);
    }
    let streams = realtime.stream_hub();
    if !streams.admit(user.as_deref()) {
        return Err(
            AppError::from_status(StatusCode::TOO_MANY_REQUESTS).with_detail(format!(
                "A user may hold at most {} live streams at once",
                options.per_user
            )),
        );
    }
    let last = header(parts, "last-event-id")
        .or(query.last_event_id)
        .as_deref()
        .and_then(EventId::parse);
    let (listener, first) = streams.open(user, listening, last);
    Ok(Opened::Stream(listener, first))
}

/// Forgets the stream when its body is dropped (the reader left, the server
/// closed it, the app shut down).
struct Leave {
    realtime: Arc<Realtime>,
    listener: Arc<Listener>,
}

impl Drop for Leave {
    fn drop(&mut self) {
        self.realtime.stream_hub().remove(&self.listener);
    }
}

/// The stream's state while it runs.
struct Running {
    leave: Leave,
    first: Option<Event>,
    shutdown: CancellationToken,
    ends: Instant,
}

fn respond(realtime: Arc<Realtime>, listener: Arc<Listener>, first: Event) -> Response {
    let lifetime = realtime.options().streams.lifetime_secs.max(1);
    // Up to a tenth more, so streams opened together don't all end together.
    let jitter = rand::random::<u64>() % (lifetime * 100).max(1);
    let ends = Instant::now() + Duration::from_secs(lifetime) + Duration::from_millis(jitter);
    let shutdown = realtime.shutdown_token().clone();
    let state = Running {
        leave: Leave { realtime, listener },
        first: Some(first),
        shutdown,
        ends,
    };
    let events = futures_util::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(first) = state.first.take() {
                return Some((Ok::<_, Infallible>(first), state));
            }
            let listener = &state.leave.listener;
            if listener.closing.is_cancelled() {
                return None;
            }
            if let Some(nudge) = listener.take() {
                return Some((Ok(nudge.into_event()), state));
            }
            tokio::select! {
                () = listener.closing.cancelled() => return None,
                () = state.shutdown.cancelled() => return None,
                () = tokio::time::sleep_until(state.ends) => return None,
                () = listener.wake.notified() => {}
            }
        }
    });
    let mut response = Sse::new(events)
        .keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
        .into_response();
    // Proxies that buffer would hold nudges back.
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(ms: u64, seq: u64) -> EventId {
        EventId { ms, seq }
    }

    fn channels(list: &[&str]) -> Vec<String> {
        list.iter().map(|c| (*c).to_owned()).collect()
    }

    #[test]
    fn ids_read_back_what_they_write_and_nothing_else() {
        let written = id(1_730_000_000_000, 7);
        assert_eq!(written.to_string(), "1730000000000-7");
        assert_eq!(EventId::parse(&written.to_string()), Some(written));
        for bad in [
            "",
            "-",
            "1-",
            "-1",
            "1-2-3",
            "a-1",
            "1 -2",
            "1-+2",
            &"9".repeat(21),
        ] {
            assert_eq!(EventId::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn ids_never_go_back() {
        let streams = Streams::new(StreamOptions::default());
        let mut last = streams.next_id();
        for _ in 0..1_000 {
            let next = streams.next_id();
            assert!(next > last, "{next} after {last}");
            last = next;
        }
    }

    #[test]
    fn nothing_is_kept_before_a_stream_was_served() {
        // Lazy by default.
        let mut recent = Recent::new(8);
        recent.record(id(10, 0), &channels(&["a"]), "changed");
        assert!(recent.entries.is_empty());
        assert_eq!(recent.missed(id(10, 0), |c| Some(c.to_owned())), None);
        recent.start(10);
        recent.record(id(11, 1), &channels(&["a"]), "changed");
        assert_eq!(recent.entries.len(), 1);
        // A resume from an id kept here: nothing came after it.
        assert_eq!(
            recent.missed(id(11, 1), |c| Some(c.to_owned())),
            Some(Vec::new())
        );
        // One from before the start, never kept here, can't be sure: fresh.
        assert_eq!(recent.missed(id(9, 0), |c| Some(c.to_owned())), None);
    }

    #[test]
    fn what_was_missed_is_one_nudge_per_name_the_newest_oldest_first() {
        let mut recent = Recent::new(16);
        recent.start(0);
        let t = 100_000;
        recent.record(id(t, 1), &channels(&["a"]), "one");
        recent.record(id(t + 10, 2), &channels(&["b"]), "two");
        recent.record(id(t + 20, 3), &channels(&["a", "x"]), "three");
        recent.record(id(t + 30, 4), &channels(&["other"]), "four");
        let names = |c: &str| match c {
            "a" | "x" => Some("topic".to_owned()),
            "b" => Some("b".to_owned()),
            _ => None,
        };
        let missed = recent.missed(id(t, 1), names).expect("all kept");
        assert_eq!(
            missed,
            vec![
                Nudge {
                    name: "b".into(),
                    id: id(t + 10, 2),
                    event: "two".into()
                },
                Nudge {
                    name: "topic".into(),
                    id: id(t + 20, 3),
                    event: "three".into()
                },
            ]
        );
    }

    #[test]
    fn a_resume_looks_five_seconds_back_for_slow_clocks() {
        // Everything after the last id here, and what came in the five
        // seconds before it, which a stream on another process may have seen
        // in another order.
        let mut recent = Recent::new(16);
        recent.start(0);
        let t = 1_000_000;
        recent.record(id(t - 6_000, 9), &channels(&["b"]), "too far back");
        recent.record(id(t - 3_000, 9), &channels(&["a"]), "late clock");
        recent.record(id(t, 1), &channels(&["a"]), "seen");
        recent.record(
            id(t - 9_000, 4),
            &channels(&["c"]),
            "after, however late its clock",
        );
        let missed = recent.missed(id(t, 1), |c| Some(c.to_owned())).unwrap();
        let events: Vec<&str> = missed.iter().map(|n| n.event.as_str()).collect();
        assert_eq!(events, ["late clock", "after, however late its clock"]);

        // An id from another process that never reached this one: the five
        // seconds before it on, all kept (one nudge per name, the newest).
        let missed = recent
            .missed(id(t - 2_000, 77), |c| Some(c.to_owned()))
            .unwrap();
        let events: Vec<&str> = missed.iter().map(|n| n.event.as_str()).collect();
        assert_eq!(events, ["too far back", "seen"]);
    }

    #[test]
    fn a_fresh_start_is_kept_so_a_stream_can_come_back_from_it() {
        let mut recent = Recent::new(4);
        recent.start(0);
        recent.mark_start(id(500, 1));
        recent.record(id(600, 2), &channels(&["a"]), "changed");
        let missed = recent.missed(id(500, 1), |c| Some(c.to_owned())).unwrap();
        assert_eq!(missed.len(), 1, "the marker itself names nothing");
        assert_eq!(recent.head(), Some(id(600, 2)));
    }

    #[test]
    fn a_full_ring_cannot_resume_what_it_dropped() {
        let mut recent = Recent::new(2);
        recent.start(0);
        let t = 1_000_000;
        for (n, at) in [t, t + 10_000, t + 20_000, t + 30_000]
            .into_iter()
            .enumerate()
        {
            recent.record(id(at, n as u64), &channels(&["a"]), "changed");
        }
        // The first two are gone: a reader last told about the first may have missed the second.
        assert_eq!(recent.missed(id(t, 0), |c| Some(c.to_owned())), None);
        // A reader told about the third missed only the fourth, which is kept.
        let missed = recent
            .missed(id(t + 20_000, 2), |c| Some(c.to_owned()))
            .unwrap();
        assert_eq!(missed.len(), 1);
        assert_eq!(missed[0].id, id(t + 30_000, 3));
        // Replay off: never resumed.
        let mut off = Recent::new(0);
        off.start(0);
        assert_eq!(off.missed(id(t, 0), |c| Some(c.to_owned())), None);
    }

    fn listener(channels: &[(&str, &str)]) -> Listener {
        Listener {
            id: 1,
            user: None,
            channels: channels
                .iter()
                .map(|(c, n)| ((*c).to_owned(), (*n).to_owned()))
                .collect(),
            pending: Mutex::new(Pending::default()),
            wake: Notify::new(),
            closing: CancellationToken::new(),
        }
    }

    #[test]
    fn a_reader_that_does_not_read_holds_one_nudge_per_name() {
        // Backpressure. However many broadcasts come, a stream that
        // doesn't read holds one nudge per name, never more.
        let listener = listener(&[
            ("navree-live.orders", "orders"),
            ("private-navree-live.user.u.orders", "orders"),
            ("navree-live.stats", "stats"),
        ]);
        for seq in 0..10_000 {
            let channel = if seq % 2 == 0 {
                "navree-live.orders"
            } else {
                "private-navree-live.user.u.orders"
            };
            listener.mark(channel, id(1, seq), "changed");
        }
        listener.mark("navree-live.stats", id(2, 10_000), "changed");
        listener.mark("not-mine", id(2, 10_001), "changed");
        assert_eq!(listener.unread(), 2);
        let first = listener.take().unwrap();
        assert_eq!((first.name.as_str(), first.id), ("orders", id(1, 9_999)));
        let second = listener.take().unwrap();
        assert_eq!(second.name, "stats");
        assert!(listener.take().is_none());
        // A name heard again moves to the end, so ids go out in order.
        listener.mark("navree-live.orders", id(3, 1), "changed");
        listener.mark("navree-live.stats", id(3, 2), "changed");
        listener.mark("navree-live.orders", id(3, 3), "changed");
        assert_eq!(listener.take().unwrap().name, "stats");
        assert_eq!(listener.take().unwrap().id, id(3, 3));
    }

    #[tokio::test(start_paused = true)]
    async fn a_reader_that_stops_reading_while_more_come_is_let_go_of() {
        let listener = listener(&[("a", "a")]);
        listener.mark("a", id(1, 1), "changed");
        tokio::time::advance(STALL / 2).await;
        listener.mark("a", id(1, 2), "changed");
        assert!(!listener.closing.is_cancelled(), "not long enough");
        tokio::time::advance(STALL).await;
        listener.mark("a", id(1, 3), "changed");
        assert!(listener.closing.is_cancelled(), "stalled");
        // One that reads is never let go of.
        let reading = self::listener(&[("a", "a")]);
        for n in 0..5 {
            reading.mark("a", id(1, n), "changed");
            tokio::time::advance(STALL / 2).await;
            assert!(reading.take().is_some());
        }
        assert!(!reading.closing.is_cancelled());
    }

    #[test]
    fn lists_are_trimmed_and_named_once() {
        assert_eq!(names(Some(" a, b,,a ,c ")), ["a", "b", "c"]);
        assert!(names(None).is_empty());
        assert!(is_listenable_channel("private-crm.deals"));
        assert!(!is_listenable_channel("private-a=b"), "not an event name");
        assert!(!is_listenable_channel(&format!(
            "private-{}",
            "x".repeat(60)
        )));
    }

    #[test]
    fn a_user_holds_at_most_their_share_of_streams() {
        let streams = Streams::new(StreamOptions {
            per_user: 2,
            ..StreamOptions::default()
        });
        assert!(streams.admit(Some("ada")));
        assert!(streams.admit(Some("ada")));
        assert!(!streams.admit(Some("ada")));
        assert!(streams.admit(Some("bob")));
        assert!(
            streams.admit(None),
            "guests are held by the server's limits"
        );
        streams.release(Some("ada"));
        assert!(streams.admit(Some("ada")));
    }
}
