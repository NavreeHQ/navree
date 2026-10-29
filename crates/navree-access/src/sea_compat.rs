//! SeaORM compatibility shim. **Remove in step 8 of the data-layer move**
//! (docs/design, §4).
//!
//! navree-access itself runs on navree-db statements; this module only keeps
//! the crates that still hold SeaORM entities working until they are
//! moved:
//!
//! - every SeaORM entity that is a [`Subject`] is a [`SubjectRecords`] (its
//!   records are its models);
//! - [`Accessible::condition`] and [`Accessible::apply`] on a SeaORM
//!   `Select<E>`;
//! - [`EntityColumns`] (an entity's columns for
//!   [`Ability::sql_filter_with`](crate::Ability::sql_filter_with)) and
//!   [`SqlFilter::to_sea_query`];
//! - [`record_json`] of a SeaORM model;
//! - `?` from a SeaORM `DbErr` into [`AccessError`].
//!
//! New code must not use any of it: ORM models get all of this from
//! `#[derive(Model)]` and navree-orm's `AccessibleQuery`; records below the
//! ORM use [`TypedColumns`](crate::TypedColumns), [`row_json`](crate::row_json)
//! and [`SqlFilter::to_statement`].

use std::fmt;
use std::marker::PhantomData;

use chrono::{DateTime, SecondsFormat, Utc};
use navree_db::DbError;
use navree_db::compat::to_sea_value;
use navree_db::sea_orm::sea_query::value::sea_value_to_json_value;
use navree_db::sea_orm::sea_query::{
    Alias, BinOper, CaseStatement, Condition, Expr, ExprTrait, SimpleExpr, Value as SeaValue,
};
use navree_db::sea_orm::{
    ColumnTrait, ColumnType, DbErr, EntityTrait, IdenStatic, Iterable, ModelTrait,
    PrimaryKeyToColumn, QueryFilter, QuerySelect, Select,
};
use navree_db::{Kind, Uow};
use navree_kernel::{BoxError, BoxFuture};
use serde_json::Value;

use crate::engine::{
    ActionMarker, SqlColumn, SqlColumns, SqlFilter, SqlOp, Subject, accepts_kind, sql_value,
    typed_value,
};
use crate::error::AccessError;
use crate::extract::Accessible;
use crate::records::SubjectRecords;

/// Every SeaORM entity that is a [`Subject`]: its records are its models,
/// loaded by single-column primary key.
impl<E> SubjectRecords for E
where
    E: EntityTrait + Subject,
    E::Model: Sync,
{
    type Record = E::Model;

    fn load<'a>(
        uow: &'a Uow,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<Self::Record>, BoxError>> {
        Box::pin(async move {
            let Some(mut select) = select_by_key::<E>(id)? else {
                return Ok(None);
            };
            if uow.lock_for_update() {
                select = select.lock_exclusive();
            }
            let tx = uow.tx().await?;
            Ok(select.one(&*tx).await?)
        })
    }

    fn record_json(record: &Self::Record) -> Value {
        record_json(record)
    }
}

/// `SELECT ... WHERE pk = id` for a single-column primary key; `None` when
/// `id` cannot be a key of the column's type.
fn select_by_key<E: EntityTrait>(id: &str) -> Result<Option<Select<E>>, DbErr> {
    let mut keys = E::PrimaryKey::iter();
    let (Some(key), None) = (keys.next(), keys.next()) else {
        return Err(DbErr::Custom(format!(
            "`{}` needs a single-column primary key to be loaded by id",
            E::default().table_name()
        )));
    };
    let column = key.into_column();
    Ok(key_value(column.def().get_column_type(), id)
        .map(|value| E::find().filter(column.eq(value))))
}

/// The primary key value for path parameter `id`, typed like the column;
/// `None` when `id` cannot be a key of that type.
fn key_value(column_type: &ColumnType, id: &str) -> Option<SeaValue> {
    match column_type {
        ColumnType::Uuid => uuid::Uuid::parse_str(id).ok().map(SeaValue::from),
        ColumnType::TinyInteger
        | ColumnType::SmallInteger
        | ColumnType::Integer
        | ColumnType::BigInteger
        | ColumnType::TinyUnsigned
        | ColumnType::SmallUnsigned
        | ColumnType::Unsigned
        | ColumnType::BigUnsigned => id.parse::<i64>().ok().map(SeaValue::from),
        _ => Some(SeaValue::from(id.to_owned())),
    }
}

