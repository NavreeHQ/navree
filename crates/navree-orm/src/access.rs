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
use navree_access::{Ability, Accessible, ActionMarker, SqlColumns, Subject, sql_value};
use navree_db::Uow;
use navree_http::AppError;
use navree_kernel::{BoxError, BoxFuture};
use sea_query::{Alias, Expr as SqlExpr};
use serde_json::Value as Json;

use crate::binding::route_value;
use crate::column::{Filter, col};
use crate::model::Model;
use crate::query::Builder;
use crate::record::Record;
use crate::value::{Decimal, Value};

/// The column mapping of ability conditions on model `M`: rule fields are
/// `M`'s columns, qualified with its table, and operands are bound with the
/// column's type (UUIDs, date-times, dates and decimals as typed values,
/// enums stored as text or numbers as such). A field that is not a column is
/// SQL `NULL`, as [`record_json`] has no such member and a record check sees
/// `null`. Use it with [`Ability::sql_filter_with`] (which
/// [`Builder::accessible`] does).
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
    Number,
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
            "int_enum" => Stored::Number,
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
        "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize" | "f32"
        | "f64" => Stored::Number,
        "String" | "Ulid" => Stored::Text,
        "Uuid" => Stored::Uuid,
        "Decimal" => Stored::Decimal,
        "NaiveDate" => Stored::Date,
        "NaiveDateTime" => Stored::NaiveDateTime,
        _ if last.starts_with("DateTime") => Stored::DateTimeUtc,
        _ => Stored::Other,
    })
}

impl<M: Model> SqlColumns for ModelColumns<M> {
    fn column(&self, field: &str) -> SqlExpr {
        if M::meta().has_column(field) {
            SqlExpr::col((Alias::new(M::meta().table()), Alias::new(field)))
        } else {
            SqlExpr::cust("NULL")
        }
    }

    fn value(&self, field: &str, value: &Json) -> sea_query::Value {
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
        typed.unwrap_or_else(|| sql_value(value))
    }

    fn accepts(&self, field: &str, value: &Json) -> bool {
        match stored::<M>(field) {
            // Not a column: SQL `NULL`, which matches nothing either way.
            None => true,
            Some(Stored::Bool) => value.is_boolean(),
            Some(Stored::Number) => value.is_number(),
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
        Value::Uuid(Some(uuid)) => Json::String(uuid.hyphenated().to_string()),
        Value::ChronoDateTimeUtc(Some(at)) => micros(*at),
        Value::ChronoDateTimeLocal(Some(at)) => micros(at.with_timezone(&Utc)),
        Value::ChronoDateTimeWithTimeZone(Some(at)) => micros(at.with_timezone(&Utc)),
        Value::ChronoDateTime(Some(at)) => micros(NaiveDateTime::and_utc(at)),
        Value::ChronoDate(Some(date)) => Json::String(date.format("%Y-%m-%d").to_string()),
        Value::ChronoTime(Some(time)) => Json::String(time.format("%H:%M:%S%.f").to_string()),
        // Binary columns have no condition form (and may not be UTF-8).
        Value::Bytes(_) => Json::Null,
        other => sea_query::value::sea_value_to_json_value(other),
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
        let condition = ability.sql_filter_with(action, M::SUBJECT, &ModelColumns::<M>::new());
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
