//! TimescaleDB: [`Hypertable`], [`Compression`], [`ContinuousAggregate`] and
//! [`RefreshPolicy`].
//!
//! A hypertable and its compression are configured in place, the way a
//! blueprint's table is: `t.hypertable("occurred_at").chunk_interval("1 day")`
//! inside a blueprint, or a `let mut` [`Hypertable`] handed to
//! [`Schema::convert_to_hypertable`](crate::Schema::convert_to_hypertable).
//! Continuous aggregates and refresh policies are values built with
//! chained calls. On a database without TimescaleDB the schema falls back
//! to what the database can do and reports a notice for the rest.

use crate::index::Columns;
use crate::options::KeyStrategy;

/// A TimescaleDB hypertable: a table partitioned into chunks by a time
/// column, optionally by a space column too, with compression, retention
/// and reorder policies.
///
/// Configure it in place: every setter takes `&mut self` and returns it,
/// and [`compress`](Hypertable::compress) returns the [`Compression`] to
/// configure.
///
/// ```
/// use navree_schema::Hypertable;
///
/// let mut audit = Hypertable::on("created_at");
/// audit
///     .chunk_interval("1 day")
///     .partition_by("workspace_id", 4)
///     .migrate_data()
///     .retain_for("365 days");
/// audit
///     .compress()
///     .segment_by(["workspace_id"])
///     .order_by("created_at DESC")
///     .after("7 days");
/// assert_eq!(audit.time_column(), "created_at");
/// assert_eq!(audit.partition(), Some(("workspace_id", 4)));
/// assert_eq!(audit.compression().unwrap().delay(), Some("7 days"));
/// ```
///
/// A description that fits in one expression can be passed as it is
/// built, since `&mut Hypertable` converts into a `Hypertable`:
/// `schema.convert_to_hypertable("audit_logs", Hypertable::on("created_at").chunk_interval("1 day"))`.
#[derive(Clone, Debug, PartialEq)]
pub struct Hypertable {
    pub(crate) time_column: String,
    pub(crate) chunk_interval: Option<String>,
    pub(crate) partition: Option<(String, u32)>,
    pub(crate) migrate_data: bool,
    pub(crate) create_default_indexes: bool,
    pub(crate) compression: Option<Compression>,
    pub(crate) retention: Option<String>,
    pub(crate) reorder_index: Option<String>,
    pub(crate) keys: Option<KeyStrategy>,
}

impl Hypertable {
    /// A hypertable partitioned by `time_column`.
    pub fn on(time_column: impl Into<String>) -> Hypertable {
        Hypertable {
            time_column: time_column.into(),
            chunk_interval: None,
            partition: None,
            migrate_data: false,
            create_default_indexes: true,
            compression: None,
            retention: None,
            reorder_index: None,
            keys: None,
        }
    }

    /// The span of time per chunk (`"1 day"`, `"7 days"`); a whole number
    /// for integer time columns (`"86400"`). TimescaleDB's default is 7
    /// days; aim for chunks whose indexes fit in about a quarter of the
    /// server's memory.
    pub fn chunk_interval(&mut self, interval: impl Into<String>) -> &mut Hypertable {
        self.chunk_interval = Some(interval.into());
        self
    }

    /// Also partitions by hashing `column` into `partitions` (space
    /// partitioning). Every unique key must then include `column` too.
    pub fn partition_by(&mut self, column: impl Into<String>, partitions: u32) -> &mut Hypertable {
        self.partition = Some((column.into(), partitions));
        self
    }

    /// Moves the rows of an existing table into chunks when converting it
    /// (`migrate_data => true`; it locks the table while it runs). Without
    /// it, converting a table that has rows fails.
    pub fn migrate_data(&mut self) -> &mut Hypertable {
        self.migrate_data = true;
        self
    }

    /// Skips TimescaleDB's default index on the time column (declare your
    /// own, for example `t.index(["device_id", "taken_at"])`).
    pub fn without_default_indexes(&mut self) -> &mut Hypertable {
        self.create_default_indexes = false;
        self
    }

