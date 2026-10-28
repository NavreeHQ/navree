//! The SQL syntax tree: statements, queries and expressions, the same for
//! every database. A [grammar](super::grammar) writes it for one dialect.
//!
//! Values are always bound ([`Expr::Value`]); [`Expr::Literal`]s are
//! written into the SQL (escaped for the dialect), for the constants a
//! statement's shape depends on (JSON paths, format strings, flags).
//!
//! ```
//! use navree_orm::Dialect;
//! use navree_orm::render::ast::{Expr, Query, Select};
//! use navree_orm::render::render_query;
//!
//! let query = Query::from(
//!     Select::from_table("deals")
//!         .column(Expr::col("title"))
//!         .filter(Expr::col("stage").eq(Expr::value("won"))),
//! )
//! .order_by(Expr::col("title").asc())
//! .limit(10);
//! let statement = render_query(&query, Dialect::Postgres).unwrap();
//! assert_eq!(
//!     statement.sql,
//!     r#"SELECT "title" FROM "deals" WHERE "stage" = $1 ORDER BY "title" ASC LIMIT $2"#
//! );
//! ```

use std::borrow::Cow;

use crate::ir::JsonSegment;
use crate::value::Value;

/// An identifier (a table, column, alias or CTE name), quoted when it is
/// written: `"name"`, `` `name` ``. A dot is part of the name; use
/// [`ObjectName`] for schema-qualified names.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ident(pub Cow<'static, str>);

impl Ident {
    /// The identifier `name`.
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Ident(name.into())
    }

    /// The name, unquoted.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&'static str> for Ident {
    fn from(name: &'static str) -> Self {
        Ident::new(name)
    }
}

impl From<String> for Ident {
    fn from(name: String) -> Self {
        Ident::new(name)
    }
}

impl From<Cow<'static, str>> for Ident {
    fn from(name: Cow<'static, str>) -> Self {
        Ident(name)
    }
}

/// A name of one or more parts, each quoted on its own: `deals`,
/// `audit.events` (`"audit"."events"`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ObjectName(pub Vec<Ident>);

impl ObjectName {
    /// A name of one part (dots included in it).
    pub fn single(name: impl Into<Ident>) -> Self {
        ObjectName(vec![name.into()])
    }

    /// `text` split at its dots: `audit.events` is schema `audit`, table
    /// `events`.
    ///
    /// ```
    /// use navree_orm::render::ast::{Ident, ObjectName};
    ///
    /// assert_eq!(
    ///     ObjectName::parse("audit.events"),
    ///     ObjectName(vec![Ident::new("audit"), Ident::new("events")])
    /// );
    /// ```
    pub fn parse(text: impl Into<Cow<'static, str>>) -> Self {
        let text = text.into();
        if !text.contains('.') {
            return ObjectName(vec![Ident(text)]);
        }
        ObjectName(
            text.split('.')
                .map(|part| Ident::new(part.to_owned()))
                .collect(),
        )
    }

    /// The last part (a table's name without its schema).
    pub fn last(&self) -> Option<&Ident> {
        self.0.last()
    }
}

impl From<&'static str> for ObjectName {
    fn from(name: &'static str) -> Self {
        ObjectName::parse(name)
    }
}

impl From<String> for ObjectName {
    fn from(name: String) -> Self {
        ObjectName::parse(name)
    }
}

impl From<Ident> for ObjectName {
    fn from(name: Ident) -> Self {
        ObjectName(vec![name])
    }
}

/// A statement.
#[derive(Clone, Debug)]
pub enum Statement {
    /// A query (`SELECT`, a compound query, `WITH ..`).
    Query(Box<Query>),
    /// An `INSERT`.
    Insert(Box<Insert>),
    /// An `UPDATE`.
    Update(Box<Update>),
    /// A `DELETE`.
    Delete(Box<Delete>),
    /// Emptying a table (two statements on SQLite).
    Truncate(Truncate),
}

impl From<Query> for Statement {
    fn from(query: Query) -> Self {
        Statement::Query(Box::new(query))
    }
}

impl From<Insert> for Statement {
    fn from(insert: Insert) -> Self {
        Statement::Insert(Box::new(insert))
    }
}

impl From<Update> for Statement {
    fn from(update: Update) -> Self {
        Statement::Update(Box::new(update))
    }
}

impl From<Delete> for Statement {
    fn from(delete: Delete) -> Self {
        Statement::Delete(Box::new(delete))
    }
}

impl From<Truncate> for Statement {
    fn from(truncate: Truncate) -> Self {
        Statement::Truncate(truncate)
    }
}

/// A query: common table expressions, a body (a `SELECT` or a compound
/// of queries), and the order, limit, offset and row lock of the whole.
#[derive(Clone, Debug)]
pub struct Query {
    /// `WITH [RECURSIVE] name AS (..), ..`.
    pub with: Option<With>,
    /// The rows.
    pub body: SetExpr,
    /// `ORDER BY`, sorting the body's rows (a compound's as a whole).
    pub order_by: Vec<OrderBy>,
    /// `LIMIT`, bound as a value.
    pub limit: Option<u64>,
    /// `OFFSET`, bound as a value (MySQL and SQLite get a `LIMIT` before
    /// it).
    pub offset: Option<u64>,
    /// A row lock (none on SQLite, whose write transactions are
    /// serialised).
    pub lock: Option<Lock>,
    /// MySQL's `/*+ MAX_EXECUTION_TIME(ms) */` hint on the query's first
    /// `SELECT`; other databases set their timeout around the statement.
    pub max_execution_time: Option<u64>,
}

