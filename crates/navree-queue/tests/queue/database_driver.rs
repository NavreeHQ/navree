//! Database-driver behaviour beyond the shared suite: several processes
//! (apps with their own pools) on one database, and the stored rows.

use std::sync::Arc;
use std::time::Duration;

use navree_contracts::FixedClock;
use navree_db::{Db, Executor};
use navree_queue::{Queue, QueueDriver, QueueError, QueueOptions, WorkOptions};

use crate::suite::{assert_handled_once, wait_until};
use crate::support::{
    HideJobsTable, Journal, Record, app_on, app_without_jobs_on, harness, harness_with, options,
    start,
};

/// Every harness (and so every suite) really runs on the server
/// `NAVREE_TEST_DATABASE` names.
#[tokio::test]
async fn harnesses_run_on_the_configured_server() {
    let h = harness(QueueDriver::Database).await;
    let db = h.app.get::<Db>().await;
    assert_eq!(
        Executor::backend(&db),
        crate::servers::Server::current().backend()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_workers_in_two_apps_never_run_same_job() {
    let (url, _guard) = crate::servers::scratch();
    let journal = Arc::new(Journal::default());
    let clock = Arc::new(FixedClock::new(start()));
    let first = app_on(
        &url,
        options(QueueDriver::Database),
        journal.clone(),
        clock.clone(),
    )
    .await;
    let second = app_on(&url, options(QueueDriver::Database), journal.clone(), clock).await;
    let (a, b) = (first.get::<Queue>().await, second.get::<Queue>().await);
    for i in 0..50 {
        a.dispatch(Record::new(i.to_string())).await.unwrap();
    }
    let drain = WorkOptions {
        stop_when_empty: true,
        ..WorkOptions::default()
    };

    let (from_a, from_b) = tokio::join!(a.work(drain.clone()), b.work(drain));

    let (from_a, from_b) = (from_a.unwrap(), from_b.unwrap());
    assert_eq!(from_a.succeeded + from_b.succeeded, 50);
    assert_handled_once(&journal.labels(), 50);
    assert_eq!(b.pending_count("default").await.unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn job_without_handler_fails_without_retries() {
    let (url, _guard) = crate::servers::scratch();
    let journal = Arc::new(Journal::default());
    let clock = Arc::new(FixedClock::new(start()));
    let dispatcher = app_on(
        &url,
        options(QueueDriver::Database),
        journal.clone(),
        clock.clone(),
    )
    .await;
    let worker = app_without_jobs_on(&url, options(QueueDriver::Database), journal, clock).await;
    let id = dispatcher
        .get::<Queue>()
        .await
        .dispatch(Record::new("orphan"))
        .await
        .unwrap();

    let stats = worker
        .get::<Queue>()
        .await
        .work(WorkOptions {
            stop_when_empty: true,
            ..WorkOptions::default()
        })
        .await
        .unwrap();

    assert_eq!(stats.failed, 1);
    let failed = worker.get::<Queue>().await.failed().await.unwrap();
    assert_eq!(failed[0].id, id);
    assert_eq!(failed[0].attempts, 1);
    assert!(
        failed[0]
            .error
            .contains("no handler is registered for job type `test.record`"),
        "{}",
        failed[0].error
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn long_running_worker_survives_database_errors() {
    let h = harness(QueueDriver::Database).await;
    let db = h.app.get::<Db>().await;
    let queue = h.queue.clone();
    let worker = tokio::spawn(async move { queue.work(WorkOptions::default()).await });
    tokio::time::sleep(Duration::from_millis(50)).await;

    // The table disappears under the worker, then comes back.
    db.execute_script("ALTER TABLE jobs RENAME TO jobs_away")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!worker.is_finished(), "the worker keeps polling");
    db.execute_script("ALTER TABLE jobs_away RENAME TO jobs")
        .await
        .unwrap();
    h.queue.dispatch(Record::new("after")).await.unwrap();
    wait_until(|| h.journal.labels() == ["after"]).await;

    h.app.ctx().shutdown_token().cancel();
    let stats = tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("stops at shutdown")
        .unwrap()
        .unwrap();
    assert_eq!(stats.succeeded, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn finite_worker_returns_database_errors() {
    let h = harness(QueueDriver::Database).await;
    let db = h.app.get::<Db>().await;
    db.execute_script("DROP TABLE jobs").await.unwrap();

    let error = h
        .queue
        .work(WorkOptions {
            stop_when_empty: true,
            ..WorkOptions::default()
        })
        .await
        .unwrap_err();

    assert!(matches!(error, QueueError::Db(_)), "{error:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn long_running_worker_survives_errors_recording_an_outcome() {
    let h = harness_with(QueueOptions {
        workers: 1,
        ..options(QueueDriver::Database)
    })
    .await;
    let db = h.app.get::<Db>().await;
    let queue = h.queue.clone();
    let worker = tokio::spawn(async move { queue.work(WorkOptions::default()).await });

    h.queue.dispatch(HideJobsTable).await.unwrap();
    wait_until(|| h.journal.runs().len() == 1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !worker.is_finished(),
        "the worker outlives the failed completion"
    );
    db.execute_script("ALTER TABLE jobs_away RENAME TO jobs")
        .await
        .unwrap();
    h.queue.dispatch(Record::new("after")).await.unwrap();
    wait_until(|| h.journal.labels().contains(&"after".to_owned())).await;

    h.app.ctx().shutdown_token().cancel();
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("stops at shutdown")
        .unwrap()
        .unwrap();
}

/// Takes SQLite's write lock for `hold`, on a connection of its own.
async fn hold_write_lock(url: &str, hold: Duration) -> tokio::task::JoinHandle<()> {
    let db = Db::connect(&navree_db::DbOptions {
        url: url.to_owned(),
        ..navree_db::DbOptions::default()
    })
    .await
    .unwrap();
    let tx = db
        .begin_with(navree_db::TxOptions::new().immediate())
        .await
        .unwrap();
    tokio::spawn(async move {
        tokio::time::sleep(hold).await;
        tx.rollback().await.unwrap();
    })
}

/// A write lock held longer than `busy_timeout` is the database being busy: a
/// worker draining the queue waits it out, as a long-running one does.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lock_held_past_the_busy_timeout_does_not_stop_a_draining_worker() {
    if crate::servers::Server::current() != crate::servers::Server::Sqlite {
        return;
    }
    let (url, _guard) = crate::servers::scratch();
    let journal = Arc::new(Journal::default());
    let clock = Arc::new(FixedClock::new(start()));
    let app = crate::support::app_builder_on(
        &url,
        options(QueueDriver::Database),
        journal.clone(),
        clock,
    )
    .config(
        "database.sqlite",
        serde_json::json!({ "busy_timeout_ms": 100 }),
    )
    .module(crate::support::JobsModule)
    .build()
    .await;
    let queue = app.get::<Queue>().await;
    queue.dispatch(Record::new("0")).await.unwrap();
    let holder = hold_write_lock(&url, Duration::from_millis(600)).await;
    let stats = queue
        .work(WorkOptions {
            stop_when_empty: true,
            ..WorkOptions::default()
        })
        .await
        .unwrap();
    holder.await.unwrap();
    assert_eq!(stats.succeeded, 1);
    assert_handled_once(&journal.labels(), 1);
}

/// The same while the job's outcome is stored: the job ran, and it isn't run
/// again for a lock that was busy for a moment.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lock_held_while_an_outcome_is_stored_does_not_lose_the_outcome() {
    use navree_contracts::{BoxError, Job, JobContext, ModuleBuilderJobsExt};
    use navree_kernel::{Module, ModuleBuilder};
    use std::sync::Mutex;

    if crate::servers::Server::current() != crate::servers::Server::Sqlite {
        return;
    }
    static HOLDING: Mutex<Option<tokio::task::JoinHandle<()>>> = Mutex::new(None);
    static URL: Mutex<String> = Mutex::new(String::new());

    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
    struct TakesTheLock;
    impl Job for TakesTheLock {
        const NAME: &'static str = "test.takes_the_lock";
        async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
            let url = URL.lock().unwrap().clone();
            let holder = hold_write_lock(&url, Duration::from_millis(600)).await;
            *HOLDING.lock().unwrap() = Some(holder);
            Ok(())
        }
    }
    struct Jobs;
    impl Module for Jobs {
        fn register(&self, m: &mut ModuleBuilder) {
            m.job::<TakesTheLock>();
        }
    }

    let (url, _guard) = crate::servers::scratch();
    URL.lock().unwrap().clone_from(&url);
    let journal = Arc::new(Journal::default());
    let clock = Arc::new(FixedClock::new(start()));
    let app = crate::support::app_builder_on(&url, options(QueueDriver::Database), journal, clock)
        .config(
            "database.sqlite",
            serde_json::json!({ "busy_timeout_ms": 100 }),
        )
        .module(Jobs)
        .build()
        .await;
    let queue = app.get::<Queue>().await;
    queue.dispatch(TakesTheLock).await.unwrap();
    let stats = queue
        .work(WorkOptions {
            once: true,
            ..WorkOptions::default()
        })
        .await
        .unwrap();
    let holder = HOLDING.lock().unwrap().take().unwrap();
    holder.await.unwrap();
    assert_eq!(stats.succeeded, 1);
    assert_eq!(queue.pending_count("default").await.unwrap(), 0);
}

/// A job that takes SQLite's write lock for `hold_ms` as it ends, so that its
/// outcome can't be stored for that long; counted by the database it runs on.
mod outcomes {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use navree_contracts::{BoxError, Job, JobContext, JobOptions, ModuleBuilderJobsExt};
    use navree_kernel::{Module, ModuleBuilder};

    use super::*;

    static RUNS: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);
    static HOLDING: Mutex<Option<HashMap<String, tokio::task::JoinHandle<()>>>> = Mutex::new(None);

    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
    struct LocksAsItEnds {
        url: String,
        hold_ms: u64,
    }

    impl Job for LocksAsItEnds {
        const NAME: &'static str = "test.locks_as_it_ends";

        fn options() -> JobOptions {
            JobOptions {
                timeout_secs: 1,
                ..JobOptions::default()
            }
        }

        async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
            let first = {
                let mut runs = RUNS.lock().unwrap();
                let count = runs
                    .get_or_insert_with(HashMap::new)
                    .entry(self.url.clone())
                    .or_insert(0);
                *count += 1;
                *count == 1
            };
            if first {
                let holder = hold_write_lock(&self.url, Duration::from_millis(self.hold_ms)).await;
                HOLDING
                    .lock()
                    .unwrap()
                    .get_or_insert_with(HashMap::new)
                    .insert(self.url.clone(), holder);
            }
            Ok(())
        }
    }

    struct Jobs;

    impl Module for Jobs {
        fn register(&self, m: &mut ModuleBuilder) {
            m.job::<LocksAsItEnds>();
        }
    }

    fn runs(url: &str) -> u32 {
        RUNS.lock()
            .unwrap()
            .as_ref()
            .and_then(|runs| runs.get(url).copied())
            .unwrap_or(0)
    }

    async fn holder_done(url: &str) {
        let holder = HOLDING
            .lock()
            .unwrap()
            .as_mut()
            .and_then(|holding| holding.remove(url));
        if let Some(holder) = holder {
            holder.await.unwrap();
        }
    }

    /// Runs the job with the lock held for `hold_ms`: what the call returned, the
    /// runs by then and what a later worker makes of it.
    async fn run(
        hold_ms: u64,
        opts: WorkOptions,
    ) -> (Result<navree_queue::WorkStats, QueueError>, u32, u32, u64) {
        let (url, _guard) = crate::servers::scratch();
        let journal = Arc::new(Journal::default());
        let clock = Arc::new(FixedClock::new(start()));
        let app = crate::support::app_builder_on(
            &url,
            options(QueueDriver::Database),
            journal,
            clock.clone(),
        )
        .config(
            "database.sqlite",
            serde_json::json!({ "busy_timeout_ms": 100 }),
        )
        .module(Jobs)
        .build()
        .await;
        let queue = app.get::<Queue>().await;
        queue
            .dispatch(LocksAsItEnds {
                url: url.clone(),
                hold_ms,
            })
            .await
            .unwrap();
        let result = queue.work(opts).await;
        holder_done(&url).await;
        let first = runs(&url);
        clock.advance(Duration::from_secs(3600));
        queue
            .work(WorkOptions {
                stop_when_empty: true,
                ..WorkOptions::default()
            })
            .await
            .unwrap();
        let pending = queue.pending_count("default").await.unwrap();
        let later = runs(&url);
        app.shutdown().await.unwrap();
        (result, first, later, pending)
    }

    fn sqlite() -> bool {
        crate::servers::Server::current() == crate::servers::Server::Sqlite
    }

    fn drain() -> WorkOptions {
        WorkOptions {
            stop_when_empty: true,
            ..WorkOptions::default()
        }
    }

    fn once() -> WorkOptions {
        WorkOptions {
            once: true,
            ..WorkOptions::default()
        }
    }

    /// While the worker still holds the job, it keeps trying to store the outcome.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_long_lock_does_not_run_a_job_twice_while_its_worker_holds_it() {
        if !sqlite() {
            return;
        }
        let (result, first, later, pending) = run(2500, once()).await;
        assert_eq!(result.unwrap().succeeded, 1);
        assert_eq!((first, later, pending), (1, 1, 0));
        // With a worker of its own, draining says the same. Others of the call may
        // find the database busy too long when they look for a job, and say that.
        let (result, first, later, pending) = run(2500, drain()).await;
        if let Ok(stats) = result {
            assert_eq!(stats.succeeded, 1);
        }
        assert_eq!((first, later, pending), (1, 1, 0));
    }

    /// Past the reservation another worker may take the job: the outcome is lost,
    /// and the call that ran it says so.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn an_outcome_lost_past_the_reservation_is_an_error_and_never_ok() {
        if !sqlite() {
            return;
        }
        let (result, first, later, pending) = run(33_000, once()).await;
        let error = result.expect_err("the call does not say all is well");
        assert!(error.to_string().contains("locked"), "{error}");
        assert_eq!(first, 1);
        // The job runs again, which the error said it would: at least once.
        assert_eq!((later, pending), (2, 0));
    }
}
