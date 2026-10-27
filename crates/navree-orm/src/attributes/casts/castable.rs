//! [`Castable`]: value types that pick their own cast.

use crate::cast::Cast;

/// A value type that names the cast its fields are stored through, so a
/// model only writes `#[model(cast)]` on them.
///
/// The alternative is implementing [`Field`](crate::Field) on the type
/// directly, which also makes it usable in rows, plucks and conditions on
/// untyped columns.
///
/// ```
/// use navree_orm::cast::{Cast, CastError, Castable};
///
/// /// An amount in cents with its currency, stored as `"12.50 EUR"`.
/// #[derive(Clone, Debug, PartialEq)]
/// pub struct Money {
///     pub cents: i64,
///     pub currency: String,
/// }
///
/// pub struct MoneyText;
///
/// impl Cast<Money> for MoneyText {
///     type Stored = String;
///
///     fn get(stored: String) -> Result<Money, CastError> {
///         let (amount, currency) = stored
///             .split_once(' ')
///             .ok_or_else(|| CastError::new(format!("`{stored}` is not an amount and a currency")))?;
///         let (units, cents) = amount.split_once('.').unwrap_or((amount, "0"));
///         let parse = |s: &str| s.parse::<i64>().map_err(|_| CastError::new(format!("`{stored}` is not an amount")));
///         Ok(Money { cents: parse(units)? * 100 + parse(cents)?, currency: currency.to_owned() })
///     }
///
///     fn set(money: &Money) -> Result<String, CastError> {
///         Ok(format!("{}.{:02} {}", money.cents / 100, money.cents % 100, money.currency))
///     }
/// }
///
/// impl Castable for Money {
///     type Cast = MoneyText;
/// }
///
/// let price = Money { cents: 1250, currency: "EUR".into() };
/// assert_eq!(<Money as Castable>::Cast::set(&price)?, "12.50 EUR");
/// # Ok::<(), CastError>(())
/// ```
pub trait Castable: Sized {
    /// The cast fields of this type are stored through.
    type Cast: Cast<Self>;
}
