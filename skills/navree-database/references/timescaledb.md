# Time-series tables (TimescaleDB) in Navree

TimescaleDB is Navree's default database. Make a table a **hypertable**
when it is append-mostly, keyed by time and large: logs, events, analytics,
audit entries, metrics, delivery and job history. Keep users, workspaces
and business records (anything updated in place or referenced by foreign
keys) as regular tables.

## In a migration

```rust
schema
    .create_hypertable("events", "occurred_at", |t| {
        t.uuid_primary();                       // widened to (id, occurred_at)
        t.foreign_uuid("workspace_id").index();
        t.string("kind");
        t.timestamp_tz("occurred_at");
        let events = t.hypertable("occurred_at");
        events.chunk_interval("1 day").retain_for("90 days");
        events.compress().segment_by(["workspace_id"]).order_by("occurred_at DESC").after("7 days");
    })
    .await
```

An existing table: `schema.convert_to_hypertable("audit_logs",
Hypertable::on("created_at").chunk_interval("1 week").migrate_data())`
(`.migrate_data()` when it has rows).

- Every primary key and unique index must include the time column (and a
  `partition_by` column): the builder widens the primary key, on every
  database, and rejects a unique index that lacks it. Use UUID v7 or ULID
  ids so ids sort by time.
- Chunk size: TimescaleDB's default is 7 days; use `"1 day"` or less for
  millions of rows a day. Integer time columns take integer intervals.
- Compression: `segment_by` the columns queries filter on (workspace,
  device), `order_by` time descending, `after` a delay past the chunks still
  written.
- Policies on existing hypertables: `schema.compress`,
  `add_compression_policy`, `add_retention_policy`, `add_reorder_policy`,
  `set_chunk_interval`, and their `remove_*`.

## Continuous aggregates

```rust
schema
    .create_continuous_aggregate(
        ContinuousAggregate::over("events_daily", "events")
            .time_bucket("1 day", "occurred_at", "day")
            .group_by("workspace_id")
            .aggregate("count(*)", "total")
            .refresh(RefreshPolicy::every("1 hour").start_offset("3 days").end_offset("1 hour")),
    )
    .await
```

Describe them (as above) rather than writing SQL with `time_bucket`: the
described form also compiles for databases without TimescaleDB.
`refresh_continuous_aggregate` needs a migration with
`within_transaction() == false`.

## Everywhere else

The same migration runs on plain PostgreSQL, MySQL, MariaDB and SQLite: a
regular table with a `(time DESC)` index, aggregates as plain views, and a
notice for each skipped policy, so tests run the production migrations on
SQLite. `schema.timescaledb = "require"` (production) fails instead of
falling back on PostgreSQL; `"off"` never uses TimescaleDB; `"auto"`
installs the extension when the server has it and the user is a superuser.
