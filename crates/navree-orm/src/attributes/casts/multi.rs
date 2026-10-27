//! Fields stored across several columns: [`MultiColumnCast`] and the
//! tuples of [`StoredColumns`].

use std::borrow::Cow;

use crate::attributes::CastContext;
use crate::cast::CastError;
use crate::row::Row;
use crate::value::{Field, FieldError, FieldKind, Value};

/// The columns a [`MultiColumnCast`] stores a field in: a tuple of
/// [`Field`]s, `(A, B)` up to eight, one per column, in the order of the
/// field's `columns(..)`.
pub trait StoredColumns: Sized + 'static {
    /// How many columns.
    const COUNT: usize;

    /// The column values, in order.
    fn to_values(&self) -> Vec<Value>;

    /// The tuple from column values; the error carries the index of the
    /// value that does not fit.
    fn from_values(values: Vec<Value>) -> Result<Self, (usize, FieldError)>;

    /// The tuple from `columns` of `row`, each read with its type's decoder.
    fn from_row(row: &Row, columns: &[&str]) -> Result<Self, (usize, FieldError)>;

    /// Like [`from_row`](Self::from_row), `None` when every column is NULL
    /// (some, but not all, NULL is an error).
    fn from_row_option(row: &Row, columns: &[&str]) -> Result<Option<Self>, (usize, FieldError)>;

    /// A NULL of each column's type.
    fn nulls() -> Vec<Value>;

    /// The Rust type of column `index`, for messages.
    fn type_name(index: usize) -> Cow<'static, str>;
}

/// The column name at `index`, or a placeholder when the list is short.
fn column<'a>(columns: &[&'a str], index: usize) -> Result<&'a str, (usize, FieldError)> {
    columns
        .get(index)
        .copied()
        .ok_or((index, FieldError::Missing))
}

macro_rules! stored_columns {
    ($count:literal => $($ty:ident $index:tt),+) => {
        impl<$($ty: Field),+> StoredColumns for ($($ty,)+) {
            const COUNT: usize = $count;

            fn to_values(&self) -> Vec<Value> {
                vec![$(self.$index.to_value()),+]
            }

            fn from_values(values: Vec<Value>) -> Result<Self, (usize, FieldError)> {
                if values.len() != $count {
                    return Err((values.len().min($count - 1), FieldError::Missing));
                }
                let mut values = values.into_iter();
                Ok(($(
                    $ty::from_value(values.next().unwrap_or_else($ty::null))
                        .map_err(|error| ($index, error))?,
                )+))
            }

            fn from_row(row: &Row, columns: &[&str]) -> Result<Self, (usize, FieldError)> {
                Ok(($(
                    row.field::<$ty>(column(columns, $index)?).map_err(|error| ($index, error))?,
                )+))
            }

            fn from_row_option(
                row: &Row,
                columns: &[&str],
            ) -> Result<Option<Self>, (usize, FieldError)> {
                let values = ($(
                    row.field::<Option<$ty>>(column(columns, $index)?).map_err(|error| ($index, error))?,
                )+);
                let nulls = [$(values.$index.is_none()),+];
                if nulls.iter().all(|null| *null) {
                    return Ok(None);
                }
                Ok(Some(($(
                    values.$index.ok_or(($index, FieldError::Null))?,
                )+)))
            }

            fn nulls() -> Vec<Value> {
                vec![$($ty::null()),+]
            }

            fn type_name(index: usize) -> Cow<'static, str> {
                match index {
                    $($index => $ty::type_name(),)+
                    _ => Cow::Borrowed("?"),
                }
            }
        }
    };
}

stored_columns!(1 => A 0);
stored_columns!(2 => A 0, B 1);
stored_columns!(3 => A 0, B 1, C 2);
stored_columns!(4 => A 0, B 1, C 2, D 3);
stored_columns!(5 => A 0, B 1, C 2, D 3, E 4);
stored_columns!(6 => A 0, B 1, C 2, D 3, E 4, F 5);
stored_columns!(7 => A 0, B 1, C 2, D 3, E 4, F 5, G 6);
stored_columns!(8 => A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);

