//! TimescaleDB statements: hypertables (created or converted), their keys,
//! compression, retention and reorder policies, continuous aggregates, and
//! the fallbacks on databases without TimescaleDB.

use crate::blueprint::{Blueprint, Command};
use crate::column::ColumnType;
use crate::error::{SchemaError, SchemaResult};
use crate::grammar::{Compiled, Context, ident, string, table};
use crate::index::{IndexDefinition, IndexKind};
use crate::interval::{Interval, IntervalUnit};
use crate::naming::index_name;
use crate::options::{KeyStrategy, TimescaleMode};
use crate::target::{Dialect, Version};
use crate::timescale::{
    AggregateQuery, Bucket, Compression, ContinuousAggregate, Hypertable, RefreshPolicy,
};

/// Installs the extension.
pub(crate) const CREATE_EXTENSION: &str = "CREATE EXTENSION IF NOT EXISTS timescaledb";

/// The first version with `by_range` / `by_hash` dimension builders.
const DIMENSION_BUILDERS: Version = Version::new(2, 13, 0);

/// The origin of `time_bucket` for intervals: Monday 2000-01-03 00:00 UTC,
/// as a Unix timestamp. The fallbacks align buckets the same way.
const BUCKET_ORIGIN: i64 = 946_857_600;

/// Whether a database uses TimescaleDB for a hypertable, and why not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// TimescaleDB `version` is (or, with `install`, will be) installed.
    Timescale { version: Version, install: bool },
    /// A regular table; `reason` says why, for the notice.
    Fallback { reason: String },
}

/// The mode for a hypertable on `table_name`; an error when
/// `schema.timescaledb = "require"` and TimescaleDB is not usable.
pub(crate) fn mode(table_name: &str, ctx: &Context<'_>) -> SchemaResult<Mode> {
    let target = ctx.target;
    let reason = match target.dialect() {
        Dialect::Postgres if ctx.options.timescaledb == TimescaleMode::Off => {
            "TimescaleDB is turned off (`schema.timescaledb = \"off\"`)".to_owned()
        }
        Dialect::Postgres => {
            if let Some(version) = target.timescaledb() {
                return Ok(Mode::Timescale {
                    version,
                    install: false,
                });
            }
            match target.timescaledb_installable() {
                Some(version) if ctx.options.create_extensions => {
                    return Ok(Mode::Timescale {
                        version,
                        install: true,
                    });
                }
                Some(_) => "TimescaleDB can be installed in this database, but `schema.create_extensions` is off (run `CREATE EXTENSION timescaledb`)".to_owned(),
                None => "TimescaleDB is not installed in this PostgreSQL database".to_owned(),
            }
        }
        Dialect::MySql if target.is_mariadb() => "MariaDB does not support TimescaleDB".to_owned(),
        Dialect::MySql => "MySQL does not support TimescaleDB".to_owned(),
        Dialect::Sqlite => "SQLite does not support TimescaleDB".to_owned(),
    };
    if ctx.options.timescaledb == TimescaleMode::Require && target.dialect() == Dialect::Postgres {
        return Err(SchemaError::hypertable(
            table_name,
            format!(
                "{reason}, and `schema.timescaledb = \"require\"`: install TimescaleDB in {} (it must be in `shared_preload_libraries`, and a superuser must run `CREATE EXTENSION timescaledb`)",
                target.describe()
            ),
        ));
    }
    Ok(Mode::Fallback { reason })
}

/// A string literal naming a relation for TimescaleDB's functions
/// (`'events'`, `'"Events"'`, `'analytics.events'`).
pub(crate) fn regclass(name: &str) -> String {
    let simple = |part: &str| {
        let mut chars = part.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    let text = name
        .split('.')
        .map(|part| {
            if simple(part) {
                part.to_owned()
            } else {
                format!("\"{}\"", part.replace('"', "\"\""))
            }
        })
        .collect::<Vec<_>>()
        .join(".");
    string(Dialect::Postgres, &text)
}

/// An interval argument: a whole number for integer time columns, else an
/// `INTERVAL` literal.
pub(crate) fn interval_arg(value: &str) -> SchemaResult<String> {
    let trimmed = value.trim();
    if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Ok(trimmed.to_owned());
    }
    Ok(Interval::parse(trimmed)?.to_postgres())
}

/// The kind of a hypertable's time column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TimeKind {
    Temporal,
    Integer,
}

/// The time kind of a declared column type.
pub(crate) fn time_kind_of(kind: &ColumnType) -> Option<TimeKind> {
    if kind.is_integer() {
        Some(TimeKind::Integer)
    } else if matches!(
        kind,
        ColumnType::Date
            | ColumnType::DateTime { .. }
            | ColumnType::DateTimeTz { .. }
            | ColumnType::Timestamp { .. }
            | ColumnType::TimestampTz { .. }
    ) {
        Some(TimeKind::Temporal)
    } else {
        None
    }
}

/// The time kind of an introspected type name (`timestamptz`, `int8`,
/// `timestamp with time zone`, ...).
pub(crate) fn time_kind_of_type_name(type_name: &str) -> Option<TimeKind> {
    let name = type_name.to_ascii_lowercase();
    if name.starts_with("timestamp") || name == "date" || name.starts_with("datetime") {
        Some(TimeKind::Temporal)
    } else if [
        "int2", "int4", "int8", "smallint", "integer", "bigint", "int", "tinyint", "mediumint",
    ]
    .contains(&name.as_str())
    {
        Some(TimeKind::Integer)
    } else {
        None
    }
}

