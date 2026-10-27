//! The driver-neutral query representation (IR).
//!
//! Every builder ([`Builder`](crate::Builder),
//! [`TableBuilder`](crate::TableBuilder)) produces a [`QueryIr`]; writes
//! produce an [`InsertIr`], [`UpdateIr`] or [`DeleteIr`]. The SQL compiler
//! ([`crate::sql`]) turns them into statements for each dialect, and a
//! [`DocumentDriver`](crate::DocumentDriver) turns them into document
//! commands. Apps rarely touch the IR; drivers read it.
//!
//! The enums are `#[non_exhaustive]`: a driver answers a variant it cannot
//! express with [`OrmError::Unsupported`](crate::OrmError::Unsupported).

use std::borrow::Cow;
use std::fmt;

use crate::value::Value;

/// A column, optionally qualified by its table, optionally followed by a
/// JSON path (`meta->status`, `meta->tags->0`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ColumnRef {
    /// The table (or alias) qualifying the column.
    pub table: Option<Cow<'static, str>>,
    /// The column name.
    pub name: Cow<'static, str>,
    /// The JSON path into the column's value; empty for the column itself.
    pub path: Vec<JsonSegment>,
}

/// One step of a JSON path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum JsonSegment {
    /// An object key.
    Key(String),
    /// An array index (negative counts from the end where the database
    /// supports it).
    Index(i64),
}

impl ColumnRef {
    /// `name` in `table` (or unqualified), without a JSON path.
    pub fn new(
        table: Option<impl Into<Cow<'static, str>>>,
        name: impl Into<Cow<'static, str>>,
    ) -> Self {
        ColumnRef {
            table: table.map(Into::into),
            name: name.into(),
            path: Vec::new(),
        }
    }

    /// Parses `column`, `table.column` and either followed by a JSON path:
    /// `meta->status`, `deals.meta->tags->0`, `meta->items[2]`.
    ///
    /// ```
    /// use navree_orm::ir::{ColumnRef, JsonSegment};
    ///
    /// let column = ColumnRef::parse("deals.meta->tags[1]->name");
    /// assert_eq!(column.table.as_deref(), Some("deals"));
    /// assert_eq!(column.name, "meta");
    /// assert_eq!(
    ///     column.path,
    ///     [
    ///         JsonSegment::Key("tags".into()),
    ///         JsonSegment::Index(1),
    ///         JsonSegment::Key("name".into()),
    ///     ]
    /// );
    /// ```
    pub fn parse(text: impl Into<Cow<'static, str>>) -> Self {
        let text = text.into();
        let (head, path) = match text.split_once("->") {
            Some((head, rest)) => (head.to_owned(), parse_json_path(rest)),
            None if text.trim_end().ends_with(']') && text.contains('[') => {
                // `tags[1]`: an index into the column itself.
                (text.to_string(), Vec::new())
            }
            None => {
                return Self::parse_head(text);
            }
        };
        let (head, mut indexes) = match head.find('[') {
            Some(at) if head.trim_end().ends_with(']') => {
                (head[..at].to_owned(), parse_json_path(&head[at..]))
            }
            _ => (head, Vec::new()),
        };
        let mut column = Self::parse_head(Cow::Owned(head));
        indexes.extend(path);
        column.path = indexes;
        column
    }

    fn parse_head(head: Cow<'static, str>) -> Self {
        match head.rsplit_once('.') {
            Some((table, name)) => ColumnRef {
                table: Some(Cow::Owned(table.trim().to_owned())),
                name: Cow::Owned(name.trim().to_owned()),
                path: Vec::new(),
            },
            None => ColumnRef {
                table: None,
                name: match head {
                    Cow::Borrowed(name) => Cow::Borrowed(name.trim()),
                    Cow::Owned(name) => Cow::Owned(name.trim().to_owned()),
                },
                path: Vec::new(),
            },
        }
    }

    /// The same column without its JSON path.
    pub fn without_path(&self) -> Self {
        ColumnRef {
            table: self.table.clone(),
            name: self.name.clone(),
            path: Vec::new(),
        }
    }
}

/// Parses `a->b[0]->1` into segments: keys, and indexes written as `[n]`
/// or as a whole-number segment.
fn parse_json_path(path: &str) -> Vec<JsonSegment> {
    let mut segments = Vec::new();
    for part in path.split("->") {
        let part = part.trim().trim_matches(|c| c == '\'' || c == '"');
        let (key, indexes) = match part.find('[') {
            Some(at) if part.ends_with(']') => (&part[..at], &part[at..]),
            _ => (part, ""),
        };
        if !key.is_empty() {
            match key.parse::<i64>() {
                Ok(index) => segments.push(JsonSegment::Index(index)),
                Err(_) => segments.push(JsonSegment::Key(key.to_owned())),
            }
        }
        for index in indexes.split(['[', ']']).filter(|piece| !piece.is_empty()) {
            match index.trim().parse::<i64>() {
                Ok(index) => segments.push(JsonSegment::Index(index)),
                Err(_) => segments.push(JsonSegment::Key(index.to_owned())),
            }
        }
    }
    segments
}

