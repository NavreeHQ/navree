//! [`Batch`]: a request, job or scheduled task with everything it did.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::{
    AiEntry, BroadcastEntry, CacheEntry, Content, DumpEntry, Entry, EntryKind, EventEntry,
    ExceptionEntry, HttpClientEntry, JobEntry, LogEntry, MailEntry, ModelEntry, NotificationEntry,
    QueryEntry, RequestEntry, ScheduleEntry, SearchEntry, SignInEntry, StorageEntry, UiEntry,
    WebhookEntry,
};

/// A batch: the entries recorded while one request, job or scheduled task
/// ran, in order, with its own entry among them.
///
/// ```
/// # use navree_logbook::Batch;
/// fn check(batch: &Batch) {
///     assert!(batch.queries().len() <= 3, "{:#?}", batch.queries());
///     assert!(batch.duplicate_queries().is_empty());
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Batch {
    id: String,
    entries: Vec<Entry>,
    duplicate_threshold: usize,
}

/// A statement that ran `logbook.duplicate_queries` times or more in one
/// batch (bindings aside): usually a loop loading records one at a time.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DuplicateQuery {
    /// The statement.
    pub sql: String,
    /// How many times it ran.
    pub count: usize,
    /// Their total duration.
    pub total_ms: f64,
}

impl Batch {
    /// The batch `id` with its `entries` (oldest first); statements
    /// repeated `duplicate_threshold` times are duplicates.
    pub fn new(
        id: impl Into<String>,
        mut entries: Vec<Entry>,
        duplicate_threshold: usize,
    ) -> Batch {
        entries.sort_by_key(|entry| entry.seq);
        Batch {
            id: id.into(),
            entries,
            duplicate_threshold: duplicate_threshold.max(2),
        }
    }

    /// The batch id: the request id, the job id or the scheduled run's
    /// trace id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Every entry, in the order they were recorded.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The batch's own entry (the request, the job attempt or the task
    /// run); for a job with several attempts, the last one.
    pub fn head(&self) -> Option<&Entry> {
        self.entries.iter().rev().find(|e| e.kind().is_batch())
    }

    /// What kind of batch it is.
    pub fn kind(&self) -> Option<EntryKind> {
        self.head().map(Entry::kind)
    }

    /// The tags of the batch.
    pub fn tags(&self) -> &[String] {
        self.head()
            .or(self.entries.first())
            .map_or(&[][..], |entry| entry.tags.as_slice())
    }

    /// The request, for a request batch.
    pub fn request(&self) -> Option<&RequestEntry> {
        self.pick(|c| match c {
            Content::Request(x) => Some(x),
            _ => None,
        })
        .pop()
    }

    /// The job attempts, for a job batch (and jobs never nest otherwise).
    pub fn jobs(&self) -> Vec<&JobEntry> {
        self.pick(|c| match c {
            Content::Job(x) => Some(x),
            _ => None,
        })
    }

    /// The scheduled task run, for a scheduled batch.
    pub fn schedule(&self) -> Option<&ScheduleEntry> {
        self.pick(|c| match c {
            Content::Schedule(x) => Some(x),
            _ => None,
        })
        .pop()
    }

    /// The queries, in order.
    pub fn queries(&self) -> Vec<&QueryEntry> {
        self.pick(|c| match c {
            Content::Query(x) => Some(x),
            _ => None,
        })
    }

    /// The statements that ran `logbook.duplicate_queries` times or more,
    /// most repeated first.
    pub fn duplicate_queries(&self) -> Vec<DuplicateQuery> {
        duplicates(self.queries(), self.duplicate_threshold)
    }

    /// The events emitted.
    pub fn events(&self) -> Vec<&EventEntry> {
        self.pick(|c| match c {
            Content::Event(x) => Some(x),
            _ => None,
        })
    }

    /// The mail sent.
    pub fn mail(&self) -> Vec<&MailEntry> {
        self.pick(|c| match c {
            Content::Mail(x) => Some(x),
            _ => None,
        })
    }

