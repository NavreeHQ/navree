//! [`Search`]: the service behind queries, syncing and imports.

use std::any::TypeId;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{self, Display};
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use navree_contracts::{Dispatcher, JobEnvelope, JobOptions};
use navree_db::{Db, Reader, Uow};
use navree_kernel::{AppContext, BoxError, BoxFuture, Lifecycle, WeakAppContext};
use navree_queue::Queue;
use navree_workspace::Workspaces;
use sea_orm::{DatabaseTransaction, DbBackend, TransactionTrait};
use serde::Serialize;
use tokio::sync::{Notify, mpsc};
use uuid::Uuid;

use crate::engine::{
    DatabaseEngine, MeilisearchEngine, MemoryEngine, SearchEngine, TypesenseEngine,
};
use crate::index::{IndexOps, IndexRegistration, embedding_text, engine_document};
use crate::query::{AcrossQuery, SearchQuery, SimilarQuery};
use crate::searchable::SearchContext;
use crate::sync::SyncDocuments;
use crate::vector::Vectors;
use crate::{Engine, IndexSettings, SearchError, SearchOptions, Searchable, SyncMode};

tokio::task_local! {
    /// Set while [`Search::without_syncing`] runs its future.
    static PAUSED: ();
}

/// Whether syncing is paused for the running task.
pub(crate) fn syncing_paused() -> bool {
    PAUSED.try_with(|()| ()).is_ok()
}

/// The search service: queries, imports and index syncing.
///
/// Provided and exported by `SearchModule` (global), so feature modules
/// inject `Arc<Search>`. Cheap to clone.
#[derive(Clone)]
pub struct Search {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) options: SearchOptions,
    pub(crate) engine: Arc<dyn SearchEngine>,
    pub(crate) entries: Vec<Arc<Entry>>,
    pub(crate) db: Option<Db>,
    queue: Option<Arc<Queue>>,
    dispatcher: Option<Arc<dyn Dispatcher>>,
    workspaces: Option<Arc<Workspaces>>,
    pub(crate) app: WeakAppContext,
    pub(crate) vectors: Option<Vectors>,
    prepared: Mutex<HashSet<String>>,
    inline: InlineSyncs,
    warned_no_queue: AtomicBool,
}

/// A registered index.
pub(crate) struct Entry {
    pub(crate) ops: Arc<dyn IndexOps>,
    /// `E::settings()` with the configured synonyms and stop words.
    pub(crate) settings: IndexSettings,
    /// The engine's index name (`search.prefix` + index).
    pub(crate) index: String,
    /// The entity's table (what change events name).
    pub(crate) table: String,
}

/// What an import did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    /// The records' name (`Contact`).
    pub name: String,
    /// The engine's index name.
    pub index: String,
    /// Rows read.
    pub rows: u64,
    /// Documents written.
    pub documents: u64,
    /// Rows kept out of the index (soft-deleted, or `document` returned
    /// `None`).
    pub skipped: u64,
}

/// One index, as `search:status` shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IndexStatus {
    /// The records' name (`Contact`).
    pub name: String,
    /// The engine's index name.
    pub index: String,
    /// The engine.
    pub engine: String,
    /// Documents in the index.
    pub documents: u64,
    /// Rows in the table.
    pub rows: u64,
    /// Jobs not finished yet on `search.queue` (sync jobs included).
    pub pending_jobs: u64,
}

