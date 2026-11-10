//! A Navree UI part for an operation (the `ui` feature): [`OperationProgress`], a region that
//! follows a queued operation and ends as a link to its result.
//!
//! An export or an import page starts an operation, then has nothing to show but "working".
//! The region polls the operation's page fragment (and, with the live hub, hears it change),
//! draws a progress bar and the operation's note, and when the operation finishes stops polling
//! and shows the result: a link when the result holds a `url`, the error when it failed.
//!
//! ```ignore
//! let op = operations.start(&uow, Operation::new("deals.export").owner(user)).await?;
//! operations.dispatch(&uow, &op, ExportDeals { operation: op.id }).await?;
//! // On the page:
//! OperationProgress::new(op.id).live()
//! // In the job, when it is done:
//! operations.complete(op.id, json!({ "url": "/exports/deals.csv" })).await?;
//! ```
//!
//! `QueueModule` mounts the fragment at `GET /operations/{id}/progress` (the same sign-in and
//! visibility rules as the JSON route: the owner and holders of `operations.view`, a 404 for
//! everyone else) when the app has `DatabaseModule` and an authenticator. It draws the
//! English words; to draw other words, mount a route of your own that answers with
//! `OperationProgress::new(id).labels(words).record(record)`.

use std::sync::Arc;
use std::time::Duration;

use http::Method;
use navree_contracts::BoxFuture;
use navree_http::{ApiMeta, AppError, Authenticator, Controller, CurrentPrincipal, Path, RouteDef};
use navree_kernel::{Injectable, KernelError, ProviderKey, Resolver};
use navree_ui::html::{Href, LocalHref};
use navree_ui::{Alert, AlertTone, Button, LiveStream, Node, Progress, Region, View, el};
use serde_json::Value;
use uuid::Uuid;

use crate::operations::{OperationRecord, OperationStatus, operation_topic, visible};
use crate::{Operations, QueueOptions};

/// Where `QueueModule` serves the fragment of an operation, with the id in place of `{id}`.
pub const PROGRESS_PATH: &str = "/operations/{id}/progress";

/// How often the region asks again while it has no live stream to hear.
const POLL: Duration = Duration::from_secs(2);

/// The words of an [`OperationProgress`]. English by default. `{done}` and `{total}` are
/// filled in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationLabels {
    /// An operation that has not begun.
    pub waiting: String,
    /// An operation at work, when it has no note of its own.
    pub working: String,
    /// How far an operation got: `{done} of {total}`.
    pub of: String,
    /// An operation that succeeded.
    pub done: String,
    /// The link to the result of an operation that succeeded.
    pub download: String,
    /// An operation that failed, when it has no message.
    pub failed: String,
    /// An operation that was cancelled.
    pub cancelled: String,
}

impl Default for OperationLabels {
    fn default() -> Self {
        Self {
            waiting: "Waiting to start".to_owned(),
            working: "Working".to_owned(),
            of: "{done} of {total}".to_owned(),
            done: "Done".to_owned(),
            download: "Download".to_owned(),
            failed: "This did not finish".to_owned(),
            cancelled: "Cancelled".to_owned(),
        }
    }
}

/// A region that follows an operation. See the [module](self) for how a page uses it.
///
/// ```
/// use navree_queue::ui::OperationProgress;
/// use navree_ui::render;
/// use uuid::Uuid;
///
/// let id = Uuid::now_v7();
/// let html = render(&OperationProgress::new(id).into());
/// assert!(html.contains(&format!(r#"data-ui-src="/operations/{id}/progress""#)));
/// assert!(html.contains("data-ui-poll"));
/// assert!(html.contains("Waiting to start"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct OperationProgress {
    id: Uuid,
    src: Href,
    poll: Duration,
    stream: Option<LiveStream>,
    labels: OperationLabels,
    result_key: String,
    record: Option<OperationRecord>,
}

impl OperationProgress {
    /// The progress of operation `id`, fetched from the fragment `QueueModule` serves.
    pub fn new(id: Uuid) -> Self {
        Self {
            id,
            src: Href::from(PROGRESS_PATH.replace("{id}", &id.to_string())),
            poll: POLL,
            stream: None,
            labels: OperationLabels::default(),
            result_key: "url".to_owned(),
            record: None,
        }
    }

    /// Fetches the fragment from `src` instead (a route of your own).
    pub fn src(mut self, src: impl Into<Href>) -> Self {
        self.src = src.into();
        self
    }

    /// Asks again this often while the operation is unfinished (2 seconds by default).
    pub fn poll(mut self, every: Duration) -> Self {
        self.poll = every;
        self
    }

    /// Also hears the operation change on the app's live hub, so the region fetches as
    /// soon as something happened rather than at the next poll. It needs `RealtimeModule`;
    /// without it the region only polls.
    pub fn live(self) -> Self {
        let topic = operation_topic(self.id);
        self.live_on(LiveStream::topics([topic]))
    }

