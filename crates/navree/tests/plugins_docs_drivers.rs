//! Proves every driver example on `docs/plugins/drivers.mdx`: each of the ten
//! open kinds registers under a name of its own, is selected by configuration,
//! and serves real work, and an unregistered name fails with the documented
//! message.

#![allow(missing_docs)]

use std::collections::BTreeMap;
use std::fs;
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use http::Method;
use navree::ai::{
    Ai, AiCapabilities, AiDriver, AiDriverAnswer, AiDriverRequest, AiDriverStream, AiError,
    AiModule, Finish, Part, ProviderConfig, Usage,
};
use navree::cache::{Cache, CacheBackend, CacheModule, MemoryLockStore, MemoryStore};
use navree::contracts::{Job, JobContext};
use navree::crypto::generate_key;
use navree::db::{DatabaseModule, Db, MigrationSet, PageQuery};
use navree::http::body::Bytes;
use navree::http::{Controller, Json, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, Resolver};
use navree::mail::{
    MailError, MailModule, MailOptions, MailTransport, Mailer, Message, TransportConfig,
};
use navree::notify::{
    Channel, ChannelDriver, ChannelError, Notifiable, Notification, NotificationDelivery, Notifier,
    NotifyModule, SentMessage, SmsDriver, SmsMessage, WhatsAppDriver, WhatsAppMessage,
};
use navree::orm::Model as _;
use navree::plugin::DriverName;
use navree::plugin::prelude::*;
use navree::prelude::*;
use navree::queue::{
    CancellationToken, FailedJob, NewJob, Queue, QueueError, QueueModule, QueueStore, ReservedJob,
    RetryTarget,
};
use navree::realtime::{Backplane, MemoryBackplane, Realtime, RealtimeModule};
use navree::search::{MemoryEngine, Search, SearchModule, Searchable};
use navree::session::{MemoryStore as SessionMemoryStore, Session, SessionModule};
use navree::storage::{
    DiskDriver, DiskDriverError, DiskUpload, FileMeta, OpenedFile, Storage, StorageModule,
};
use navree::telemetry::{
    CustomEvent, EventKind, Interest, Telemetry, TelemetryEvent, TelemetryRecord, TelemetrySink,
};
use navree::testing::TestApp;
use navree::view::ViewModule;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// 1. Cache (`cache.store = "sanduk"`, settings `[cache.sanduk]`)
// ---------------------------------------------------------------------------

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct SandukCacheSettings {
    pub capacity: usize,
}

pub struct SandukCacheModule;

impl Module for SandukCacheModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.cache_store("sanduk", |ctx| async move {
            let settings: SandukCacheSettings = ctx.settings()?;
            let store = Arc::new(MemoryStore::with_capacity(settings.capacity.max(1)));
            Ok(CacheBackend::new(store, Arc::new(MemoryLockStore::new())))
        });
    }
}

async fn cache_example_registers_and_caches() {
    let app = TestApp::builder()
        .config("cache.store", "sanduk")
        .config("cache.sanduk.capacity", 512)
        .module(CacheModule::from_config())
        .module(SandukCacheModule)
        .build()
        .await;

    let cache = app.get::<Cache>().await;
    cache
        .put("invoice:1101", &"₹1,00,000", Duration::from_secs(60))
        .await
        .expect("cache put");
    let cached: Option<String> = cache.get("invoice:1101").await.expect("cache get");
    assert_eq!(cached.as_deref(), Some("₹1,00,000"));
    app.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// 2. Queue (`queue.driver = "sanduk"`, settings `[queue.sanduk]`)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct StoredJob {
    job: NewJob,
    attempts: u32,
    reserved_by: Option<String>,
    reserved_at: Option<i64>,
}

impl StoredJob {
    fn reservable(&self, now: i64) -> bool {
        match self.reserved_at {
            None => self.job.available_at <= now,
            Some(at) => at.saturating_add(self.job.stale_after_ms()) <= now,
        }
    }
}

#[derive(Default)]
struct QueueState {
    jobs: BTreeMap<String, StoredJob>,
    failed: Vec<FailedJob>,
}

#[derive(Clone, Default)]
pub struct SandukQueue {
    state: Arc<Mutex<QueueState>>,
}