fn check_chunk_interval(
    table_name: &str,
    hypertable: &Hypertable,
    kind: TimeKind,
) -> SchemaResult<Option<String>> {
    let Some(interval) = &hypertable.chunk_interval else {
        return Ok(None);
    };
    let arg = interval_arg(interval)?;
    let integer = arg.chars().all(|c| c.is_ascii_digit());
    match (kind, integer) {
        (TimeKind::Integer, false) => Err(SchemaError::hypertable(
            table_name,
            format!(
                "time column `{}` is an integer, so the chunk interval must be a whole number of its units (got `{interval}`)",
                hypertable.time_column
            ),
        )),
        (TimeKind::Temporal, true) => Err(SchemaError::hypertable(
            table_name,
            format!(
                "time column `{}` holds dates or timestamps, so the chunk interval must be an interval such as `1 day` (got `{interval}`)",
                hypertable.time_column
            ),
        )),
        _ => Ok(Some(arg)),
    }
}

/// `create_hypertable(...)` (and `add_dimension(...)`) for `version`.
pub(crate) fn create_hypertable_statements(
    table_name: &str,
    hypertable: &Hypertable,
    kind: TimeKind,
    version: Version,
    converting: bool,
) -> SchemaResult<Vec<String>> {
    let relation = regclass(table_name);
    let time = string(Dialect::Postgres, &hypertable.time_column);
    let interval = check_chunk_interval(table_name, hypertable, kind)?;
    let mut options = Vec::new();
    if !hypertable.create_default_indexes {
        options.push("create_default_indexes => FALSE".to_owned());
    }
    if converting {
        options.push("if_not_exists => TRUE".to_owned());
    }
    if hypertable.migrate_data {
        options.push("migrate_data => TRUE".to_owned());
    }
    let mut statements = Vec::new();
    if version >= DIMENSION_BUILDERS {
        let range = match &interval {
            Some(interval) => format!("by_range({time}, {interval})"),
            None => format!("by_range({time})"),
        };
        let mut args = vec![relation.clone(), range];
        args.extend(options);
        statements.push(format!("SELECT create_hypertable({})", args.join(", ")));
        if let Some((column, partitions)) = &hypertable.partition {
            statements.push(format!(
                "SELECT add_dimension({relation}, by_hash({}, {partitions}))",
                string(Dialect::Postgres, column)
            ));
        }
    } else {
        let mut args = vec![relation, time];
        if let Some((column, partitions)) = &hypertable.partition {
            args.push(format!(
                "partitioning_column => {}",
                string(Dialect::Postgres, column)
            ));
            args.push(format!("number_partitions => {partitions}"));
        }
        if let Some(interval) = &interval {
            args.push(format!("chunk_time_interval => {interval}"));
        }
        args.extend(options);
        statements.push(format!("SELECT create_hypertable({})", args.join(", ")));
    }
    Ok(statements)
}

/// `ALTER TABLE ... SET (timescaledb.compress, ...)` and, with `after`,
/// the compression policy.
pub(crate) fn compression_statements(
    table_name: &str,
    compression: &Compression,
) -> SchemaResult<Vec<String>> {
    let mut settings = vec!["timescaledb.compress".to_owned()];
    if !compression.segment_by.is_empty() {
        settings.push(format!(
            "timescaledb.compress_segmentby = {}",
            string(Dialect::Postgres, &compression.segment_by.join(", "))
        ));
    }
    if let Some(order) = &compression.order_by {
        settings.push(format!(
            "timescaledb.compress_orderby = {}",
            string(Dialect::Postgres, order)
        ));
    }
    let mut statements = vec![format!(
        "ALTER TABLE {} SET ({})",
        table(Dialect::Postgres, table_name),
        settings.join(", ")
    )];
    if let Some(after) = &compression.after {
        statements.push(format!(
            "SELECT add_compression_policy({}, {})",
            regclass(table_name),
            interval_arg(after)?
        ));
    }
    Ok(statements)
}

/// Compression settings and the compression, retention and reorder
/// policies of a hypertable.
pub(crate) fn policy_statements(
    table_name: &str,
    hypertable: &Hypertable,
) -> SchemaResult<Vec<String>> {
    let relation = regclass(table_name);
    let mut statements = Vec::new();
    if let Some(compression) = &hypertable.compression {
        statements.extend(compression_statements(table_name, compression)?);
    }
    if let Some(retention) = &hypertable.retention {
        statements.push(format!(
            "SELECT add_retention_policy({relation}, {})",
            interval_arg(retention)?
        ));
    }
    if let Some(index) = &hypertable.reorder_index {
        statements.push(format!(
            "SELECT add_reorder_policy({relation}, {})",
            string(Dialect::Postgres, index)
        ));
    }
    Ok(statements)
}

/// The features a fallback table cannot have, for the notice.
fn skipped_features(hypertable: &Hypertable) -> Vec<String> {
    let mut skipped = Vec::new();
    if hypertable.compression.is_some() {
        skipped.push("compression".to_owned());
    }
    if let Some(retention) = &hypertable.retention {
        skipped.push(format!("retention ({retention})"));
    }
    if hypertable.reorder_index.is_some() {
        skipped.push("reorder".to_owned());
    }
    skipped
}

