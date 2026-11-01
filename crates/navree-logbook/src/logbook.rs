//! [`Logbook`]: the service that records entries, serves them and is the
//! telemetry sink behind the watchers.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::hash::Hash;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use chrono::{Duration as ChronoDuration, Utc};
use navree_kernel::{BoxFuture, WeakAppContext};
use navree_telemetry::{Interest, TelemetryContext, TelemetryRecord, TelemetrySink};
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};
use wildmatch::WildMatch;

use crate::batch::duplicates;
use crate::redact::Redactor;
use crate::store::{ListQuery, LogbookStore, MemoryStore};
use crate::{
    Batch, BatchSummary, Content, DumpEntry, Entry, EntryKind, Filter, HttpMessage, LogbookError,
    LogbookOptions, Term,
};

/// Logbook: records what the app does, while you develop, and serves it at
/// `/_logbook`.
///
/// `LogbookModule` provides one (global, exported): inject `Arc<Logbook>`
/// to [`tag`](Logbook::tag) the running request or job, to
/// [`dump`](Logbook::dump) a value onto its timeline, or, in tests, to read
/// what a request did ([`last_request`](Logbook::last_request)).
///
/// When Logbook is off (`logbook.enabled = false`, or a release build), it
/// records nothing and every method does nothing: `tag` and `dump` can stay
/// in your code.
pub struct Logbook {
    options: Arc<LogbookOptions>,
    active: Option<Arc<Active>>,
}

/// The running side of an enabled Logbook.
pub(crate) struct Active {
    pub(crate) options: Arc<LogbookOptions>,
    pub(crate) store: Arc<dyn LogbookStore>,
    writer: mpsc::UnboundedSender<Command>,
    pub(crate) redactor: Redactor,
    interest: Interest,
    ignore: Vec<WildMatch>,
    /// What the HTTP layer captured, by request id, until the request's
    /// `RequestHandled` arrives.
    captures: Mutex<Bounded<String, HttpCapture>>,
    /// Requests not recorded (ignored paths), by request id.
    ignored: Mutex<Bounded<String, ()>>,
    /// Replays in flight: new request id -> the replayed request's id.
    pub(crate) replays: Mutex<Bounded<String, String>>,
    /// The app, for replays.
    pub(crate) app: OnceLock<WeakAppContext>,
    /// The router replays run through (built on the first replay).
    pub(crate) router: tokio::sync::OnceCell<navree_http::AppRouter>,
    /// `http://<http.addr>`, for "Copy as curl".
    pub(crate) base_url: OnceLock<String>,
}

/// The request and response the HTTP layer captured.
#[derive(Clone, Debug, Default)]
pub(crate) struct HttpCapture {
    pub(crate) request: HttpMessage,
    pub(crate) response: HttpMessage,
}

/// What the writer task does, in order.
enum Command {
    Insert(Box<Entry>),
    Tags(String, Vec<String>),
    Flush(oneshot::Sender<()>),
}

/// A map that forgets its oldest keys beyond a capacity.
pub(crate) struct Bounded<K, V> {
    map: HashMap<K, V>,
    order: VecDeque<K>,
    capacity: usize,
}