    /// [`live`](Self::live) on a stream of your own, which must carry the topic
    /// [`operation_topic`].
    pub fn live_on(mut self, stream: LiveStream) -> Self {
        self.stream = Some(stream);
        self
    }

    /// Sets the words.
    pub fn labels(mut self, labels: OperationLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Reads the link to the result from this key of the operation's `result` (`url` by
    /// default). Only an address on this site becomes a link.
    pub fn result_key(mut self, key: impl Into<String>) -> Self {
        self.result_key = key.into();
        self
    }

    /// Draws the operation as it is now, the way the fragment answers.
    pub fn record(mut self, record: OperationRecord) -> Self {
        self.record = Some(record);
        self
    }

    fn status(&self) -> OperationStatus {
        self.record
            .as_ref()
            .map_or(OperationStatus::Pending, |record| record.status)
    }

    fn body(&self) -> Node {
        let words = &self.labels;
        let status = self.status();
        let state = el("div")
            .slot("operation-progress")
            .attr("data-state", status.as_str())
            .attr("role", "status");
        let Some(record) = &self.record else {
            return state
                .children((
                    Progress::indeterminate(words.waiting.clone()),
                    el("p").child(words.waiting.clone()),
                ))
                .into();
        };
        match status {
            OperationStatus::Pending => state
                .children((
                    Progress::indeterminate(words.waiting.clone()),
                    el("p").child(words.waiting.clone()),
                ))
                .into(),
            OperationStatus::Running => {
                let progress = &record.progress;
                let label = if record.progress.note.is_empty() {
                    words.working.clone()
                } else {
                    record.progress.note.clone()
                };
                let bar = if progress.total == 0 {
                    Progress::indeterminate(label.clone())
                } else {
                    Progress::new(u32::from(progress.percent), 100, label.clone())
                };
                let count = (progress.total > 0).then(|| {
                    el("p").slot("operation-count").child(
                        words
                            .of
                            .replace("{done}", &progress.done.to_string())
                            .replace("{total}", &progress.total.to_string()),
                    )
                });
                state.children((bar, el("p").child(label), count)).into()
            }
            OperationStatus::Succeeded => {
                let link = self
                    .result_link(record)
                    .map(|href| Button::outline(words.download.clone()).href(href));
                state
                    .children((el("p").child(words.done.clone()), link))
                    .into()
            }
            OperationStatus::Failed => {
                let text = record
                    .error
                    .clone()
                    .filter(|error| !error.is_empty())
                    .unwrap_or_else(|| words.failed.clone());
                state
                    .children(Alert::new(AlertTone::Danger).title(text))
                    .into()
            }
            OperationStatus::Cancelled => state
                .children(el("p").child(words.cancelled.clone()))
                .into(),
        }
    }

    /// The address on this site the operation's result names, if it names one.
    fn result_link(&self, record: &OperationRecord) -> Option<Href> {
        let text = record
            .result
            .as_ref()?
            .get(&self.result_key)
            .and_then(Value::as_str)?;
        let local = LocalHref::parse(text).ok()?;
        Some(Href::from(local.as_str()))
    }
}

impl From<OperationProgress> for Node {
    fn from(progress: OperationProgress) -> Self {
        let finished = progress
            .record
            .as_ref()
            .is_some_and(OperationRecord::is_finished);
        let mut region =
            Region::new(format!("operation-{}", progress.id)).src(progress.src.clone());
        // A finished operation never changes: nothing left to ask.
        if !finished {
            region = region.poll(progress.poll);
            if let Some(stream) = &progress.stream {
                region = region.live_on(stream.on(operation_topic(progress.id)));
            }
        }
        region.children(progress.body()).into()
    }
}

/// The fragment of an operation: `GET /operations/{id}/progress`, mounted by `QueueModule`.
pub struct OperationProgressController {
    operations: Option<Arc<Operations>>,
}

impl Injectable for OperationProgressController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<QueueOptions>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let options = r.get::<QueueOptions>().await?;
            let app = r.app();
            let mounted = options.operations.routes
                && app.try_get::<dyn Authenticator>().await.is_some()
                && app.try_get::<navree_db::Db>().await.is_some();
            let operations = if mounted {
                app.try_get::<Operations>().await
            } else {
                None
            };
            Ok(Self { operations })
        })
    }
}

