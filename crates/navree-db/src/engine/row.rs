//! [`Row`]: one row a query returned.

use std::fmt;
use std::sync::Arc;

use crate::engine::driver::DriverRow;
use crate::engine::{Backend, DbError, ErrorKind, FromValue, Value};

/// One row a query returned, read by column name or position.
///
/// [`get`](Row::get) reads a column as any [`FromValue`] type (tolerantly:
/// an `i64` from any integer column, a `Uuid` from text or bytes, a
/// `DateTime<Utc>` from SQLite's text); [`value`](Row::value) reads it as a
/// [`Value`] by the column's type; [`to_json`](Row::to_json) gives the whole
/// row as a JSON object.
///
/// ```
/// use navree_db::{Row, Value};
///
/// let row = Row::from_values([("id", Value::I64(7)), ("title", Value::from("Renewal"))]);
/// assert_eq!(row.get::<i64>("id").unwrap(), 7);
/// assert_eq!(row.get::<String>(1).unwrap(), "Renewal");
/// assert_eq!(row.get::<Option<String>>("title").unwrap().as_deref(), Some("Renewal"));
/// assert!(row.get::<i64>("missing").is_err());
/// ```
pub struct Row {
    inner: RowInner,
}

enum RowInner {
    /// As the driver returned it.
    Driver(DriverRow),
    /// A row SeaORM read (statements run through a SeaORM connection while
    /// crates move off it).
    Sea(sea_orm::QueryResult),
    /// Values built by hand.
    Values {
        columns: Arc<[String]>,
        values: Vec<Value>,
    },
}

/// A column of a [`Row`]: its position (`usize`) or its name (`&str`).
pub trait ColumnIndex: fmt::Debug {
    /// The column's position in `row`.
    fn index_in(&self, row: &Row) -> Result<usize, DbError>;
}

impl ColumnIndex for usize {
    fn index_in(&self, row: &Row) -> Result<usize, DbError> {
        if *self < row.len() {
            Ok(*self)
        } else {
            Err(DbError::new(
                ErrorKind::Decode,
                format!("no column {self}: the row has {} column(s)", row.len()),
            ))
        }
    }
}

impl ColumnIndex for &str {
    fn index_in(&self, row: &Row) -> Result<usize, DbError> {
        row.column_index(self).ok_or_else(|| {
            DbError::new(
                ErrorKind::Decode,
                format!(
                    "no column named `{self}` (the row has: {})",
                    row.columns().join(", ")
                ),
            )
        })
    }
}

impl ColumnIndex for String {
    fn index_in(&self, row: &Row) -> Result<usize, DbError> {
        self.as_str().index_in(row)
    }
}

impl ColumnIndex for &String {
    fn index_in(&self, row: &Row) -> Result<usize, DbError> {
        self.as_str().index_in(row)
    }
}

impl Row {
    /// A row of the given columns and values.
    pub fn from_values<N: Into<String>>(columns: impl IntoIterator<Item = (N, Value)>) -> Row {
        let (names, values): (Vec<String>, Vec<Value>) = columns
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .unzip();
        Row {
            inner: RowInner::Values {
                columns: names.into(),
                values,
            },
        }
    }

    pub(crate) fn from_driver(row: DriverRow) -> Row {
        Row {
            inner: RowInner::Driver(row),
        }
    }

    /// The backend the row came from (`None` for a row built by hand).
    pub fn backend(&self) -> Option<Backend> {
        match &self.inner {
            RowInner::Driver(row) => Some(row.backend()),
            RowInner::Sea(row) => sea_row(row).map(|row| row.backend()),
            RowInner::Values { .. } => None,
        }
    }

    /// How many columns.
    pub fn len(&self) -> usize {
        match &self.inner {
            RowInner::Driver(row) => row.len(),
            RowInner::Sea(row) => sea_row(row).map_or(0, |row| row.len()),
            RowInner::Values { columns, .. } => columns.len(),
        }
    }