impl Query {
    /// A query of `body`, without order, limit or lock.
    pub fn new(body: SetExpr) -> Self {
        Query {
            with: None,
            body,
            order_by: Vec::new(),
            limit: None,
            offset: None,
            lock: None,
            max_execution_time: None,
        }
    }

    /// The same query with `WITH ctes`.
    pub fn with(mut self, ctes: impl IntoIterator<Item = Cte>) -> Self {
        self.with = Some(With {
            recursive: false,
            ctes: ctes.into_iter().collect(),
        });
        self
    }

    /// The same query with `WITH RECURSIVE ctes`.
    pub fn with_recursive(mut self, ctes: impl IntoIterator<Item = Cte>) -> Self {
        self.with = Some(With {
            recursive: true,
            ctes: ctes.into_iter().collect(),
        });
        self
    }

    /// The same query sorted by `order` too.
    pub fn order_by(mut self, order: OrderBy) -> Self {
        self.order_by.push(order);
        self
    }

    /// The same query with a limit.
    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = Some(limit);
        self
    }

    /// The same query with an offset.
    pub fn offset(mut self, offset: u64) -> Self {
        self.offset = Some(offset);
        self
    }

    /// The same query locking the rows it reads.
    pub fn lock(mut self, lock: Lock) -> Self {
        self.lock = Some(lock);
        self
    }

    /// `self UNION other` (`UNION ALL` with `all`); the order and limit
    /// added afterwards apply to the whole.
    pub fn union(self, other: impl Into<Query>, all: bool) -> Self {
        self.combine(SetOperator::Union, all, other.into())
    }

    /// `self INTERSECT other`.
    pub fn intersect(self, other: impl Into<Query>, all: bool) -> Self {
        self.combine(SetOperator::Intersect, all, other.into())
    }

    /// `self EXCEPT other`.
    pub fn except(self, other: impl Into<Query>, all: bool) -> Self {
        self.combine(SetOperator::Except, all, other.into())
    }

    fn combine(self, op: SetOperator, all: bool, other: Query) -> Self {
        Query::new(SetExpr::Operation {
            op,
            all,
            left: Box::new(self.into_set_expr()),
            right: Box::new(other.into_set_expr()),
        })
    }

    /// The query as an operand of a compound: its body when it has no
    /// clauses of its own, else the parenthesized query.
    pub fn into_set_expr(self) -> SetExpr {
        if self.with.is_none()
            && self.order_by.is_empty()
            && self.limit.is_none()
            && self.offset.is_none()
            && self.lock.is_none()
            && self.max_execution_time.is_none()
        {
            self.body
        } else {
            SetExpr::Query(Box::new(self))
        }
    }
}

impl From<Select> for Query {
    fn from(select: Select) -> Self {
        Query::new(SetExpr::Select(Box::new(select)))
    }
}

/// `WITH [RECURSIVE] ..`.
#[derive(Clone, Debug)]
pub struct With {
    /// `WITH RECURSIVE`: a CTE may read itself.
    pub recursive: bool,
    /// The common table expressions, in order.
    pub ctes: Vec<Cte>,
}

/// A common table expression: `name [(columns)] AS (query)`.
#[derive(Clone, Debug)]
pub struct Cte {
    /// Its name.
    pub name: Ident,
    /// Its column names; empty takes the query's.
    pub columns: Vec<Ident>,
    /// Its rows.
    pub query: Box<Query>,
}

impl Cte {
    /// `name AS (query)`.
    pub fn new(name: impl Into<Ident>, query: impl Into<Query>) -> Self {
        Cte {
            name: name.into(),
            columns: Vec::new(),
            query: Box::new(query.into()),
        }
    }

    /// The same CTE naming its columns.
    pub fn columns(mut self, columns: impl IntoIterator<Item = impl Into<Ident>>) -> Self {
        self.columns = columns.into_iter().map(Into::into).collect();
        self
    }
}

/// The body of a query.
#[derive(Clone, Debug)]
pub enum SetExpr {
    /// A `SELECT`.
    Select(Box<Select>),
    /// A query with clauses of its own (an order, a limit), as an operand
    /// of a compound: parenthesized, or a derived table on SQLite.
    Query(Box<Query>),
    /// `left UNION [ALL] right` (or `INTERSECT`, `EXCEPT`).
    Operation {
        /// The operator.
        op: SetOperator,
        /// `ALL`: duplicates kept.
        all: bool,
        /// The left operand.
        left: Box<SetExpr>,
        /// The right operand.
        right: Box<SetExpr>,
    },
}

/// A compound operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SetOperator {
    /// `UNION`.
    Union,
    /// `INTERSECT`.
    Intersect,
    /// `EXCEPT`.
    Except,
}

/// A `SELECT` without its order and limit (those are the [`Query`]'s).
#[derive(Clone, Debug, Default)]
pub struct Select {
    /// `DISTINCT` or `DISTINCT ON (..)`.
    pub distinct: Distinct,
    /// The select list; empty selects `*`.
    pub items: Vec<SelectItem>,
    /// `FROM a, b`; empty for a `SELECT` without a table.
    pub from: Vec<Source>,
    /// Joins after the `FROM` list, in order.
    pub joins: Vec<Join>,
    /// `WHERE`.
    pub filter: Option<Expr>,
    /// `GROUP BY`.
    pub group_by: Vec<Expr>,
    /// `HAVING`.
    pub having: Option<Expr>,
}

impl Select {
    /// A `SELECT` without a table: `SELECT 1`, `SELECT EXISTS (..)`.
    pub fn new() -> Self {
        Select::default()
    }

