//! navree-queue integration tests: one binary, the driver suite run against
//! the database and the memory driver, plus driver-specific tests.

mod commands;
mod database_driver;
mod fake;
mod module;
mod observed;
mod suite;
mod support;

use navree_queue::QueueDriver;

/// Runs each named function of `suite` as a test against `$driver`.
macro_rules! driver_suite {
    ($module:ident, $driver:expr, [$($test:ident),* $(,)?]) => {
        mod $module {
            use super::*;

            $(
                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                async fn $test() {
                    crate::suite::$test($driver).await;
                }
            )*
        }
    };
}

driver_suite!(
    database,
    QueueDriver::Database,
    [
        dispatch_and_work_runs_handler,
        dispatch_in_uow_rolled_back_is_not_queued,
        dispatch_joins_the_current_unit_of_work,
        retry_with_backoff_then_fail_to_failed_jobs,
        timeout_counts_as_failure,
        panicking_job_counts_as_failure,
        unique_job_deduplicated,
        delayed_job_not_reserved_early,
        concurrent_workers_never_run_same_job,
        stale_reservation_released,
        stale_job_on_its_last_attempt_fails,
        retry_failed_requeues,
        flush_failed_deletes_failed_jobs,
        unknown_job_type_is_rejected_at_dispatch,
        queues_are_worked_in_priority_order,
        work_options_select_queues_and_limit_jobs,
        worker_polls_until_the_app_shuts_down,
        run_pending_drains_available_jobs_in_order,
    ]
);

driver_suite!(
    memory_driver_parity,
    QueueDriver::Memory,
    [
        dispatch_and_work_runs_handler,
        dispatch_in_uow_rolled_back_is_not_queued,
        dispatch_joins_the_current_unit_of_work,
        retry_with_backoff_then_fail_to_failed_jobs,
        timeout_counts_as_failure,
        panicking_job_counts_as_failure,
        unique_job_deduplicated,
        delayed_job_not_reserved_early,
        concurrent_workers_never_run_same_job,
        stale_reservation_released,
        stale_job_on_its_last_attempt_fails,
        retry_failed_requeues,
        flush_failed_deletes_failed_jobs,
        unknown_job_type_is_rejected_at_dispatch,
        queues_are_worked_in_priority_order,
        work_options_select_queues_and_limit_jobs,
        worker_polls_until_the_app_shuts_down,
        run_pending_drains_available_jobs_in_order,
    ]
);