impl fmt::Display for ColumnRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(table) = &self.table {
            write!(f, "{table}.")?;
        }
        f.write_str(&self.name)?;
        for segment in &self.path {
            match segment {
                JsonSegment::Key(key) => write!(f, "->{key}")?,
                JsonSegment::Index(index) => write!(f, "->{index}")?,
            }
        }
        Ok(())
    }
}

/// An expression: a column, a value, raw SQL, a function call, an
/// aggregate, arithmetic, a subquery, or a TimescaleDB helper.
///
/// Build one with the constructors in [`crate::expr`] (`Expr::col`,
/// `Expr::count`, `Expr::time_bucket`, ...) and compare it into a
/// [`Filter`](crate::Filter) with `eq`, `gt`, ...
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Expr {
    /// A column.
    Column(ColumnRef),
    /// A bound value.
    Value(Value),
    /// Raw SQL with `?` placeholders for `values` (`??` is a literal `?`).
    /// SQL drivers only.
    Raw {
        /// The SQL fragment.
        sql: Cow<'static, str>,
        /// The values bound to its placeholders, in order.
        values: Vec<Value>,
    },
    /// A function call, e.g. `lower(name)`.
    Function {
        /// The function's name.
        name: Cow<'static, str>,
        /// Its arguments.
        args: Vec<Expr>,
    },
    /// An aggregate: `count(*)`, `sum(amount)`, `count(DISTINCT owner_id)`.
    Aggregate {
        /// The function.
        function: AggregateFunction,
        /// The argument; `None` is `*`.
        arg: Option<Box<Expr>>,
        /// `DISTINCT` inside the call.
        distinct: bool,
    },
    /// `left op right` with an arithmetic operator.
    Arithmetic {
        /// The left operand.
        left: Box<Expr>,
        /// The operator.
        op: ArithmeticOp,
        /// The right operand.
        right: Box<Expr>,
    },
    /// A subquery returning one value per row.
    Subquery(Box<QueryIr>),
    /// TimescaleDB's `time_bucket(width, expr [, origin])`; fixed widths
    /// are emulated on databases without it.
    TimeBucket {
        /// The bucket width, an interval such as `1 day` or `15 minutes`.
        width: String,
        /// The time column or expression.
        expr: Box<Expr>,
        /// Where buckets start; TimescaleDB's default when `None`.
        origin: Option<Box<Expr>>,
    },
    /// TimescaleDB's `first(value, time)`: the value at the earliest time.
    First {
        /// The value column.
        value: Box<Expr>,
        /// The time column.
        time: Box<Expr>,
    },
    /// TimescaleDB's `last(value, time)`: the value at the latest time.
    Last {
        /// The value column.
        value: Box<Expr>,
        /// The time column.
        time: Box<Expr>,
    },
    /// `*`, or `table.*`.
    All {
        /// The table, for `table.*`.
        table: Option<Cow<'static, str>>,
    },
    /// In an upsert's update ([`OnConflict::Update`]): the value column
    /// `column` would have been inserted with (`excluded.column` on
    /// PostgreSQL and SQLite, `VALUES(column)` on MySQL).
    Excluded {
        /// The column.
        column: Cow<'static, str>,
    },
    /// `left op right` with an operator written by hand (`||`, `-`, `#-`,
    /// `->`, ...), checked against the dialect's list of value operators
    /// when the query compiles. SQL drivers only.
    Binary {
        /// The left operand.
        left: Box<Expr>,
        /// The operator.
        op: Cow<'static, str>,
        /// The right operand.
        right: Box<Expr>,
    },
    /// The distance between the vector in `expr` (an embedding column) and
    /// `vector`: pgvector's `<=>` (cosine), `<->` (Euclidean) or `<#>`
    /// (negative inner product) on PostgreSQL, MariaDB's
    /// `VEC_DISTANCE_COSINE` / `VEC_DISTANCE_EUCLIDEAN` on MySQL;
    /// unsupported on SQLite.
    VectorDistance {
        /// The vector column or expression.
        expr: Box<Expr>,
        /// The vector compared with.
        vector: Vec<f32>,
        /// How distance is measured.
        metric: VectorMetric,
    },
}

/// How a vector distance is measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VectorMetric {
    /// Cosine distance, `1 - cosine similarity` (0 for the same direction,
    /// 2 for opposite ones): the usual measure for text embeddings.
    #[default]
    Cosine,
    /// Euclidean (L2) distance.
    Euclidean,
    /// The negative inner product (PostgreSQL only): smaller is more
    /// similar, for normalized embeddings like cosine but faster.
    InnerProduct,
}

/// An aggregate function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AggregateFunction {
    /// `COUNT`.
    Count,
    /// `SUM`.
    Sum,
    /// `AVG`.
    Avg,
    /// `MIN`.
    Min,
    /// `MAX`.
    Max,
}

impl AggregateFunction {
    /// The SQL name.
    pub fn sql_name(self) -> &'static str {
        match self {
            AggregateFunction::Count => "COUNT",
            AggregateFunction::Sum => "SUM",
            AggregateFunction::Avg => "AVG",
            AggregateFunction::Min => "MIN",
            AggregateFunction::Max => "MAX",
        }
    }
}