    /// `SELECT .. FROM table` (`audit.events` is schema-qualified).
    pub fn from_table(table: impl Into<ObjectName>) -> Self {
        Select {
            from: vec![Source::table(table)],
            ..Select::default()
        }
    }

    /// `SELECT .. FROM source`.
    pub fn from_source(source: Source) -> Self {
        Select {
            from: vec![source],
            ..Select::default()
        }
    }

    /// The same select with `expr` added to its list.
    pub fn column(mut self, expr: impl Into<Expr>) -> Self {
        self.items.push(SelectItem {
            expr: expr.into(),
            alias: None,
        });
        self
    }

    /// The same select with `expr AS alias` added to its list.
    pub fn column_as(mut self, expr: impl Into<Expr>, alias: impl Into<Ident>) -> Self {
        self.items.push(SelectItem {
            expr: expr.into(),
            alias: Some(alias.into()),
        });
        self
    }

    /// The same select with `DISTINCT`.
    pub fn distinct(mut self) -> Self {
        self.distinct = Distinct::Distinct;
        self
    }

    /// The same select with a join.
    pub fn join(mut self, join: Join) -> Self {
        self.joins.push(join);
        self
    }

    /// The same select with `condition` added to its `WHERE` (with `AND`).
    pub fn filter(mut self, condition: Expr) -> Self {
        self.filter = Some(match self.filter.take() {
            Some(existing) => existing.and(condition),
            None => condition,
        });
        self
    }

    /// The same select grouped by `expr` too.
    pub fn group_by(mut self, expr: impl Into<Expr>) -> Self {
        self.group_by.push(expr.into());
        self
    }

    /// The same select with `condition` added to its `HAVING`.
    pub fn having(mut self, condition: Expr) -> Self {
        self.having = Some(match self.having.take() {
            Some(existing) => existing.and(condition),
            None => condition,
        });
        self
    }
}

/// `DISTINCT`.
#[derive(Clone, Debug, Default)]
pub enum Distinct {
    /// Every row.
    #[default]
    All,
    /// `SELECT DISTINCT`.
    Distinct,
    /// PostgreSQL's `SELECT DISTINCT ON (exprs)` (others answer
    /// `Unsupported`).
    On(Vec<Expr>),
}

/// One item of a select list (or of `RETURNING`).
#[derive(Clone, Debug)]
pub struct SelectItem {
    /// The expression (`*` and `t.*` are [`Expr::Wildcard`]).
    pub expr: Expr,
    /// `AS alias`.
    pub alias: Option<Ident>,
}

impl SelectItem {
    /// `expr`, without an alias.
    pub fn new(expr: impl Into<Expr>) -> Self {
        SelectItem {
            expr: expr.into(),
            alias: None,
        }
    }

    /// `expr AS alias`.
    pub fn aliased(expr: impl Into<Expr>, alias: impl Into<Ident>) -> Self {
        SelectItem {
            expr: expr.into(),
            alias: Some(alias.into()),
        }
    }
}

/// Where rows come from: a table, a derived table, or a table function.
#[derive(Clone, Debug)]
pub enum Source {
    /// `name [AS alias]`.
    Table {
        /// The table.
        name: ObjectName,
        /// Its alias.
        alias: Option<Ident>,
    },
    /// `[LATERAL] (query) AS alias`.
    Derived {
        /// The query.
        query: Box<Query>,
        /// Its name (every database needs one).
        alias: Ident,
        /// `LATERAL`: the query may read the columns of the tables before
        /// it (PostgreSQL and MySQL 8.0.14+; `Unsupported` on SQLite).
        lateral: bool,
    },
    /// `function(args) [AS alias]`: a set-returning function
    /// (`generate_series(?, ?) AS n`).
    Function {
        /// The call: an [`Expr::Function`].
        call: Box<Expr>,
        /// Its alias.
        alias: Option<Ident>,
    },
}

impl Source {
    /// The table `name`.
    pub fn table(name: impl Into<ObjectName>) -> Self {
        Source::Table {
            name: name.into(),
            alias: None,
        }
    }

    /// The table `name AS alias`.
    pub fn aliased(name: impl Into<ObjectName>, alias: impl Into<Ident>) -> Self {
        Source::Table {
            name: name.into(),
            alias: Some(alias.into()),
        }
    }

    /// `(query) AS alias`.
    pub fn derived(query: impl Into<Query>, alias: impl Into<Ident>) -> Self {
        Source::Derived {
            query: Box::new(query.into()),
            alias: alias.into(),
            lateral: false,
        }
    }

    /// `LATERAL (query) AS alias`.
    pub fn lateral(query: impl Into<Query>, alias: impl Into<Ident>) -> Self {
        Source::Derived {
            query: Box::new(query.into()),
            alias: alias.into(),
            lateral: true,
        }
    }

    /// `call AS alias`, a set-returning function.
    pub fn function(call: Expr, alias: impl Into<Ident>) -> Self {
        Source::Function {
            call: Box::new(call),
            alias: Some(alias.into()),
        }
    }
}

/// A join.
#[derive(Clone, Debug)]
pub struct Join {
    /// The kind.
    pub kind: JoinKind,
    /// The joined rows.
    pub source: Source,
    /// `ON condition`; `None` joins every row (`ON TRUE`; nothing for a
    /// cross join).
    pub on: Option<Expr>,
}

impl Join {
    /// `INNER JOIN source ON on`.
    pub fn inner(source: Source, on: Expr) -> Self {
        Join {
            kind: JoinKind::Inner,
            source,
            on: Some(on),
        }
    }

    /// `LEFT JOIN source ON on`.
    pub fn left(source: Source, on: Expr) -> Self {
        Join {
            kind: JoinKind::Left,
            source,
            on: Some(on),
        }
    }

