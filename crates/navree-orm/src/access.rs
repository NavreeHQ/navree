//! Access control on models (feature `access`): navree-access's abilities
//! as query filters and record checks on ORM models.
//!
//! A model becomes a subject with `#[model(subject = "Deal")]` (its
//! permission prefix is its table, or `permission_prefix = "..."`), and can
//! carry its rules with `#[model(policy = DealPolicy)]`. Register it with
//! `AccessModule::for_feature::<Deal>()`: then `#[can(Update, Deal, from =
//! "deal")]` routes load the record (by route key, in the request's unit of
//! work, inside the current workspace) and hand it to the handler as
//! `Authorized<Deal, Update>`, and the policy joins every principal's rules.
//!
//! ```no_run
//! # use navree_orm::__doc_access::*;
//! use navree_access::{Ability, Read, Update};
//! use navree_orm::access::{AbilityExt, AccessibleQuery};
//!
//! # async fn demo(db: Db, ability: Ability, deals: navree_access::Accessible<Ticket, Read>, ticket: Record<Ticket>) -> std::result::Result<(), navree_http::AppError> {
//! // The records a principal may read: one SQL filter from the same rules
//! // as record checks, so the two never disagree.
//! let visible = Ticket::query().accessible(&ability, Read).paginate(&db, 1).await?;
//! // The same through the request's `Accessible<Ticket, Read>` extractor.
//! let open = deals.apply(Ticket::query()).count(&db).await?;
//! // One record.
//! ability.authorize(Update, &ticket)?;
//! # Ok(()) }
//! ```

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

use chrono::{DateTime, NaiveDate, NaiveDateTime, SecondsFormat, Utc};
use navree_access::{
    Ability, Accessible, ActionMarker, SqlColumn, SqlColumns, SqlFilter, SqlOp, Subject,
};
use navree_db::Uow;
use navree_http::AppError;
use navree_kernel::{BoxError, BoxFuture};
use rust_decimal::prelude::ToPrimitive;
use serde_json::Value as Json;

use crate::binding::route_value;
use crate::column::{Filter, col};
use crate::model::Model;
use crate::query::Builder;
use crate::record::Record;
use crate::render::ast::{BinaryOp, Expr as SqlExpr, ObjectName};
use crate::value::{Decimal, Kind, Value, to_json};

/// The column mapping of ability conditions on model `M`: rule fields are
/// `M`'s columns, qualified with its table, and operands are bound with the
/// column's type (UUIDs, date-times, dates and decimals as typed values,
/// enums stored as text or numbers as such, numbers as the column's integer
/// or float). A field that is not a column is SQL `NULL`, as [`record_json`]
/// has no such member and a record check sees `null`.
///
/// It is navree-access's [`SqlColumns`] for the model:
/// [`ModelColumns::filter`] compiles an ability's rules with navree-access's
/// list-filter compiler and writes the result as one render-tree condition
/// (which [`Builder::accessible`] does).
pub struct ModelColumns<M>(PhantomData<fn() -> M>);

impl<M> ModelColumns<M> {
    /// The mapping for `M`.
    pub fn new() -> Self {
        ModelColumns(PhantomData)
    }
}

impl<M> Default for ModelColumns<M> {
    fn default() -> Self {
        ModelColumns::new()
    }
}

impl<M> fmt::Debug for ModelColumns<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ModelColumns<{}>", std::any::type_name::<M>())
    }
}

/// What a column stores, for typing condition operands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stored {
    Bool,
    Integer,
    Float,
    Text,
    Uuid,
    DateTimeUtc,
    NaiveDateTime,
    Date,
    Decimal,
    Other,
}

/// What column `column` of `M` stores.
fn stored<M: Model>(column: &str) -> Option<Stored> {
    let meta = M::meta().column(column)?;
    if let Some(cast) = meta.cast_name() {
        return Some(match cast {
            "string_enum" => Stored::Text,
            "int_enum" => Stored::Integer,
            _ => Stored::Other,
        });
    }
    let rust_type = meta.rust_type();
    let inner = rust_type
        .strip_prefix("Option<")
        .and_then(|rest| rest.strip_suffix('>'))
        .unwrap_or(rust_type);
    let last = inner.rsplit("::").next().unwrap_or(inner);
    Some(match last {
        "bool" => Stored::Bool,
        "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize" => {
            Stored::Integer
        }
        "f32" | "f64" => Stored::Float,
        "String" | "Ulid" => Stored::Text,
        "Uuid" => Stored::Uuid,
        "Decimal" => Stored::Decimal,
        "NaiveDate" => Stored::Date,
        "NaiveDateTime" => Stored::NaiveDateTime,
        _ if last.starts_with("DateTime") => Stored::DateTimeUtc,
        _ => Stored::Other,
    })
}