/// An arithmetic operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ArithmeticOp {
    /// `+`.
    Add,
    /// `-`.
    Sub,
    /// `*`.
    Mul,
    /// `/`.
    Div,
}

/// A comparison operator.
///
/// Built from the usual spellings with `From<&str>`: `=`, `!=` / `<>`,
/// `<`, `<=`, `>`, `>=`, `like`, `not like`, `ilike`, `not ilike`, `<=>`
/// (null-safe equality), `&`, `|`, `^`, `<<`, `>>`, `regexp`, `not
/// regexp`, and PostgreSQL's `~`, `~*`, `!~`, `!~*`, `similar to`, `not
/// similar to`, `@>`, `<@`, `?`, `?|`, `?&` and `&&`. Anything else is
/// [`Operator::Invalid`], reported when the query runs.
///
/// ```
/// use navree_orm::ir::Operator;
///
/// assert_eq!(Operator::from(">="), Operator::Gte);
/// assert_eq!(Operator::from("<>"), Operator::Ne);
/// assert_eq!(Operator::from("NOT LIKE"), Operator::NotLike);
/// assert!(matches!(Operator::from("=>"), Operator::Invalid(_)));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Operator {
    /// `=`.
    Eq,
    /// `<>`.
    Ne,
    /// `<`.
    Lt,
    /// `<=`.
    Lte,
    /// `>`.
    Gt,
    /// `>=`.
    Gte,
    /// `LIKE` as the database defines it.
    Like,
    /// `NOT LIKE`.
    NotLike,
    /// Case-insensitive `LIKE` (`ILIKE` on PostgreSQL, `LOWER(..) LIKE
    /// LOWER(..)` elsewhere).
    ILike,
    /// Case-insensitive `NOT LIKE`.
    NotILike,
    /// Null-safe equality (`IS NOT DISTINCT FROM`, `<=>`, `IS`).
    NullSafeEq,
    /// `&`.
    BitAnd,
    /// `|`.
    BitOr,
    /// `^` (`#` on PostgreSQL).
    BitXor,
    /// `<<`.
    ShiftLeft,
    /// `>>`.
    ShiftRight,
    /// Regular-expression match, case-sensitive (`~` on PostgreSQL,
    /// `REGEXP_LIKE(.., 'c')` on MySQL, `REGEXP` on SQLite).
    Regexp,
    /// Regular-expression mismatch, case-sensitive.
    NotRegexp,
    /// Regular-expression match ignoring case (`~*` on PostgreSQL,
    /// `REGEXP_LIKE(.., 'i')` on MySQL, `REGEXP` with `(?i)` on SQLite).
    RegexpIgnoreCase,
    /// Regular-expression mismatch ignoring case.
    NotRegexpIgnoreCase,
    /// Null-safe inequality (`IS DISTINCT FROM`, `NOT <=>`, `IS NOT`): true
    /// when exactly one side is NULL.
    NullSafeNe,
    /// A PostgreSQL operator written as is (`~*`, `@>`, `?|`, ...).
    Postgres(&'static str),
    /// An operator written by hand ([`Filter::op_raw`](crate::Filter::op_raw)),
    /// checked against the dialect's list of condition operators when the
    /// query compiles.
    Raw(String),
    /// An operator that is not recognised; the query fails when it runs.
    Invalid(String),
}

/// The PostgreSQL-only operators [`Operator::from`] recognises.
const POSTGRES_OPERATORS: [&str; 13] = [
    "~*",
    "!~*",
    "~",
    "!~",
    "similar to",
    "not similar to",
    "@>",
    "<@",
    "?",
    "?|",
    "?&",
    "&&",
    "||",
];

impl From<&str> for Operator {
    fn from(text: &str) -> Self {
        let normalized = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        match normalized.as_str() {
            "=" | "==" => Operator::Eq,
            "!=" | "<>" => Operator::Ne,
            "<" => Operator::Lt,
            "<=" => Operator::Lte,
            ">" => Operator::Gt,
            ">=" => Operator::Gte,
            "like" => Operator::Like,
            "not like" => Operator::NotLike,
            "ilike" => Operator::ILike,
            "not ilike" => Operator::NotILike,
            "<=>" | "is not distinct from" => Operator::NullSafeEq,
            "is distinct from" => Operator::NullSafeNe,
            "&" => Operator::BitAnd,
            "|" => Operator::BitOr,
            "^" => Operator::BitXor,
            "<<" => Operator::ShiftLeft,
            ">>" => Operator::ShiftRight,
            "regexp" | "rlike" => Operator::Regexp,
            "not regexp" | "not rlike" => Operator::NotRegexp,
            other => match POSTGRES_OPERATORS.iter().find(|op| **op == other) {
                Some(op) => Operator::Postgres(op),
                None => Operator::Invalid(text.to_owned()),
            },
        }
    }
}

impl From<String> for Operator {
    fn from(text: String) -> Self {
        Operator::from(text.as_str())
    }
}