    /// `CROSS JOIN source`.
    pub fn cross(source: Source) -> Self {
        Join {
            kind: JoinKind::Cross,
            source,
            on: None,
        }
    }
}

/// The kind of a join.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JoinKind {
    /// `INNER JOIN`.
    Inner,
    /// `LEFT JOIN`.
    Left,
    /// `RIGHT JOIN` (SQLite 3.39+).
    Right,
    /// `FULL JOIN` (PostgreSQL, SQLite 3.39+; `Unsupported` on MySQL).
    Full,
    /// `CROSS JOIN`.
    Cross,
}

/// One `ORDER BY` item.
#[derive(Clone, Debug)]
pub struct OrderBy {
    /// The expression.
    pub expr: Expr,
    /// Descending instead of ascending.
    pub descending: bool,
    /// Where NULLs sort (MySQL: a leading `expr IS NULL` key); the
    /// database's default when `None`.
    pub nulls: Option<NullsOrder>,
}

impl OrderBy {
    /// The same order with NULLs first.
    pub fn nulls_first(mut self) -> Self {
        self.nulls = Some(NullsOrder::First);
        self
    }

    /// The same order with NULLs last.
    pub fn nulls_last(mut self) -> Self {
        self.nulls = Some(NullsOrder::Last);
        self
    }
}

/// Where NULLs sort.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NullsOrder {
    /// Before the other rows.
    First,
    /// After the other rows.
    Last,
}

/// A row lock: `FOR UPDATE [OF ..] [NOWAIT | SKIP LOCKED]`.
#[derive(Clone, Debug)]
pub struct Lock {
    /// The lock's strength.
    pub strength: LockStrength,
    /// `OF tables`: the tables whose rows are locked (PostgreSQL, MySQL);
    /// empty locks every table's.
    pub of: Vec<ObjectName>,
    /// What happens to rows other transactions hold.
    pub wait: LockWait,
}

impl Lock {
    /// A lock of `strength` that waits.
    pub fn new(strength: LockStrength) -> Self {
        Lock {
            strength,
            of: Vec::new(),
            wait: LockWait::Wait,
        }
    }
}

/// How strong a row lock is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LockStrength {
    /// `FOR UPDATE`.
    Update,
    /// `FOR NO KEY UPDATE` (PostgreSQL; `FOR UPDATE` on MySQL).
    NoKeyUpdate,
    /// `FOR SHARE` (MySQL: `LOCK IN SHARE MODE` unless it waits otherwise
    /// or names tables).
    Share,
    /// `FOR KEY SHARE` (PostgreSQL; a shared lock on MySQL).
    KeyShare,
}

/// What a locking read does with rows another transaction holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LockWait {
    /// Waits for them.
    #[default]
    Wait,
    /// `NOWAIT`: fails at once.
    NoWait,
    /// `SKIP LOCKED`: leaves them out.
    SkipLocked,
}

/// An `INSERT`.
#[derive(Clone, Debug)]
pub struct Insert {
    /// The table.
    pub table: ObjectName,
    /// The columns, in the order of each row's values.
    pub columns: Vec<Ident>,
    /// The rows.
    pub source: InsertSource,
    /// What a row conflicting with a unique index does.
    pub on_conflict: Option<OnConflict>,
    /// `RETURNING items` (PostgreSQL and SQLite; `Unsupported` on MySQL).
    pub returning: Vec<SelectItem>,
}

impl Insert {
    /// `INSERT INTO table (columns) VALUES rows..`.
    pub fn values(
        table: impl Into<ObjectName>,
        columns: impl IntoIterator<Item = impl Into<Ident>>,
        rows: Vec<Vec<Expr>>,
    ) -> Self {
        Insert {
            table: table.into(),
            columns: columns.into_iter().map(Into::into).collect(),
            source: InsertSource::Values(rows),
            on_conflict: None,
            returning: Vec::new(),
        }
    }
}

/// The rows an insert adds.
#[derive(Clone, Debug)]
pub enum InsertSource {
    /// `VALUES (..), (..)`.
    Values(Vec<Vec<Expr>>),
    /// `INSERT .. SELECT`.
    Query(Box<Query>),
    /// One row of defaults: `DEFAULT VALUES` (MySQL: `VALUES ()`).
    DefaultValues,
}

/// What an insert does with a row that conflicts with a unique index.
#[derive(Clone, Debug)]
pub struct OnConflict {
    /// The unique columns (PostgreSQL and SQLite); empty for any unique
    /// index. MySQL looks at every unique index.
    pub target: Vec<Ident>,
    /// Skip or update.
    pub action: ConflictAction,
}

/// The action of [`OnConflict`].
#[derive(Clone, Debug)]
pub enum ConflictAction {
    /// Skip the row: `ON CONFLICT DO NOTHING`, MySQL's `INSERT IGNORE`.
    Ignore,
    /// Update the existing row: `ON CONFLICT (..) DO UPDATE SET .. [WHERE
    /// ..]`, MySQL's `ON DUPLICATE KEY UPDATE` (without a condition).
    Update {
        /// The assignments; [`Expr::Excluded`] is the proposed row's value.
        sets: Vec<Assignment>,
        /// Only rows matching it are updated (PostgreSQL, SQLite).
        filter: Option<Expr>,
    },
}

/// `column = value` in `SET`.
#[derive(Clone, Debug)]
pub struct Assignment {
    /// The column.
    pub column: Ident,
    /// The new value.
    pub value: Expr,
}

impl Assignment {
    /// `column = value`.
    pub fn new(column: impl Into<Ident>, value: impl Into<Expr>) -> Self {
        Assignment {
            column: column.into(),
            value: value.into(),
        }
    }
}

