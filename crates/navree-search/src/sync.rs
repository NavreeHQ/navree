//! Keeping indexes in sync: the change observer and the `search.sync` job.

use navree_contracts::{BoxError, Job, JobContext};
use navree_db::{ChangeEvent, ChangeObserver, ChangeOp, DbError, ErrorKind, Uow};
use navree_kernel::{BoxFuture, Injectable, KernelError, Lazy, Resolver};
use navree_orm::OrmError;
use serde::{Deserialize, Serialize};

use crate::{Search, SearchError};
use crate::service::{group_by_workspace, syncing_paused};

/// The job name of [`SyncDocuments`].
pub const SYNC_JOB: &str = "search.sync";

/// The job that syncs changed records into their index: it loads the
/// current rows and upserts their documents, or deletes the documents of
/// records that are gone or kept out of the index.
///
/// Queued by the change observer when a unit of work that wrote a
/// searchable model through the model API commits (one job per index and
/// workspace), and by [`Search::reindex`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncDocuments {
    /// The index, before `search.prefix` (`contacts`).
    pub index: String,
    /// The records' primary keys.
    pub ids: Vec<String>,
    /// The workspace the records belong to, if the index is scoped to
    /// workspaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
}

impl Job for SyncDocuments {
    const NAME: &'static str = SYNC_JOB;

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let search = ctx.app.get::<Search>().await?;
        if let Err(error) = search.sync_documents(&self).await {
            tracing::warn!(
                target: "navree::search",
                index = %self.index,
                ids = ?self.ids,
                %error,
                "cannot sync search documents; the job will retry"
            );
            return Err(error.into());
        }
        Ok(())
    }
}

/// Queues the syncs of every searchable entity a unit of work changed,
/// inside its commit.
pub(crate) struct SearchObserver {
    search: Lazy<Search>,
}

impl Injectable for SearchObserver {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(SearchObserver {
                search: r.lazy::<Search>(),
            })
        })
    }
}

impl ChangeObserver for SearchObserver {
    fn observe_with<'a>(
        &'a self,
        uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            if syncing_paused() {
                return Ok(());
            }
            let search = self.search.get().await.map_err(|e| other(e.to_string()))?;
            if search.options().sync == crate::SyncMode::Off {
                return Ok(());
            }
            for entry in search.entries() {
                let changed: Vec<&ChangeEvent> =
                    changes.iter().filter(|c| c.entity == entry.table).collect();
                if changed.is_empty() {
                    continue;
                }
                let mut ids: Vec<String> = Vec::new();
                for change in &changed {
                    if !ids.contains(&change.id) {
                        ids.push(change.id.clone());
                    }
                }
                let groups = match entry.settings.workspace_column() {
                    None => vec![(None, ids)],
                    Some(column) => {
                        let mut found = entry
                            .ops
                            .workspaces(uow, column, &ids)
                            .await
                            .map_err(db_error)?;
                        // Deleted rows: their old values name the workspace.
                        for change in changed.iter().filter(|c| c.op == ChangeOp::Deleted) {
                            if let Some(workspace) = change
                                .old
                                .as_ref()
                                .and_then(|old| old.get(column))
                                .and_then(json_string)
                            {
                                found.entry(change.id.clone()).or_insert(workspace);
                            }
                        }
                        group_by_workspace(&ids, &found)
                    }
                };
                for (workspace, ids) in groups {
                    let job = SyncDocuments {
                        index: entry.settings.index().to_owned(),
                        ids,
                        workspace,
                    };
                    search
                        .queue_observed(uow, job)
                        .await
                        .map_err(|e| other(format!("cannot queue a search sync: {e}")))?;
                }
            }
            Ok(())
        })
    }
}

/// An observer failure that is not the database's.
fn other(message: impl Into<String>) -> DbError {
    DbError::new(ErrorKind::Other, message)
}

/// A search error as the database error a commit fails with.
fn db_error(error: SearchError) -> DbError {
    match error {
        SearchError::Db(error) | SearchError::Orm(OrmError::Db(error)) => error,
        other_error => other(other_error.to_string()),
    }
}

fn json_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Null => None,
        other => Some(other.to_string()),
    }
}