impl Search {
    /// Builds the service for `app`: the engine from `options`, and every
    /// entity registered with `SearchModule::for_feature`.
    pub(crate) async fn build(app: &AppContext, options: SearchOptions) -> Result<Search, BoxError> {
        options
            .check()
            .map_err(|message| format!("invalid search options: {message}"))?;
        let db = app.try_get::<Db>().await.map(|db| (*db).clone());
        let engine = make_engine(&options, db.as_ref())?;
        let mut entries: Vec<Arc<Entry>> = Vec::new();
        for contributed in app.contributions::<IndexRegistration>() {
            let ops = contributed.value.ops.clone();
            if entries
                .iter()
                .any(|e| e.ops.entity_type() == ops.entity_type())
            {
                continue;
            }
            let base = ops.settings();
            let overrides = options.indexes.get(base.index()).cloned().unwrap_or_default();
            let settings = base.with_overrides(&overrides.synonyms, &overrides.stop_words);
            settings.validate().map_err(|e| e.to_string())?;
            if let Some(other) = entries.iter().find(|e| {
                e.settings.index() == settings.index() || e.settings.name() == settings.name()
            }) {
                return Err(format!(
                    "`{}` and `{}` both use the search index `{}` or the name `{}`",
                    other.ops.type_name(),
                    ops.type_name(),
                    settings.index(),
                    settings.name()
                )
                .into());
            }
            entries.push(Arc::new(Entry {
                index: options.index_name(settings.index()),
                table: ops.table(),
                settings,
                ops,
            }));
        }
        let vectors = match &options.vectors {
            Some(vector_options) => Some(Vectors::build(app, vector_options, &options, db.as_ref()).await?),
            None => None,
        };
        Ok(Search {
            inner: Arc::new(Inner {
                queue: app.try_get::<Queue>().await,
                dispatcher: app.try_get::<dyn Dispatcher>().await,
                workspaces: app.try_get::<Workspaces>().await,
                app: app.downgrade(),
                options,
                engine,
                entries,
                db,
                vectors,
                prepared: Mutex::new(HashSet::new()),
                inline: InlineSyncs::default(),
                warned_no_queue: AtomicBool::new(false),
            }),
        })
    }

    /// The options the service runs with.
    pub fn options(&self) -> &SearchOptions {
        &self.inner.options
    }

    /// The engine driver.
    pub fn engine(&self) -> &Arc<dyn SearchEngine> {
        &self.inner.engine
    }

    /// A query on `E`'s index for `text` (empty matches every document).
    pub fn query<E: Searchable>(&self, text: impl Into<String>) -> SearchQuery<E> {
        SearchQuery::new(self.clone(), text.into())
    }

    /// The records of `E` nearest in meaning to `text` (vector search).
    pub fn similar<E: Searchable>(&self, text: impl Into<String>) -> SimilarQuery<E> {
        SimilarQuery::text(self.clone(), text.into())
    }

    /// The records of `E` nearest in meaning to the record `id`.
    pub fn similar_to<E: Searchable>(&self, id: impl Display) -> SimilarQuery<E> {
        SimilarQuery::record(self.clone(), id.to_string())
    }

    /// One query over several indexes, merged by score.
    pub fn across(&self, text: impl Into<String>) -> AcrossQuery {
        AcrossQuery::new(self.clone(), text.into())
    }

    /// Re-syncs the records `ids` of `E`: queues a sync job with
    /// `sync = "queued"`, syncs them now otherwise (also with `off`).
    pub async fn reindex<E: Searchable>(
        &self,
        ids: impl IntoIterator<Item = impl Display>,
    ) -> Result<(), SearchError> {
        let entry = self.entry::<E>()?;
        let ids: Vec<String> = ids.into_iter().map(|id| id.to_string()).collect();
        if ids.is_empty() {
            return Ok(());
        }
        let groups = self.workspace_groups(&entry, &ids).await?;
        for (workspace, ids) in groups {
            let job = SyncDocuments {
                index: entry.settings.index().to_owned(),
                ids,
                workspace,
            };
            match (self.inner.options.sync, &self.inner.queue) {
                (SyncMode::Queued, Some(queue)) => {
                    queue.push(self.envelope(&job)?).await?;
                }
                (SyncMode::Queued, None) if self.inner.dispatcher.is_some() => {
                    let dispatcher = self.inner.dispatcher.as_ref().expect("checked");
                    dispatcher
                        .dispatch_envelope(self.envelope(&job)?)
                        .await
                        .map_err(|e| SearchError::Other(e.to_string()))?;
                }
                _ => self.sync_documents(&job).await?,
            }
        }
        Ok(())
    }

    /// Indexes every row of `E` (existing documents are replaced).
    pub async fn import<E: Searchable>(&self) -> Result<ImportReport, SearchError> {
        let entry = self.entry::<E>()?;
        self.import_entry(&entry, false, &mut |_, _| {}).await
    }

    /// Builds a new index for `E`, imports every row into it and swaps it
    /// in; searches use the old index until the swap.
    pub async fn import_fresh<E: Searchable>(&self) -> Result<ImportReport, SearchError> {
        let entry = self.entry::<E>()?;
        self.import_entry(&entry, true, &mut |_, _| {}).await
    }