/// An `UPDATE`.
#[derive(Clone, Debug)]
pub struct Update {
    /// The table.
    pub table: ObjectName,
    /// Its alias.
    pub alias: Option<Ident>,
    /// The assignments.
    pub sets: Vec<Assignment>,
    /// Other tables the rows are matched with: `UPDATE .. FROM ..`
    /// (PostgreSQL, SQLite), MySQL's `UPDATE t, a SET ..`; their join
    /// conditions belong in the filter.
    pub from: Vec<Source>,
    /// `WHERE`.
    pub filter: Option<Expr>,
    /// `ORDER BY` (MySQL, one table; `Unsupported` elsewhere).
    pub order_by: Vec<OrderBy>,
    /// `LIMIT` (MySQL, one table; `Unsupported` elsewhere).
    pub limit: Option<u64>,
    /// `RETURNING` (PostgreSQL and SQLite).
    pub returning: Vec<SelectItem>,
}

impl Update {
    /// `UPDATE table SET sets`.
    pub fn new(table: impl Into<ObjectName>, sets: Vec<Assignment>) -> Self {
        Update {
            table: table.into(),
            alias: None,
            sets,
            from: Vec::new(),
            filter: None,
            order_by: Vec::new(),
            limit: None,
            returning: Vec::new(),
        }
    }

    /// The same update with `condition` added to its `WHERE`.
    pub fn filter(mut self, condition: Expr) -> Self {
        self.filter = Some(match self.filter.take() {
            Some(existing) => existing.and(condition),
            None => condition,
        });
        self
    }
}

/// A `DELETE`.
#[derive(Clone, Debug)]
pub struct Delete {
    /// The table.
    pub table: ObjectName,
    /// Its alias.
    pub alias: Option<Ident>,
    /// Other tables the rows are matched with: PostgreSQL's `USING`,
    /// MySQL's `DELETE t FROM t, a`; `Unsupported` on SQLite.
    pub using: Vec<Source>,
    /// `WHERE`.
    pub filter: Option<Expr>,
    /// `ORDER BY` (MySQL, one table).
    pub order_by: Vec<OrderBy>,
    /// `LIMIT` (MySQL, one table).
    pub limit: Option<u64>,
    /// `RETURNING` (PostgreSQL and SQLite).
    pub returning: Vec<SelectItem>,
}

impl Delete {
    /// `DELETE FROM table`.
    pub fn new(table: impl Into<ObjectName>) -> Self {
        Delete {
            table: table.into(),
            alias: None,
            using: Vec::new(),
            filter: None,
            order_by: Vec::new(),
            limit: None,
            returning: Vec::new(),
        }
    }

    /// The same delete with `condition` added to its `WHERE`.
    pub fn filter(mut self, condition: Expr) -> Self {
        self.filter = Some(match self.filter.take() {
            Some(existing) => existing.and(condition),
            None => condition,
        });
        self
    }
}

/// Emptying a table: `TRUNCATE TABLE .. [RESTART IDENTITY] [CASCADE]` on
/// PostgreSQL, `TRUNCATE TABLE` on MySQL, and on SQLite a `DELETE` of every
/// row after resetting the table's `AUTOINCREMENT` sequence.
#[derive(Clone, Debug)]
pub struct Truncate {
    /// The table.
    pub table: ObjectName,
    /// Restart the table's identity (PostgreSQL; MySQL and SQLite always
    /// do).
    pub restart_identity: bool,
    /// Also empty the tables whose foreign keys point at it (PostgreSQL).
    pub cascade: bool,
}