impl<M: Model> ModelColumns<M> {
    /// The column for rule field `field`: `M`'s column, qualified with its
    /// table, or SQL `NULL` when `M` has no such column.
    pub fn column(&self, field: &str) -> SqlExpr {
        lower_column(&SqlColumns::column(self, field))
    }

    /// The value bound for the non-null JSON operand `value` on `field`:
    /// typed by the column (a UUID, a date-time, a date or a decimal parsed
    /// from its JSON form), else booleans as booleans, integers as `I64`,
    /// other numbers as `F64` and strings as text.
    pub fn value(&self, field: &str, value: &Json) -> Value {
        let typed = stored::<M>(field).and_then(|stored| {
            let text = value.as_str();
            match stored {
                Stored::Uuid => text
                    .and_then(|t| uuid::Uuid::parse_str(t).ok())
                    .map(Value::from),
                Stored::DateTimeUtc => text
                    .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                    .map(|at| Value::from(at.with_timezone(&Utc))),
                Stored::NaiveDateTime => text
                    .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                    .map(|at| Value::from(at.with_timezone(&Utc).naive_utc())),
                Stored::Date => text
                    .and_then(|t| NaiveDate::parse_from_str(t, "%Y-%m-%d").ok())
                    .map(Value::from),
                Stored::Decimal => match value {
                    Json::String(t) => t.parse::<Decimal>().ok().map(Value::from),
                    Json::Number(n) => n.to_string().parse::<Decimal>().ok().map(Value::from),
                    _ => None,
                },
                _ => None,
            }
        });
        typed.unwrap_or_else(|| navree_access::sql_value(value))
    }

    /// Whether the non-null JSON operand `value` has the JSON type of
    /// `field`'s column. A record check never matches across types, so a
    /// rejected operand is written as the check evaluates it: `$eq` and the
    /// range operators are `FALSE`, `$ne` is `TRUE`, and `$in` / `$nin`
    /// ignore it.
    pub fn accepts(&self, field: &str, value: &Json) -> bool {
        match stored::<M>(field) {
            // Not a column: SQL `NULL`, which matches nothing either way.
            None => true,
            Some(Stored::Bool) => value.is_boolean(),
            Some(Stored::Integer | Stored::Float) => value.is_number(),
            Some(
                Stored::Text
                | Stored::Uuid
                | Stored::DateTimeUtc
                | Stored::NaiveDateTime
                | Stored::Date,
            ) => value.is_string(),
            Some(Stored::Decimal | Stored::Other) => true,
        }
    }

    /// A boolean SQL condition that holds exactly for the records of `M`
    /// (subject type `subject`) on which `ability` allows `action`: the
    /// relevant conditional rules from last to first as one `CASE`, so the
    /// first `WHEN` that holds is the rule a record check picks (last match
    /// wins), `FALSE` when no rule applies.
    ///
    /// navree-access compiles it ([`Ability::sql_filter_with`] with this
    /// mapping); it is written here as a render-tree condition, so each
    /// dialect writes it in its own SQL. Each condition is `TRUE` exactly
    /// when the record check holds and `FALSE` or `NULL` otherwise (`CASE`
    /// treats `NULL` as not matched), so the condition is always `TRUE` or
    /// `FALSE` and safe to negate. As in record checks, `$eq: null` is
    /// `IS NULL`, `$ne` is null-safe (`IS DISTINCT FROM` on PostgreSQL,
    /// `NOT (a <=> b)` on MySQL, `IS NOT` on SQLite), and `$in` / `$nin`
    /// treat a `null` member as matching null columns.
    pub fn filter(&self, ability: &Ability, action: &str, subject: &str) -> SqlExpr {
        lower(&ability.sql_filter_with(action, subject, self))
    }
}

/// navree-access's view of the model's columns.
impl<M: Model> SqlColumns for ModelColumns<M> {
    fn column(&self, field: &str) -> SqlColumn {
        if M::meta().has_column(field) {
            SqlColumn::qualified(M::meta().table(), field)
        } else {
            SqlColumn::Null
        }
    }

