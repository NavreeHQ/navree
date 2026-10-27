//! `cast = "decimal(N)"`: decimals at a fixed scale.

use navree_kernel::BoxFuture;
use rust_decimal::RoundingStrategy;

use crate::attributes::CastContext;
use crate::cast::{Cast, CastError};
use crate::value::{Decimal, FieldKind};

/// A [`Decimal`] kept at `SCALE` decimal places (`#[model(cast =
/// "decimal(2)")]`): rounded half away from zero when written (`12.345` →
/// `12.35`, `-12.345` → `-12.35`) and when read, and always carrying the
/// scale (`12.5` reads as `12.50`), so amounts compare and print alike
/// on every database.
///
/// The record holds the rounded value once it is saved, as the database
/// does. Input that is not a decimal is a cast error naming the column.
///
/// ```
/// use navree_orm::Decimal;
/// use navree_orm::cast::{Cast, DecimalScale};
///
/// let stored = DecimalScale::<2>::set(&Decimal::new(12_345, 3))?; // 12.345
/// assert_eq!(stored.to_string(), "12.35");
/// let read: Decimal = DecimalScale::<2>::get(Decimal::new(125, 1))?; // 12.5
/// assert_eq!(read.to_string(), "12.50");
/// assert_eq!(DecimalScale::<0>::set(&Decimal::new(-25, 1))?.to_string(), "-3");
/// # Ok::<(), navree_orm::cast::CastError>(())
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct DecimalScale<const SCALE: u32>;

impl<const SCALE: u32> DecimalScale<SCALE> {
    /// `value` at the scale, rounded half away from zero.
    pub fn round(value: Decimal) -> Decimal {
        let mut rounded = value.round_dp_with_strategy(SCALE, RoundingStrategy::MidpointAwayFromZero);
        rounded.rescale(SCALE);
        rounded
    }
}

impl<const SCALE: u32> Cast<Decimal> for DecimalScale<SCALE> {
    type Stored = Decimal;

    const KIND: FieldKind = FieldKind::Decimal;

    fn get(stored: Decimal) -> Result<Decimal, CastError> {
        Ok(Self::round(stored))
    }

    fn set(value: &Decimal) -> Result<Decimal, CastError> {
        Ok(Self::round(*value))
    }

    /// The record keeps the value the database stores.
    fn prepare<'a>(
        value: &'a mut Decimal,
        context: &'a CastContext,
    ) -> BoxFuture<'a, Result<(), CastError>> {
        let _ = context;
        *value = Self::round(*value);
        Box::pin(std::future::ready(Ok(())))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn rounds_half_away_from_zero_and_keeps_the_scale() {
        let cases = [
            ("12.345", "12.35"),
            ("12.344", "12.34"),
            ("-12.345", "-12.35"),
            ("12.5", "12.50"),
            ("0", "0.00"),
            ("1.005", "1.01"),
        ];
        for (input, expected) in cases {
            let value: Decimal = input.parse().unwrap();
            assert_eq!(
                DecimalScale::<2>::set(&value).unwrap().to_string(),
                expected,
                "{input}"
            );
        }
    }

    #[test]
    fn input_must_be_a_decimal() {
        let context = CastContext::new();
        assert_eq!(
            DecimalScale::<2>::from_json(json!("19.999"), &context)
                .unwrap()
                .to_string(),
            "20.00"
        );
        assert_eq!(
            DecimalScale::<1>::from_json(json!(2.25), &context)
                .unwrap()
                .to_string(),
            "2.3"
        );
        let error = DecimalScale::<2>::from_json(json!("twelve"), &context).unwrap_err();
        assert_eq!(error.to_string(), "`twelve` is not a decimal");
    }

    #[tokio::test]
    async fn preparation_rounds_the_field_in_place() {
        let mut value: Decimal = "7.125".parse().unwrap();
        DecimalScale::<2>::prepare(&mut value, &CastContext::new())
            .await
            .unwrap();
        assert_eq!(value.to_string(), "7.13");
    }
}