impl<K: Hash + Eq + Clone, V> Bounded<K, V> {
    fn new(capacity: usize) -> Self {
        Bounded {
            map: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    pub(crate) fn insert(&mut self, key: K, value: V) {
        if self.map.insert(key.clone(), value).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > self.capacity {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }

    pub(crate) fn remove(&mut self, key: &K) -> Option<V> {
        let value = self.map.remove(key)?;
        self.order.retain(|k| k != key);
        Some(value)
    }

    fn contains(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

tokio::task_local! {
    /// Set while Logbook's own background work (the writer, the pruner)
    /// runs: what it emits (a store's log lines, say) is never recorded, so
    /// recording can't feed itself.
    static OWN_WORK: ();
}

/// Runs `future` as Logbook's own work: nothing it emits is recorded.
pub(crate) fn own_work<F: Future>(future: F) -> impl Future<Output = F::Output> {
    OWN_WORK.scope((), future)
}

/// Whether the current task is Logbook's own work.
pub(crate) fn in_own_work() -> bool {
    OWN_WORK.try_with(|()| ()).is_ok()
}

impl Logbook {
    /// A Logbook that records nothing (`logbook.enabled = false`).
    pub fn disabled(options: LogbookOptions) -> Logbook {
        Logbook {
            options: Arc::new(options),
            active: None,
        }
    }

    /// A Logbook recording into a [`MemoryStore`], its writer running on
    /// the current Tokio runtime until the Logbook is dropped: for tests
    /// of code that takes a `Logbook`.
    pub fn in_memory(options: LogbookOptions) -> Logbook {
        Logbook::start(
            options,
            Arc::new(MemoryStore::new()),
            std::future::pending(),
            |writer| {
                tokio::spawn(writer);
            },
        )
    }

    /// A Logbook recording into `store` (whatever `logbook.enabled` says).
    /// Its writer task is handed to `spawn` and stops when `stop` completes
    /// or the Logbook is dropped.
    pub fn start(
        options: LogbookOptions,
        store: Arc<dyn LogbookStore>,
        stop: impl Future<Output = ()> + Send + 'static,
        spawn: impl FnOnce(BoxFuture<'static, ()>),
    ) -> Logbook {
        Self::start_with(options, store, Redactor::default(), stop, spawn)
    }

    pub(crate) fn start_with(
        options: LogbookOptions,
        store: Arc<dyn LogbookStore>,
        redactor: Redactor,
        stop: impl Future<Output = ()> + Send + 'static,
        spawn: impl FnOnce(BoxFuture<'static, ()>),
    ) -> Logbook {
        let options = Arc::new(options);
        let (writer, commands) = mpsc::unbounded_channel();
        let mut ignore: Vec<WildMatch> = options
            .ignore
            .routes
            .iter()
            .map(|pattern| WildMatch::new(pattern))
            .collect();
        ignore.push(WildMatch::new(&options.path));
        ignore.push(WildMatch::new(&format!("{}/*", options.path)));
        let active = Arc::new(Active {
            interest: options.interest(),
            options: options.clone(),
            store: store.clone(),
            writer,
            redactor,
            ignore,
            captures: Mutex::new(Bounded::new(512)),
            ignored: Mutex::new(Bounded::new(512)),
            replays: Mutex::new(Bounded::new(64)),
            app: OnceLock::new(),
            router: tokio::sync::OnceCell::new(),
            base_url: OnceLock::new(),
        });
        spawn(Box::pin(own_work(write(
            commands,
            store,
            options.duplicate_queries,
            stop,
        ))));
        Logbook {
            options,
            active: Some(active),
        }
    }

    pub(crate) fn active(&self) -> Option<&Arc<Active>> {
        self.active.as_ref()
    }

    /// Whether Logbook records and serves anything.
    pub fn is_enabled(&self) -> bool {
        self.active.is_some()
    }

    /// The options it runs with.
    pub fn options(&self) -> &LogbookOptions {
        &self.options
    }

    /// The store, when Logbook is on.
    pub fn store(&self) -> Option<&Arc<dyn LogbookStore>> {
        self.active.as_ref().map(|active| &active.store)
    }

    /// Adds `tag` to the running request, job or scheduled task (its
    /// batch); does nothing outside one, or when Logbook is off.
    ///
    /// Tags are searched with `tag:<prefix>` (`tag:import:` finds
    /// `import:42`).
    pub fn tag(&self, tag: impl Into<String>) {
        let Some(active) = &self.active else {
            return;
        };
        let Some(batch) =
            TelemetryContext::with_current(|context| active.batch_of(&context.event_context()))
                .flatten()
        else {
            return;
        };
        let tag = tag.into();
        if !tag.trim().is_empty() {
            active.send_command(Command::Tags(batch, vec![tag]));
        }
    }

    /// Records `value` (any `Serialize` value) as an entry of its own,
    /// labelled `label`, in its place on the running batch's timeline (or
    /// on its own outside a batch). Does nothing when Logbook is off.
    pub fn dump<T: Serialize + ?Sized>(&self, label: &str, value: &T) {
        let Some(active) = &self.active else {
            return;
        };
        let value = serde_json::to_value(value).unwrap_or_else(|error| {
            serde_json::Value::String(format!("[not serializable: {error}]"))
        });
        let context = navree_telemetry::Telemetry::current_context();
        let mut entry = Entry::new(
            Utc::now(),
            Content::Dump(DumpEntry {
                label: label.to_owned(),
                value,
            }),
        );
        active.apply_context(&mut entry, &context);
        active.send(entry);
    }

    /// Waits until everything recorded so far is in the store.
    pub async fn flush(&self) {
        let Some(active) = &self.active else {
            return;
        };
        let (done, wait) = oneshot::channel();
        if active.writer.send(Command::Flush(done)).is_ok() {
            let _ = wait.await;
        }
    }

    /// Entries matching `query`, newest first (after a [`flush`](Logbook::flush)).
    pub async fn entries(&self, query: &ListQuery) -> Result<Vec<Entry>, LogbookError> {
        let active = self.active.as_ref().ok_or(LogbookError::Disabled)?;
        self.flush().await;
        active.store.list(query).await
    }

    /// One entry by id.
    pub async fn entry(&self, id: &str) -> Result<Option<Entry>, LogbookError> {
        let active = self.active.as_ref().ok_or(LogbookError::Disabled)?;
        self.flush().await;
        active.store.entry(id).await
    }

    /// The batch `id` (a request id, a job id or a scheduled run's trace
    /// id), with every entry recorded in it.
    pub async fn batch(&self, id: &str) -> Option<Batch> {
        let active = self.active.as_ref()?;
        self.flush().await;
        match active.store.batch(id).await {
            Ok(entries) if !entries.is_empty() => {
                Some(Batch::new(id, entries, self.options.duplicate_queries))
            }
            Ok(_) => None,
            Err(error) => {
                warn_read(&error);
                None
            }
        }
    }

    /// The most recent request, with everything it did.
    pub async fn last_request(&self) -> Option<Batch> {
        self.last_batch(EntryKind::Request).await
    }

    /// The most recent job attempt, with everything it did.
    pub async fn last_job(&self) -> Option<Batch> {
        self.last_batch(EntryKind::Job).await
    }

    /// The most recent scheduled task run, with everything it did.
    pub async fn last_schedule(&self) -> Option<Batch> {
        self.last_batch(EntryKind::Schedule).await
    }

    async fn last_batch(&self, kind: EntryKind) -> Option<Batch> {
        let query = ListQuery::new(Filter::of([Term::Type(kind)]), 1);
        let head = match self.entries(&query).await {
            Ok(mut entries) => entries.pop()?,
            Err(LogbookError::Disabled) => return None,
            Err(error) => {
                warn_read(&error);
                return None;
            }
        };
        self.batch(head.batch_id.as_deref()?).await
    }

    /// How many entries are stored.
    pub async fn count(&self) -> Result<u64, LogbookError> {
        let active = self.active.as_ref().ok_or(LogbookError::Disabled)?;
        self.flush().await;
        active.store.count().await
    }

    /// Deletes every entry; returns how many went.
    pub async fn clear(&self) -> Result<u64, LogbookError> {
        let active = self.active.as_ref().ok_or(LogbookError::Disabled)?;
        self.flush().await;
        active.store.clear().await
    }

    /// Prunes now: batches older than `logbook.keep_hours`, then the oldest
    /// beyond `logbook.max_entries`. Returns how many entries went.
    pub async fn prune(&self) -> Result<u64, LogbookError> {
        let active = self.active.as_ref().ok_or(LogbookError::Disabled)?;
        self.flush().await;
        let hours = i64::try_from(self.options.keep_hours).unwrap_or(i64::MAX / 3_600_000);
        let cutoff = Utc::now() - ChronoDuration::hours(hours.min(24 * 365 * 100));
        active.store.prune(self.options.max_entries, cutoff).await
    }
}

fn warn_read(error: &LogbookError) {
    tracing::warn!(target: "navree::logbook", error = %error, "cannot read the Logbook store");
}

impl Active {
    /// Hands `entry` to the writer.
    pub(crate) fn send(&self, entry: Entry) {
        let _ = self.writer.send(Command::Insert(Box::new(entry)));
    }

    fn send_command(&self, command: Command) {
        let _ = self.writer.send(command);
    }

    /// Whether `path` is never recorded.
    pub(crate) fn ignores_path(&self, path: &str) -> bool {
        self.ignore.iter().any(|pattern| pattern.matches(path))
    }

    /// Drops whatever request `request_id` records.
    pub(crate) fn mark_ignored(&self, request_id: &str) {
        lock(&self.ignored).insert(request_id.to_owned(), ());
    }

    pub(crate) fn is_ignored(&self, request_id: &str) -> bool {
        lock(&self.ignored).contains(&request_id.to_owned())
    }

    pub(crate) fn store_capture(&self, request_id: String, capture: HttpCapture) {
        lock(&self.captures).insert(request_id, capture);
    }

    pub(crate) fn take_capture(&self, request_id: &str) -> Option<HttpCapture> {
        lock(&self.captures).remove(&request_id.to_owned())
    }

    pub(crate) fn take_replay(&self, request_id: &str) -> Option<String> {
        lock(&self.replays).remove(&request_id.to_owned())
    }

    pub(crate) fn interest(&self) -> Interest {
        self.interest
    }
}

impl TelemetrySink for Logbook {
    fn interest(&self) -> Interest {
        self.active
            .as_ref()
            .map_or(Interest::NONE, |active| active.interest())
    }

    fn record(&self, record: &TelemetryRecord) {
        if let Some(active) = &self.active {
            active.record(record);
        }
    }
}

impl fmt::Debug for Logbook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Logbook")
            .field("enabled", &self.is_enabled())
            .field("store", &self.options.store)
            .finish_non_exhaustive()
    }
}

/// The writer task: stores entries in order, filling in each batch's
/// summary when its own entry arrives (everything recorded inside it is
/// stored by then).
async fn write(
    mut commands: mpsc::UnboundedReceiver<Command>,
    store: Arc<dyn LogbookStore>,
    duplicate_threshold: usize,
    stop: impl Future<Output = ()> + Send + 'static,
) {
    tokio::pin!(stop);
    let mut buffer = Vec::new();
    loop {
        tokio::select! {
            biased;
            received = commands.recv_many(&mut buffer, 256) => {
                if received == 0 {
                    return;
                }
                apply(&mut buffer, &*store, duplicate_threshold).await;
            }
            () = &mut stop => {
                while let Ok(command) = commands.try_recv() {
                    buffer.push(command);
                }
                apply(&mut buffer, &*store, duplicate_threshold).await;
                return;
            }
        }
    }
}

/// Applies `commands` in order, inserting runs of plain entries together.
async fn apply(commands: &mut Vec<Command>, store: &dyn LogbookStore, duplicate_threshold: usize) {
    let mut pending: Vec<Entry> = Vec::new();
    for command in commands.drain(..) {
        match command {
            Command::Insert(entry) if entry.kind().is_batch() => {
                insert(store, std::mem::take(&mut pending)).await;
                let entry = summarize(store, *entry, duplicate_threshold).await;
                insert(store, vec![entry]).await;
            }
            Command::Insert(entry) => pending.push(*entry),
            Command::Tags(key, tags) => {
                insert(store, std::mem::take(&mut pending)).await;
                if let Err(error) = store.add_tags(key, tags).await {
                    warn_write(&error);
                }
            }
            Command::Flush(done) => {
                insert(store, std::mem::take(&mut pending)).await;
                let _ = done.send(());
            }
        }
    }
    insert(store, pending).await;
}

async fn insert(store: &dyn LogbookStore, entries: Vec<Entry>) {
    if entries.is_empty() {
        return;
    }
    if let Err(error) = store.insert(entries).await {
        warn_write(&error);
    }
}

fn warn_write(error: &LogbookError) {
    tracing::warn!(target: "navree::logbook", error = %error, "cannot write to the Logbook store");
}

/// `head` (a request, job or task entry) with the summary of what its batch
/// recorded so far.
async fn summarize(store: &dyn LogbookStore, mut head: Entry, duplicate_threshold: usize) -> Entry {
    let Some(batch_id) = head.batch_id.clone() else {
        return head;
    };
    let inside = match store.batch(&batch_id).await {
        Ok(entries) => entries,
        Err(error) => {
            warn_write(&error);
            Vec::new()
        }
    };
    let mut summary = BatchSummary::default();
    let mut queries = Vec::new();
    for entry in &inside {
        summary.count(entry.kind());
        if let Content::Query(query) = &entry.content {
            if query.slow {
                summary.slow_queries += 1;
            }
            queries.push(query);
        }
    }
    let repeated = duplicates(queries, duplicate_threshold);
    summary.duplicate_queries = u32::try_from(repeated.len()).unwrap_or(u32::MAX);
    head.slow |= summary.slow_queries > 0;
    if let Some(slot) = head.summary_mut() {
        *slot = summary;
    }
    head
}

#[cfg(test)]
mod tests {
    use navree_telemetry::{EventContext, LogLevel, LogRecord, TelemetryEvent};

    use super::*;
    use crate::StoreKind;

    fn line() -> TelemetryRecord {
        TelemetryRecord {
            at: Utc::now(),
            context: EventContext::default(),
            event: TelemetryEvent::Log(LogRecord {
                level: LogLevel::Warn,
                target: "sqlx::pool".into(),
                message: "acquired".into(),
                fields: Default::default(),
            }),
        }
    }

    #[tokio::test]
    async fn what_logbook_does_itself_is_never_recorded() {
        let logbook = Logbook::in_memory(LogbookOptions {
            enabled: Some(true),
            store: StoreKind::Memory,
            ..LogbookOptions::default()
        });
        own_work(async { logbook.record(&line()) }).await;
        assert_eq!(logbook.count().await.unwrap(), 0);
        logbook.record(&line());
        assert_eq!(logbook.count().await.unwrap(), 1);
    }

    #[test]
    fn ignored_paths_use_star_and_question_mark_only() {
        let options = |routes: &[&str]| LogbookOptions {
            enabled: Some(true),
            store: StoreKind::Memory,
            ignore: crate::IgnoreOptions {
                routes: routes.iter().map(|r| (*r).to_owned()).collect(),
            },
            ..LogbookOptions::default()
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let _guard = rt.enter();
        let logbook = Logbook::in_memory(options(&["/health/*", "/v?/ping", "/a*z", "/[ab]"]));
        let ignored = |path: &str| logbook.active().expect("enabled").ignores_path(path);
        // `*` crosses `/` and may match nothing; `?` is one character.
        assert!(ignored("/health/db"));
        assert!(ignored("/health/db/deep/er"));
        assert!(ignored("/health/"));
        assert!(!ignored("/health"));
        assert!(ignored("/v1/ping"));
        assert!(!ignored("/v/ping"));
        assert!(!ignored("/v10/ping"));
        assert!(ignored("/az"));
        assert!(ignored("/a/b/z"));
        assert!(!ignored("/a/b/zy"));
        // Brackets are literal, matching is case-sensitive.
        assert!(!ignored("/a"));
        assert!(ignored("/[ab]"));
        assert!(!ignored("/HEALTH/db"));
        // The logbook's own path and everything under it are never recorded.
        assert!(ignored("/_logbook"));
        assert!(ignored("/_logbook/entries/1"));
        assert!(!ignored("/_logbooks"));
        // A run of `*` between letters still matches in linear time.
        let logbook = Logbook::in_memory(options(&["/*a*a*a*a*a*a*a*b"]));
        assert!(
            !logbook
                .active()
                .expect("enabled")
                .ignores_path(&format!("/{}", "a".repeat(4000)))
        );
    }

    #[test]
    fn bounded_maps_forget_their_oldest_keys() {
        let mut map = Bounded::new(2);
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        assert!(!map.contains(&"a"));
        assert_eq!(map.remove(&"c"), Some(3));
        assert_eq!(map.remove(&"c"), None);
        assert!(map.contains(&"b"));
    }
}