    fn value(&self, field: &str, value: &Json) -> Value {
        ModelColumns::value(self, field, value)
    }

    fn accepts(&self, field: &str, value: &Json) -> bool {
        ModelColumns::accepts(self, field, value)
    }

    fn kind(&self, field: &str) -> Option<Kind> {
        match stored::<M>(field)? {
            Stored::Integer => Some(Kind::I64),
            Stored::Float => Some(Kind::F64),
            _ => None,
        }
    }
}

/// A compiled list filter as a render-tree condition.
fn lower(filter: &SqlFilter) -> SqlExpr {
    match filter {
        SqlFilter::Bool(value) => SqlExpr::bool(*value),
        SqlFilter::Case { whens, otherwise } => SqlExpr::case(
            whens
                .iter()
                .map(|(condition, allow)| (lower(condition), SqlExpr::bool(*allow)))
                .collect(),
            Some(SqlExpr::bool(*otherwise)),
        ),
        SqlFilter::And(parts) => SqlExpr::And(parts.iter().map(lower).collect()),
        SqlFilter::Or(parts) => SqlExpr::Or(parts.iter().map(lower).collect()),
        SqlFilter::IsNull(column) => lower_column(column).is_null(),
        SqlFilter::IsNotNull(column) => lower_column(column).is_not_null(),
        SqlFilter::Compare(column, op, value) => {
            let (column, value) = (lower_column(column), SqlExpr::Value(value.clone()));
            match op {
                SqlOp::Eq => column.eq(value),
                SqlOp::DistinctFrom => column.binary(BinaryOp::NullSafeNotEq, value),
                SqlOp::Lt => column.lt(value),
                SqlOp::Lte => column.lte(value),
                SqlOp::Gt => column.gt(value),
                SqlOp::Gte => column.gte(value),
            }
        }
        SqlFilter::In(column, values) => {
            lower_column(column).in_list(values.iter().cloned().map(SqlExpr::Value).collect())
        }
        SqlFilter::NotIn(column, values) => {
            lower_column(column).not_in_list(values.iter().cloned().map(SqlExpr::Value).collect())
        }
    }
}

fn lower_column(column: &SqlColumn) -> SqlExpr {
    match column {
        SqlColumn::Column {
            table: Some(table),
            name,
        } => SqlExpr::qualified(ObjectName::parse(table.clone()), name.clone()),
        SqlColumn::Column { table: None, name } => SqlExpr::col(name.clone()),
        SqlColumn::Null => SqlExpr::null(),
    }
}

/// `record` as ability conditions see it: one member per column, named
/// like the column, with the stored value: UUIDs as hyphenated strings,
/// date-times as RFC 3339 UTC strings with microseconds
/// (`2026-10-25T12:00:00.250000Z`), dates as `YYYY-MM-DD`, enums as stored,
/// JSON as itself. The SQL list filter ([`ModelColumns`]) reads the same
/// values, so a record check and a list filter agree.
pub fn record_json<M: Model>(record: &Record<M>) -> Json {
    model_json(record.model())
}

/// [`record_json`] of a model.
pub fn model_json<M: Model>(model: &M) -> Json {
    match model.values() {
        Ok(values) => Json::Object(
            values
                .into_iter()
                .map(|(column, value)| (column.to_owned(), condition_json(&value)))
                .collect(),
        ),
        Err(error) => {
            tracing::error!(target: "navree::access", model = M::meta().name(), %error, "a record cannot be checked");
            Json::Null
        }
    }
}

/// A stored value as a condition operand spells it.
fn condition_json(value: &Value) -> Json {
    let micros = |at: DateTime<Utc>| Json::String(at.to_rfc3339_opts(SecondsFormat::Micros, true));
    match value {
        Value::Uuid(uuid) => Json::String(uuid.hyphenated().to_string()),
        Value::Timestamp(at) => micros(*at),
        Value::TimestampTz(at) => micros(at.with_timezone(&Utc)),
        Value::DateTime(at) => micros(NaiveDateTime::and_utc(at)),
        Value::Date(date) => Json::String(date.format("%Y-%m-%d").to_string()),
        Value::Time(time) => Json::String(time.format("%H:%M:%S%.f").to_string()),
        // Binary columns have no condition form (and may not be UTF-8).
        Value::Bytes(_) => Json::Null,
        // Decimals are numbers to conditions (equality on them is exact
        // only as strings, which the SQL filter binds as decimals).
        Value::Decimal(decimal) => decimal.to_f64().map_or(Json::Null, Json::from),
        other => to_json(other),
    }
}

