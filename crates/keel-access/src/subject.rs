//! Subjects: the permission prefix map, record loaders for `can(.., from)`
//! checks, and the record/SQL mapping of entities ([`record_json`],
//! [`EntityColumns`]) that keeps record checks and list filters in step.

use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use chrono::{DateTime, SecondsFormat, Utc};
use keel_db::Uow;
use keel_kernel::{AppContext, BoxFuture};
use sea_orm::sea_query::value::sea_value_to_json_value;
use sea_orm::sea_query::{Expr, Value as SqlValue};
use sea_orm::{
    ColumnTrait, ColumnType, DbErr, EntityTrait, IdenStatic, Iterable, ModelTrait,
    PrimaryKeyToColumn, QueryFilter, QuerySelect, Select,
};
use serde_json::Value;

use crate::engine::{SqlColumns, Subject, sql_value};
use crate::error::AccessError;

/// A subject registered with the access module (a module contribution).
pub(crate) struct SubjectRegistration {
    pub(crate) subject: &'static str,
    pub(crate) prefix: &'static str,
    pub(crate) type_name: &'static str,
    pub(crate) loader: Option<Arc<dyn SubjectLoader>>,
}

impl SubjectRegistration {
    /// A subject without a record loader.
    pub(crate) fn of<S: Subject + 'static>() -> Self {
        Self {
            subject: S::SUBJECT,
            prefix: S::PERMISSION_PREFIX,
            type_name: std::any::type_name::<S>(),
            loader: None,
        }
    }

    /// An entity subject whose records `can(.., from = "id")` routes load.
    pub(crate) fn entity<E>() -> Self
    where
        E: EntityTrait + Subject,
        E::Model: Send + Sync + 'static,
    {
        Self {
            loader: Some(Arc::new(EntityLoader::<E>(PhantomData))),
            ..Self::of::<E>()
        }
    }
}

impl fmt::Debug for SubjectRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubjectRegistration")
            .field("subject", &self.subject)
            .field("prefix", &self.prefix)
            .field("type", &self.type_name)
            .field("loader", &self.loader.is_some())
            .finish()
    }
}

/// A loaded record: its JSON form (column names → values, as conditions see
/// it) and the model itself, for `Authorized<E, A>`.
pub(crate) struct LoadedRecord {
    pub(crate) json: Value,
    pub(crate) model: Arc<dyn Any + Send + Sync>,
}

/// Loads records of one subject by the id in a path parameter.
pub(crate) trait SubjectLoader: Send + Sync + 'static {
    /// The record with primary key `id` in `uow`'s transaction (`FOR UPDATE`
    /// where the backend supports it); `None` when there is none or `id`
    /// is not a valid key.
    fn load<'a>(
        &'a self,
        uow: &'a Uow,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<LoadedRecord>, DbErr>>;
}

struct EntityLoader<E>(PhantomData<fn() -> E>);