    /// Whether the row has no columns.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The column names, in order.
    pub fn columns(&self) -> Vec<String> {
        match &self.inner {
            RowInner::Driver(row) => row.columns().into_iter().map(str::to_owned).collect(),
            RowInner::Sea(row) => sea_row(row).map_or_else(Vec::new, |row| row.columns()),
            RowInner::Values { columns, .. } => columns.to_vec(),
        }
    }

    /// The position of the first column named `name`.
    pub fn column_index(&self, name: &str) -> Option<usize> {
        match &self.inner {
            RowInner::Driver(row) => row.columns().iter().position(|column| *column == name),
            RowInner::Sea(_) => self.columns().iter().position(|column| column == name),
            RowInner::Values { columns, .. } => columns.iter().position(|column| column == name),
        }
    }

    /// Whether the row has a column named `name`.
    pub fn has_column(&self, name: &str) -> bool {
        self.column_index(name).is_some()
    }

    /// A column as a [`Value`], read by the column's type.
    pub fn value(&self, column: impl ColumnIndex) -> Result<Value, DbError> {
        let index = column.index_in(self)?;
        match &self.inner {
            RowInner::Driver(row) => row.value(index),
            RowInner::Sea(row) => sea_row(row)
                .ok_or_else(|| {
                    DbError::unsupported("a row of a scripted connection cannot be read")
                })?
                .value(index),
            RowInner::Values { values, .. } => Ok(values[index].clone()),
        }
    }

    /// A column as `T` (see [`FromValue`]); `Option<T>` reads `NULL` as
    /// `None`.
    pub fn get<T: FromValue>(&self, column: impl ColumnIndex) -> Result<T, DbError> {
        let index = column.index_in(self)?;
        let value = self.value(index)?;
        T::from_value(value).map_err(|error| {
            let name = self.columns().get(index).cloned().unwrap_or_default();
            DbError::new(ErrorKind::Decode, format!("column `{name}`: {error}"))
        })
    }

    /// Every column as a [`Value`], in order.
    pub fn values(&self) -> Result<Vec<Value>, DbError> {
        (0..self.len()).map(|index| self.value(index)).collect()
    }

    /// The row as a JSON object (see [`Value::to_json`]); a repeated
    /// column name keeps its first value.
    pub fn to_json(&self) -> Result<serde_json::Map<String, serde_json::Value>, DbError> {
        let mut object = serde_json::Map::new();
        for (index, name) in self.columns().into_iter().enumerate() {
            if !object.contains_key(&name) {
                object.insert(name, self.value(index)?.to_json());
            }
        }
        Ok(object)
    }

    /// The SeaORM row, when the row came through a SeaORM connection.
    pub fn as_sea_orm(&self) -> Option<&sea_orm::QueryResult> {
        match &self.inner {
            RowInner::Sea(row) => Some(row),
            _ => None,
        }
    }
}

impl From<sea_orm::QueryResult> for Row {
    /// A SeaORM row: read through its driver row, or (a row of a scripted
    /// connection, which has none) by its values.
    fn from(row: sea_orm::QueryResult) -> Row {
        if sea_row(&row).is_none() {
            return crate::engine::compat::scripted_row(&row);
        }
        Row {
            inner: RowInner::Sea(row),
        }
    }
}

impl fmt::Debug for Row {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for (index, name) in self.columns().iter().enumerate() {
            match self.value(index) {
                Ok(value) => map.entry(name, &value),
                Err(error) => map.entry(name, &error.to_string()),
            };
        }
        map.finish()
    }
}

/// The driver row inside a SeaORM row.
fn sea_row(row: &sea_orm::QueryResult) -> Option<SeaRow<'_>> {
    #[cfg(feature = "postgres")]
    if let Some(row) = row.try_as_pg_row() {
        return Some(SeaRow::Postgres(row));
    }
    #[cfg(feature = "mysql")]
    if let Some(row) = row.try_as_mysql_row() {
        return Some(SeaRow::MySql(row));
    }
    #[cfg(feature = "sqlite")]
    if let Some(row) = row.try_as_sqlite_row() {
        return Some(SeaRow::Sqlite(row));
    }
    let _ = row;
    None
}