/// The JSON form of SeaORM `model` that ability conditions are checked
/// against: one member per column, named like the column (see
/// [`row_json`](crate::row_json) for navree-db records).
pub fn record_json<M: ModelTrait>(model: &M) -> Value {
    let object = <M::Entity as EntityTrait>::Column::iter()
        .map(|column| {
            let name = column.as_str().to_owned();
            (name, sea_to_json(&model.get(column)))
        })
        .collect();
    Value::Object(object)
}

/// A SeaORM value as a condition operand would spell it.
fn sea_to_json(value: &SeaValue) -> Value {
    let micros = |dt: DateTime<Utc>| Value::String(dt.to_rfc3339_opts(SecondsFormat::Micros, true));
    match value {
        SeaValue::Uuid(Some(uuid)) => Value::String(uuid.hyphenated().to_string()),
        SeaValue::ChronoDateTimeUtc(Some(dt)) => micros(*dt),
        SeaValue::ChronoDateTimeLocal(Some(dt)) => micros(dt.with_timezone(&Utc)),
        SeaValue::ChronoDateTimeWithTimeZone(Some(dt)) => micros(dt.with_timezone(&Utc)),
        SeaValue::ChronoDateTime(Some(dt)) => micros(dt.and_utc()),
        SeaValue::ChronoDate(Some(date)) => Value::String(date.format("%Y-%m-%d").to_string()),
        SeaValue::ChronoTime(Some(time)) => Value::String(time.format("%H:%M:%S%.f").to_string()),
        // Binary columns have no condition form (and may not be UTF-8).
        SeaValue::Bytes(_) => Value::Null,
        other => sea_value_to_json_value(other),
    }
}

/// [`SqlColumns`] for SeaORM entity `E`: rule fields are column names,
/// qualified with the entity's table, and operands are bound with the
/// column's type. A field that is not a column of `E` is SQL `NULL`.
pub struct EntityColumns<E>(PhantomData<fn() -> E>);

impl<E> EntityColumns<E> {
    /// The mapping for `E`.
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<E> Default for EntityColumns<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E> fmt::Debug for EntityColumns<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EntityColumns<{}>", std::any::type_name::<E>())
    }
}

impl<E: EntityTrait> EntityColumns<E> {
    /// The kind of `field`'s column as list filters type it (dates and
    /// times stay text, as they always have here).
    fn column_kind(field: &str) -> Option<Kind> {
        let column = E::Column::iter().find(|column| column.as_str() == field)?;
        Some(match column.def().get_column_type() {
            ColumnType::Boolean => Kind::Bool,
            ColumnType::TinyInteger
            | ColumnType::SmallInteger
            | ColumnType::Integer
            | ColumnType::BigInteger
            | ColumnType::TinyUnsigned
            | ColumnType::SmallUnsigned
            | ColumnType::Unsigned
            | ColumnType::BigUnsigned => Kind::I64,
            ColumnType::Float | ColumnType::Double => Kind::F64,
            ColumnType::Uuid => Kind::Uuid,
            ColumnType::TimestampWithTimeZone => Kind::Timestamp,
            ColumnType::DateTime | ColumnType::Timestamp => Kind::DateTime,
            ColumnType::Char(_)
            | ColumnType::String(_)
            | ColumnType::Text
            | ColumnType::Date
            | ColumnType::Time => Kind::String,
            // Decimals, JSON, enums and the rest compare as SQL compares
            // them.
            _ => Kind::Json,
        })
    }
}

impl<E: EntityTrait> SqlColumns for EntityColumns<E> {
    fn column(&self, field: &str) -> SqlColumn {
        match Self::column_kind(field) {
            Some(_) => SqlColumn::qualified(E::default().table_name(), field),
            None => SqlColumn::Null,
        }
    }