    /// Compresses chunks (TimescaleDB's columnstore): returns the
    /// [`Compression`] to configure, with TimescaleDB's defaults until set.
    ///
    /// ```
    /// use navree_schema::Hypertable;
    ///
    /// let mut events = Hypertable::on("occurred_at");
    /// events
    ///     .compress()
    ///     .segment_by(["workspace_id"])
    ///     .order_by("occurred_at DESC")
    ///     .after("7 days");
    /// assert_eq!(events.compression().unwrap().segments(), ["workspace_id"]);
    /// ```
    pub fn compress(&mut self) -> &mut Compression {
        self.compression.get_or_insert_with(Compression::new)
    }

    /// Compresses chunks as `compression` describes (instead of configuring
    /// [`compress`](Hypertable::compress) in place).
    pub fn compress_with(&mut self, compression: impl Into<Compression>) -> &mut Hypertable {
        self.compression = Some(compression.into());
        self
    }

    /// Drops chunks older than `interval` (`"90 days"`), with a retention
    /// policy.
    pub fn retain_for(&mut self, interval: impl Into<String>) -> &mut Hypertable {
        self.retention = Some(interval.into());
        self
    }

    /// Reorders chunks by the index named `index` once they stop receiving
    /// writes, with a reorder policy.
    pub fn reorder_by(&mut self, index: impl Into<String>) -> &mut Hypertable {
        self.reorder_index = Some(index.into());
        self
    }

    /// What to do with a primary key lacking the time column, instead of
    /// `schema.hypertable_keys`.
    pub fn keys(&mut self, strategy: KeyStrategy) -> &mut Hypertable {
        self.keys = Some(strategy);
        self
    }

    /// The time column.
    pub fn time_column(&self) -> &str {
        &self.time_column
    }

    /// The chunk interval, as given.
    pub fn chunk_interval_value(&self) -> Option<&str> {
        self.chunk_interval.as_deref()
    }

    /// The space-partition column and its number of partitions.
    pub fn partition(&self) -> Option<(&str, u32)> {
        self.partition.as_ref().map(|(c, n)| (c.as_str(), *n))
    }

    /// The compression settings.
    pub fn compression(&self) -> Option<&Compression> {
        self.compression.as_ref()
    }

    /// The retention period, as given.
    pub fn retention(&self) -> Option<&str> {
        self.retention.as_deref()
    }

    /// The index of the reorder policy.
    pub fn reorder_index(&self) -> Option<&str> {
        self.reorder_index.as_deref()
    }

    /// The columns every primary key and unique index must include: the
    /// time column, then the space-partition column.
    pub fn partitioning_columns(&self) -> Vec<&str> {
        let mut columns = vec![self.time_column.as_str()];
        if let Some((column, _)) = &self.partition {
            columns.push(column.as_str());
        }
        columns
    }
}

impl From<&str> for Hypertable {
    /// A hypertable on the time column `time_column`.
    fn from(time_column: &str) -> Hypertable {
        Hypertable::on(time_column)
    }
}

impl From<String> for Hypertable {
    fn from(time_column: String) -> Hypertable {
        Hypertable::on(time_column)
    }
}

impl From<&Hypertable> for Hypertable {
    fn from(hypertable: &Hypertable) -> Hypertable {
        hypertable.clone()
    }
}

impl From<&mut Hypertable> for Hypertable {
    /// The description built so far, so a chain can be passed as it is
    /// built: `Hypertable::on("at").chunk_interval("1 day")`.
    fn from(hypertable: &mut Hypertable) -> Hypertable {
        hypertable.clone()
    }
}

/// How a hypertable's chunks are compressed (TimescaleDB's columnstore).
///
/// ```
/// use navree_schema::Compression;
///
/// let mut compression = Compression::new();
/// compression
///     .segment_by(["workspace_id", "kind"])
///     .order_by("occurred_at DESC")
///     .after("7 days");
/// assert_eq!(compression.segments(), ["workspace_id", "kind"]);
/// assert_eq!(compression.order(), Some("occurred_at DESC"));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Compression {
    pub(crate) segment_by: Vec<String>,
    pub(crate) order_by: Option<String>,
    pub(crate) after: Option<String>,
}

