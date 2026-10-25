//! The job contract: modules define and dispatch jobs without depending on keel-queue.

use std::future::Future;
use std::time::Duration;

use keel_kernel::{AppContext, ModuleBuilder};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{BoxError, BoxFuture};

/// A unit of background work. The job value is its payload (serialized as JSON).
pub trait Job: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// The stable job type name stored with queued jobs, e.g. `mail.send`.
    const NAME: &'static str;

    /// Queue, retries, backoff and timeout for this job type.
    fn options() -> JobOptions {
        JobOptions::default()
    }

    /// Runs the job.
    fn handle(self, ctx: JobContext) -> impl Future<Output = Result<(), BoxError>> + Send;
}

/// How a job is queued and retried.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct JobOptions {
    /// The queue name (`default`).
    pub queue: String,
    /// Attempts before the job is moved to failed jobs (3).
    pub max_attempts: u32,
    /// Delay between attempts (exponential from 1 s, capped at 300 s).
    pub backoff: Backoff,
    /// Time limit per attempt, in seconds (120).
    pub timeout_secs: u64,
    /// If set, an identical job dispatched while one is pending within this
    /// window is not queued again.
    pub unique_for_secs: Option<u64>,
}

impl Default for JobOptions {
    fn default() -> Self {
        Self {
            queue: "default".to_owned(),
            max_attempts: 3,
            backoff: Backoff::default(),
            timeout_secs: 120,
            unique_for_secs: None,
        }
    }
}

/// The delay before retrying a failed attempt.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Backoff {
    /// The same delay every time.
    Fixed {
        /// The delay in seconds.
        secs: u64,
    },
    /// `base_secs` after the first attempt, doubling each attempt, capped at `max_secs`.
    Exponential {
        /// The first delay in seconds.
        base_secs: u64,
        /// The largest delay in seconds.
        max_secs: u64,
    },
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff::Exponential {
            base_secs: 1,
            max_secs: 300,
        }
    }
}

impl Backoff {
    /// The delay after failed attempt number `attempt` (1-based): for
    /// exponential backoff, attempt 1 waits `base_secs`, attempt 2 twice that,
    /// and so on up to `max_secs`.
    pub fn delay_for(&self, attempt: u32) -> Duration {
        match *self {
            Backoff::Fixed { secs } => Duration::from_secs(secs),
            Backoff::Exponential {
                base_secs,
                max_secs,
            } => {
                let factor = 1_u64
                    .checked_shl(attempt.saturating_sub(1))
                    .unwrap_or(u64::MAX);
                Duration::from_secs(base_secs.saturating_mul(factor).min(max_secs))
            }
        }
    }
}

/// What a running job gets besides its payload.
#[derive(Clone, Debug)]
pub struct JobContext {
    /// The app, to resolve services.
    pub app: AppContext,
    /// The queued job's id.
    pub job_id: String,
    /// The attempt number, starting at 1.
    pub attempt: u32,
}

/// A serialized job as handed to a [`Dispatcher`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobEnvelope {
    /// [`Job::NAME`].
    pub job_type: String,
    /// The job value as JSON.
    pub payload: Value,
    /// [`Job::options`].
    pub options: JobOptions,
    /// Run no earlier than this many seconds from now.
    pub delay_secs: Option<u64>,
    /// Deduplication key for unique jobs (the queue fills it in when `None`).
    pub unique_key: Option<String>,
}

impl JobEnvelope {
    /// The envelope for `job`, to run as soon as possible.
    pub fn of<J: Job>(job: &J) -> Result<Self, BoxError> {
        Ok(Self {
            job_type: J::NAME.to_owned(),
            payload: serde_json::to_value(job)?,
            options: J::options(),
            delay_secs: None,
            unique_key: None,
        })
    }
}

/// Queues jobs (implemented by keel-queue; tests use fakes).
pub trait Dispatcher: Send + Sync + 'static {
    /// Queues a serialized job and returns its id.
    fn dispatch_envelope(&self, env: JobEnvelope) -> BoxFuture<'_, Result<String, BoxError>>;
}

impl dyn Dispatcher {
    /// Queues `job` to run as soon as possible; returns the job id.
    pub async fn dispatch<J: Job>(&self, job: J) -> Result<String, BoxError> {
        self.dispatch_envelope(JobEnvelope::of(&job)?).await
    }

    /// Queues `job` to run after `delay` (rounded up to whole seconds).
    pub async fn dispatch_later<J: Job>(
        &self,
        job: J,
        delay: Duration,
    ) -> Result<String, BoxError> {
        let mut envelope = JobEnvelope::of(&job)?;
        let whole_secs = delay.as_secs() + u64::from(delay.subsec_nanos() > 0);
        envelope.delay_secs = Some(whole_secs);
        self.dispatch_envelope(envelope).await
    }
}

/// How a worker runs one job type: contributed by modules (see
/// [`ModuleBuilderJobsExt::job`]) and collected by the queue.
#[derive(Clone, Debug)]
pub struct JobRegistration {
    /// [`Job::NAME`].
    pub name: &'static str,
    /// [`Job::options`].
    pub options: JobOptions,
    /// Deserializes the payload and runs the job's handler.
    pub run: fn(Value, JobContext) -> BoxFuture<'static, Result<(), BoxError>>,
}

impl JobRegistration {
    /// The registration of job type `J`.
    pub fn of<J: Job>() -> Self {
        Self {
            name: J::NAME,
            options: J::options(),
            run: run_job::<J>,
        }
    }
}

fn run_job<J: Job>(payload: Value, ctx: JobContext) -> BoxFuture<'static, Result<(), BoxError>> {
    Box::pin(async move {
        let job: J = serde_json::from_value(payload)
            .map_err(|error| format!("invalid payload for job `{}`: {error}", J::NAME))?;
        job.handle(ctx).await
    })
}

/// Registers job types on a module.
pub trait ModuleBuilderJobsExt {
    /// Contributes [`JobRegistration::of::<J>()`](JobRegistration::of) so the
    /// queue can run jobs of type `J`.
    fn job<J: Job>(&mut self) -> &mut Self;
}

impl ModuleBuilderJobsExt for ModuleBuilder {
    fn job<J: Job>(&mut self) -> &mut Self {
        self.contribute(JobRegistration::of::<J>())
    }
}