/// A part of a date or date-time, for `where_date` and friends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DatePart {
    /// The date (`YYYY-MM-DD`).
    Date,
    /// The time of day (`HH:MM:SS`).
    Time,
    /// The day of the month.
    Day,
    /// The month.
    Month,
    /// The year.
    Year,
}

/// How a clause joins the clauses before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Boolean {
    /// `AND`.
    And,
    /// `OR`.
    Or,
}

/// One clause of a `WHERE` (or `HAVING`, or join `ON`) list.
///
/// Clauses combine with SQL's precedence: `a AND b OR c AND d` is
/// `(a AND b) OR (c AND d)`.
#[derive(Clone, Debug)]
pub struct WhereClause {
    /// How the clause joins the ones before it (ignored for the first).
    pub boolean: Boolean,
    /// `NOT (...)`.
    pub negated: bool,
    /// The condition.
    pub filter: Where,
}

impl WhereClause {
    /// An `AND` clause.
    pub fn and(filter: Where) -> Self {
        WhereClause {
            boolean: Boolean::And,
            negated: false,
            filter,
        }
    }

    /// An `OR` clause.
    pub fn or(filter: Where) -> Self {
        WhereClause {
            boolean: Boolean::Or,
            negated: false,
            filter,
        }
    }
}

/// A condition.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Where {
    /// `left op right`.
    Compare {
        /// The left side.
        left: Expr,
        /// The operator.
        op: Operator,
        /// The right side.
        right: Expr,
    },
    /// `expr IS [NOT] NULL`.
    Null {
        /// The expression.
        expr: Expr,
        /// `IS NOT NULL`.
        not: bool,
    },
    /// `expr [NOT] IN (...)`.
    In {
        /// The expression.
        expr: Expr,
        /// The list or subquery.
        values: InValues,
        /// `NOT IN`.
        not: bool,
    },
    /// `expr [NOT] BETWEEN low AND high`.
    Between {
        /// The expression.
        expr: Expr,
        /// The lower bound.
        low: Expr,
        /// The upper bound.
        high: Expr,
        /// `NOT BETWEEN`.
        not: bool,
    },
    /// A `LIKE` pattern match (`%` and `_` wildcards).
    Like {
        /// The expression.
        expr: Expr,
        /// The pattern.
        pattern: Expr,
        /// Case-sensitive on every database (`LIKE BINARY` on MySQL, `GLOB`
        /// on SQLite) instead of case-insensitive on every database.
        case_sensitive: bool,
        /// `NOT LIKE`.
        not: bool,
    },
    /// A comparison of part of a date-time: `date(created_at) = ?`,
    /// `month(created_at) = ?`, ...
    DatePart {
        /// The part.
        part: DatePart,
        /// The date or date-time expression.
        expr: Expr,
        /// The operator.
        op: Operator,
        /// The value compared with (a date, a time, or a number).
        value: Expr,
    },
    /// `[NOT] EXISTS (subquery)`.
    Exists {
        /// The subquery.
        query: Box<QueryIr>,
        /// `NOT EXISTS`.
        not: bool,
    },
    /// A parenthesised group of clauses.
    Group(Vec<WhereClause>),
    /// The JSON column (or path) contains `value` (an array element, or an
    /// object's keys and values).
    JsonContains {
        /// The column and path.
        column: ColumnRef,
        /// The value looked for.
        value: serde_json::Value,
        /// Does not contain.
        not: bool,
    },
    /// The JSON at the column (or path) is contained in `value`: every
    /// element (or key and value) it has, `value` has too (`<@` on
    /// PostgreSQL, `json_contains(value, column)` on MySQL).
    JsonContainedBy {
        /// The column and path.
        column: ColumnRef,
        /// The value it must fit in.
        value: serde_json::Value,
        /// Is not contained.
        not: bool,
    },
    /// The JSON array at the column (or path) shares an element with
    /// `value` (an array; a scalar stands for a one-element array).
    JsonOverlaps {
        /// The column and path.
        column: ColumnRef,
        /// The elements looked for.
        value: serde_json::Value,
        /// Shares none.
        not: bool,
    },
    /// The JSON column has the object key (or array index) its path ends
    /// with: `meta->owner` has `owner`, `meta->tags[2]` a third tag.
    JsonContainsKey {
        /// The column, with the path whose last step is looked for.
        column: ColumnRef,
        /// Does not have it.
        not: bool,
    },
    /// The length of the JSON array at the column (or path), compared.
    JsonLength {
        /// The column and path.
        column: ColumnRef,
        /// The operator.
        op: Operator,
        /// The length compared with.
        length: i64,
    },
    /// A full-text match of `query` against `columns`.
    FullText {
        /// The columns searched.
        columns: Vec<Expr>,
        /// The search text.
        query: String,
        /// The mode and language.
        options: FullText,
    },
    /// Raw SQL with `?` placeholders (`??` is a literal `?`). SQL drivers
    /// only.
    Raw {
        /// The SQL fragment.
        sql: Cow<'static, str>,
        /// Its values, in order.
        values: Vec<Value>,
    },
    /// A sea-query condition (navree-access SQL filters, navree-db's text
    /// helpers). SQL drivers only.
    Sql(sea_query::Condition),
    /// A condition that could not be built (a value its column could not
    /// encode); the query fails with this message when it runs.
    Invalid(String),
}