impl<E> SubjectLoader for EntityLoader<E>
where
    E: EntityTrait,
    E::Model: Send + Sync + 'static,
{
    fn load<'a>(
        &'a self,
        uow: &'a Uow,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<LoadedRecord>, DbErr>> {
        Box::pin(async move {
            let Some(mut select) = select_by_key::<E>(id)? else {
                return Ok(None);
            };
            if uow.lock_for_update() {
                select = select.lock_exclusive();
            }
            let model = {
                let tx = uow.tx().await?;
                select.one(&*tx).await?
            };
            Ok(model.map(|model| LoadedRecord {
                json: record_json(&model),
                model: Arc::new(model),
            }))
        })
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
fn key_value(column_type: &ColumnType, id: &str) -> Option<SqlValue> {
    match column_type {
        ColumnType::Uuid => uuid::Uuid::parse_str(id).ok().map(SqlValue::from),
        ColumnType::TinyInteger
        | ColumnType::SmallInteger
        | ColumnType::Integer
        | ColumnType::BigInteger
        | ColumnType::TinyUnsigned
        | ColumnType::SmallUnsigned
        | ColumnType::Unsigned
        | ColumnType::BigUnsigned => id.parse::<i64>().ok().map(SqlValue::from),
        _ => Some(SqlValue::from(id.to_owned())),
    }
}

/// The JSON form of `model` that ability conditions are checked against:
/// one member per column, named like the column, so a record check sees
/// exactly what the SQL list filter ([`EntityColumns`]) sees.
///
/// UUIDs become hyphenated lower-case strings and timestamps RFC 3339 UTC
/// strings with microseconds (`2026-10-25T12:00:00.250000Z`), the forms
/// [`IntoCondValue`](crate::IntoCondValue) gives condition operands.
pub fn record_json<M: ModelTrait>(model: &M) -> Value {
    let object = <M::Entity as EntityTrait>::Column::iter()
        .map(|column| {
            let name = column.as_str().to_owned();
            (name, sql_to_json(&model.get(column)))
        })
        .collect();
    Value::Object(object)
}

/// A database value as a condition operand would spell it.
fn sql_to_json(value: &SqlValue) -> Value {
    let micros = |dt: DateTime<Utc>| Value::String(dt.to_rfc3339_opts(SecondsFormat::Micros, true));
    match value {
        SqlValue::Uuid(Some(uuid)) => Value::String(uuid.hyphenated().to_string()),
        SqlValue::ChronoDateTimeUtc(Some(dt)) => micros(*dt),
        SqlValue::ChronoDateTimeLocal(Some(dt)) => micros(dt.with_timezone(&Utc)),
        SqlValue::ChronoDateTimeWithTimeZone(Some(dt)) => micros(dt.with_timezone(&Utc)),
        SqlValue::ChronoDateTime(Some(dt)) => micros(dt.and_utc()),
        SqlValue::ChronoDate(Some(date)) => Value::String(date.format("%Y-%m-%d").to_string()),
        SqlValue::ChronoTime(Some(time)) => Value::String(time.format("%H:%M:%S%.f").to_string()),
        // Binary columns have no condition form (and may not be UTF-8).
        SqlValue::Bytes(_) => Value::Null,
        other => sea_value_to_json_value(other),
    }
}

/// [`SqlColumns`] for entity `E`: rule fields are column names, qualified
/// with the entity's table, and operands are bound with the column's type
/// (UUID and timestamp columns get typed values, so they compare like the
/// stored values). A field that is not a column of `E` is SQL `NULL`, as
/// [`record_json`] has no such member and a record check sees `null`.
///
/// Use it with [`Ability::sql_filter_with`](crate::Ability::sql_filter_with)
/// (which [`Accessible`](crate::Accessible) does for you).
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
    fn find(field: &str) -> Option<E::Column> {
        E::Column::iter().find(|column| column.as_str() == field)
    }
}

impl<E: EntityTrait> SqlColumns for EntityColumns<E> {
    fn column(&self, field: &str) -> Expr {
        match Self::find(field) {
            Some(column) => Expr::col((E::default(), column)),
            None => Expr::cust("NULL"),
        }
    }

    fn value(&self, field: &str, value: &Value) -> SqlValue {
        let typed = Self::find(field).and_then(|column| {
            let text = value.as_str()?;
            match column.def().get_column_type() {
                ColumnType::Uuid => uuid::Uuid::parse_str(text).ok().map(SqlValue::from),
                ColumnType::TimestampWithTimeZone => DateTime::parse_from_rfc3339(text)
                    .ok()
                    .map(|dt| SqlValue::from(dt.with_timezone(&Utc))),
                ColumnType::DateTime | ColumnType::Timestamp => DateTime::parse_from_rfc3339(text)
                    .ok()
                    .map(|dt| SqlValue::from(dt.with_timezone(&Utc).naive_utc())),
                _ => None,
            }
        });
        typed.unwrap_or_else(|| sql_value(value))
    }

