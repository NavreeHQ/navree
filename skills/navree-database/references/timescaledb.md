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

An existing table: `schema.convert_to_hypertable("visits",
Hypertable::on("created_at").chunk_interval("1 week").migrate_data())`
(`.migrate_data()` when it has rows). `schema.is_hypertable("visits")` says
whether it already is one; `schema.time_dimension("visits")` reads its time
column, type and chunk interval.

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

## Navree's own tables

| Table | Time column | Chunks |
|---|---|---|
| `audit_logs` | `created_at` | 7 days |
| `webhook_deliveries`, `webhook_attempts` | `created_at` | 1 day |
| `fleet_jobs` | `finished_at` (Unix ms) | 1 hour |
| `fleet_metrics` | `at` (Unix ms) | 1 day |
| `schedule_runs` | `tick` | 1 day |
| `lighthouse_samples`, `lighthouse_occurrences` | `at` | 1 day |
| `lighthouse_aggregates` | `bucket` | 1 day |

Their modules convert them; their prune commands (`audit:prune`,
`webhooks:prune`, `lighthouse:prune`, Fleet's trim, the scheduler) drop
whole chunks. Tune them in an app migration:
`schema.set_chunk_interval("audit_logs", "1 day")`,
`schema.compress("lighthouse_samples", &compression)`.

## Pruning in code

```rust
use navree::db::prune_time_series;

// Inside a transaction: count, drop the chunks before the cutoff, delete the rest.
let deleted = prune_time_series(&tx, "events", "occurred_at", Utc::now() - TimeDelta::days(90)).await?;
```

The cutoff is a `DateTime<Utc>` for timestamp columns, an `i64` for integer
ones. On anything but a TimescaleDB hypertable it is one `DELETE`.

## Queries

```rust
use navree::orm::{Expr, col, table};

let points = table("readings")
    .select_time_bucket("15 minutes", "taken_at", "bucket")
    .select_expr(Expr::avg(col("celsius")), "average")
    .group_by(["bucket"])
    .order_by("bucket")
    .get(&db)
    .await?;                                    // point.get::<DateTime<Utc>>("bucket")
```

`select_first(value, time, alias)` / `select_last(...)` and month or year
buckets need TimescaleDB; fixed-width buckets also run on SQLite and MySQL,
aligned like TimescaleDB's (Monday 2000-01-03).

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