/// An expression.
#[derive(Clone, Debug)]
pub enum Expr {
    /// A column: `"name"`, `"table"."name"`.
    Column(ColumnName),
    /// A bound value: `$1` on PostgreSQL, `?` elsewhere.
    Value(Value),
    /// A constant written into the SQL.
    Literal(Literal),
    /// SQL written by hand with portable placeholders: `?` binds the next
    /// value, `??` is a literal `?`; quoted text is left alone. As an
    /// operand it is parenthesized.
    Raw {
        /// The fragment.
        sql: Cow<'static, str>,
        /// Its values, in order.
        values: Vec<Value>,
    },
    /// SQL with `{}` holes, each filled with the next argument (quoted text
    /// is left alone). As an operand it is parenthesized.
    Template {
        /// The fragment.
        sql: Cow<'static, str>,
        /// The expressions for its holes.
        args: Vec<Expr>,
    },
    /// A function call, `name(args)`; the name must be a plain identifier.
    Function(Function),
    /// `*`, or `table.*`.
    Wildcard(Option<ObjectName>),
    /// `-expr`.
    Neg(Box<Expr>),
    /// `left op right`.
    Binary {
        /// The left operand.
        left: Box<Expr>,
        /// The operator.
        op: BinaryOp,
        /// The right operand.
        right: Box<Expr>,
    },
    /// Every condition (`TRUE` when empty).
    And(Vec<Expr>),
    /// Any of the conditions (`FALSE` when empty).
    Or(Vec<Expr>),
    /// `NOT expr`.
    Not(Box<Expr>),
    /// `expr IS [NOT] NULL`.
    IsNull {
        /// The expression.
        expr: Box<Expr>,
        /// `IS NOT NULL`.
        negated: bool,
    },
    /// `expr [NOT] IN (list)`; an empty list is `1 = 0` (`1 = 1` negated).
    InList {
        /// The expression.
        expr: Box<Expr>,
        /// The values.
        list: Vec<Expr>,
        /// `NOT IN`.
        negated: bool,
    },
    /// `expr [NOT] IN (query)`.
    InQuery {
        /// The expression.
        expr: Box<Expr>,
        /// The query, selecting one column.
        query: Box<Query>,
        /// `NOT IN`.
        negated: bool,
    },
    /// `expr [NOT] BETWEEN low AND high`.
    Between {
        /// The expression.
        expr: Box<Expr>,
        /// The lower bound.
        low: Box<Expr>,
        /// The upper bound.
        high: Box<Expr>,
        /// `NOT BETWEEN`.
        negated: bool,
    },
    /// `expr [NOT] LIKE pattern [ESCAPE 'c']`.
    Like {
        /// The text.
        expr: Box<Expr>,
        /// The pattern (`%` and `_` wildcards).
        pattern: Box<Expr>,
        /// The escape character.
        escape: Option<char>,
        /// `NOT LIKE`.
        negated: bool,
        /// Letter case: the database's own, ignored, or matched.
        case: LikeCase,
    },
    /// `[NOT] EXISTS (query)`.
    Exists {
        /// The query.
        query: Box<Query>,
        /// `NOT EXISTS`.
        negated: bool,
    },
    /// `CAST(expr AS type)`.
    Cast {
        /// The expression.
        expr: Box<Expr>,
        /// The type.
        ty: SqlType,
    },
    /// A scalar subquery, `(SELECT ..)`.
    Subquery(Box<Query>),
    /// A row value, `(a, b)`.
    Tuple(Vec<Expr>),
    /// `CASE [operand] WHEN .. THEN .. [ELSE ..] END`.
    Case {
        /// The value compared with each `WHEN` (a simple `CASE`).
        operand: Option<Box<Expr>>,
        /// `WHEN condition THEN result` pairs.
        whens: Vec<(Expr, Expr)>,
        /// `ELSE result`.
        otherwise: Option<Box<Expr>>,
    },
    /// The value at a JSON path inside `expr`: PostgreSQL's `->` / `->>`,
    /// MySQL's `json_extract` (`json_unquote` for text), SQLite's
    /// `json_extract` (text) or `->` (JSON). Negative indexes count from
    /// the end.
    JsonPath {
        /// The JSON expression.
        expr: Box<Expr>,
        /// The path.
        path: Vec<JsonSegment>,
        /// As text (for comparisons) or as JSON.
        mode: JsonMode,
    },
    /// In an upsert's update: the value the conflicting row proposed for
    /// `column` (`excluded.column`, MySQL's `VALUES(column)`).
    Excluded(Ident),
    /// A window function: `function OVER (PARTITION BY .. ORDER BY ..)`.
    Window {
        /// The function (`ROW_NUMBER()`, an aggregate).
        function: Box<Expr>,
        /// `PARTITION BY`.
        partition_by: Vec<Expr>,
        /// `ORDER BY` (MySQL sorts NULLs with a leading `IS NULL` key).
        order_by: Vec<OrderBy>,
    },
    /// A random number for random order: `RANDOM()`, MySQL's `RAND()`.
    Random,
    /// `DEFAULT`, the column's default (in `VALUES` and `SET`; SQLite has
    /// none in `VALUES`).
    Default,
}

/// A column reference.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ColumnName {
    /// The table (or alias) qualifying it.
    pub table: Option<ObjectName>,
    /// The column.
    pub name: Ident,
}

/// A function call.
#[derive(Clone, Debug)]
pub struct Function {
    /// The name, written as is: a plain identifier (`COUNT`,
    /// `json_extract`, `public.my_fn`).
    pub name: Cow<'static, str>,
    /// The arguments (`*` is [`Expr::Wildcard`]).
    pub args: Vec<Expr>,
    /// `DISTINCT` before the arguments (aggregates).
    pub distinct: bool,
}

/// A constant written into the SQL.
#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    /// `NULL`.
    Null,
    /// `TRUE` / `FALSE`.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A string, quoted and escaped for the dialect.
    String(Cow<'static, str>),
}

/// A binary operator.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    /// `=`.
    Eq,
    /// `<>`.
    NotEq,
    /// `<`.
    Lt,
    /// `<=`.
    LtEq,
    /// `>`.
    Gt,
    /// `>=`.
    GtEq,
    /// Null-safe equality: `IS NOT DISTINCT FROM`, MySQL's `<=>`, SQLite's
    /// `IS`.
    NullSafeEq,
    /// Null-safe inequality: `IS DISTINCT FROM`, MySQL's `NOT (.. <=> ..)`,
    /// SQLite's `IS NOT`.
    NullSafeNotEq,
    /// `+`.
    Add,
    /// `-`.
    Sub,
    /// `*`.
    Mul,
    /// `/`.
    Div,
    /// `%`.
    Mod,
    /// `&`.
    BitAnd,
    /// `|`.
    BitOr,
    /// Exclusive or: PostgreSQL's `#`, MySQL's `^`, `(a | b) - (a & b)` on
    /// SQLite.
    BitXor,
    /// `<<`.
    ShiftLeft,
    /// `>>`.
    ShiftRight,
    /// Text concatenation: `||`, MySQL's `CONCAT(a, b)`.
    Concat,
    /// An operator written as is (`@>`, `GLOB`, `SOUNDS LIKE`); its
    /// operands are parenthesized unless they are atoms.
    Custom(Cow<'static, str>),
}