impl SandukQueue {
    fn lock(&self) -> MutexGuard<'_, QueueState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl QueueStore for SandukQueue {
    fn insert(&self, job: NewJob) -> BoxFuture<'_, std::result::Result<String, QueueError>> {
        Box::pin(async move {
            let mut state = self.lock();
            if let Some(key) = &job.unique_key
                && let Some(pending) = state
                    .jobs
                    .values_mut()
                    .find(|s| s.job.unique_key.as_ref() == Some(key))
            {
                if job.blocked_by(pending.job.created_at) {
                    return Ok(pending.job.id.clone());
                }
                pending.job.unique_key = None;
            }
            let id = job.id.clone();
            state.jobs.insert(
                id.clone(),
                StoredJob {
                    job,
                    attempts: 0,
                    reserved_by: None,
                    reserved_at: None,
                },
            );
            Ok(id)
        })
    }

    fn reserve<'a>(
        &'a self,
        queue: &'a str,
        worker: &'a str,
        now: i64,
    ) -> BoxFuture<'a, std::result::Result<Option<ReservedJob>, QueueError>> {
        Box::pin(async move {
            let mut state = self.lock();
            let Some(id) = state
                .jobs
                .values()
                .filter(|s| s.job.queue == queue && s.reservable(now))
                .min_by(|a, b| {
                    (a.job.available_at, &a.job.id).cmp(&(b.job.available_at, &b.job.id))
                })
                .map(|s| s.job.id.clone())
            else {
                return Ok(None);
            };
            let stored = state.jobs.get_mut(&id).expect("found");
            stored.attempts += 1;
            stored.reserved_by = Some(worker.to_owned());
            stored.reserved_at = Some(now);
            Ok(Some(ReservedJob {
                id: stored.job.id.clone(),
                queue: stored.job.queue.clone(),
                job_type: stored.job.job_type.clone(),
                payload: stored.job.payload.clone(),
                options: stored.job.options.clone(),
                attempts: stored.attempts,
                trace_parent: stored.job.trace_parent.clone(),
                membership: stored.job.membership.clone(),
            }))
        })
    }

    fn complete<'a>(
        &'a self,
        job: &'a ReservedJob,
    ) -> BoxFuture<'a, std::result::Result<(), QueueError>> {
        Box::pin(async move {
            self.lock().jobs.remove(&job.id);
            Ok(())
        })
    }

    fn release<'a>(
        &'a self,
        job: &'a ReservedJob,
        worker: &'a str,
        available_at: i64,
    ) -> BoxFuture<'a, std::result::Result<bool, QueueError>> {
        Box::pin(async move {
            let mut state = self.lock();
            Ok(match state.jobs.get_mut(&job.id) {
                Some(stored) if stored.reserved_by.as_deref() == Some(worker) => {
                    stored.reserved_by = None;
                    stored.reserved_at = None;
                    stored.job.available_at = available_at;
                    true
                }
                _ => false,
            })
        })
    }

    fn fail<'a>(
        &'a self,
        job: &'a ReservedJob,
        worker: &'a str,
        error: &'a str,
        now: i64,
    ) -> BoxFuture<'a, std::result::Result<bool, QueueError>> {
        Box::pin(async move {
            let mut state = self.lock();
            let held = state
                .jobs
                .get(&job.id)
                .is_some_and(|s| s.reserved_by.as_deref() == Some(worker));
            if !held {
                return Ok(false);
            }
            state.jobs.remove(&job.id);
            state.failed.push(FailedJob {
                id: job.id.clone(),
                queue: job.queue.clone(),
                job_type: job.job_type.clone(),
                payload: job.payload.clone(),
                options: job.options.clone(),
                error: error.to_owned(),
                attempts: job.attempts,
                failed_at: DateTime::from_timestamp_millis(now).unwrap_or_default(),
            });
            Ok(true)
        })
    }

    fn failed(&self) -> BoxFuture<'_, std::result::Result<Vec<FailedJob>, QueueError>> {
        Box::pin(async move { Ok(self.lock().failed.clone()) })
    }

    fn retry<'a>(
        &'a self,
        target: &'a RetryTarget,
        now: i64,
    ) -> BoxFuture<'a, std::result::Result<u64, QueueError>> {
        Box::pin(async move {
            let mut state = self.lock();
            let (retried, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut state.failed)
                .into_iter()
                .partition(|failed| match target {
                    RetryTarget::All => true,
                    RetryTarget::Id(id) => &failed.id == id,
                });
            state.failed = kept;
            let count = retried.len() as u64;
            for failed in retried {
                state.jobs.insert(
                    failed.id.clone(),
                    StoredJob {
                        job: failed.requeued(now),
                        attempts: 0,
                        reserved_by: None,
                        reserved_at: None,
                    },
                );
            }
            Ok(count)
        })
    }

    fn flush_failed(&self) -> BoxFuture<'_, std::result::Result<u64, QueueError>> {
        Box::pin(async move {
            let mut state = self.lock();
            let count = state.failed.len() as u64;
            state.failed.clear();
            Ok(count)
        })
    }

    fn pending_count<'a>(
        &'a self,
        queue: &'a str,
    ) -> BoxFuture<'a, std::result::Result<u64, QueueError>> {
        Box::pin(async move {
            Ok(self
                .lock()
                .jobs
                .values()
                .filter(|s| s.job.queue == queue)
                .count() as u64)
        })
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct SandukQueueSettings {
    pub depot: String,
}

pub struct SandukQueueModule;

impl Module for SandukQueueModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.queue_store("sanduk", |ctx| async move {
            let _settings: SandukQueueSettings = ctx.settings()?;
            Ok(SandukQueue::default())
        });
    }
}

#[derive(Default)]
struct ReceiptJournal(Mutex<Vec<String>>);

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SendReceipt {
    customer: String,
}

impl Job for SendReceipt {
    const NAME: &'static str = "drivers_page.send_receipt";

    async fn handle(self, ctx: JobContext) -> std::result::Result<(), BoxError> {
        let journal = ctx.app.get::<ReceiptJournal>().await?;
        journal.0.lock().unwrap().push(self.customer);
        Ok(())
    }
}

struct ReceiptJobModule(Arc<ReceiptJournal>);

impl Module for ReceiptJobModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value_arc::<ReceiptJournal>(self.0.clone())
            .exports::<ReceiptJournal>();
        m.job::<SendReceipt>();
    }
}