    /// Removes every document from `E`'s index.
    pub async fn flush<E: Searchable>(&self) -> Result<(), SearchError> {
        let entry = self.entry::<E>()?;
        self.flush_entry(&entry).await
    }

    /// Pushes `E`'s index settings (fields, synonyms, stop words) to the
    /// engine.
    pub async fn push_settings<E: Searchable>(&self) -> Result<(), SearchError> {
        let entry = self.entry::<E>()?;
        self.push_entry_settings(&entry).await
    }

    /// Every registered index with its document count, row count and the
    /// jobs pending on `search.queue`.
    pub async fn status(&self) -> Result<Vec<IndexStatus>, SearchError> {
        let pending = match &self.inner.queue {
            Some(queue) => queue.pending_count(&self.inner.options.queue).await?,
            None => 0,
        };
        let mut out = Vec::new();
        for entry in &self.inner.entries {
            let documents = self.inner.engine.count(&entry.index).await.unwrap_or(0);
            let rows = match &self.inner.db {
                Some(db) => entry.ops.count_rows(db.conn()).await?,
                None => 0,
            };
            out.push(IndexStatus {
                name: entry.settings.name().to_owned(),
                index: entry.index.clone(),
                engine: self.inner.engine.name().to_owned(),
                documents,
                rows,
                pending_jobs: pending,
            });
        }
        Ok(out)
    }

    /// Runs `future` with change syncing paused for the task: changes
    /// committed while it runs are not synced. Commit inside the future,
    /// then rebuild the index (`import`).
    pub async fn without_syncing<F: Future>(&self, future: F) -> F::Output {
        PAUSED.scope((), future).await
    }

    /// Waits until the inline syncs queued so far have finished (tests).
    pub async fn flush_pending(&self) -> Result<(), SearchError> {
        self.inner.inline.wait().await;
        Ok(())
    }

    /// Panics unless `E`'s index holds the record `id` (tests).
    pub async fn assert_indexed<E: Searchable>(&self, id: impl Display) {
        let entry = self.entry::<E>().expect("the entity is searchable");
        let id = id.to_string();
        let found = self
            .inner
            .engine
            .contains(&entry.index, &id)
            .await
            .expect("the engine answers");
        assert!(found, "expected {} {id} in the `{}` index", entry.settings.name(), entry.index);
    }

    /// Panics if `E`'s index holds the record `id` (tests).
    pub async fn assert_not_indexed<E: Searchable>(&self, id: impl Display) {
        let entry = self.entry::<E>().expect("the entity is searchable");
        let id = id.to_string();
        let found = self
            .inner
            .engine
            .contains(&entry.index, &id)
            .await
            .expect("the engine answers");
        assert!(!found, "expected no {} {id} in the `{}` index", entry.settings.name(), entry.index);
    }

    /// The registered entry of `E`.
    pub(crate) fn entry<E: Searchable>(&self) -> Result<Arc<Entry>, SearchError> {
        self.entry_of_type(TypeId::of::<E>())
            .ok_or(SearchError::NotRegistered(std::any::type_name::<E>()))
    }

    pub(crate) fn entry_of_type(&self, id: TypeId) -> Option<Arc<Entry>> {
        self.inner
            .entries
            .iter()
            .find(|e| e.ops.entity_type() == id)
            .cloned()
    }

    /// The entry named `name` (`Contact`) or with index `name` (`contacts`).
    pub(crate) fn entry_named(&self, name: &str) -> Result<Arc<Entry>, SearchError> {
        self.inner
            .entries
            .iter()
            .find(|e| {
                e.settings.name().eq_ignore_ascii_case(name) || e.settings.index() == name
            })
            .cloned()
            .ok_or_else(|| SearchError::UnknownIndex(name.to_owned()))
    }

    pub(crate) fn entries(&self) -> &[Arc<Entry>] {
        &self.inner.entries
    }

    pub(crate) fn db(&self) -> Result<&Db, SearchError> {
        self.inner
            .db
            .as_ref()
            .ok_or_else(|| SearchError::Other("search needs DatabaseModule in the app".to_owned()))
    }

    pub(crate) fn app(&self) -> Result<AppContext, SearchError> {
        self.inner
            .app
            .upgrade()
            .ok_or_else(|| SearchError::Other("the app has shut down".to_owned()))
    }