/// How a full-text condition matches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FullTextMode {
    /// Words, in any order (`plainto_tsquery`; MySQL's natural language
    /// mode).
    #[default]
    Natural,
    /// The words as a phrase (`phraseto_tsquery`; natural language mode
    /// on MySQL).
    Phrase,
    /// Search-engine syntax: quotes, `or`, `-word` (`websearch_to_tsquery`;
    /// boolean mode on MySQL).
    Websearch,
    /// MySQL's boolean mode (`+word -word "phrase"`); websearch syntax on
    /// PostgreSQL.
    Boolean,
    /// The text search query syntax itself (`to_tsquery`: `'fat & (rat |
    /// cat)'`); boolean mode on MySQL. The text must be a valid query.
    Raw,
}

/// The options of a full-text condition
/// ([`where_full_text_with`](crate::Builder::where_full_text_with)).
///
/// ```
/// use navree_orm::ir::{FullText, FullTextMode};
///
/// let options = FullText::websearch().language("german");
/// assert_eq!(options.mode, FullTextMode::Websearch);
/// assert_eq!(options.language.as_deref(), Some("german"));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FullText {
    /// How the text matches.
    pub mode: FullTextMode,
    /// PostgreSQL's text search configuration (`english` by default); one
    /// of PostgreSQL's built-in languages.
    pub language: Option<String>,
    /// MySQL's `WITH QUERY EXPANSION` (natural language mode).
    pub expanded: bool,
    /// PostgreSQL: the columns hold `tsvector`s already (a generated
    /// column, indexed), so they are matched as they are instead of
    /// through `to_tsvector`.
    pub vectors: bool,
}

impl FullText {
    /// Words in English (the default language), in any order.
    pub fn english() -> Self {
        FullText::default().language("english")
    }

    /// PostgreSQL's text search syntax (`to_tsquery`), boolean mode on
    /// MySQL.
    pub fn raw() -> Self {
        FullText {
            mode: FullTextMode::Raw,
            ..FullText::default()
        }
    }

    /// The same options matching `tsvector` columns as they are
    /// (PostgreSQL).
    ///
    /// ```
    /// use navree_orm::ir::FullText;
    ///
    /// let options = FullText::english().websearch_mode().vectors();
    /// assert!(options.vectors);
    /// ```
    pub fn vectors(mut self) -> Self {
        self.vectors = true;
        self
    }

    /// The same options in search-engine syntax (`websearch_to_tsquery`).
    pub fn websearch_mode(mut self) -> Self {
        self.mode = FullTextMode::Websearch;
        self
    }

    /// The same options matching the words as a phrase.
    pub fn phrase_mode(mut self) -> Self {
        self.mode = FullTextMode::Phrase;
        self
    }

    /// The words as a phrase.
    pub fn phrase() -> Self {
        FullText {
            mode: FullTextMode::Phrase,
            ..FullText::default()
        }
    }

    /// Search-engine syntax.
    pub fn websearch() -> Self {
        FullText {
            mode: FullTextMode::Websearch,
            ..FullText::default()
        }
    }

    /// MySQL's boolean mode.
    pub fn boolean() -> Self {
        FullText {
            mode: FullTextMode::Boolean,
            ..FullText::default()
        }
    }

    /// PostgreSQL's text search language.
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// MySQL's query expansion.
    pub fn expanded(mut self) -> Self {
        self.expanded = true;
        self
    }
}

/// The right side of `IN`.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum InValues {
    /// A list (empty: `IN` matches nothing, `NOT IN` everything).
    List(Vec<Expr>),
    /// A subquery returning one column.
    Subquery(Box<QueryIr>),
}

/// A table, optionally aliased, a subquery used as a table (`FROM
/// (SELECT ...) AS alias`, `JOIN (SELECT ...) AS alias`), or the rows of a
/// set-returning function (`FROM generate_series(1, 10) AS n`).
#[derive(Clone, Debug)]
pub struct TableRef {
    /// The table name (for a subquery or a function, its alias).
    pub name: Cow<'static, str>,
    /// The alias.
    pub alias: Option<Cow<'static, str>>,
    /// The subquery the rows come from, when the table is a subquery.
    pub subquery: Option<Box<QueryIr>>,
    /// The function call (an [`Expr::Function`]) the rows come from, when
    /// the table is a set-returning function. SQL drivers only.
    pub function: Option<Box<Expr>>,
}

