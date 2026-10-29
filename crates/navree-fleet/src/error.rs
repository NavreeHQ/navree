//! [`FleetError`].

use navree_db::DbError;
use navree_queue::QueueError;

/// What a [`Fleet`](crate::Fleet) operation or the `fleet` command can fail
/// with.
#[derive(Debug, thiserror::Error)]
pub enum FleetError {
    /// No supervisor has this name (`fleet:pause <name>`, `fleet.only`).
    #[error("there is no supervisor named `{0}` in `fleet.supervisors`")]
    UnknownSupervisor(String),
    /// The queue runs on the `memory` driver, whose jobs only exist in one
    /// process.
    #[error(
        "Fleet needs the `database` queue driver (`queue.driver = \"database\"`): the `memory` driver's jobs exist in one process only"
    )]
    MemoryDriver,
    /// There is no failed job with this id.
    #[error("there is no failed job with id `{id}`")]
    FailedJobNotFound {
        /// The id asked for.
        id: String,
    },
    /// A worker process could not be started.
    #[error("cannot start a worker process: {0}")]
    Spawn(String),
    /// The queue failed.
    #[error(transparent)]
    Queue(#[from] QueueError),
    /// The database failed.
    #[error("Fleet database error: {0}")]
    Database(#[from] DbError),
    /// A stored value could not be read back.
    #[error("Fleet cannot read a stored value: {0}")]
    Corrupt(String),
}

impl From<serde_json::Error> for FleetError {
    fn from(error: serde_json::Error) -> Self {
        FleetError::Corrupt(error.to_string())
    }
}
