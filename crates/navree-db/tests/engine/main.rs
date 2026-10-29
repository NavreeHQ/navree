//! navree-db's own data layer on SQLx (pools, statements, values, rows,
//! errors, transactions, savepoints, units of work, the SeaORM bridge,
//! records),
//! run on every database: SQLite always, PostgreSQL with TimescaleDB and
//! MySQL when `NAVREE_TEST_POSTGRES_URL` / `NAVREE_TEST_MYSQL_URL` are set
//! (or `target/test-databases.env` has them) and the `postgres` / `mysql`
//! features are on. Each scenario runs once per database, each time in a
//! database of its own.
//!
//! `cargo test -p navree-db --features postgres,mysql --test engine`

mod placeholder_types;
mod records;
mod statements;
mod support;
mod transactions;
mod units;
mod values;
mod watching;

use placeholder_types::{
    changing_value_types_across_connections_and_transactions,
    changing_value_types_in_units_of_work, changing_value_types_on_one_connection,
    every_kind_after_every_other, logs_show_the_sql_as_written, stable_types_timing,
    typed_nulls_then_other_kinds,
};
use records::{
    read_executor_follows_the_unit_of_work, records_in_units_of_work, records_round_trip,
};
use statements::{errors_are_classified, placeholders_scripts_and_keys};
use transactions::{
    commit_rollback_and_drop, options_and_finished_transactions, rows_stream, savepoints_nest,
};
use units::{
    bridge_values, bridged_units_of_work_share_one_transaction,
    db_statements_join_the_current_unit_of_work, native_units_of_work,
    observers_run_inside_the_commit, savepoints_and_settings_in_units_of_work,
};
use values::{every_kind_round_trips, expressions_read_tolerantly, rows_read_dynamically};
use watching::{sessions_follow_the_options, statements_are_watched};

on_every_server!(
    every_kind_round_trips,
    rows_read_dynamically,
    expressions_read_tolerantly,
    placeholders_scripts_and_keys,
    errors_are_classified,
    commit_rollback_and_drop,
    savepoints_nest,
    options_and_finished_transactions,
    rows_stream,
    native_units_of_work,
    savepoints_and_settings_in_units_of_work,
    observers_run_inside_the_commit,
    bridged_units_of_work_share_one_transaction,
    db_statements_join_the_current_unit_of_work,
    bridge_values,
    statements_are_watched,
    sessions_follow_the_options,
    records_round_trip,
    records_in_units_of_work,
    read_executor_follows_the_unit_of_work,
    changing_value_types_on_one_connection,
    typed_nulls_then_other_kinds,
    changing_value_types_across_connections_and_transactions,
    every_kind_after_every_other,
    logs_show_the_sql_as_written,
    changing_value_types_in_units_of_work,
);

#[test]
fn rows_built_by_hand_share_the_decoders() {
    values::hand_rows_share_the_decoders();
}

/// Timings of repeated statements with stable value types, printed (see
/// `placeholder_types::stable_types_timing`).
mod stable_types_timing {
    use super::support::{Server, run};

    #[tokio::test]
    #[ignore = "a timing, printed: run with --ignored --nocapture"]
    async fn postgres() {
        run(
            Server::Postgres,
            "stable_types_timing on PostgreSQL",
            super::stable_types_timing,
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "a timing, printed: run with --ignored --nocapture"]
    async fn mysql() {
        run(
            Server::MySql,
            "stable_types_timing on MySQL",
            super::stable_types_timing,
        )
        .await;
    }
}
