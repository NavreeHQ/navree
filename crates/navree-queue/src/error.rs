//! [`QueueError`].

use sea_orm::DbErr;

/// What can go wrong when queueing or working jobs.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum QueueError {
    /// No module registered a handler for the job type (`m.job::<J>()`).
    #[error(
        "no handler is registered for job type `{job_type}`; register it with `m.job::<J>()` (ModuleBuilderJobsExt)"
    )]
    UnknownJob {
        /// The job type ([`Job::NAME`](navree_contracts::Job::NAME)).
        job_type: String,
    },
    /// `retry` was given the id of a failed job that does not exist.
    #[error("there is no failed job with id `{id}`")]
    FailedJobNotFound {
        /// The id given.
        id: String,
    },
    /// The job could not be serialized, or a stored job could not be read back.
    #[error("invalid job payload: {0}")]
    Payload(#[source] serde_json::Error),
    /// A database error (database driver).
    #[error(transparent)]
    Db(#[from] DbErr),
    /// The app owning the queue has been dropped.
    #[error("the app owning this queue has shut down")]
    ShutDown,
    /// Operations are stored in the app database, and the app has no
    /// `DatabaseModule`.
    #[error("operations need the app database: add DatabaseModule to the app")]
    NoDatabase,
}