    /// The `search.sync` job envelope for `job`, on `search.queue`, unique
    /// for `search.unique_for_secs`.
    pub(crate) fn envelope(&self, job: &SyncDocuments) -> Result<JobEnvelope, SearchError> {
        let options = &self.inner.options;
        Ok(JobEnvelope {
            job_type: crate::sync::SYNC_JOB.to_owned(),
            payload: serde_json::to_value(job).map_err(|e| SearchError::Other(e.to_string()))?,
            options: JobOptions {
                queue: options.queue.clone(),
                unique_for_secs: (options.unique_for_secs > 0).then_some(options.unique_for_secs),
                ..JobOptions::default()
            },
            delay_secs: None,
            unique_key: None,
        })
    }

    /// Queues `job` as part of the commit a change observer sees.
    pub(crate) async fn queue_observed(
        &self,
        tx: &DatabaseTransaction,
        uow: &Uow,
        job: SyncDocuments,
    ) -> Result<(), SearchError> {
        match self.inner.options.sync {
            SyncMode::Off => Ok(()),
            SyncMode::Queued => {
                if let Some(queue) = &self.inner.queue {
                    queue.push_observed(tx, uow, self.envelope(&job)?).await?;
                    return Ok(());
                }
                if let Some(dispatcher) = self.inner.dispatcher.clone() {
                    let envelope = self.envelope(&job)?;
                    uow.after_commit(move || {
                        Box::pin(async move {
                            if let Err(error) = dispatcher.dispatch_envelope(envelope).await {
                                tracing::error!(target: "navree::search", %error, "cannot queue a search sync");
                            }
                        })
                    });
                    return Ok(());
                }
                if !self.inner.warned_no_queue.swap(true, Ordering::Relaxed) {
                    tracing::warn!(
                        target: "navree::search",
                        "search.sync is `queued` but the app has no queue; syncing inline"
                    );
                }
                self.sync_after_commit(uow, job);
                Ok(())
            }
            SyncMode::Inline => {
                self.sync_after_commit(uow, job);
                Ok(())
            }
        }
    }

    fn sync_after_commit(&self, uow: &Uow, job: SyncDocuments) {
        let weak = Arc::downgrade(&self.inner);
        uow.after_commit(move || {
            Box::pin(async move {
                if let Some(inner) = weak.upgrade() {
                    Search { inner }.submit_inline(job);
                }
            })
        });
    }

    /// Hands `job` to the inline sync worker.
    pub(crate) fn submit_inline(&self, job: SyncDocuments) {
        self.inner.inline.submit(Arc::downgrade(&self.inner), job);
    }

    /// The ids grouped by the workspace of their records (one group with
    /// no workspace for indexes without a workspace column).
    async fn workspace_groups(
        &self,
        entry: &Entry,
        ids: &[String],
    ) -> Result<Vec<(Option<String>, Vec<String>)>, SearchError> {
        let Some(column) = entry.settings.workspace_column() else {
            return Ok(vec![(None, ids.to_vec())]);
        };
        let ctx = self.read_context(None).await?;
        let found = entry.ops.workspaces(ctx.conn(), column, ids).await;
        self.finish_context(ctx).await;
        Ok(group_by_workspace(ids, &found?))
    }

    /// Syncs the records of one sync job: upserts the documents of the
    /// records that exist, deletes the others.
    pub(crate) async fn sync_documents(&self, job: &SyncDocuments) -> Result<(), SearchError> {
        let entry = self.entry_named(&job.index)?;
        let workspace = job
            .workspace
            .as_deref()
            .and_then(|w| Uuid::parse_str(w).ok());
        let ctx = self.read_context(workspace).await?;
        let documents = entry.ops.documents(&ctx, &entry.settings, &job.ids).await;
        self.finish_context(ctx).await;
        let documents = documents?;
        let mut upserts = Vec::new();
        let mut deletes = Vec::new();
        for (id, document) in documents {
            match document {
                Some(document) => upserts.push(engine_document(document, &entry.settings)),
                None => deletes.push(id),
            }
        }
        self.write(&entry, &entry.index, upserts, &deletes).await
    }

