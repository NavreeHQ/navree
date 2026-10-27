//! navree-queue integration tests: one binary, the driver suite run against
//! the database and the memory driver, plus driver-specific tests.

mod batches;
mod commands;
mod database_driver;
mod fake;
mod module;
mod operations;
mod suite;
mod support;
mod telemetry;

/// Runs each named function of `operations` as a test against `$driver`.
macro_rules! operations_suite {
    ($module:ident, $driver:expr, [$($test:ident),* $(,)?]) => {
        mod $module {
            use super::*;

            $(
                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                async fn $test() {
                    crate::operations::$test($driver).await;
                }
            )*
        }
    };
}

operations_suite!(
    database_operations,
    QueueDriver::Database,
    [
        an_operation_follows_its_job,
        the_jobs_outcome_finishes_the_operation,
        a_finished_operation_never_changes,
        cancel_skips_work_not_started,
        cancel_stops_a_running_job_at_its_next_report,
        a_batch_drives_its_operation,
        a_failing_batch_fails_its_operation,
        cancelling_a_batch_operation_cancels_the_batch,
        the_watchdog_fails_stalled_operations,
        access_is_the_owner_or_the_permission,
        owners_list_and_prune_operations,
    ]
);

operations_suite!(
    memory_operations,
    QueueDriver::Memory,
    [
        an_operation_follows_its_job,
        the_jobs_outcome_finishes_the_operation,
        a_finished_operation_never_changes,
        cancel_skips_work_not_started,
        cancel_stops_a_running_job_at_its_next_report,
        a_batch_drives_its_operation,
        a_failing_batch_fails_its_operation,
        cancelling_a_batch_operation_cancels_the_batch,
        the_watchdog_fails_stalled_operations,
        access_is_the_owner_or_the_permission,
        owners_list_and_prune_operations,
    ]
);

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

/// Runs each named function of `batches` as a test against `$driver`.
macro_rules! batch_suite {
    ($module:ident, $driver:expr, [$($test:ident),* $(,)?]) => {
        mod $module {
            use super::*;

            $(
                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                async fn $test() {
                    crate::batches::$test($driver).await;
                }
            )*
        }
    };
}

batch_suite!(
    database_batches,
    QueueDriver::Database,
    [
        batch_runs_its_jobs_then_and_finally,
        first_failure_runs_catch_cancels_the_rest_and_skips_then,
        allow_failures_keeps_the_batch_running,
        cancel_batch_skips_jobs_not_started,
        running_jobs_see_their_batch_cancelled,
        batch_in_a_rolled_back_unit_of_work_is_not_stored,
        batch_dispatch_joins_the_current_unit_of_work,
        empty_batch_finishes_at_once,
        concurrent_workers_finish_a_batch_once,
        unique_jobs_in_a_batch_all_count,
        unknown_job_types_reject_the_whole_batch,
        batches_are_listed_newest_first_and_pruned,
    ]
);

batch_suite!(
    memory_batches,
    QueueDriver::Memory,
    [
        batch_runs_its_jobs_then_and_finally,
        first_failure_runs_catch_cancels_the_rest_and_skips_then,
        allow_failures_keeps_the_batch_running,
        cancel_batch_skips_jobs_not_started,
        running_jobs_see_their_batch_cancelled,
        batch_in_a_rolled_back_unit_of_work_is_not_stored,
        batch_dispatch_joins_the_current_unit_of_work,
        empty_batch_finishes_at_once,
        concurrent_workers_finish_a_batch_once,
        unique_jobs_in_a_batch_all_count,
        unknown_job_types_reject_the_whole_batch,
        batches_are_listed_newest_first_and_pruned,
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
