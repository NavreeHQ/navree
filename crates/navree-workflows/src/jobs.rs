//! [`RunWorkflow`]: the job that executes a run.

use navree_contracts::{BoxError, Job, JobContext, JobOptions};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Workflows;

/// Executes workflow run `run_id` once (job `workflows.run`): from where it
/// stopped until it finishes or waits again. Dispatched when a run starts,
/// when its timer is due (delayed until then) and when an event resolves
/// one of its waits; `workflows:tick` dispatches it again for runs whose
/// job was lost. A run that is not ready (or held by another execution) is
/// skipped, so extra jobs are harmless.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunWorkflow {
    /// The run.
    pub run_id: Uuid,
}

impl Job for RunWorkflow {
    const NAME: &'static str = "workflows.run";

    fn options() -> JobOptions {
        JobOptions {
            timeout_secs: crate::engine::RUN_JOB_TIMEOUT_SECS,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let workflows = ctx.app.get::<Workflows>().await?;
        workflows.execute(self.run_id).await?;
        Ok(())
    }
}