impl Controller for OperationProgressController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let Some(operations) = self.operations.clone() else {
            return Vec::new();
        };
        let show = move |CurrentPrincipal(principal): CurrentPrincipal, Path(id): Path<String>| {
            let operations = operations.clone();
            async move {
                let record = visible(&operations, &principal, &id).await?;
                let id = record.id;
                Ok::<_, AppError>(
                    View::new("Operation").body(OperationProgress::new(id).record(record)),
                )
            }
        };
        vec![
            RouteDef::on(Method::GET, format!("!{PROGRESS_PATH}"), show)
                .authenticated()
                .name("operations.progress")
                .api(ApiMeta {
                    summary: Some("An operation's progress, drawn for a page".to_owned()),
                    description: Some(
                        "The `OperationProgress` region as HTML: a page asks for it again while \
                         the operation runs."
                            .to_owned(),
                    ),
                    tags: vec!["Operations".to_owned()],
                    responses: vec![401, 404],
                    ..ApiMeta::default()
                }),
        ]
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn record(
        status: OperationStatus,
        done: u64,
        total: u64,
        note: &str,
        result: Option<Value>,
    ) -> OperationRecord {
        let percent = (done * 100)
            .checked_div(total)
            .map_or(0, |share| u8::try_from(share.min(100)).unwrap_or(100));
        OperationRecord {
            id: Uuid::nil(),
            kind: "export".to_owned(),
            status,
            progress: crate::operations::OperationProgress {
                done,
                total,
                percent,
                note: note.to_owned(),
            },
            result,
            error: (status == OperationStatus::Failed)
                .then(|| "The file could not be made.".to_owned()),
            owner_id: None,
            workspace_id: None,
            batch_id: None,
            created_at: chrono::Utc::now(),
            started_at: None,
            heartbeat_at: None,
            finished_at: None,
            updated_at: chrono::Utc::now(),
            version: 0,
            job: None,
            idempotent: false,
            redispatches: 0,
        }
    }

    fn html(progress: OperationProgress) -> String {
        navree_ui::render(&progress.into())
    }

    #[test]
    fn an_unfinished_operation_polls_and_shows_how_far_it_got() {
        let at = |done, total| {
            html(OperationProgress::new(Uuid::nil()).record(record(
                OperationStatus::Running,
                done,
                total,
                "Writing rows",
                None,
            )))
        };
        let page = at(40, 100);
        assert!(page.contains("data-ui-poll"), "{page}");
        assert!(page.contains(r#"data-state="running""#), "{page}");
        assert!(page.contains("Writing rows"), "{page}");
        assert!(page.contains("40 of 100"), "{page}");
        assert!(page.contains(r#"value="40""#), "{page}");
    }

    #[test]
    fn a_finished_operation_stops_polling_and_links_to_its_result() {
        let done = html(OperationProgress::new(Uuid::nil()).record(record(
            OperationStatus::Succeeded,
            100,
            100,
            "",
            Some(json!({ "url": "/exports/deals.csv" })),
        )));
        assert!(!done.contains("data-ui-poll"), "{done}");
        assert!(done.contains(r#"href="/exports/deals.csv""#), "{done}");
        assert!(done.contains("Done"), "{done}");

        // An address off this site is not a link.
        let off = html(OperationProgress::new(Uuid::nil()).record(record(
            OperationStatus::Succeeded,
            1,
            1,
            "",
            Some(json!({ "url": "https://evil.example/x" })),
        )));
        assert!(!off.contains("evil.example"), "{off}");
        let script = html(OperationProgress::new(Uuid::nil()).record(record(
            OperationStatus::Succeeded,
            1,
            1,
            "",
            Some(json!({ "url": "javascript:alert(1)" })),
        )));
        assert!(!script.contains("javascript"), "{script}");
    }

    #[test]
    fn a_failed_operation_says_why_and_a_cancelled_one_says_so() {
        let failed = html(OperationProgress::new(Uuid::nil()).record(record(
            OperationStatus::Failed,
            3,
            10,
            "",
            None,
        )));
        assert!(failed.contains("The file could not be made."), "{failed}");
        assert!(!failed.contains("data-ui-poll"), "{failed}");
        let cancelled = html(OperationProgress::new(Uuid::nil()).record(record(
            OperationStatus::Cancelled,
            3,
            10,
            "",
            None,
        )));
        assert!(cancelled.contains("Cancelled"), "{cancelled}");
    }

    #[test]
    fn the_live_hub_is_heard_for_the_operation_until_it_finishes() {
        let id = Uuid::now_v7();
        let page = html(OperationProgress::new(id).live());
        assert!(page.contains("data-ui-live-src"), "{page}");
        assert!(page.contains(&operation_topic(id)), "{page}");
        let done = html(OperationProgress::new(id).live().record(record(
            OperationStatus::Succeeded,
            1,
            1,
            "",
            None,
        )));
        assert!(!done.contains("data-ui-live-src"), "{done}");
    }

    #[test]
    fn the_words_are_the_pages_own() {
        let words = OperationLabels {
            waiting: "शुरू होने की प्रतीक्षा".to_owned(),
            ..OperationLabels::default()
        };
        let page = html(OperationProgress::new(Uuid::nil()).labels(words));
        assert!(page.contains("शुरू होने की प्रतीक्षा"), "{page}");
    }
}