async fn queue_example_registers_and_runs_job() {
    let journal = Arc::new(ReceiptJournal::default());
    let app = TestApp::builder()
        .config("queue.driver", "sanduk")
        .config("queue.sanduk.depot", "Ludhiana")
        .module(QueueModule::from_config())
        .module(SandukQueueModule)
        .module(ReceiptJobModule(journal.clone()))
        .build()
        .await;

    let queue = app.get::<Queue>().await;
    queue
        .dispatch(SendReceipt {
            customer: "Meera Sharma".to_owned(),
        })
        .await
        .expect("dispatch job");
    let stats = queue.run_pending().await.expect("run pending");
    assert_eq!(stats.succeeded, 1);
    assert_eq!(*journal.0.lock().unwrap(), ["Meera Sharma"]);
    app.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// 3. Sessions (`session.store = "sanduk"`, settings `[session.sanduk]`)
// ---------------------------------------------------------------------------

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct SandukSessionSettings {
    pub shelf: String,
}

pub struct SandukSessionModule;

impl Module for SandukSessionModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.session_store("sanduk", |ctx| async move {
            let _settings: SandukSessionSettings = ctx.settings()?;
            Ok(SessionMemoryStore::new())
        });
    }
}

struct SessionProbeController;

impl Injectable for SessionProbeController {
    fn construct(_r: Resolver) -> BoxFuture<'static, std::result::Result<Self, KernelError>> {
        Box::pin(async { Ok(SessionProbeController) })
    }
}

#[derive(Deserialize)]
struct SessionPut {
    city: String,
}

async fn put_session(session: Session, Json(input): Json<SessionPut>) -> Result<Json<Value>> {
    session.insert("city", &input.city)?;
    Ok(Json(json!({ "ok": true })))
}

async fn get_session(session: Session) -> Json<Value> {
    Json(json!({ "city": session.get::<String>("city") }))
}

impl Controller for SessionProbeController {
    fn prefix(&self) -> &'static str {
        "/session-probe"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::POST, "/", put_session).public(),
            RouteDef::on(Method::GET, "/", get_session).public(),
        ]
    }
}

struct SessionProbeModule;

impl Module for SessionProbeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<SessionProbeController>();
    }
}

async fn session_example_registers_and_keeps_session() {
    let app = TestApp::builder()
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .config("session.store", "sanduk")
        .config("session.sanduk.shelf", "Doaba")
        .module(CryptoModule)
        .module(SessionModule::from_config())
        .module(SandukSessionModule)
        .module(SessionProbeModule)
        .build()
        .await;

    let client = app.client();
    client.csrf_cookie().await;
    client
        .post("/api/session-probe")
        .xsrf()
        .json(&json!({ "city": "Ludhiana" }))
        .send()
        .await
        .assert_status(200);
    let body = client
        .get("/api/session-probe")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["city"], "Ludhiana");
    app.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// 4. Backplane (`realtime.backplane = "sanduk"`, needs `realtime.secret`)
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct SandukBackplaneModule {
    pub backplane: MemoryBackplane,
}

impl Module for SandukBackplaneModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let backplane = self.backplane.clone();
        m.backplane("sanduk", move |_ctx| {
            let backplane = backplane.clone();
            async move { Ok(backplane) }
        });
    }
}

async fn backplane_example_registers_and_broadcasts() {
    let backplane = MemoryBackplane::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let stop = CancellationToken::new();
    let sub_stop = stop.clone();
    let sub_backplane = backplane.clone();
    tokio::spawn(async move {
        let _ = sub_backplane
            .subscribe(
                "realtime:navree",
                Arc::new(move |bytes: Bytes| {
                    let _ = tx.send(String::from_utf8_lossy(&bytes).into_owned());
                }),
                sub_stop,
            )
            .await;
    });

    // Wait briefly until the subscriber registers on the shared backplane.
    for _ in 0..50 {
        if backplane.subscribers("realtime:navree") > 0 {
            break;
        }
        tokio::task::yield_now().await;
    }

    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("realtime.backplane", "sanduk")
        .config("realtime.secret", "ludhiana-amritsar-shared-secret")
        .module(RealtimeModule::from_config())
        .module(SandukBackplaneModule { backplane })
        .build()
        .await;

    let realtime = app.get::<Realtime>().await;
    realtime
        .broadcast(
            ["orders"],
            "order.dispatched",
            json!({ "city": "Amritsar" }),
        )
        .expect("broadcast");

    let found = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(payload) = rx.recv().await {
            if payload.contains("order.dispatched") && payload.contains("Amritsar") {
                return true;
            }
        }
        false
    })
    .await
    .expect("subscriber received broadcast");
    assert!(found);

    stop.cancel();
    app.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// 5. Mail (`mail.transport = "dakghar"`, settings `[mail.dakghar]`)
// ---------------------------------------------------------------------------

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct DakgharSettings {
    pub sender_id: String,
}

#[derive(Clone, Default)]
pub struct Dakghar {
    pub sender_id: String,
    pub delivered: Arc<Mutex<Vec<(Option<String>, Vec<String>, String)>>>,
}