    /// Writes documents to `target` (an index of `entry`), with their
    /// vectors.
    async fn write(
        &self,
        entry: &Entry,
        target: &str,
        upserts: Vec<crate::Document>,
        deletes: &[String],
    ) -> Result<(), SearchError> {
        self.ensure_prepared(entry, target).await?;
        let engine = &self.inner.engine;
        if let Some(vectors) = &self.inner.vectors
            && entry.settings.embed_fields().next().is_some()
        {
            let texts: Vec<(String, Option<String>, BTreeMap<String, serde_json::Value>, String)> =
                upserts
                    .iter()
                    .filter_map(|doc| {
                        embedding_text(doc, &entry.settings).map(|text| {
                            (
                                doc.id().to_owned(),
                                doc.workspace_id().map(str::to_owned),
                                filter_payload(doc, &entry.settings),
                                text,
                            )
                        })
                    })
                    .collect();
            vectors.upsert(&entry.index, texts).await?;
            if !deletes.is_empty() {
                vectors.delete(&entry.index, deletes).await?;
            }
        }
        if !upserts.is_empty() {
            engine.upsert(target, &entry.settings, upserts).await?;
        }
        if !deletes.is_empty() {
            engine.delete(target, deletes).await?;
        }
        Ok(())
    }

    /// Prepares `index` on the engine once per process.
    async fn ensure_prepared(&self, entry: &Entry, index: &str) -> Result<(), SearchError> {
        if lock(&self.inner.prepared).contains(index) {
            return Ok(());
        }
        self.inner.engine.prepare(index, &entry.settings).await?;
        lock(&self.inner.prepared).insert(index.to_owned());
        Ok(())
    }

    pub(crate) async fn push_entry_settings(&self, entry: &Entry) -> Result<(), SearchError> {
        self.inner.engine.prepare(&entry.index, &entry.settings).await?;
        if let Some(vectors) = &self.inner.vectors
            && entry.settings.embed_fields().next().is_some()
        {
            vectors.prepare(&entry.index).await?;
        }
        lock(&self.inner.prepared).insert(entry.index.clone());
        Ok(())
    }

    pub(crate) async fn flush_entry(&self, entry: &Entry) -> Result<(), SearchError> {
        self.ensure_prepared(entry, &entry.index).await?;
        self.inner.engine.flush(&entry.index).await?;
        if let Some(vectors) = &self.inner.vectors {
            vectors.flush(&entry.index).await?;
        }
        Ok(())
    }

    /// Imports every row of `entry` (into a new index swapped in at the end
    /// when `fresh`), calling `progress(done, total)` after each batch.
    pub(crate) async fn import_entry(
        &self,
        entry: &Entry,
        fresh: bool,
        progress: &mut (dyn FnMut(u64, u64) + Send),
    ) -> Result<ImportReport, SearchError> {
        let engine = &self.inner.engine;
        let staged = format!("{}__fresh", entry.index);
        let target = if fresh {
            // A leftover from an interrupted import.
            engine.drop_index(&staged).await?;
            lock(&self.inner.prepared).remove(&staged);
            if let Some(vectors) = &self.inner.vectors {
                vectors.flush(&entry.index).await?;
            }
            staged.clone()
        } else {
            entry.index.clone()
        };
        self.ensure_prepared(entry, &entry.index).await?;
        self.ensure_prepared(entry, &target).await?;
        let total = match &self.inner.db {
            Some(db) => entry.ops.count_rows(db.conn()).await.unwrap_or(0),
            None => 0,
        };
        let mut report = ImportReport {
            name: entry.settings.name().to_owned(),
            index: entry.index.clone(),
            ..ImportReport::default()
        };
        let chunk_size = self.inner.options.chunk_size;
        for workspace in self.import_workspaces(entry).await? {
            let mut offset = 0;
            loop {
                let ctx = self.read_context(workspace).await?;
                let workspace_text = workspace.map(|w| w.to_string());
                let chunk = entry
                    .ops
                    .chunk(&ctx, &entry.settings, workspace_text.as_deref(), offset, chunk_size)
                    .await;
                self.finish_context(ctx).await;
                let chunk = chunk?;
                if chunk.rows == 0 {
                    break;
                }
                report.rows += chunk.rows;
                report.documents += chunk.documents.len() as u64;
                report.skipped += chunk.skipped.len() as u64;
                let documents = chunk
                    .documents
                    .into_iter()
                    .map(|d| engine_document(d, &entry.settings))
                    .collect();
                let deletes = if fresh { Vec::new() } else { chunk.skipped };
                self.write(entry, &target, documents, &deletes).await?;
                offset += chunk.rows;
                progress(report.rows, total.max(report.rows));
                if chunk.rows < chunk_size {
                    break;
                }
            }
        }
        if fresh {
            engine.swap(&entry.index, &staged).await?;
            lock(&self.inner.prepared).remove(&staged);
        }
        Ok(report)
    }