impl Compression {
    /// Compression with TimescaleDB's defaults.
    pub fn new() -> Compression {
        Compression::default()
    }

    /// Groups compressed rows by these columns, so queries filtering on
    /// them stay fast (pick columns with many rows per value, such as a
    /// workspace or device id).
    pub fn segment_by(&mut self, columns: impl Columns) -> &mut Compression {
        self.segment_by = columns.into_columns();
        self
    }

    /// The order of rows inside a compressed segment, e.g. `occurred_at DESC`.
    pub fn order_by(&mut self, order: impl Into<String>) -> &mut Compression {
        self.order_by = Some(order.into());
        self
    }

    /// Compresses chunks once they are older than `interval`, with a
    /// compression policy.
    pub fn after(&mut self, interval: impl Into<String>) -> &mut Compression {
        self.after = Some(interval.into());
        self
    }

    /// The segment-by columns.
    pub fn segments(&self) -> &[String] {
        &self.segment_by
    }

    /// The order-by clause.
    pub fn order(&self) -> Option<&str> {
        self.order_by.as_deref()
    }

    /// The policy's delay, as given.
    pub fn delay(&self) -> Option<&str> {
        self.after.as_deref()
    }
}

impl From<&Compression> for Compression {
    fn from(compression: &Compression) -> Compression {
        compression.clone()
    }
}

impl From<&mut Compression> for Compression {
    /// The settings built so far, so a chain can be passed as it is built.
    fn from(compression: &mut Compression) -> Compression {
        compression.clone()
    }
}

/// How often a continuous aggregate is refreshed, and over which window.
///
/// ```
/// use navree_schema::RefreshPolicy;
///
/// let policy = RefreshPolicy::every("1 hour").start_offset("1 month").end_offset("1 hour");
/// assert_eq!(policy.schedule(), "1 hour");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefreshPolicy {
    pub(crate) schedule_interval: String,
    pub(crate) start_offset: Option<String>,
    pub(crate) end_offset: Option<String>,
}

impl RefreshPolicy {
    /// Refreshes every `interval`.
    pub fn every(interval: impl Into<String>) -> RefreshPolicy {
        RefreshPolicy {
            schedule_interval: interval.into(),
            start_offset: None,
            end_offset: None,
        }
    }

    /// The window starts this long ago (`None`: from the beginning).
    pub fn start_offset(mut self, interval: impl Into<String>) -> RefreshPolicy {
        self.start_offset = Some(interval.into());
        self
    }

    /// The window ends this long ago (`None`: up to now).
    pub fn end_offset(mut self, interval: impl Into<String>) -> RefreshPolicy {
        self.end_offset = Some(interval.into());
        self
    }

    /// The schedule interval, as given.
    pub fn schedule(&self) -> &str {
        &self.schedule_interval
    }
}

/// What a continuous aggregate selects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AggregateQuery {
    /// SQL, written for TimescaleDB (and used as a plain view elsewhere).
    Raw(String),
    /// A description compiled for each database.
    Structured {
        from: String,
        bucket: Option<Bucket>,
        groups: Vec<String>,
        aggregates: Vec<(String, String)>,
        filter: Option<String>,
    },
}

/// `time_bucket(width, column) AS alias`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Bucket {
    pub(crate) width: String,
    pub(crate) column: String,
    pub(crate) alias: String,
}