impl MailTransport for Dakghar {
    fn send<'a>(
        &'a self,
        message: &'a Message,
    ) -> BoxFuture<'a, std::result::Result<(), MailError>> {
        Box::pin(async move {
            self.delivered.lock().unwrap().push((
                message.from.clone(),
                message.to.clone(),
                message.subject.clone(),
            ));
            Ok(())
        })
    }

    fn name(&self) -> &'static str {
        "dakghar"
    }
}

#[derive(Clone, Default)]
pub struct DakgharModule {
    pub delivered: Arc<Mutex<Vec<(Option<String>, Vec<String>, String)>>>,
}

impl Module for DakgharModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let delivered = self.delivered.clone();
        m.mail_transport("dakghar", move |ctx| {
            let delivered = delivered.clone();
            async move {
                let settings: DakgharSettings = ctx.settings()?;
                Ok(Dakghar {
                    sender_id: settings.sender_id,
                    delivered,
                })
            }
        });
    }
}

async fn mail_example_registers_and_delivers() {
    let module = DakgharModule::default();
    let delivered = module.delivered.clone();

    let app = TestApp::builder()
        .config("mail.from", "Harpreet Sandhu <harpreet@ludhiana.example>")
        .config("mail.transport", "dakghar")
        .config("mail.dakghar.sender_id", "LDN-01")
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .module(module)
        .build()
        .await;

    let mailer = app.get::<Mailer>().await;
    mailer
        .send_message(
            Message::new()
                .to("Meera Sharma <meera@amritsar.example>")
                .subject("Rent receipt, ₹25,000")
                .text("Received with thanks."),
        )
        .await
        .expect("send mail");

    let sent = delivered.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].0.as_deref(),
        Some("Harpreet Sandhu <harpreet@ludhiana.example>")
    );
    assert_eq!(sent[0].1, ["Meera Sharma <meera@amritsar.example>"]);
    assert_eq!(sent[0].2, "Rent receipt, ₹25,000");
    app.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// 6. Storage (`storage.disks.invoices.driver = "sanduk"`)
// ---------------------------------------------------------------------------

type StoredFiles = Arc<Mutex<BTreeMap<String, (Bytes, DateTime<Utc>)>>>;

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct SandukDiskSettings {
    pub host: String,
}

#[derive(Clone, Default)]
pub struct SandukDisk {
    files: StoredFiles,
}

impl SandukDisk {
    fn get_entry(
        &self,
        path: &str,
    ) -> std::result::Result<(Bytes, DateTime<Utc>), DiskDriverError> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or(DiskDriverError::NotFound)
    }
}

struct SandukUpload {
    files: StoredFiles,
    path: String,
    buffer: Vec<u8>,
}

impl DiskUpload for SandukUpload {
    fn write<'a>(
        &'a mut self,
        chunk: Bytes,
    ) -> BoxFuture<'a, std::result::Result<(), DiskDriverError>> {
        self.buffer.extend_from_slice(&chunk);
        Box::pin(async { Ok(()) })
    }

    fn finish(self: Box<Self>) -> BoxFuture<'static, std::result::Result<(), DiskDriverError>> {
        let SandukUpload {
            files,
            path,
            buffer,
        } = *self;
        files
            .lock()
            .unwrap()
            .insert(path, (Bytes::from(buffer), Utc::now()));
        Box::pin(async { Ok(()) })
    }

    fn abort(self: Box<Self>) -> BoxFuture<'static, std::result::Result<(), DiskDriverError>> {
        Box::pin(async { Ok(()) })
    }
}

impl DiskDriver for SandukDisk {
    fn describe(&self) -> String {
        "sanduk disk in memory".to_owned()
    }

    fn put<'a>(
        &'a self,
        path: &'a str,
        contents: Bytes,
    ) -> BoxFuture<'a, std::result::Result<(), DiskDriverError>> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_owned(), (contents, Utc::now()));
        Box::pin(async { Ok(()) })
    }

    fn begin_upload<'a>(
        &'a self,
        path: &'a str,
    ) -> BoxFuture<'a, std::result::Result<Box<dyn DiskUpload>, DiskDriverError>> {
        let upload = SandukUpload {
            files: self.files.clone(),
            path: path.to_owned(),
            buffer: Vec::new(),
        };
        Box::pin(async move { Ok(Box::new(upload) as Box<dyn DiskUpload>) })
    }

    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, std::result::Result<Bytes, DiskDriverError>> {
        let result = self.get_entry(path).map(|(contents, _)| match range {
            Some(range) => {
                let end = usize::try_from(range.end)
                    .map_or(contents.len(), |end| end.min(contents.len()));
                let start = usize::try_from(range.start).map_or(end, |start| start.min(end));
                contents.slice(start..end)
            }
            None => contents,
        });
        Box::pin(async move { result })
    }

    fn open<'a>(
        &'a self,
        path: &'a str,
    ) -> BoxFuture<'a, std::result::Result<OpenedFile, DiskDriverError>> {
        let result = self.get_entry(path).map(|(contents, _)| {
            let size = contents.len() as u64;
            OpenedFile::new(size, Box::pin(tokio_stream::once(Ok(contents))))
        });
        Box::pin(async move { result })
    }

    fn head<'a>(
        &'a self,
        path: &'a str,
    ) -> BoxFuture<'a, std::result::Result<FileMeta, DiskDriverError>> {
        let result = self
            .get_entry(path)
            .map(|(contents, last_modified)| FileMeta {
                path: path.to_owned(),
                size: contents.len() as u64,
                last_modified,
            });
        Box::pin(async move { result })
    }

    fn delete<'a>(
        &'a self,
        path: &'a str,
    ) -> BoxFuture<'a, std::result::Result<(), DiskDriverError>> {
        let removed = self.files.lock().unwrap().remove(path);
        Box::pin(async move { removed.map(drop).ok_or(DiskDriverError::NotFound) })
    }

    fn list<'a>(
        &'a self,
        prefix: &'a str,
    ) -> BoxFuture<'a, std::result::Result<Vec<FileMeta>, DiskDriverError>> {
        let folder = format!("{prefix}/");
        let files = self
            .files
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| prefix.is_empty() || path.starts_with(&folder))
            .map(|(path, (contents, last_modified))| FileMeta {
                path: path.clone(),
                size: contents.len() as u64,
                last_modified: *last_modified,
            })
            .collect();
        Box::pin(async move { Ok(files) })
    }
}