impl<M: Model + Subject> Builder<M> {
    /// Only the records on which `ability` allows action `A`: one SQL
    /// condition compiled from the same rules as record checks (last match
    /// wins), so a record is listed exactly when [`AbilityExt::allows`]
    /// holds for it. Counts, sums and pages of the query respect it.
    pub fn accessible<A: ActionMarker>(self, ability: &Ability, action: A) -> Self {
        let _ = action;
        self.accessible_for(ability, A::NAME)
    }

    /// [`accessible`](Builder::accessible) for an action named at run time
    /// (`"publish"`).
    pub fn accessible_for(self, ability: &Ability, action: &str) -> Self {
        let condition = ModelColumns::<M>::new().filter(ability, action, M::SUBJECT);
        self.where_(Filter::from(condition))
    }
}

/// `Accessible<M, A>` (navree-access's list-filter extractor) on model
/// queries.
pub trait AccessibleQuery<M: Model> {
    /// `query` restricted to the accessible records.
    fn apply(&self, query: Builder<M>) -> Builder<M>;

    /// A query of the accessible records.
    fn query(&self) -> Builder<M> {
        self.apply(M::query())
    }
}

impl<M: Model + Subject, A: ActionMarker> AccessibleQuery<M> for Accessible<M, A> {
    fn apply(&self, query: Builder<M>) -> Builder<M> {
        query.accessible_for(self.ability(), A::NAME)
    }
}

/// Record checks on ORM records.
pub trait AbilityExt {
    /// Whether action `A` is allowed on `record`.
    fn allows<M: Model + Subject, A: ActionMarker>(&self, action: A, record: &Record<M>) -> bool;

    /// Whether `action` (named at run time) is allowed on `record`.
    fn allows_action<M: Model + Subject>(&self, action: &str, record: &Record<M>) -> bool;

    /// `Ok` when action `A` is allowed on `record`, else a 403 in Navree's
    /// error shape.
    fn authorize<M: Model + Subject, A: ActionMarker>(
        &self,
        action: A,
        record: &Record<M>,
    ) -> Result<(), AppError>;
}

impl AbilityExt for Ability {
    fn allows<M: Model + Subject, A: ActionMarker>(&self, action: A, record: &Record<M>) -> bool {
        let _ = action;
        self.allows_action(A::NAME, record)
    }

    fn allows_action<M: Model + Subject>(&self, action: &str, record: &Record<M>) -> bool {
        self.can(action, M::SUBJECT, Some(&record_json(record)))
    }

    fn authorize<M: Model + Subject, A: ActionMarker>(
        &self,
        action: A,
        record: &Record<M>,
    ) -> Result<(), AppError> {
        if self.allows(action, record) {
            Ok(())
        } else {
            Err(AppError::forbidden())
        }
    }
}

/// Loads the record of `M` whose route key is `id` for an access check
/// (what `#[model(subject)]` wires into navree-access's `SubjectRecords`):
/// the model's query with its global scopes (so inside the current
/// workspace), locked `FOR UPDATE` where the database supports it, in
/// `uow`'s transaction. `None` when there is none or `id` cannot be a route
/// key.
pub fn load_record<'a, M: Model>(
    uow: &'a Uow,
    id: &'a str,
) -> BoxFuture<'a, Result<Option<Record<M>>, BoxError>> {
    Box::pin(async move {
        let column = M::route_key_name();
        let Some(meta) = M::meta().column(column) else {
            return Ok(None);
        };
        let Some(value) = route_value(meta, id) else {
            return Ok(None);
        };
        let mut query =
            M::query().where_(col(Cow::Owned(format!("{}.{column}", M::meta().table()))).eq(value));
        if uow.lock_for_update() {
            query = query.lock_for_update();
        }
        Ok(query.first(uow).await?)
    })
}

/// What `#[derive(Model)]` expands to for `subject` and `policy`. Not API.
#[doc(hidden)]
pub mod derive_support {
    pub use navree_access::{AbilityCtx, Policy, Rules, Subject, SubjectRecords};
    pub use navree_db::Uow;
    pub use navree_kernel::{BoxError, BoxFuture};
    pub use serde_json::Value as Json;
}