impl TableRef {
    /// The table `name`.
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        TableRef {
            name: name.into(),
            alias: None,
            subquery: None,
            function: None,
        }
    }

    /// The table `name` as `alias`.
    pub fn aliased(
        name: impl Into<Cow<'static, str>>,
        alias: impl Into<Cow<'static, str>>,
    ) -> Self {
        TableRef {
            name: name.into(),
            alias: Some(alias.into()),
            subquery: None,
            function: None,
        }
    }

    /// The rows of `query`, named `alias`.
    pub fn subquery(query: QueryIr, alias: impl Into<Cow<'static, str>>) -> Self {
        let alias = alias.into();
        TableRef {
            name: alias.clone(),
            alias: Some(alias),
            subquery: Some(Box::new(query)),
            function: None,
        }
    }

    /// The rows the set-returning function call `call` (an
    /// [`Expr::Function`]) returns, named `alias`.
    pub fn function(call: Expr, alias: impl Into<Cow<'static, str>>) -> Self {
        let alias = alias.into();
        TableRef {
            name: alias.clone(),
            alias: Some(alias),
            subquery: None,
            function: Some(Box::new(call)),
        }
    }

    /// The name columns of this table are qualified with: the alias, else
    /// the name.
    pub fn qualifier(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }
}

/// `DISTINCT`.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub enum Distinct {
    /// No `DISTINCT`.
    #[default]
    No,
    /// `SELECT DISTINCT`.
    Yes,
    /// PostgreSQL's `SELECT DISTINCT ON (...)`.
    On(Vec<Expr>),
}

/// One item of a `SELECT` list.
#[derive(Clone, Debug)]
pub struct SelectItem {
    /// The expression.
    pub expr: Expr,
    /// `AS alias`.
    pub alias: Option<Cow<'static, str>>,
}

/// The kind of a join.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum JoinKind {
    /// `INNER JOIN`.
    Inner,
    /// `LEFT JOIN`.
    Left,
    /// `RIGHT JOIN`.
    Right,
    /// `CROSS JOIN`.
    Cross,
    /// `INNER JOIN LATERAL (subquery) AS alias ON true`: the subquery
    /// runs once per row and may use its columns (PostgreSQL, MySQL
    /// 8.0.14+).
    Lateral,
    /// `LEFT JOIN LATERAL (subquery) AS alias ON true`.
    LeftLateral,
}

/// At most [`limit`](GroupLimit::limit) rows per value of
/// [`column`](GroupLimit::column) (top N per group), in the query's order:
/// `ROW_NUMBER() OVER (PARTITION BY column ORDER BY ..)` in a derived
/// table. The query's offset skips rows per group; its limit caps the whole
/// result.
#[derive(Clone, Debug)]
pub struct GroupLimit {
    /// The most rows per group.
    pub limit: u64,
    /// The column (or expression) whose values form the groups.
    pub column: Expr,
}

/// A join.
#[derive(Clone, Debug)]
pub struct Join {
    /// The kind.
    pub kind: JoinKind,
    /// The joined table.
    pub table: TableRef,
    /// The `ON` conditions (empty for cross joins).
    pub on: Vec<WhereClause>,
}

/// A sort direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Ascending.
    Asc,
    /// Descending.
    Desc,
}

impl Direction {
    /// The opposite direction.
    pub fn reversed(self) -> Self {
        match self {
            Direction::Asc => Direction::Desc,
            Direction::Desc => Direction::Asc,
        }
    }
}

/// Where NULLs sort.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Nulls {
    /// NULLs first.
    First,
    /// NULLs last.
    Last,
}

/// One `ORDER BY` item.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Order {
    /// An expression and a direction.
    By {
        /// The expression.
        expr: Expr,
        /// The direction.
        direction: Direction,
        /// Where NULLs sort; the database's default when `None`.
        nulls: Option<Nulls>,
    },
    /// Random order.
    Random,
    /// Raw SQL with `?` placeholders (`??` is a literal `?`), direction
    /// included. SQL drivers only.
    Raw {
        /// The SQL fragment.
        sql: Cow<'static, str>,
        /// Its values, in order.
        values: Vec<Value>,
    },
}

impl Order {
    /// Raw SQL with `?` placeholders for `values`: `Order::raw("field(stage, ?, ?)", ["won", "lost"])`.
    pub fn raw(
        sql: impl Into<Cow<'static, str>>,
        values: impl IntoIterator<Item = impl Into<Value>>,
    ) -> Self {
        Order::Raw {
            sql: sql.into(),
            values: values.into_iter().map(Into::into).collect(),
        }
    }

    /// `expr ASC`.
    pub fn asc(expr: impl Into<Expr>) -> Self {
        Order::By {
            expr: expr.into(),
            direction: Direction::Asc,
            nulls: None,
        }
    }

    /// `expr DESC`.
    pub fn desc(expr: impl Into<Expr>) -> Self {
        Order::By {
            expr: expr.into(),
            direction: Direction::Desc,
            nulls: None,
        }
    }

    /// The same order with NULLs first.
    pub fn nulls_first(self) -> Self {
        self.with_nulls(Nulls::First)
    }

    /// The same order with NULLs last.
    pub fn nulls_last(self) -> Self {
        self.with_nulls(Nulls::Last)
    }

    fn with_nulls(self, which: Nulls) -> Self {
        match self {
            Order::By {
                expr, direction, ..
            } => Order::By {
                expr,
                direction,
                nulls: Some(which),
            },
            other => other,
        }
    }
}