pub struct SandukDiskModule;

impl Module for SandukDiskModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.disk_driver("sanduk", |ctx| async move {
            let _settings: SandukDiskSettings = ctx.settings()?;
            Ok(SandukDisk::default())
        });
    }
}

async fn storage_example_registers_and_stores_invoice() {
    let app = TestApp::builder()
        .config("app.key", generate_key())
        .config("storage.default", "invoices")
        .config(
            "storage.disks.invoices",
            json!({ "driver": "sanduk", "host": "files.ludhiana.example" }),
        )
        .module(CryptoModule)
        .module(StorageModule::from_config())
        .module(SandukDiskModule)
        .build()
        .await;

    let storage = app.get::<Storage>().await;
    let disk = storage.disk("invoices").expect("invoices disk");
    disk.put("2026/1101.txt", "Invoice 1101, ₹1,00,000")
        .await
        .expect("put invoice");
    let text = String::from_utf8(
        disk.get("2026/1101.txt")
            .await
            .expect("get invoice")
            .to_vec(),
    )
    .expect("utf-8");
    assert_eq!(text, "Invoice 1101, ₹1,00,000");
    app.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// 7. Notifications (`m.notification_channel::<SandeshChannel>()`, SMS & WhatsApp)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct SandeshJournal(Mutex<Vec<(String, Value)>>);

pub struct SandeshChannel {
    journal: Arc<SandeshJournal>,
}

impl ChannelDriver for SandeshChannel {
    const NAME: &'static str = "sandesh";

    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a Value,
    ) -> BoxFuture<'a, std::result::Result<SentMessage, ChannelError>> {
        Box::pin(async move {
            self.journal
                .0
                .lock()
                .unwrap()
                .push((to.to_owned(), message.clone()));
            Ok(SentMessage::new("sandesh"))
        })
    }
}

impl Injectable for SandeshChannel {
    fn construct(r: Resolver) -> BoxFuture<'static, std::result::Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(SandeshChannel {
                journal: r.get::<SandeshJournal>().await?,
            })
        })
    }
}

pub struct SandeshSms;

impl SmsDriver for SandeshSms {
    fn send<'a>(
        &'a self,
        _to: &'a str,
        _message: &'a SmsMessage,
    ) -> BoxFuture<'a, std::result::Result<SentMessage, ChannelError>> {
        Box::pin(async { Ok(SentMessage::new("sandesh_sms")) })
    }
}

pub struct SandeshWhatsApp;

impl WhatsAppDriver for SandeshWhatsApp {
    fn send<'a>(
        &'a self,
        _to: &'a str,
        _message: &'a WhatsAppMessage,
    ) -> BoxFuture<'a, std::result::Result<SentMessage, ChannelError>> {
        Box::pin(async { Ok(SentMessage::new("sandesh_wa")) })
    }
}

#[derive(Clone, Default)]
pub struct SandeshModule {
    pub journal: Arc<SandeshJournal>,
}

impl Module for SandeshModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value_arc::<SandeshJournal>(self.journal.clone());
        m.notification_channel::<SandeshChannel>();
        m.sms_driver("sandesh_sms", |_ctx| async { Ok(SandeshSms) });
        m.whatsapp_driver("sandesh_wa", |_ctx| async { Ok(SandeshWhatsApp) });
    }
}

struct SocietyMember {
    id: Uuid,
    handle: String,
}

impl Notifiable for SocietyMember {
    fn notifiable_type(&self) -> &'static str {
        "user"
    }

    fn notifiable_id(&self) -> String {
        self.id.to_string()
    }

    fn route_mail(&self) -> Option<String> {
        None
    }

    fn route_channel(&self, _channel: DriverName) -> Option<String> {
        Some(self.handle.clone())
    }
}

#[derive(Serialize, Deserialize)]
struct MaintenanceAlert {
    channel: Channel,
    summary: String,
}

impl Notification for MaintenanceAlert {
    const TYPE: &'static str = "society.maintenance";

    fn via(&self, _notifiable: &dyn Notifiable) -> Vec<Channel> {
        vec![self.channel.clone()]
    }

    fn to_channel(&self, _channel: DriverName, _notifiable: &dyn Notifiable) -> Option<Value> {
        Some(json!({ "summary": self.summary }))
    }
}