/// How [`Expr::Like`] treats letter case.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LikeCase {
    /// The database's own `LIKE`: case-sensitive on PostgreSQL, the
    /// column's collation on MySQL, ASCII case ignored on SQLite.
    #[default]
    Database,
    /// Case ignored: PostgreSQL's `ILIKE`, `LOWER(..) LIKE LOWER(..)` on
    /// MySQL, SQLite's `LIKE`.
    Ignore,
    /// Case matched: `LIKE` on PostgreSQL, `LIKE BINARY` on MySQL, a
    /// `GLOB` of the converted pattern on SQLite.
    Match,
}

/// How [`Expr::JsonPath`] reads the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JsonMode {
    /// As text: a string without its quotes, other values as their JSON.
    Text,
    /// As JSON.
    Json,
}

/// A type for [`Expr::Cast`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SqlType {
    /// Text: `TEXT`, MySQL's `CHAR`.
    Text,
    /// A 64-bit integer: `BIGINT`, MySQL's `SIGNED`, SQLite's `INTEGER`.
    Integer,
    /// A double: `DOUBLE PRECISION`, MySQL's `DOUBLE`, SQLite's `REAL`.
    Double,
    /// An exact number: `NUMERIC(p, s)`, MySQL's `DECIMAL(p, s)`, SQLite's
    /// `NUMERIC`.
    Decimal {
        /// Digits in all.
        precision: u8,
        /// Digits after the point.
        scale: u8,
    },
    /// A date (SQLite: `date(..)`).
    Date,
    /// A time of day (SQLite: `time(..)`).
    Time,
    /// A date-time without time zone: `TIMESTAMP`, MySQL's `DATETIME(6)`
    /// (SQLite: `datetime(..)`).
    DateTime,
    /// JSON: `JSONB`, MySQL's `JSON` (SQLite: `json(..)`).
    Json,
    /// A type written as is (letters, digits, `_`, spaces, parentheses and
    /// commas): `INTERVAL`, `vector`, `DATETIME(6)`.
    Custom(Cow<'static, str>),
}

impl From<Value> for Expr {
    fn from(value: Value) -> Self {
        Expr::Value(value)
    }
}

impl From<Literal> for Expr {
    fn from(literal: Literal) -> Self {
        Expr::Literal(literal)
    }
}

impl From<ColumnName> for Expr {
    fn from(column: ColumnName) -> Self {
        Expr::Column(column)
    }
}

impl From<Query> for Expr {
    fn from(query: Query) -> Self {
        Expr::Subquery(Box::new(query))
    }
}

impl Expr {
    /// The column `name`, or `table.name` when it has a dot (split at the
    /// last one: `audit.events.id` is column `id` of `audit.events`).
    pub fn col(name: impl Into<Cow<'static, str>>) -> Expr {
        let name = name.into();
        match name.rsplit_once('.') {
            Some((table, column)) => Expr::qualified(table.to_owned(), column.to_owned()),
            None => Expr::Column(ColumnName {
                table: None,
                name: Ident(name),
            }),
        }
    }

    /// Column `name` of `table` (`table` split at its dots).
    pub fn qualified(table: impl Into<ObjectName>, name: impl Into<Ident>) -> Expr {
        Expr::Column(ColumnName {
            table: Some(table.into()),
            name: name.into(),
        })
    }

    /// A bound value.
    pub fn value(value: impl Into<Value>) -> Expr {
        Expr::Value(value.into())
    }

    /// An integer written into the SQL.
    pub fn int(value: i64) -> Expr {
        Expr::Literal(Literal::Int(value))
    }

    /// A string written into the SQL, quoted and escaped for the dialect.
    pub fn string(text: impl Into<Cow<'static, str>>) -> Expr {
        Expr::Literal(Literal::String(text.into()))
    }

    /// `TRUE` or `FALSE`.
    pub fn bool(value: bool) -> Expr {
        Expr::Literal(Literal::Bool(value))
    }

    /// `NULL`.
    pub fn null() -> Expr {
        Expr::Literal(Literal::Null)
    }

    /// SQL written by hand, `?` binding `values` in order.
    pub fn raw(
        sql: impl Into<Cow<'static, str>>,
        values: impl IntoIterator<Item = impl Into<Value>>,
    ) -> Expr {
        Expr::Raw {
            sql: sql.into(),
            values: values.into_iter().map(Into::into).collect(),
        }
    }

    /// SQL with `{}` holes filled by `args`.
    pub fn template(sql: impl Into<Cow<'static, str>>, args: Vec<Expr>) -> Expr {
        Expr::Template {
            sql: sql.into(),
            args,
        }
    }

    /// `name(args)`.
    pub fn func(name: impl Into<Cow<'static, str>>, args: Vec<Expr>) -> Expr {
        Expr::Function(Function {
            name: name.into(),
            args,
            distinct: false,
        })
    }

    /// `name(DISTINCT args)`.
    pub fn func_distinct(name: impl Into<Cow<'static, str>>, args: Vec<Expr>) -> Expr {
        Expr::Function(Function {
            name: name.into(),
            args,
            distinct: true,
        })
    }

    /// `COUNT(*)`.
    pub fn count_all() -> Expr {
        Expr::func("COUNT", vec![Expr::Wildcard(None)])
    }

    /// `ROW_NUMBER()`, for [`over`](Expr::over).
    pub fn row_number() -> Expr {
        Expr::func("ROW_NUMBER", Vec::new())
    }

    /// `self OVER (PARTITION BY partition ORDER BY order)`.
    pub fn over(self, partition_by: Vec<Expr>, order_by: Vec<OrderBy>) -> Expr {
        Expr::Window {
            function: Box::new(self),
            partition_by,
            order_by,
        }
    }

    /// `left op right`.
    pub fn binary(self, op: BinaryOp, right: impl Into<Expr>) -> Expr {
        Expr::Binary {
            left: Box::new(self),
            op,
            right: Box::new(right.into()),
        }
    }