    /// The workspaces to import one by one: each workspace when rows are
    /// guarded by PostgreSQL row-level security (an import reads them
    /// inside each workspace), else one pass over every row.
    async fn import_workspaces(&self, entry: &Entry) -> Result<Vec<Option<Uuid>>, SearchError> {
        let per_workspace = entry.settings.workspace_column().is_some()
            && self
                .inner
                .db
                .as_ref()
                .is_some_and(|db| db.backend() == DbBackend::Postgres);
        match (&self.inner.workspaces, per_workspace) {
            (Some(workspaces), true) => Ok(workspaces
                .all()
                .await
                .map_err(|e| SearchError::Other(e.to_string()))?
                .into_iter()
                .map(|w| Some(w.id))
                .collect()),
            _ => Ok(vec![None]),
        }
    }

    /// A read context: the current unit of work's transaction when one is
    /// open on the app's database (a request that already wrote), else a
    /// new transaction, inside `workspace` when given (PostgreSQL
    /// row-level security).
    pub(crate) async fn read_context(
        &self,
        workspace: Option<Uuid>,
    ) -> Result<SearchContext, SearchError> {
        let db = self.db()?;
        let app = self.app()?;
        if workspace.is_none()
            && let Reader::Transaction(tx) = db.reader().await
        {
            return Ok(SearchContext::new(tx, app, None).borrowed());
        }
        if let (Some(workspace), Some(workspaces)) = (workspace, &self.inner.workspaces)
            && db.backend() == DbBackend::Postgres
        {
            let uow = db.uow(None, None).await;
            workspaces
                .enter(&uow, workspace)
                .await
                .map_err(|e| SearchError::Other(e.to_string()))?;
            let tx = uow.tx().await?;
            return Ok(SearchContext::new(tx, app, Some(workspace)).with_uow(uow));
        }
        let tx = db.conn().begin().await?;
        Ok(SearchContext::new(Arc::new(tx), app, workspace))
    }

    /// Ends a read context: rolls back the transaction it opened.
    pub(crate) async fn finish_context(&self, ctx: SearchContext) {
        let (tx, uow, borrowed) = ctx.into_parts();
        if borrowed {
            return;
        }
        if let Some(uow) = uow {
            drop(tx);
            if let Err(error) = uow.rollback().await {
                tracing::warn!(target: "navree::search", %error, "cannot end a read transaction");
            }
            return;
        }
        if let Ok(tx) = Arc::try_unwrap(tx)
            && let Err(error) = tx.rollback().await
        {
            tracing::warn!(target: "navree::search", %error, "cannot end a read transaction");
        }
    }
}

impl fmt::Debug for Search {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self
            .inner
            .entries
            .iter()
            .map(|e| e.settings.name())
            .collect();
        f.debug_struct("Search")
            .field("engine", &self.inner.engine.name())
            .field("indexes", &names)
            .finish_non_exhaustive()
    }
}

/// Pushes every index's settings to the engine at boot; an engine that is
/// down is logged and retried on first use.
impl Lifecycle for Search {
    fn on_bootstrap(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            for entry in &self.inner.entries {
                if let Err(error) = self.push_entry_settings(entry).await {
                    tracing::warn!(
                        target: "navree::search",
                        index = %entry.index,
                        %error,
                        "cannot push the index settings to the search engine; retrying on first use"
                    );
                }
            }
            Ok(())
        })
    }
}