async fn notifications_example_delivers_and_records() {
    let module = SandeshModule::default();
    let journal = module.journal.clone();

    let app = TestApp::builder()
        .temp_database()
        .config("notify.routes", false)
        .config("notify.sms.driver", "sandesh_sms")
        .config("notify.whatsapp.driver", "sandesh_wa")
        .module(DatabaseModule::from_config())
        .module(NotifyModule::from_config())
        .module(module)
        .build()
        .await;

    let simran = SocietyMember {
        id: Uuid::now_v7(),
        handle: "@simran_kaur".to_owned(),
    };
    app.get::<Notifier>()
        .await
        .send_now(
            &simran,
            MaintenanceAlert {
                channel: Channel::named("sandesh"),
                summary: "Lift servicing at Jalandhar block".to_owned(),
            },
        )
        .await
        .expect("send notification");

    let recorded = journal.0.lock().unwrap().clone();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, "@simran_kaur");

    let db = app.get::<Db>().await;
    let rows = NotificationDelivery::query()
        .where_(NotificationDelivery::CHANNEL.eq("sandesh"))
        .get(&*db)
        .await
        .expect("query deliveries");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "sent");
    app.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// 8. Search (`search.engine = "sanduk"`, settings `[search.sanduk]`)
// ---------------------------------------------------------------------------

pub struct SandukSearchModule;

impl Module for SandukSearchModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.search_engine("sanduk", |_ctx| async move { Ok(MemoryEngine::new()) });
    }
}

#[derive(Model, Searchable, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "search_entries")]
#[searchable(name = "SearchEntry")]
pub struct SearchEntry {
    pub id: Uuid,
    #[searchable(boost = 2)]
    pub title: String,
}

mod m20261115_000001_create_search_entries {
    use navree::schema::prelude::*;

    pub struct Migration;

    impl navree::schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .create("search_entries", |t| {
                    t.uuid("id").primary();
                    t.string("title");
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.drop_if_exists("search_entries").await
        }
    }
}

#[module(
    imports = [SearchModule::for_feature::<SearchEntry>()],
    register = Self::wire,
)]
struct SearchEntriesModule;

impl SearchEntriesModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(
            MigrationSet::new("search_entries").migration(navree::schema::migration!(
                m20261115_000001_create_search_entries
            )),
        );
    }
}

async fn search_example_indexes_and_finds_record() {
    let app = TestApp::builder()
        .temp_database()
        .config("search.engine", "sanduk")
        .config("search.sync", "inline")
        .module(DatabaseModule::from_config())
        .module(SearchModule::from_config())
        .module(SandukSearchModule)
        .module(SearchEntriesModule)
        .build()
        .await;

    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    SearchEntry::create(
        &uow,
        SearchEntry {
            id: Uuid::now_v7(),
            title: "Ludhiana water supply schedule".to_owned(),
        },
    )
    .await
    .expect("insert searchable entry");
    uow.commit().await.expect("commit");

    let search = app.get::<Search>().await;
    search.flush_pending().await.expect("flush search");
    let page = search
        .query::<SearchEntry>("ludhiana")
        .paginate(PageQuery::default())
        .await
        .expect("search query");
    assert_eq!(page.meta.total, 1);
    assert_eq!(page.data[0].title, "Ludhiana water supply schedule");
    app.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// 9. AI (`ai.providers.vaani`, `m.ai_provider("vaani", ..)`)
// ---------------------------------------------------------------------------

pub struct Vaani {
    pub url: String,
}

impl AiDriver for Vaani {
    fn capabilities(&self) -> AiCapabilities {
        AiCapabilities::default()
    }

    fn chat<'a>(
        &'a self,
        _request: &'a AiDriverRequest,
    ) -> BoxFuture<'a, std::result::Result<AiDriverAnswer, AiError>> {
        Box::pin(async move {
            let part = Part::Text {
                text: format!("Namaste from {}", self.url),
            };
            Ok(AiDriverAnswer::new(
                vec![part],
                Usage::new(4, 3),
                Finish::Stop,
            ))
        })
    }

    fn stream<'a>(
        &'a self,
        _request: &'a AiDriverRequest,
    ) -> BoxFuture<'a, std::result::Result<AiDriverStream, AiError>> {
        Box::pin(async { Err(AiError::unavailable("vaani")) })
    }
}

pub struct VaaniModule;

impl Module for VaaniModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.ai_provider("vaani", |ctx| async move {
            let table: ProviderConfig = ctx.settings()?;
            Ok(Vaani {
                url: table.url.unwrap_or_default(),
            })
        });
    }
}

async fn ai_example_registers_and_answers_chat() {
    let app = TestApp::builder()
        .config("ai.default", "assistant")
        .config(
            "ai.providers.vaani",
            json!({ "url": "https://vaani.ludhiana.example" }),
        )
        .config(
            "ai.models.assistant",
            json!({ "provider": "vaani", "model": "vaani-1" }),
        )
        .module(AiModule::from_config())
        .module(VaaniModule)
        .build()
        .await;

    let answer = app
        .get::<Ai>()
        .await
        .text("Sat Sri Akal")
        .await
        .expect("ai chat");
    assert_eq!(answer.text, "Namaste from https://vaani.ludhiana.example");
    app.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// 10. Telemetry events (`CustomEvent::new("society.ludhiana_visit")`)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct VisitSink {
    seen: Mutex<Vec<CustomEvent>>,
}