    /// The cache calls.
    pub fn cache_calls(&self) -> Vec<&CacheEntry> {
        self.pick(|c| match c {
            Content::Cache(x) => Some(x),
            _ => None,
        })
    }

    /// The log lines.
    pub fn logs(&self) -> Vec<&LogEntry> {
        self.pick(|c| match c {
            Content::Log(x) => Some(x),
            _ => None,
        })
    }

    /// The exceptions.
    pub fn exceptions(&self) -> Vec<&ExceptionEntry> {
        self.pick(|c| match c {
            Content::Exception(x) => Some(x),
            _ => None,
        })
    }

    /// The outgoing HTTP calls.
    pub fn http_calls(&self) -> Vec<&HttpClientEntry> {
        self.pick(|c| match c {
            Content::HttpClient(x) => Some(x),
            _ => None,
        })
    }

    /// The notifications.
    pub fn notifications(&self) -> Vec<&NotificationEntry> {
        self.pick(|c| match c {
            Content::Notification(x) => Some(x),
            _ => None,
        })
    }

    /// The data changes.
    pub fn models(&self) -> Vec<&ModelEntry> {
        self.pick(|c| match c {
            Content::Model(x) => Some(x),
            _ => None,
        })
    }

    /// The dumped values.
    pub fn dumps(&self) -> Vec<&DumpEntry> {
        self.pick(|c| match c {
            Content::Dump(x) => Some(x),
            _ => None,
        })
    }

    /// The calls to a model provider.
    pub fn ai_calls(&self) -> Vec<&AiEntry> {
        self.pick(|c| match c {
            Content::Ai(x) => Some(x),
            _ => None,
        })
    }

    /// The broadcasts.
    pub fn broadcasts(&self) -> Vec<&BroadcastEntry> {
        self.pick(|c| match c {
            Content::Broadcast(x) => Some(x),
            _ => None,
        })
    }

    /// The file operations.
    pub fn storage_ops(&self) -> Vec<&StorageEntry> {
        self.pick(|c| match c {
            Content::Storage(x) => Some(x),
            _ => None,
        })
    }

    /// The webhook delivery attempts.
    pub fn webhooks(&self) -> Vec<&WebhookEntry> {
        self.pick(|c| match c {
            Content::Webhook(x) => Some(x),
            _ => None,
        })
    }

    /// The searches.
    pub fn searches(&self) -> Vec<&SearchEntry> {
        self.pick(|c| match c {
            Content::Search(x) => Some(x),
            _ => None,
        })
    }

    /// The sign-in attempts.
    pub fn sign_ins(&self) -> Vec<&SignInEntry> {
        self.pick(|c| match c {
            Content::SignIn(x) => Some(x),
            _ => None,
        })
    }

    /// The Navree UI operations.
    pub fn ui_operations(&self) -> Vec<&UiEntry> {
        self.pick(|c| match c {
            Content::Ui(x) => Some(x),
            _ => None,
        })
    }

    fn pick<'a, T>(&'a self, f: impl Fn(&'a Content) -> Option<&'a T>) -> Vec<&'a T> {
        self.entries.iter().filter_map(|e| f(&e.content)).collect()
    }
}

/// The statements among `queries` that ran `threshold` times or more.
pub(crate) fn duplicates(queries: Vec<&QueryEntry>, threshold: usize) -> Vec<DuplicateQuery> {
    let mut by_sql: BTreeMap<&str, (usize, f64)> = BTreeMap::new();
    for query in queries {
        let slot = by_sql.entry(query.sql.as_str()).or_default();
        slot.0 += 1;
        slot.1 += query.duration_ms;
    }
    let mut found: Vec<DuplicateQuery> = by_sql
        .into_iter()
        .filter(|(_, (count, _))| *count >= threshold)
        .map(|(sql, (count, total_ms))| DuplicateQuery {
            sql: sql.to_owned(),
            count,
            total_ms,
        })
        .collect();
    found.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.sql.cmp(&b.sql)));
    found
}