/// A borrowed driver row of a SeaORM row.
enum SeaRow<'a> {
    #[cfg(feature = "postgres")]
    Postgres(&'a sqlx::postgres::PgRow),
    #[cfg(feature = "mysql")]
    MySql(&'a sqlx::mysql::MySqlRow),
    #[cfg(feature = "sqlite")]
    Sqlite(&'a sqlx::sqlite::SqliteRow),
    #[allow(dead_code)]
    Never(std::marker::PhantomData<&'a ()>, std::convert::Infallible),
}

impl SeaRow<'_> {
    fn backend(&self) -> Backend {
        match self {
            #[cfg(feature = "postgres")]
            SeaRow::Postgres(_) => Backend::Postgres,
            #[cfg(feature = "mysql")]
            SeaRow::MySql(_) => Backend::MySql,
            #[cfg(feature = "sqlite")]
            SeaRow::Sqlite(_) => Backend::Sqlite,
            SeaRow::Never(_, never) => match *never {},
        }
    }

    fn len(&self) -> usize {
        use sqlx::Row as _;
        match self {
            #[cfg(feature = "postgres")]
            SeaRow::Postgres(row) => row.len(),
            #[cfg(feature = "mysql")]
            SeaRow::MySql(row) => row.len(),
            #[cfg(feature = "sqlite")]
            SeaRow::Sqlite(row) => row.len(),
            SeaRow::Never(_, never) => match *never {},
        }
    }

    fn columns(&self) -> Vec<String> {
        use sqlx::{Column as _, Row as _};
        match self {
            #[cfg(feature = "postgres")]
            SeaRow::Postgres(row) => row.columns().iter().map(|c| c.name().to_owned()).collect(),
            #[cfg(feature = "mysql")]
            SeaRow::MySql(row) => row.columns().iter().map(|c| c.name().to_owned()).collect(),
            #[cfg(feature = "sqlite")]
            SeaRow::Sqlite(row) => row.columns().iter().map(|c| c.name().to_owned()).collect(),
            SeaRow::Never(_, never) => match *never {},
        }
    }

    fn value(&self, index: usize) -> Result<Value, DbError> {
        match self {
            #[cfg(feature = "postgres")]
            SeaRow::Postgres(row) => crate::engine::driver::postgres::value(row, index),
            #[cfg(feature = "mysql")]
            SeaRow::MySql(row) => crate::engine::driver::mysql::value(row, index),
            #[cfg(feature = "sqlite")]
            SeaRow::Sqlite(row) => crate::engine::driver::sqlite::value(row, index),
            SeaRow::Never(_, never) => match *never {},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_built_by_hand_read_by_name_and_position() {
        let row = Row::from_values([
            ("id", Value::I32(1)),
            ("note", Value::Null(crate::Kind::String)),
            ("id", Value::I32(2)),
        ]);
        assert_eq!(row.len(), 3);
        assert_eq!(row.columns(), ["id", "note", "id"]);
        assert_eq!(row.get::<i64>("id").unwrap(), 1);
        assert_eq!(row.get::<i64>(2).unwrap(), 2);
        assert_eq!(row.get::<Option<String>>("note").unwrap(), None);
        let error = row.get::<String>("note").unwrap_err();
        assert_eq!(
            error.to_string(),
            "column `note`: expected String, found NULL"
        );
        assert_eq!(
            row.get::<i64>("nope").unwrap_err().to_string(),
            "no column named `nope` (the row has: id, note, id)"
        );
        assert_eq!(
            row.to_json().unwrap(),
            serde_json::json!({"id": 1, "note": null})
                .as_object()
                .unwrap()
                .clone()
        );
        assert!(row.backend().is_none());
    }
}