impl TelemetrySink for VisitSink {
    fn interest(&self) -> Interest {
        Interest::only([EventKind::Custom])
    }

    fn record(&self, record: &TelemetryRecord) {
        if let TelemetryEvent::Custom(custom) = &record.event {
            self.seen.lock().unwrap().push(custom.clone());
        }
    }
}

#[derive(Clone, Default)]
pub struct VisitSinkModule {
    pub sink: Arc<VisitSink>,
}

impl Module for VisitSinkModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let sink = self.sink.clone();
        m.telemetry_sink_factory(Factory::<dyn TelemetrySink>::new(move |_r: Resolver| {
            let sink = sink.clone();
            async move { Ok(sink as Arc<dyn TelemetrySink>) }
        }));
    }
}

async fn telemetry_example_delivers_custom_event() {
    let module = VisitSinkModule::default();
    let sink = module.sink.clone();

    let app = TestApp::builder().module(module).build().await;
    let event = CustomEvent::new("society.ludhiana_visit")
        .field("visitor", "Gurpreet Singh")
        .field("gate", "Gate 2");
    app.get::<Telemetry>()
        .await
        .emit(TelemetryEvent::Custom(event));

    let events = sink.seen.lock().unwrap().clone();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name, "society.ludhiana_visit");
    assert_eq!(events[0].fields["visitor"], "Gurpreet Singh");
    app.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// Slice check keeping `docs/plugins/drivers.mdx` tied to this file
// ---------------------------------------------------------------------------