/// Stores one field across several columns: money as cents and a currency,
/// an address as its lines, a range as its bounds.
///
/// The model names the columns and their types on the field
/// (`#[model(cast = MoneyColumns, columns(price_cents: i64, price_currency:
/// String))]`). Each stored column gets its own typed constant
/// (`Deal::PRICE_CENTS: Column<Deal, i64>`) for conditions and orders, its
/// own dirty tracking and its own key in `fill`; serialized records write
/// the field under its own name (its `Serialize`). A nullable field
/// (`Option<Money>`) is `None` when every column is NULL.
///
/// ```
/// use navree_orm::cast::{CastError, MultiColumnCast};
/// use navree_orm::{Model, Value};
///
/// #[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
/// pub struct Money {
///     pub cents: i64,
///     pub currency: String,
/// }
///
/// pub struct MoneyColumns;
///
/// impl MultiColumnCast<Money> for MoneyColumns {
///     type Stored = (i64, String);
///
///     fn get((cents, currency): (i64, String)) -> Result<Money, CastError> {
///         if currency.len() != 3 {
///             return Err(CastError::new(format!("`{currency}` is not a currency code")));
///         }
///         Ok(Money { cents, currency })
///     }
///
///     fn set(money: &Money) -> Result<(i64, String), CastError> {
///         Ok((money.cents, money.currency.clone()))
///     }
/// }
///
/// #[derive(Model, Clone, Debug, Default, serde::Serialize)]
/// #[model(table = "deals")]
/// pub struct Deal {
///     pub id: i64,
///     #[model(cast = MoneyColumns, columns(price_cents: i64, price_currency: String))]
///     pub price: Money,
/// }
///
/// let deal = Deal { id: 1, price: Money { cents: 999, currency: "EUR".into() } };
/// assert_eq!(deal.value("price_cents")?, Some(Value::from(999_i64)));
/// assert_eq!(deal.value("price_currency")?, Some(Value::from("EUR")));
/// assert_eq!(Deal::PRICE_CURRENCY.name(), "price_currency");
/// assert!(MoneyColumns::get((1, "EURO".into())).is_err());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub trait MultiColumnCast<T>: 'static {
    /// The stored columns' types, as a tuple in the order of `columns(..)`.
    type Stored: StoredColumns;

    /// Whether the cast reads services from a [`CastContext`].
    const CONTEXT: bool = false;

    /// What the field's values are in serialized records.
    const KIND: FieldKind = FieldKind::Any;

    /// The field from its stored columns.
    fn get(stored: Self::Stored) -> Result<T, CastError>;

    /// The stored columns of the field.
    fn set(value: &T) -> Result<Self::Stored, CastError>;

    /// [`get`](MultiColumnCast::get) with the services of `context`.
    fn get_with(stored: Self::Stored, context: &CastContext) -> Result<T, CastError> {
        let _ = context;
        Self::get(stored)
    }

    /// [`set`](MultiColumnCast::set) with the services of `context`.
    fn set_with(value: &T, context: &CastContext) -> Result<Self::Stored, CastError> {
        let _ = context;
        Self::set(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tuples_convert_and_report_the_failing_column() {
        let stored = (7_i64, "EUR".to_owned());
        let values = stored.to_values();
        assert_eq!(values, [Value::BigInt(Some(7)), Value::from("EUR")]);
        assert_eq!(<(i64, String)>::from_values(values).unwrap(), stored);
        let (index, error) =
            <(i64, String)>::from_values(vec![Value::from("x"), Value::from("EUR")]).unwrap_err();
        assert_eq!(index, 0);
        assert_eq!(error, FieldError::Invalid("`x` is not an integer".into()));
        assert_eq!(
            <(i64, String)>::from_values(vec![Value::BigInt(Some(1))])
                .unwrap_err()
                .0,
            1
        );
        assert_eq!(
            <(i64, String)>::nulls(),
            [Value::BigInt(None), Value::String(None)]
        );
        assert_eq!(<(i64, Option<String>)>::type_name(1), "Option<String>");
    }

    #[test]
    fn rows_read_every_column_or_none_when_all_are_null() {
        let row = Row::from_values(vec![
            ("cents".into(), Value::BigInt(Some(5))),
            ("currency".into(), Value::from("USD")),
        ]);
        let read = <(i64, String)>::from_row(&row, &["cents", "currency"]).unwrap();
        assert_eq!(read, (5, "USD".to_owned()));
        let nulls = Row::from_values(vec![
            ("cents".into(), Value::BigInt(None)),
            ("currency".into(), Value::String(None)),
        ]);
        assert_eq!(
            <(i64, String)>::from_row_option(&nulls, &["cents", "currency"]).unwrap(),
            None
        );
        let half = Row::from_values(vec![
            ("cents".into(), Value::BigInt(Some(5))),
            ("currency".into(), Value::String(None)),
        ]);
        assert_eq!(
            <(i64, String)>::from_row_option(&half, &["cents", "currency"]).unwrap_err(),
            (1, FieldError::Null)
        );
        assert_eq!(
            <(i64, String)>::from_row(&row, &["cents"]).unwrap_err(),
            (1, FieldError::Missing)
        );
    }
}