    /// `self = other`.
    pub fn eq(self, other: impl Into<Expr>) -> Expr {
        self.binary(BinaryOp::Eq, other)
    }

    /// `self <> other`.
    pub fn ne(self, other: impl Into<Expr>) -> Expr {
        self.binary(BinaryOp::NotEq, other)
    }

    /// `self < other`.
    pub fn lt(self, other: impl Into<Expr>) -> Expr {
        self.binary(BinaryOp::Lt, other)
    }

    /// `self <= other`.
    pub fn lte(self, other: impl Into<Expr>) -> Expr {
        self.binary(BinaryOp::LtEq, other)
    }

    /// `self > other`.
    pub fn gt(self, other: impl Into<Expr>) -> Expr {
        self.binary(BinaryOp::Gt, other)
    }

    /// `self >= other`.
    pub fn gte(self, other: impl Into<Expr>) -> Expr {
        self.binary(BinaryOp::GtEq, other)
    }

    /// `self + other`.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, other: impl Into<Expr>) -> Expr {
        self.binary(BinaryOp::Add, other)
    }

    /// `self - other`.
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, other: impl Into<Expr>) -> Expr {
        self.binary(BinaryOp::Sub, other)
    }

    /// `self * other`.
    #[allow(clippy::should_implement_trait)]
    pub fn mul(self, other: impl Into<Expr>) -> Expr {
        self.binary(BinaryOp::Mul, other)
    }

    /// `self / other`.
    #[allow(clippy::should_implement_trait)]
    pub fn div(self, other: impl Into<Expr>) -> Expr {
        self.binary(BinaryOp::Div, other)
    }

    /// Both conditions (flattening nested `AND`s).
    pub fn and(self, other: Expr) -> Expr {
        let mut items = match self {
            Expr::And(items) => items,
            single => vec![single],
        };
        match other {
            Expr::And(more) => items.extend(more),
            single => items.push(single),
        }
        Expr::And(items)
    }

    /// Either condition (flattening nested `OR`s).
    pub fn or(self, other: Expr) -> Expr {
        let mut items = match self {
            Expr::Or(items) => items,
            single => vec![single],
        };
        match other {
            Expr::Or(more) => items.extend(more),
            single => items.push(single),
        }
        Expr::Or(items)
    }

    /// `NOT self`.
    #[allow(clippy::should_implement_trait)]
    pub fn not(self) -> Expr {
        Expr::Not(Box::new(self))
    }

    /// `self IS NULL`.
    pub fn is_null(self) -> Expr {
        Expr::IsNull {
            expr: Box::new(self),
            negated: false,
        }
    }

    /// `self IS NOT NULL`.
    pub fn is_not_null(self) -> Expr {
        Expr::IsNull {
            expr: Box::new(self),
            negated: true,
        }
    }

    /// `self IN (list)`.
    pub fn in_list(self, list: Vec<Expr>) -> Expr {
        Expr::InList {
            expr: Box::new(self),
            list,
            negated: false,
        }
    }

    /// `self NOT IN (list)`.
    pub fn not_in_list(self, list: Vec<Expr>) -> Expr {
        Expr::InList {
            expr: Box::new(self),
            list,
            negated: true,
        }
    }

    /// `self IN (query)`.
    pub fn in_query(self, query: impl Into<Query>) -> Expr {
        Expr::InQuery {
            expr: Box::new(self),
            query: Box::new(query.into()),
            negated: false,
        }
    }

    /// `self BETWEEN low AND high`.
    pub fn between(self, low: impl Into<Expr>, high: impl Into<Expr>) -> Expr {
        Expr::Between {
            expr: Box::new(self),
            low: Box::new(low.into()),
            high: Box::new(high.into()),
            negated: false,
        }
    }

    /// `self LIKE pattern` with the database's letter case.
    pub fn like(self, pattern: impl Into<Expr>) -> Expr {
        Expr::Like {
            expr: Box::new(self),
            pattern: Box::new(pattern.into()),
            escape: None,
            negated: false,
            case: LikeCase::Database,
        }
    }

    /// `EXISTS (query)`.
    pub fn exists(query: impl Into<Query>) -> Expr {
        Expr::Exists {
            query: Box::new(query.into()),
            negated: false,
        }
    }

    /// `CAST(self AS ty)`.
    pub fn cast(self, ty: SqlType) -> Expr {
        Expr::Cast {
            expr: Box::new(self),
            ty,
        }
    }

    /// The JSON value at `path` inside `self`, as text.
    pub fn json_text(self, path: Vec<JsonSegment>) -> Expr {
        Expr::JsonPath {
            expr: Box::new(self),
            path,
            mode: JsonMode::Text,
        }
    }

    /// The JSON value at `path` inside `self`, as JSON.
    pub fn json(self, path: Vec<JsonSegment>) -> Expr {
        Expr::JsonPath {
            expr: Box::new(self),
            path,
            mode: JsonMode::Json,
        }
    }

    /// `self ASC`.
    pub fn asc(self) -> OrderBy {
        OrderBy {
            expr: self,
            descending: false,
            nulls: None,
        }
    }

    /// `self DESC`.
    pub fn desc(self) -> OrderBy {
        OrderBy {
            expr: self,
            descending: true,
            nulls: None,
        }
    }

    /// `CASE WHEN .. THEN .. ELSE .. END`.
    pub fn case(whens: Vec<(Expr, Expr)>, otherwise: Option<Expr>) -> Expr {
        Expr::Case {
            operand: None,
            whens,
            otherwise: otherwise.map(Box::new),
        }
    }
}