fn assert_drivers_page_slices_match_source() {
    let mdx_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugins/drivers.mdx");
    let mdx = fs::read_to_string(&mdx_path)
        .unwrap_or_else(|err| panic!("read {}: {err}", mdx_path.display()))
        .replace("\r\n", "\n");
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root");

    let mut open: Option<(usize, usize, String)> = None;
    let mut code = String::new();
    let mut checked = 0usize;

    for (idx, line) in mdx.split('\n').enumerate() {
        let trimmed = line.trim_start();
        let ticks = trimmed.chars().take_while(|c| *c == '`').count();
        match &open {
            None if ticks >= 3 => {
                open = Some((idx + 1, ticks, trimmed[ticks..].trim().to_owned()));
                code.clear();
            }
            None => {}
            Some((start_line, opened_ticks, info)) => {
                if ticks >= *opened_ticks && trimmed.trim_end().len() == ticks {
                    let lang = info.split([',', ' ']).next().unwrap_or_default();
                    if lang == "rust" {
                        let mut lines = code.lines();
                        if let Some(first) = lines.next()
                            && let Some(rel) =
                                first.trim().strip_prefix("// ").map(str::trim).filter(|p| {
                                    p.starts_with("crates/") || p.starts_with("plugins/")
                                })
                        {
                            let target = fs::read_to_string(repo_root.join(rel))
                                .unwrap_or_else(|err| {
                                    panic!("line {start_line}: cannot read `{rel}`: {err}")
                                })
                                .replace("\r\n", "\n");
                            let hay: Vec<&str> = target.lines().map(str::trim_end).collect();
                            let needle: Vec<&str> = lines.map(str::trim_end).collect();
                            assert!(
                                hay.windows(needle.len()).any(|w| w == needle.as_slice()),
                                "drivers.mdx block at line {start_line} is not a verbatim slice of `{rel}`"
                            );
                            checked += 1;
                        }
                    }
                    open = None;
                } else {
                    code.push_str(line);
                    code.push('\n');
                }
            }
        }
    }
    assert!(
        checked >= 9,
        "expected drivers.mdx to quote all driver examples from source, checked {checked}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_drivers_page_example_registers_and_selects_its_driver() {
    assert_drivers_page_slices_match_source();
    cache_example_registers_and_caches().await;
    queue_example_registers_and_runs_job().await;
    session_example_registers_and_keeps_session().await;
    backplane_example_registers_and_broadcasts().await;
    mail_example_registers_and_delivers().await;
    storage_example_registers_and_stores_invoice().await;
    notifications_example_delivers_and_records().await;
    search_example_indexes_and_finds_record().await;
    ai_example_registers_and_answers_chat().await;
    telemetry_example_delivers_custom_event().await;
}

#[tokio::test]
async fn an_unregistered_name_fails_the_boot_naming_the_setting_and_the_method() {
    // 1. Cache
    let err = TestApp::builder()
        .config("cache.store", "pigeon")
        .module(CacheModule::from_config())
        .try_build()
        .await
        .expect_err("unregistered cache store")
        .to_string();
    assert!(
        err.contains(
            "cache.store does not name a built-in cache store or one a module registers with m.cache_store (known: memory, redis)"
        ),
        "{err}"
    );

    // 2. Queue
    let err = TestApp::builder()
        .config("queue.driver", "pigeon")
        .module(QueueModule::from_config())
        .try_build()
        .await
        .expect_err("unregistered queue store")
        .to_string();
    assert!(
        err.contains(
            "queue.driver does not name a built-in queue store or one a module registers with m.queue_store (known: database, memory, redis)"
        ),
        "{err}"
    );

    // 3. Sessions
    let err = TestApp::builder()
        .config("app.key", generate_key())
        .config("session.store", "pigeon")
        .module(CryptoModule)
        .module(SessionModule::from_config())
        .try_build()
        .await
        .expect_err("unregistered session store")
        .to_string();
    assert!(
        err.contains(
            "session.store does not name a built-in session store or one a module registers with m.session_store (known: memory, database, redis)"
        ),
        "{err}"
    );

    // 4. Backplane
    let err = TestApp::builder()
        .config("realtime.backplane", "pigeon")
        .config("realtime.secret", "ludhiana-secret")
        .module(RealtimeModule::from_config())
        .try_build()
        .await
        .expect_err("unregistered backplane")
        .to_string();
    assert!(
        err.contains(
            "realtime.backplane does not name a built-in backplane or one a module registers with m.backplane (known: none, redis)"
        ),
        "{err}"
    );

    // 5. Mail: bare name never registered in the process, plus a DriverName not registered on the app.
    let err = TestApp::builder()
        .config("mail.transport", "pigeon")
        .module(ViewModule::from_config())
        .module(MailModule::from_config())
        .try_build()
        .await
        .expect_err("unknown bare mail transport")
        .to_string();
    assert!(err.contains("unknown mail transport (expected"), "{err}");

    let err = TestApp::builder()
        .module(ViewModule::from_config())
        .module(MailModule::for_root(MailOptions {
            transport: TransportConfig::Registered(DriverName::from_static("pigeon")),
            ..MailOptions::default()
        }))
        .try_build()
        .await
        .expect_err("unregistered mail DriverName")
        .to_string();
    assert!(
        err.contains(
            "mail.transport does not name a built-in mail transport or one a module registers with m.mail_transport (known: log, array, smtp, smtps, ses, mailgun, postmark, resend, failover)"
        ),
        "{err}"
    );

    // 6. Storage
    let err = TestApp::builder()
        .config("app.key", generate_key())
        .config("storage.default", "invoices")
        .config("storage.disks.invoices", json!({ "driver": "pigeon" }))
        .module(CryptoModule)
        .module(StorageModule::from_config())
        .try_build()
        .await
        .expect_err("unregistered disk driver")
        .to_string();
    assert!(
        err.contains(
            "storage.disks.invoices.driver does not name a built-in disk driver or one a module registers with m.disk_driver (known: local, memory, s3, gcs, azure)"
        ),
        "{err}"
    );

    // 7. Notifications: SMS driver, WhatsApp driver, and named delivery channel
    let err = TestApp::builder()
        .temp_database()
        .config("notify.routes", false)
        .config("notify.sms.driver", "pigeon")
        .module(DatabaseModule::from_config())
        .module(NotifyModule::from_config())
        .try_build()
        .await
        .expect_err("unregistered sms driver")
        .to_string();
    assert!(
        err.contains(
            "notify.sms.driver does not name a built-in SMS driver or one a module registers with m.sms_driver (known: twilio, vonage, sns, aws_sns, log)"
        ),
        "{err}"
    );

    let err = TestApp::builder()
        .temp_database()
        .config("notify.routes", false)
        .config("notify.whatsapp.driver", "pigeon")
        .module(DatabaseModule::from_config())
        .module(NotifyModule::from_config())
        .try_build()
        .await
        .expect_err("unregistered whatsapp driver")
        .to_string();
    assert!(
        err.contains(
            "notify.whatsapp.driver does not name a built-in WhatsApp driver or one a module registers with m.whatsapp_driver (known: twilio, meta, log)"
        ),
        "{err}"
    );

    let app = TestApp::builder()
        .temp_database()
        .config("notify.routes", false)
        .module(DatabaseModule::from_config())
        .module(NotifyModule::from_config())
        .build()
        .await;
    let err = app
        .get::<Notifier>()
        .await
        .send_now(
            &SocietyMember {
                id: Uuid::now_v7(),
                handle: "@kuldeep".to_owned(),
            },
            MaintenanceAlert {
                channel: Channel::named("pigeon"),
                summary: "Gate notice".to_owned(),
            },
        )
        .await
        .expect_err("unregistered notification channel")
        .to_string();
    assert!(
        err.contains(
            "no module registers the notification channel `pigeon` with m.notification_channel"
        ),
        "{err}"
    );
    app.shutdown().await.expect("shutdown");

    // 8. Search
    let err = TestApp::builder()
        .temp_database()
        .config("search.engine", "pigeon")
        .module(DatabaseModule::from_config())
        .module(SearchModule::from_config())
        .try_build()
        .await
        .expect_err("unregistered search engine")
        .to_string();
    assert!(
        err.contains(
            "search.engine does not name a built-in search engine or one a module registers with m.search_engine (known: postgres, database, meilisearch, typesense, tantivy, memory)"
        ),
        "{err}"
    );

    // 9. AI
    let err = TestApp::builder()
        .config("ai.providers.backup.driver", "pigeon")
        .module(AiModule::from_config())
        .try_build()
        .await
        .expect_err("unregistered ai provider")
        .to_string();
    assert!(
        err.contains(
            "ai.providers.backup.driver does not name a built-in AI provider driver or one a module registers with m.ai_provider (known: anthropic, openai, gemini, mistral, ollama, bedrock)"
        ),
        "{err}"
    );
}