/// A TimescaleDB continuous aggregate: a materialized view over a
/// hypertable, refreshed incrementally by a policy. Elsewhere it becomes a
/// plain view.
///
/// From SQL:
///
/// ```
/// use navree_schema::{ContinuousAggregate, RefreshPolicy};
///
/// let daily = ContinuousAggregate::new(
///     "events_daily",
///     "SELECT time_bucket(INTERVAL '1 day', occurred_at) AS bucket, count(*) AS total \
///      FROM events GROUP BY bucket",
/// )
/// .refresh(RefreshPolicy::every("1 hour").start_offset("3 days").end_offset("1 hour"));
/// assert_eq!(daily.name(), "events_daily");
/// ```
///
/// Or described, which also compiles for PostgreSQL without TimescaleDB,
/// MySQL and SQLite:
///
/// ```
/// use navree_schema::ContinuousAggregate;
///
/// let daily = ContinuousAggregate::over("events_daily", "events")
///     .time_bucket("1 day", "occurred_at", "bucket")
///     .group_by("workspace_id")
///     .aggregate("count(*)", "total")
///     .aggregate("max(amount)", "largest");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContinuousAggregate {
    pub(crate) name: String,
    pub(crate) query: AggregateQuery,
    pub(crate) refresh: Option<RefreshPolicy>,
    pub(crate) materialized_only: Option<bool>,
}

impl ContinuousAggregate {
    /// A continuous aggregate `name` selecting `sql` (which uses
    /// `time_bucket`).
    pub fn new(name: impl Into<String>, sql: impl Into<String>) -> ContinuousAggregate {
        ContinuousAggregate {
            name: name.into(),
            query: AggregateQuery::Raw(sql.into()),
            refresh: None,
            materialized_only: None,
        }
    }

    /// A described continuous aggregate `name` over `table`; add a bucket,
    /// groups and aggregates.
    pub fn over(name: impl Into<String>, table: impl Into<String>) -> ContinuousAggregate {
        ContinuousAggregate {
            name: name.into(),
            query: AggregateQuery::Structured {
                from: table.into(),
                bucket: None,
                groups: Vec::new(),
                aggregates: Vec::new(),
                filter: None,
            },
            refresh: None,
            materialized_only: None,
        }
    }

    /// Buckets rows by `width` (`"1 day"`) of `column`, selected as `alias`
    /// (described aggregates only).
    pub fn time_bucket(
        mut self,
        width: impl Into<String>,
        column: impl Into<String>,
        alias: impl Into<String>,
    ) -> ContinuousAggregate {
        if let AggregateQuery::Structured { bucket, .. } = &mut self.query {
            *bucket = Some(Bucket {
                width: width.into(),
                column: column.into(),
                alias: alias.into(),
            });
        }
        self
    }

    /// Groups by (and selects) `column` (described aggregates only).
    pub fn group_by(mut self, column: impl Into<String>) -> ContinuousAggregate {
        if let AggregateQuery::Structured { groups, .. } = &mut self.query {
            groups.push(column.into());
        }
        self
    }

    /// Selects `expression AS alias` (`"count(*)"`, `"avg(duration_ms)"`;
    /// described aggregates only).
    pub fn aggregate(
        mut self,
        expression: impl Into<String>,
        alias: impl Into<String>,
    ) -> ContinuousAggregate {
        if let AggregateQuery::Structured { aggregates, .. } = &mut self.query {
            aggregates.push((expression.into(), alias.into()));
        }
        self
    }

    /// Only aggregates rows matching `predicate` (described aggregates only).
    pub fn where_raw(mut self, predicate: impl Into<String>) -> ContinuousAggregate {
        if let AggregateQuery::Structured { filter, .. } = &mut self.query {
            *filter = Some(predicate.into());
        }
        self
    }

    /// Refreshes with `policy`.
    pub fn refresh(mut self, policy: RefreshPolicy) -> ContinuousAggregate {
        self.refresh = Some(policy);
        self
    }

    /// Whether queries read only materialized data (`true`), or also the
    /// newest raw rows (`false`, real-time aggregation).
    pub fn materialized_only(mut self, value: bool) -> ContinuousAggregate {
        self.materialized_only = Some(value);
        self
    }

    /// The view's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The refresh policy.
    pub fn refresh_policy(&self) -> Option<&RefreshPolicy> {
        self.refresh.as_ref()
    }
}

impl From<&ContinuousAggregate> for ContinuousAggregate {
    fn from(aggregate: &ContinuousAggregate) -> ContinuousAggregate {
        aggregate.clone()
    }
}