/// The filter field values of `doc`, for vector payloads.
fn filter_payload(
    doc: &crate::Document,
    settings: &IndexSettings,
) -> BTreeMap<String, serde_json::Value> {
    doc.fields()
        .iter()
        .filter(|(name, _)| settings.is_filterable(name))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

/// `ids` grouped by their workspace in `found` (ids without one, such as
/// deleted records, in a group without a workspace), in first-seen order.
pub(crate) fn group_by_workspace(
    ids: &[String],
    found: &HashMap<String, String>,
) -> Vec<(Option<String>, Vec<String>)> {
    let mut groups: Vec<(Option<String>, Vec<String>)> = Vec::new();
    for id in ids {
        let workspace = found.get(id).cloned();
        match groups.iter_mut().find(|(w, _)| *w == workspace) {
            Some((_, group)) => group.push(id.clone()),
            None => groups.push((workspace, vec![id.clone()])),
        }
    }
    groups
}

fn make_engine(options: &SearchOptions, db: Option<&Db>) -> Result<Arc<dyn SearchEngine>, BoxError> {
    let timeout = std::time::Duration::from_secs(options.timeout_secs);
    Ok(match &options.engine {
        Engine::Postgres(pg) => {
            let db = db.ok_or("the postgres search engine needs DatabaseModule in the app")?;
            Arc::new(DatabaseEngine::new(db.clone(), pg.clone()))
        }
        Engine::Meilisearch { url, key } => {
            Arc::new(MeilisearchEngine::new(url, key.clone(), timeout)?)
        }
        Engine::Typesense { url, nodes, key } => Arc::new(TypesenseEngine::new(
            url,
            nodes,
            key.clone().unwrap_or_default(),
            timeout,
        )?),
        #[cfg(feature = "tantivy")]
        Engine::Tantivy(tantivy) => Arc::new(crate::engine::TantivyEngine::open(tantivy)?),
        #[cfg(not(feature = "tantivy"))]
        Engine::Tantivy(_) => {
            return Err(
                "`search.engine = \"tantivy\"` needs the `search-tantivy` feature of navree".into(),
            );
        }
        Engine::Memory => Arc::new(MemoryEngine::new()),
        Engine::Custom(engine) => engine.clone(),
    })
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Inline syncs run one at a time, in commit order, on one background
/// task, so a later change is never overwritten by an earlier one.
#[derive(Default)]
struct InlineSyncs {
    sender: Mutex<Option<mpsc::UnboundedSender<SyncDocuments>>>,
    pending: Arc<Pending>,
}

#[derive(Default)]
struct Pending {
    count: AtomicUsize,
    idle: Notify,
}

impl InlineSyncs {
    fn submit(&self, search: Weak<Inner>, job: SyncDocuments) {
        self.pending.count.fetch_add(1, Ordering::SeqCst);
        let mut sender = lock(&self.sender);
        if sender.as_ref().is_none_or(mpsc::UnboundedSender::is_closed) {
            let (tx, rx) = mpsc::unbounded_channel();
            tokio::spawn(run_inline(search, rx, self.pending.clone()));
            *sender = Some(tx);
        }
        if let Some(tx) = sender.as_ref()
            && tx.send(job).is_err()
        {
            self.pending.done();
        }
    }

    async fn wait(&self) {
        loop {
            let idle = self.pending.idle.notified();
            if self.pending.count.load(Ordering::SeqCst) == 0 {
                return;
            }
            idle.await;
        }
    }
}

impl Pending {
    fn done(&self) {
        if self.count.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.idle.notify_waiters();
        }
    }
}

async fn run_inline(
    search: Weak<Inner>,
    mut jobs: mpsc::UnboundedReceiver<SyncDocuments>,
    pending: Arc<Pending>,
) {
    while let Some(job) = jobs.recv().await {
        if let Some(inner) = search.upgrade() {
            let search = Search { inner };
            if let Err(error) = search.sync_documents(&job).await {
                tracing::error!(
                    target: "navree::search",
                    index = %job.index,
                    ids = ?job.ids,
                    %error,
                    "cannot sync search documents"
                );
            }
        }
        pending.done();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_group_by_workspace_in_order() {
        let ids: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        let found = HashMap::from([
            ("a".to_owned(), "w1".to_owned()),
            ("b".to_owned(), "w2".to_owned()),
            ("c".to_owned(), "w1".to_owned()),
        ]);
        assert_eq!(
            group_by_workspace(&ids, &found),
            vec![
                (Some("w1".to_owned()), vec!["a".to_owned(), "c".to_owned()]),
                (Some("w2".to_owned()), vec!["b".to_owned()]),
                (None, vec!["d".to_owned()]),
            ]
        );
    }
}