/// A row lock on the rows a query reads, held until its transaction ends:
/// a [`LockMode`] and what to do about rows another transaction holds
/// ([`LockWait`]).
///
/// | | PostgreSQL | MySQL | SQLite |
/// |---|---|---|---|
/// | [`Update`](LockMode::Update) | `FOR UPDATE` | `FOR UPDATE` | none |
/// | [`NoKeyUpdate`](LockMode::NoKeyUpdate) | `FOR NO KEY UPDATE` | `FOR UPDATE` | none |
/// | [`Share`](LockMode::Share) | `FOR SHARE` | `LOCK IN SHARE MODE` (`FOR SHARE` with `NOWAIT` or `SKIP LOCKED`) | none |
/// | [`KeyShare`](LockMode::KeyShare) | `FOR KEY SHARE` | as `Share` | none |
///
/// SQLite has no row locks: its write transactions are serialised
/// (navree-db begins them `IMMEDIATE`), so the clause is dropped there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Lock {
    /// What the lock keeps other transactions from doing.
    pub mode: LockMode,
    /// What happens to rows another transaction has locked.
    pub wait: LockWait,
}

impl Lock {
    /// A lock of `mode` that waits for rows other transactions hold.
    pub const fn new(mode: LockMode) -> Self {
        Lock {
            mode,
            wait: LockWait::Wait,
        }
    }
}

/// What a row lock keeps other transactions from doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LockMode {
    /// Changing or locking the rows (`FOR UPDATE`): read them, then update
    /// them in the same transaction.
    Update,
    /// Like [`Update`](LockMode::Update), without blocking `KeyShare`
    /// locks (foreign keys pointing at the rows); for updates that leave
    /// the key alone (`FOR NO KEY UPDATE`, PostgreSQL; `FOR UPDATE`
    /// elsewhere).
    NoKeyUpdate,
    /// Changing the rows; other shared locks are allowed (`FOR SHARE`).
    Share,
    /// Deleting the rows or changing their key (`FOR KEY SHARE`,
    /// PostgreSQL; a shared lock elsewhere).
    KeyShare,
}

/// What a locking read does with rows another transaction has locked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LockWait {
    /// Wait until they are released (the database's lock timeout applies).
    #[default]
    Wait,
    /// Fail at once (`NOWAIT`) with the database's lock error.
    NoWait,
    /// Leave them out of the result (`SKIP LOCKED`): workers claiming jobs
    /// or outbox entries never wait for each other.
    SkipLocked,
}

/// A `UNION`.
#[derive(Clone, Debug)]
pub struct Union {
    /// The other query.
    pub query: Box<QueryIr>,
    /// `UNION ALL`.
    pub all: bool,
}

/// An aggregate over a query's rows: `count(*)`, `sum(amount)`, ...
#[derive(Clone, Debug)]
pub struct Aggregate {
    /// The function.
    pub function: AggregateFunction,
    /// The argument; `None` is `*`.
    pub arg: Option<Expr>,
    /// `DISTINCT` inside the call.
    pub distinct: bool,
}

/// A read query.
#[derive(Clone, Debug)]
pub struct QueryIr {
    /// The table (a collection for document drivers).
    pub table: TableRef,
    /// `DISTINCT`.
    pub distinct: Distinct,
    /// The selected items; empty selects every column.
    pub columns: Vec<SelectItem>,
    /// Joins, in order.
    pub joins: Vec<Join>,
    /// `WHERE` clauses.
    pub wheres: Vec<WhereClause>,
    /// `GROUP BY` expressions.
    pub groups: Vec<Expr>,
    /// `HAVING` clauses.
    pub havings: Vec<WhereClause>,
    /// `ORDER BY` items.
    pub orders: Vec<Order>,
    /// `LIMIT`.
    pub limit: Option<u64>,
    /// `OFFSET`.
    pub offset: Option<u64>,
    /// A row lock.
    pub lock: Option<Lock>,
    /// `UNION`s.
    pub unions: Vec<Union>,
    /// At most so many rows per group (top N per parent).
    pub group_limit: Option<GroupLimit>,
    /// Problems found while building the query; it fails with them when
    /// it runs.
    pub errors: Vec<String>,
}

impl QueryIr {
    /// A query of every row of `table`.
    pub fn new(table: impl Into<Cow<'static, str>>) -> Self {
        QueryIr {
            table: TableRef::new(table),
            distinct: Distinct::No,
            columns: Vec::new(),
            joins: Vec::new(),
            wheres: Vec::new(),
            groups: Vec::new(),
            havings: Vec::new(),
            orders: Vec::new(),
            limit: None,
            offset: None,
            lock: None,
            unions: Vec::new(),
            group_limit: None,
            errors: Vec::new(),
        }
    }

    /// The first problem found while building, as an error.
    pub fn check(&self) -> Result<(), crate::OrmError> {
        match self.errors.first() {
            Some(error) => Err(crate::OrmError::invalid(error.clone())),
            None => Ok(()),
        }
    }
}