    fn value(&self, field: &str, value: &Value) -> navree_db::Value {
        Self::column_kind(field)
            .and_then(|kind| typed_value(kind, value))
            .unwrap_or_else(|| sql_value(value))
    }

    fn accepts(&self, field: &str, value: &Value) -> bool {
        // Not a column: SQL `NULL`, which matches nothing either way.
        Self::column_kind(field).is_none_or(|kind| accepts_kind(kind, value))
    }

    fn kind(&self, field: &str) -> Option<Kind> {
        Self::column_kind(field)
    }
}

impl SqlFilter {
    /// The filter as a sea-query expression, for a SeaORM select. Part of
    /// the SeaORM shim: use [`SqlFilter::to_statement`] in new code.
    pub fn to_sea_query(&self) -> SimpleExpr {
        sea_expr(self)
    }
}

fn sea_expr(filter: &SqlFilter) -> Expr {
    match filter {
        SqlFilter::Bool(value) => boolean(*value),
        SqlFilter::Case { whens, otherwise } => whens
            .iter()
            .fold(CaseStatement::new(), |case, (condition, allow)| {
                case.case(sea_condition(condition), boolean(*allow))
            })
            .finally(boolean(*otherwise))
            .into(),
        SqlFilter::And(parts) => parts
            .iter()
            .fold(Condition::all(), |all, part| all.add(sea_expr(part)))
            .into(),
        SqlFilter::Or(parts) => parts
            .iter()
            .fold(Condition::any(), |any, part| any.add(sea_expr(part)))
            .into(),
        SqlFilter::IsNull(column) => sea_column(column).is_null(),
        SqlFilter::IsNotNull(column) => sea_column(column).is_not_null(),
        SqlFilter::Compare(column, op, value) => {
            let column = sea_column(column);
            let value = Expr::val(to_sea_value(value));
            match op {
                SqlOp::Eq => column.eq(value),
                SqlOp::DistinctFrom => column.binary(BinOper::Custom("IS DISTINCT FROM"), value),
                SqlOp::Lt => column.lt(value),
                SqlOp::Lte => column.lte(value),
                SqlOp::Gt => column.gt(value),
                SqlOp::Gte => column.gte(value),
            }
        }
        SqlFilter::In(column, values) => {
            sea_column(column).is_in(values.iter().map(|v| Expr::val(to_sea_value(v))))
        }
        SqlFilter::NotIn(column, values) => {
            sea_column(column).is_not_in(values.iter().map(|v| Expr::val(to_sea_value(v))))
        }
    }
}

/// A `WHEN` condition: a rule's conditions, joined with `AND`.
fn sea_condition(filter: &SqlFilter) -> Condition {
    match filter {
        SqlFilter::And(parts) => parts
            .iter()
            .fold(Condition::all(), |all, part| all.add(sea_expr(part))),
        other => Condition::all().add(sea_expr(other)),
    }
}

fn sea_column(column: &SqlColumn) -> Expr {
    match column {
        SqlColumn::Column {
            table: Some(table),
            name,
        } => Expr::col((Alias::new(table.as_str()), Alias::new(name.as_str()))),
        SqlColumn::Column { table: None, name } => Expr::col(Alias::new(name.as_str())),
        SqlColumn::Null => Expr::cust("NULL"),
    }
}

/// An inlined `TRUE` / `FALSE` (not a bound parameter).
fn boolean(value: bool) -> Expr {
    Expr::Constant(value.into())
}

impl<E, A> Accessible<E, A>
where
    E: EntityTrait + Subject,
    A: ActionMarker,
{
    /// The SQL condition on SeaORM entity `E` (see [`EntityColumns`]). Part
    /// of the SeaORM shim.
    pub fn condition(&self) -> SimpleExpr {
        self.sql_filter(&EntityColumns::<E>::new()).to_sea_query()
    }

    /// `select` restricted to the accessible rows. Part of the SeaORM shim.
    pub fn apply(self, select: Select<E>) -> Select<E> {
        let condition = self.condition();
        select.filter(condition)
    }
}

/// `?` from SeaORM's error, for the crates that still speak it.
impl From<DbErr> for AccessError {
    fn from(error: DbErr) -> Self {
        AccessError::Db(DbError::from(error))
    }
}