    fn accepts(&self, field: &str, value: &Value) -> bool {
        let Some(column) = Self::find(field) else {
            // Not a column: SQL `NULL`, which matches nothing either way.
            return true;
        };
        match column.def().get_column_type() {
            ColumnType::Boolean => value.is_boolean(),
            ColumnType::TinyInteger
            | ColumnType::SmallInteger
            | ColumnType::Integer
            | ColumnType::BigInteger
            | ColumnType::TinyUnsigned
            | ColumnType::SmallUnsigned
            | ColumnType::Unsigned
            | ColumnType::BigUnsigned
            | ColumnType::Float
            | ColumnType::Double => value.is_number(),
            ColumnType::Char(_)
            | ColumnType::String(_)
            | ColumnType::Text
            | ColumnType::Uuid
            | ColumnType::Date
            | ColumnType::Time
            | ColumnType::DateTime
            | ColumnType::Timestamp
            | ColumnType::TimestampWithTimeZone => value.is_string(),
            // Decimals travel as strings or numbers; JSON, enums and the
            // rest are compared as SQL compares them.
            _ => true,
        }
    }
}

/// Every registered subject: permission prefixes and record loaders.
#[derive(Default)]
pub(crate) struct Subjects {
    /// Permission prefix → subject type.
    pub(crate) prefixes: Arc<BTreeMap<String, String>>,
    loaders: HashMap<String, Arc<dyn SubjectLoader>>,
}

impl Subjects {
    /// Collects the subjects contributed by the modules of `ctx`.
    pub(crate) fn collect(ctx: &AppContext) -> Result<Subjects, AccessError> {
        let mut prefixes: BTreeMap<String, String> = BTreeMap::new();
        let mut owners: HashMap<&'static str, &'static str> = HashMap::new();
        let mut loaders = HashMap::new();
        for contributed in ctx.contributions::<SubjectRegistration>() {
            let registration = &contributed.value;
            if let Some(other) = owners.insert(registration.subject, registration.type_name)
                && other != registration.type_name
            {
                return Err(AccessError::Config(format!(
                    "subject `{}` is registered by both `{other}` and `{}`",
                    registration.subject, registration.type_name
                )));
            }
            match prefixes.get(registration.prefix) {
                Some(subject) if subject != registration.subject => {
                    return Err(AccessError::Config(format!(
                        "permission prefix `{}` maps to both `{subject}` and `{}`",
                        registration.prefix, registration.subject
                    )));
                }
                _ => {
                    prefixes.insert(
                        registration.prefix.to_owned(),
                        registration.subject.to_owned(),
                    );
                }
            }
            if let Some(loader) = &registration.loader {
                loaders.insert(registration.subject.to_owned(), loader.clone());
            }
        }
        Ok(Subjects {
            prefixes: Arc::new(prefixes),
            loaders,
        })
    }

    /// The record loader of `subject`.
    pub(crate) fn loader(&self, subject: &str) -> Option<&Arc<dyn SubjectLoader>> {
        self.loaders.get(subject)
    }

    /// The (action, subject type) a permission string checks: `posts.publish`
    /// → (`publish`, `Post`); `*` means `manage`. An unknown prefix stands
    /// for itself (`audit.read` → (`read`, `audit`)), as in
    /// [`Rules::from_permissions`](crate::Rules::from_permissions), so the
    /// permission itself (or `manage all`) grants it.
    pub(crate) fn permission_target<'a>(&'a self, permission: &'a str) -> (&'a str, &'a str) {
        let (prefix, action) = permission.rsplit_once('.').unwrap_or((permission, ""));
        let action = if action == "*" { crate::MANAGE } else { action };
        let subject = self
            .prefixes
            .get(prefix)
            .map(String::as_str)
            .unwrap_or(prefix);
        (action, subject)
    }
}