/// An `INSERT` of one or more rows, or of the rows of a query.
#[derive(Clone, Debug)]
pub struct InsertIr {
    /// The table.
    pub table: Cow<'static, str>,
    /// The columns, in the order of each row's values.
    pub columns: Vec<Cow<'static, str>>,
    /// The rows (empty when [`select`](InsertIr::select) provides them).
    pub rows: Vec<Vec<Expr>>,
    /// Columns to read back from the inserted rows (generated keys).
    pub returning: Vec<Cow<'static, str>>,
    /// A query whose rows are inserted (`INSERT … SELECT`) instead of
    /// [`rows`](InsertIr::rows); it selects one value per column.
    pub select: Option<Box<QueryIr>>,
    /// What happens to a row that conflicts with a unique index; `None`
    /// fails the statement.
    pub on_conflict: Option<OnConflict>,
}

impl InsertIr {
    /// An insert of `rows` (each with one value per column) into `table`,
    /// returning nothing and failing on conflicts.
    pub fn new(
        table: impl Into<Cow<'static, str>>,
        columns: Vec<Cow<'static, str>>,
        rows: Vec<Vec<Expr>>,
    ) -> Self {
        InsertIr {
            table: table.into(),
            columns,
            rows,
            returning: Vec::new(),
            select: None,
            on_conflict: None,
        }
    }

    /// The same insert, reading `columns` back from the inserted rows.
    pub fn returning(mut self, columns: Vec<Cow<'static, str>>) -> Self {
        self.returning = columns;
        self
    }

    /// The same insert, handling conflicting rows with `on_conflict`.
    pub fn on_conflict(mut self, on_conflict: OnConflict) -> Self {
        self.on_conflict = Some(on_conflict);
        self
    }
}

/// What an insert does with a row that conflicts with a unique index
/// (an "upsert" when it updates).
///
/// | | PostgreSQL, SQLite | MySQL |
/// |---|---|---|
/// | [`Ignore`](OnConflict::Ignore) | `ON CONFLICT [(target)] DO NOTHING` | `INSERT IGNORE` |
/// | [`Update`](OnConflict::Update) | `ON CONFLICT (target) DO UPDATE SET ..` | `ON DUPLICATE KEY UPDATE ..` |
///
/// PostgreSQL and SQLite only skip or update rows that conflict on a
/// unique index (a target names its columns); MySQL looks at every unique
/// index, and its `INSERT IGNORE` also turns other errors (a value that
/// does not fit its column) into warnings.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum OnConflict {
    /// Skip the conflicting rows.
    Ignore {
        /// The unique columns whose conflicts are skipped; empty for any
        /// unique index.
        target: Vec<Cow<'static, str>>,
    },
    /// Update the existing row instead.
    Update {
        /// The unique columns the conflict is on (required on PostgreSQL
        /// and SQLite; MySQL uses every unique index).
        target: Vec<Cow<'static, str>>,
        /// `SET column = expr` on the existing row; [`Expr::Excluded`] is
        /// the value the row would have been inserted with.
        sets: Vec<(Cow<'static, str>, Expr)>,
    },
}

/// An `UPDATE`.
#[derive(Clone, Debug)]
pub struct UpdateIr {
    /// The table.
    pub table: TableRef,
    /// `SET column = expr`, in order.
    pub sets: Vec<(Cow<'static, str>, Expr)>,
    /// `WHERE` clauses.
    pub wheres: Vec<WhereClause>,
}

/// A `DELETE`.
#[derive(Clone, Debug)]
pub struct DeleteIr {
    /// The table.
    pub table: TableRef,
    /// `WHERE` clauses.
    pub wheres: Vec<WhereClause>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_refs_parse_tables_and_json_paths() {
        assert_eq!(
            ColumnRef::parse("title"),
            ColumnRef::new(None::<&str>, "title")
        );
        assert_eq!(
            ColumnRef::parse("deals.title"),
            ColumnRef::new(Some("deals"), "title")
        );
        let column = ColumnRef::parse("meta->a->0->b");
        assert_eq!(
            column.path,
            [
                JsonSegment::Key("a".into()),
                JsonSegment::Index(0),
                JsonSegment::Key("b".into())
            ]
        );
        let indexed = ColumnRef::parse("deals.tags[1]");
        assert_eq!(
            (indexed.table.as_deref(), indexed.name.as_ref()),
            (Some("deals"), "tags")
        );
        assert_eq!(indexed.path, [JsonSegment::Index(1)]);
        let nested = ColumnRef::parse("tags[-1]->name");
        assert_eq!(
            nested.path,
            [JsonSegment::Index(-1), JsonSegment::Key("name".into())]
        );
        assert_eq!(column.to_string(), "meta->a->0->b");
        assert_eq!(column.without_path().to_string(), "meta");
    }

    #[test]
    fn operators_parse_case_and_whitespace_insensitively() {
        assert_eq!(Operator::from("  not   LIKE "), Operator::NotLike);
        assert_eq!(Operator::from("@>"), Operator::Postgres("@>"));
        assert_eq!(Operator::from("rlike"), Operator::Regexp);
        assert_eq!(Operator::from("bogus"), Operator::Invalid("bogus".into()));
    }
}