/// The notice for a hypertable that falls back to a regular table.
pub(crate) fn fallback_notice(table_name: &str, hypertable: &Hypertable, reason: &str) -> String {
    let mut notice = format!(
        "{reason}: `{table_name}` is a regular table with an index on `{}`",
        hypertable.time_column
    );
    let skipped = skipped_features(hypertable);
    if !skipped.is_empty() {
        notice.push_str(&format!(
            "; its {} {} skipped",
            join_words(&skipped),
            if skipped.len() == 1 {
                "policy is"
            } else {
                "policies are"
            }
        ));
    }
    notice
}

fn join_words(words: &[String]) -> String {
    match words {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// `(time DESC)` (and `(partition, time DESC)`) indexes standing in for a
/// hypertable's default indexes, unless `existing` already covers them.
pub(crate) fn fallback_indexes(
    dialect: Dialect,
    table_name: &str,
    hypertable: &Hypertable,
    existing: &[Vec<String>],
) -> Vec<String> {
    let starts_with = |columns: &[&str]| {
        existing.iter().any(|index| {
            index.len() >= columns.len() && index.iter().zip(columns).all(|(a, b)| a == b)
        })
    };
    let mut wanted = vec![vec![hypertable.time_column.as_str()]];
    if let Some((column, _)) = &hypertable.partition {
        wanted.push(vec![column.as_str(), hypertable.time_column.as_str()]);
    }
    let mut statements = Vec::new();
    for columns in wanted {
        if starts_with(&columns) {
            continue;
        }
        let owned: Vec<String> = columns.iter().map(|c| (*c).to_owned()).collect();
        let name = ident(dialect, &index_name(table_name, &owned, "index"));
        let (last, init) = owned.split_last().expect("at least the time column");
        let mut parts: Vec<String> = init.iter().map(|c| ident(dialect, c)).collect();
        parts.push(format!("{} DESC", ident(dialect, last)));
        let parts = parts.join(", ");
        statements.push(match dialect {
            Dialect::MySql => format!(
                "ALTER TABLE {} ADD INDEX {name}({parts})",
                table(dialect, table_name)
            ),
            Dialect::Postgres | Dialect::Sqlite => format!(
                "CREATE INDEX {name} ON {} ({parts})",
                table(dialect, table_name)
            ),
        });
    }
    statements
}

/// What creating a hypertable adds to `CREATE TABLE`.
#[derive(Clone, Debug, Default)]
pub(crate) struct CreatePlan {
    /// Run `CREATE EXTENSION` first.
    pub(crate) install_extension: bool,
    /// The primary key to declare (widened with the partitioning columns).
    pub(crate) primary: Option<IndexDefinition>,
    /// Statements right after `CREATE TABLE`.
    pub(crate) after_create: Vec<String>,
    /// Statements after every other command (policies).
    pub(crate) policies: Vec<String>,
    /// Notices.
    pub(crate) notices: Vec<String>,
}

/// Checks a created hypertable's columns and keys and plans its statements
/// for the target: TimescaleDB, or the fallback.
pub(crate) fn plan_create(
    blueprint: &Blueprint,
    hypertable: &Hypertable,
    commands: &[Command],
    ctx: &Context<'_>,
) -> SchemaResult<CreatePlan> {
    let table_name = blueprint.table();
    let dialect = ctx.target.dialect();
    let fail = |message: String| Err(SchemaError::hypertable(table_name, message));
    let find = |name: &str| blueprint.columns().iter().find(|c| c.name == name);
    let Some(time) = find(&hypertable.time_column) else {
        return fail(format!(
            "its time column `{}` is not a column of the table; add it, e.g. `t.timestamp_tz(\"{}\")`",
            hypertable.time_column, hypertable.time_column
        ));
    };
    let Some(kind) = time_kind_of(&time.kind) else {
        return fail(format!(
            "time column `{}` is a {} column; a hypertable is partitioned by a date, timestamp or integer column",
            time.name,
            time.kind.name()
        ));
    };
    if let Some((column, partitions)) = &hypertable.partition {
        if find(column).is_none() {
            return fail(format!(
                "its partition column `{column}` is not a column of the table"
            ));
        }
        if *partitions == 0 {
            return fail(format!(
                "partition_by(\"{column}\", 0) needs at least one partition"
            ));
        }
    }
    let required: Vec<String> = hypertable
        .partitioning_columns()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let missing = |columns: &[String]| -> Vec<String> {
        required
            .iter()
            .filter(|r| !columns.contains(r))
            .cloned()
            .collect()
    };

    for command in commands {
        if let Command::Index(index) = command
            && index.kind == IndexKind::Unique
        {
            let lacking = missing(&index.columns);
            if !lacking.is_empty() {
                return fail(unique_message(&index.name, &index.columns, &lacking));
            }
        }
    }

    let mut plan = CreatePlan::default();
    let strategy = hypertable.keys.unwrap_or(ctx.options.hypertable_keys);
    let explicit = crate::grammar::primary_command(commands).cloned();
    let auto = blueprint
        .columns()
        .iter()
        .find(|c| c.is_auto_increment() && !c.change);
    let current = explicit.clone().or_else(|| {
        auto.map(|column| {
            let columns = vec![column.name.clone()];
            IndexDefinition::new(
                IndexKind::Primary,
                blueprint.conventional_name(IndexKind::Primary, &columns),
                columns,
            )
        })
    });
    plan.primary = explicit;
    if let Some(primary) = current {
        let lacking = missing(&primary.columns);
        if !lacking.is_empty() {
            if strategy == KeyStrategy::Reject {
                return fail(format!(
                    "its primary key ({}) lacks {}: TimescaleDB enforces uniqueness per chunk, so the key must include the time{} column. Declare `t.primary([{}])`, or let the builder widen it (`hypertable_keys = \"extend\"`)",
                    primary.columns.join(", "),
                    lacking.join(" and "),
                    if hypertable.partition.is_some() {
                        " and partition"
                    } else {
                        ""
                    },
                    primary
                        .columns
                        .iter()
                        .chain(&lacking)
                        .map(|c| format!("\"{c}\""))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            let sqlite_rowid = dialect == Dialect::Sqlite
                && auto.is_some_and(|a| primary.columns == [a.name.clone()]);
            if sqlite_rowid {
                plan.notices.push(format!(
                    "SQLite keeps `{}` as the primary key of `{table_name}` (AUTOINCREMENT needs it alone); TimescaleDB widens it to ({})",
                    primary.columns.join(", "),
                    primary.columns.iter().chain(&lacking).cloned().collect::<Vec<_>>().join(", ")
                ));
            } else {
                let mut widened = primary.clone();
                widened.columns.extend(lacking);
                plan.notices.push(format!(
                    "the primary key of `{table_name}` is ({}), not ({}): a hypertable's unique keys must include its time{} column",
                    widened.columns.join(", "),
                    primary.columns.join(", "),
                    if hypertable.partition.is_some() {
                        " and partition"
                    } else {
                        ""
                    }
                ));
                plan.primary = Some(widened);
            }
        }
    }

    match mode(table_name, ctx)? {
        Mode::Timescale { version, install } => {
            plan.install_extension = install;
            plan.after_create =
                create_hypertable_statements(table_name, hypertable, kind, version, false)?;
            plan.policies = policy_statements(table_name, hypertable)?;
        }
        Mode::Fallback { reason } => {
            // Check the intervals anyway, so a typo fails on every database.
            check_chunk_interval(table_name, hypertable, kind)?;
            policy_statements(table_name, hypertable)?;
            let existing: Vec<Vec<String>> = commands
                .iter()
                .filter_map(|c| match c {
                    Command::Index(index) => Some(index.columns.clone()),
                    _ => None,
                })
                .collect();
            plan.policies = fallback_indexes(dialect, table_name, hypertable, &existing);
            plan.notices
                .push(fallback_notice(table_name, hypertable, &reason));
        }
    }
    Ok(plan)
}

/// Why a unique index cannot stay as it is on a hypertable.
pub(crate) fn unique_message(name: &str, columns: &[String], lacking: &[String]) -> String {
    format!(
        "unique index `{name}` ({}) does not include {}: TimescaleDB enforces uniqueness per chunk, so every unique index of a hypertable must include its partitioning columns. Add {} to the index, or drop it",
        columns.join(", "),
        lacking.join(" and "),
        lacking.join(" and ")
    )
}

/// The keys of an existing table, read before converting it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeyState {
    /// Every column and its type name.
    pub(crate) columns: Vec<(String, String)>,
    /// The primary key constraint's name and columns.
    pub(crate) primary: Option<(String, Vec<String>)>,
    /// Unique indexes and constraints (other than the primary key): name,
    /// columns.
    pub(crate) uniques: Vec<(String, Vec<String>)>,
    /// Every index's columns (to skip fallback indexes that exist).
    pub(crate) indexes: Vec<Vec<String>>,
    /// Foreign keys of other tables referencing this one: constraint name,
    /// referencing table.
    pub(crate) referenced_by: Vec<(String, String)>,
    /// Whether the table has rows.
    pub(crate) has_rows: bool,
}

/// Converts an existing table: checks and widens its keys, then creates
/// the hypertable (or the fallback indexes).
pub(crate) fn plan_convert(
    table_name: &str,
    hypertable: &Hypertable,
    state: &KeyState,
    ctx: &Context<'_>,
) -> SchemaResult<Compiled> {
    let fail = |message: String| Err(SchemaError::hypertable(table_name, message));
    let column_type = |name: &str| {
        state
            .columns
            .iter()
            .find(|(c, _)| c == name)
            .map(|(_, t)| t.as_str())
    };
    let Some(time_type) = column_type(&hypertable.time_column) else {
        return fail(format!(
            "its time column `{}` is not a column of the table",
            hypertable.time_column
        ));
    };
    let Some(kind) = time_kind_of_type_name(time_type) else {
        return fail(format!(
            "time column `{}` has type `{time_type}`; a hypertable is partitioned by a date, timestamp or integer column",
            hypertable.time_column
        ));
    };
    if let Some((column, _)) = &hypertable.partition
        && column_type(column).is_none()
    {
        return fail(format!(
            "its partition column `{column}` is not a column of the table"
        ));
    }
    let required: Vec<String> = hypertable
        .partitioning_columns()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let missing = |columns: &[String]| -> Vec<String> {
        required
            .iter()
            .filter(|r| !columns.contains(r))
            .cloned()
            .collect()
    };
    for (name, columns) in &state.uniques {
        let lacking = missing(columns);
        if !lacking.is_empty() {
            return fail(unique_message(name, columns, &lacking));
        }
    }

    let mut out = Compiled::default();
    let dialect = ctx.target.dialect();
    let mode = mode(table_name, ctx)?;
    let timescale = matches!(mode, Mode::Timescale { .. });
    if let Some((constraint, columns)) = &state.primary {
        let lacking = missing(columns);
        if !lacking.is_empty() && timescale {
            let strategy = hypertable.keys.unwrap_or(ctx.options.hypertable_keys);
            let widened: Vec<String> = columns.iter().chain(&lacking).cloned().collect();
            if strategy == KeyStrategy::Reject {
                return fail(format!(
                    "its primary key `{constraint}` ({}) lacks {}: rebuild it as ({}) first (`ALTER TABLE ... DROP CONSTRAINT {constraint}, ADD PRIMARY KEY (...)`), or let the builder do it (`hypertable_keys = \"extend\"`)",
                    columns.join(", "),
                    lacking.join(" and "),
                    widened.join(", ")
                ));
            }
            if !state.referenced_by.is_empty() {
                return fail(format!(
                    "its primary key ({}) must widen to ({}), but {} {} it: {}. Point those foreign keys at a key that includes {}, or drop them, then convert",
                    columns.join(", "),
                    widened.join(", "),
                    state.referenced_by.len(),
                    if state.referenced_by.len() == 1 {
                        "foreign key references"
                    } else {
                        "foreign keys reference"
                    },
                    state
                        .referenced_by
                        .iter()
                        .map(|(name, table)| format!("`{table}` ({name})"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    lacking.join(" and ")
                ));
            }
            out.push(format!(
                "ALTER TABLE {} DROP CONSTRAINT {}, ADD PRIMARY KEY ({})",
                table(Dialect::Postgres, table_name),
                ident(Dialect::Postgres, constraint),
                crate::grammar::columnize(Dialect::Postgres, &widened)
            ));
            out.notice(format!(
                "the primary key of `{table_name}` is now ({}), not ({}): a hypertable's unique keys must include its partitioning columns",
                widened.join(", "),
                columns.join(", ")
            ));
        }
    }
    match mode {
        Mode::Timescale { version, install } => {
            if state.has_rows && !hypertable.migrate_data {
                return fail(
                    "the table has rows: call `.migrate_data()` on the hypertable to move them into chunks (it locks the table while it runs)"
                        .to_owned(),
                );
            }
            if install {
                out.statements.insert(0, CREATE_EXTENSION.to_owned());
            }
            out.statements.extend(create_hypertable_statements(
                table_name, hypertable, kind, version, true,
            )?);
            out.statements
                .extend(policy_statements(table_name, hypertable)?);
        }
        Mode::Fallback { reason } => {
            check_chunk_interval(table_name, hypertable, kind)?;
            policy_statements(table_name, hypertable)?;
            out.statements.extend(fallback_indexes(
                dialect,
                table_name,
                hypertable,
                &state.indexes,
            ));
            out.notice(fallback_notice(table_name, hypertable, &reason));
        }
    }
    Ok(out)
}

/// A statement on an existing hypertable or continuous aggregate, or, when
/// the database has no TimescaleDB, the notice `"{reason}: {skipped}"`.
pub(crate) fn policy_or_notice(
    table_name: &str,
    statement: String,
    skipped: &str,
    ctx: &Context<'_>,
) -> SchemaResult<Compiled> {
    let mut out = Compiled::default();
    match mode(table_name, ctx)? {
        Mode::Timescale { install, .. } => {
            if install {
                out.push(CREATE_EXTENSION);
            }
            out.push(statement);
        }
        Mode::Fallback { reason } => out.notice(format!("{reason}: {skipped}")),
    }
    Ok(out)
}

/// `CREATE MATERIALIZED VIEW ... WITH (timescaledb.continuous)` and its
/// refresh policy, or a plain view elsewhere.
pub(crate) fn create_continuous_aggregate(
    aggregate: &ContinuousAggregate,
    ctx: &Context<'_>,
) -> SchemaResult<Compiled> {
    let dialect = ctx.target.dialect();
    let mut out = Compiled::default();
    let name = &aggregate.name;
    if let Some(policy) = &aggregate.refresh {
        interval_arg(&policy.schedule_interval)?;
        for offset in [&policy.start_offset, &policy.end_offset].into_iter().flatten() {
            interval_arg(offset)?;
        }
    }
    match mode(name, ctx)? {
        Mode::Timescale { install, .. } => {
            if install {
                out.push(CREATE_EXTENSION);
            }
            let query = aggregate_query(&aggregate.query, BucketStyle::TimeBucket)?;
            let mut with = vec!["timescaledb.continuous".to_owned()];
            if let Some(only) = aggregate.materialized_only {
                with.push(format!(
                    "timescaledb.materialized_only = {}",
                    if only { "TRUE" } else { "FALSE" }
                ));
            }
            out.push(format!(
                "CREATE MATERIALIZED VIEW {} WITH ({}) AS {query} WITH NO DATA",
                table(dialect, name),
                with.join(", ")
            ));
            if let Some(policy) = &aggregate.refresh {
                out.push(refresh_policy_statement(name, policy)?);
            }
        }
        Mode::Fallback { reason } => {
            if let AggregateQuery::Raw(sql) = &aggregate.query
                && calls_time_bucket(sql)
            {
                return Err(SchemaError::definition(
                    name,
                    format!(
                        "{reason}, and this continuous aggregate's SQL calls `time_bucket`, a TimescaleDB function: describe it with `ContinuousAggregate::over(\"{name}\", table).time_bucket(width, column, alias)...` so it compiles for every database, or install TimescaleDB"
                    ),
                ));
            }
            let style = match dialect {
                Dialect::Postgres if ctx.target.at_least(14) => BucketStyle::DateBin,
                Dialect::Postgres => BucketStyle::PostgresEpoch,
                Dialect::MySql => BucketStyle::MySql,
                Dialect::Sqlite => BucketStyle::Sqlite,
            };
            let query = aggregate_query(&aggregate.query, style)?;
            out.push(format!("CREATE VIEW {} AS {query}", table(dialect, name)));
            out.notice(format!(
                "{reason}: continuous aggregate `{name}` is a plain view, computed when read"
            ));
        }
    }
    Ok(out)
}

/// `add_continuous_aggregate_policy(...)` for `policy`; an unbounded side
/// is `NULL`.
pub(crate) fn refresh_policy_statement(name: &str, policy: &RefreshPolicy) -> SchemaResult<String> {
    let offset = |o: &Option<String>| match o {
        Some(value) => interval_arg(value),
        None => Ok("NULL".to_owned()),
    };
    Ok(format!(
        "SELECT add_continuous_aggregate_policy({}, start_offset => {}, end_offset => {}, schedule_interval => {})",
        regclass(name),
        offset(&policy.start_offset)?,
        offset(&policy.end_offset)?,
        interval_arg(&policy.schedule_interval)?
    ))
}

/// Whether `sql` calls TimescaleDB's `time_bucket` (or one of its
/// variants, `time_bucket_gapfill`), outside strings and comments.
fn calls_time_bucket(sql: &str) -> bool {
    use crate::rebuild::{TokenKind, tokenize};
    let tokens = tokenize(sql);
    tokens.windows(2).any(|pair| {
        pair[0].kind == TokenKind::Word
            && sql[pair[0].start..pair[0].end]
                .to_ascii_lowercase()
                .starts_with("time_bucket")
            && pair[1].kind == TokenKind::Open
    })
}

/// Drops a continuous aggregate (a materialized view on TimescaleDB, a view
/// elsewhere).
pub(crate) fn drop_continuous_aggregate(name: &str, ctx: &Context<'_>) -> String {
    let dialect = ctx.target.dialect();
    let timescale = dialect == Dialect::Postgres
        && ctx.options.timescaledb != TimescaleMode::Off
        && ctx.target.timescaledb().is_some();
    format!(
        "DROP {}VIEW IF EXISTS {}",
        if timescale { "MATERIALIZED " } else { "" },
        table(dialect, name)
    )
}

/// How a described aggregate's bucket is computed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BucketStyle {
    TimeBucket,
    DateBin,
    PostgresEpoch,
    MySql,
    Sqlite,
}

impl BucketStyle {
    fn dialect(self) -> Dialect {
        match self {
            BucketStyle::TimeBucket | BucketStyle::DateBin | BucketStyle::PostgresEpoch => {
                Dialect::Postgres
            }
            BucketStyle::MySql => Dialect::MySql,
            BucketStyle::Sqlite => Dialect::Sqlite,
        }
    }
}

fn aggregate_query(query: &AggregateQuery, style: BucketStyle) -> SchemaResult<String> {
    let dialect = style.dialect();
    let AggregateQuery::Structured {
        from,
        bucket,
        groups,
        aggregates,
        filter,
    } = query
    else {
        let AggregateQuery::Raw(sql) = query else {
            unreachable!()
        };
        return Ok(sql.trim().trim_end_matches(';').to_owned());
    };
    let mut select = Vec::new();
    let mut group = Vec::new();
    if let Some(bucket) = bucket {
        let expression = bucket_expression(bucket, style)?;
        select.push(format!("{expression} AS {}", ident(dialect, &bucket.alias)));
        group.push(expression);
    }
    for column in groups {
        select.push(ident(dialect, column));
        group.push(ident(dialect, column));
    }
    for (expression, alias) in aggregates {
        select.push(format!("{expression} AS {}", ident(dialect, alias)));
    }
    if select.is_empty() {
        return Err(SchemaError::definition(
            from,
            "a continuous aggregate needs a bucket, a group or an aggregate",
        ));
    }
    let mut sql = format!("SELECT {} FROM {}", select.join(", "), table(dialect, from));
    if let Some(filter) = filter {
        sql.push_str(&format!(" WHERE {filter}"));
    }
    if !group.is_empty() {
        sql.push_str(&format!(" GROUP BY {}", group.join(", ")));
    }
    Ok(sql)
}

fn bucket_expression(bucket: &Bucket, style: BucketStyle) -> SchemaResult<String> {
    let interval = Interval::parse(&bucket.width)?;
    let column = ident(style.dialect(), &bucket.column);
    let calendar = interval.as_seconds().is_none();
    let unsupported = |what: &str| {
        Err(SchemaError::unsupported(
            style.dialect(),
            format!("{what} buckets of {interval} outside TimescaleDB"),
            "use 1 month or 1 year, or a fixed width (seconds to weeks)",
        ))
    };
    if calendar && interval.amount() != 1 && style != BucketStyle::TimeBucket {
        return unsupported("calendar");
    }
    Ok(match style {
        BucketStyle::TimeBucket => format!("time_bucket({}, {column})", interval.to_postgres()),
        BucketStyle::DateBin | BucketStyle::PostgresEpoch if calendar => format!(
            "date_trunc({}, {column})",
            string(
                Dialect::Postgres,
                if interval.unit() == IntervalUnit::Month {
                    "month"
                } else {
                    "year"
                }
            )
        ),
        BucketStyle::DateBin => format!(
            "date_bin({}, {column}, '2000-01-03 00:00:00+00')",
            interval.to_postgres()
        ),
        BucketStyle::PostgresEpoch => {
            let width = interval.as_seconds().unwrap_or(1);
            format!(
                "to_timestamp(floor((extract(epoch from {column}) - {BUCKET_ORIGIN}) / {width}) * {width} + {BUCKET_ORIGIN})"
            )
        }
        BucketStyle::MySql if calendar => format!(
            "CAST(DATE_FORMAT({column}, {}) AS DATETIME)",
            string(
                Dialect::MySql,
                if interval.unit() == IntervalUnit::Month {
                    "%Y-%m-01"
                } else {
                    "%Y-01-01"
                }
            )
        ),
        BucketStyle::MySql => {
            let width = interval.as_seconds().unwrap_or(1);
            format!(
                "FROM_UNIXTIME(FLOOR((UNIX_TIMESTAMP({column}) - {BUCKET_ORIGIN}) / {width}) * {width} + {BUCKET_ORIGIN})"
            )
        }
        BucketStyle::Sqlite if calendar => format!(
            "strftime({}, {column})",
            string(
                Dialect::Sqlite,
                if interval.unit() == IntervalUnit::Month {
                    "%Y-%m-01 00:00:00"
                } else {
                    "%Y-01-01 00:00:00"
                }
            )
        ),
        BucketStyle::Sqlite => {
            let width = interval.as_seconds().unwrap_or(1);
            format!(
                "datetime(((CAST(strftime('%s', {column}) AS INTEGER) - {BUCKET_ORIGIN}) / {width}) * {width} + {BUCKET_ORIGIN}, 'unixepoch')"
            )
        }
    })
}

/// `set_chunk_time_interval(...)` for an existing hypertable.
pub(crate) fn set_chunk_interval_statement(table_name: &str, interval: &str) -> SchemaResult<String> {
    Ok(format!(
        "SELECT set_chunk_time_interval({}, {})",
        regclass(table_name),
        interval_arg(interval)?
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::SchemaOptions;
    use crate::target::Target;

    fn audit_state() -> KeyState {
        KeyState {
            columns: vec![
                ("id".into(), "uuid".into()),
                ("workspace_id".into(), "uuid".into()),
                ("created_at".into(), "timestamptz".into()),
                ("action".into(), "varchar".into()),
            ],
            primary: Some(("audit_logs_pkey".into(), vec!["id".into()])),
            indexes: vec![vec!["id".into()], vec!["created_at".into()]],
            ..KeyState::default()
        }
    }

    fn convert(
        target: Target,
        options: SchemaOptions,
        hypertable: &Hypertable,
        state: &KeyState,
    ) -> SchemaResult<Compiled> {
        let ctx = Context {
            target: &target,
            options: &options,
            state: None,
        };
        plan_convert("audit_logs", hypertable, state, &ctx)
    }

    fn timescaledb() -> Target {
        Target::postgres(17).with_timescaledb((2, 17))
    }

    #[test]
    fn converting_widens_the_primary_key_then_creates_the_hypertable() {
        let mut audit = Hypertable::on("created_at");
        audit.chunk_interval("1 day").partition_by("workspace_id", 4);
        let compiled = convert(timescaledb(), SchemaOptions::default(), &audit, &audit_state())
            .unwrap();
        assert_eq!(
            compiled.statements,
            [
                "ALTER TABLE \"audit_logs\" DROP CONSTRAINT \"audit_logs_pkey\", ADD PRIMARY KEY (\"id\", \"created_at\", \"workspace_id\")",
                "SELECT create_hypertable('audit_logs', by_range('created_at', INTERVAL '1 day'), if_not_exists => TRUE)",
                "SELECT add_dimension('audit_logs', by_hash('workspace_id', 4))",
            ]
        );
        assert_eq!(
            compiled.notices,
            ["the primary key of `audit_logs` is now (id, created_at, workspace_id), not (id): a hypertable's unique keys must include its partitioning columns"]
        );
    }

    #[test]
    fn a_table_with_rows_needs_migrate_data() {
        let state = KeyState {
            has_rows: true,
            primary: Some(("audit_logs_pkey".into(), vec!["id".into(), "created_at".into()])),
            ..audit_state()
        };
        let error = convert(
            timescaledb(),
            SchemaOptions::default(),
            &Hypertable::on("created_at"),
            &state,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot make `audit_logs` a hypertable: the table has rows: call `.migrate_data()` on the hypertable to move them into chunks (it locks the table while it runs)"
        );
        let mut audit = Hypertable::on("created_at");
        audit.migrate_data();
        let compiled = convert(timescaledb(), SchemaOptions::default(), &audit, &state).unwrap();
        assert_eq!(
            compiled.statements,
            ["SELECT create_hypertable('audit_logs', by_range('created_at'), if_not_exists => TRUE, migrate_data => TRUE)"]
        );
        assert!(compiled.notices.is_empty(), "the key already includes the time");
    }

    #[test]
    fn keys_that_cannot_change_stop_the_conversion() {
        let mut audit = Hypertable::on("created_at");
        audit.keys(KeyStrategy::Reject);
        let rejected = convert(timescaledb(), SchemaOptions::default(), &audit, &audit_state())
            .unwrap_err();
        assert_eq!(
            rejected.to_string(),
            "cannot make `audit_logs` a hypertable: its primary key `audit_logs_pkey` (id) lacks created_at: rebuild it as (id, created_at) first (`ALTER TABLE ... DROP CONSTRAINT audit_logs_pkey, ADD PRIMARY KEY (...)`), or let the builder do it (`hypertable_keys = \"extend\"`)"
        );

        let referenced = KeyState {
            referenced_by: vec![
                ("audit_notes_audit_log_id_foreign".into(), "audit_notes".into()),
                ("exports_audit_log_id_foreign".into(), "exports".into()),
            ],
            ..audit_state()
        };
        let error = convert(
            timescaledb(),
            SchemaOptions::default(),
            &Hypertable::on("created_at"),
            &referenced,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot make `audit_logs` a hypertable: its primary key (id) must widen to (id, created_at), but 2 foreign keys reference it: `audit_notes` (audit_notes_audit_log_id_foreign), `exports` (exports_audit_log_id_foreign). Point those foreign keys at a key that includes created_at, or drop them, then convert"
        );

        let unique = KeyState {
            uniques: vec![("audit_logs_request_id_unique".into(), vec!["request_id".into()])],
            ..audit_state()
        };
        let error = convert(
            Target::sqlite((3, 46)),
            SchemaOptions::default(),
            &Hypertable::on("created_at"),
            &unique,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unique index `audit_logs_request_id_unique` (request_id) does not include created_at"),
            "{error}"
        );
    }

    #[test]
    fn databases_without_timescaledb_get_the_missing_indexes() {
        let mut audit = Hypertable::on("created_at");
        audit.partition_by("workspace_id", 4).retain_for("1 year");
        let compiled =
            convert(Target::mysql((8, 4)), SchemaOptions::default(), &audit, &audit_state())
                .unwrap();
        assert_eq!(
            compiled.statements,
            ["ALTER TABLE `audit_logs` ADD INDEX `audit_logs_workspace_id_created_at_index`(`workspace_id`, `created_at` DESC)"],
            "the primary key stays; the time index exists already"
        );
        assert_eq!(
            compiled.notices,
            ["MySQL does not support TimescaleDB: `audit_logs` is a regular table with an index on `created_at`; its retention (1 year) policy is skipped"]
        );
    }

    #[test]
    fn the_time_column_is_checked_against_the_table() {
        let error = convert(
            timescaledb(),
            SchemaOptions::default(),
            &Hypertable::on("happened_at"),
            &audit_state(),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot make `audit_logs` a hypertable: its time column `happened_at` is not a column of the table"
        );
        let error = convert(
            timescaledb(),
            SchemaOptions::default(),
            &Hypertable::on("action"),
            &audit_state(),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot make `audit_logs` a hypertable: time column `action` has type `varchar`; a hypertable is partitioned by a date, timestamp or integer column"
        );
    }

    #[test]
    fn installs_the_extension_first_when_it_can() {
        let target = Target::postgres(17).with_timescaledb_installable((2, 17));
        let state = KeyState {
            primary: Some(("audit_logs_pkey".into(), vec!["id".into(), "created_at".into()])),
            ..audit_state()
        };
        let compiled = convert(
            target,
            SchemaOptions::default(),
            &Hypertable::on("created_at"),
            &state,
        )
        .unwrap();
        assert_eq!(compiled.statements[0], CREATE_EXTENSION);
        assert_eq!(compiled.statements.len(), 2);
    }

    #[test]
    fn raw_aggregates_are_scanned_for_time_bucket_calls() {
        assert!(calls_time_bucket("SELECT time_bucket('1 day', at) FROM t"));
        assert!(calls_time_bucket("SELECT TIME_BUCKET_GAPFILL ('1 day', at) FROM t"));
        assert!(!calls_time_bucket("SELECT 'time_bucket(' AS label FROM t"));
        assert!(!calls_time_bucket("SELECT date_trunc('day', at) -- time_bucket(\n FROM t"));
    }

    #[test]
    fn regclass_quotes_only_what_needs_it() {
        assert_eq!(regclass("events"), "'events'");
        assert_eq!(regclass("analytics.events"), "'analytics.events'");
        assert_eq!(regclass("Events"), "'\"Events\"'");
        assert_eq!(regclass("o'brien"), "'\"o''brien\"'");
    }
}
