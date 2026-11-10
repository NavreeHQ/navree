//! A value a chart can draw, and the cleaning every value goes through.

/// Something a series can hold: a number, or nothing.
///
/// Integers and floats of any width convert, and an `Option` of one is a value or a
/// gap. NaN and the infinities become gaps, so no value can break a scale.
pub trait IntoValue {
    /// The value as a finite number, or `None` for a gap.
    fn into_value(self) -> Option<f64>;
}

macro_rules! lossless {
    ($($t:ty),+) => {
        $(impl IntoValue for $t {
            #[inline]
            fn into_value(self) -> Option<f64> {
                Some(f64::from(self))
            }
        })+
    };
}
lossless!(i8, i16, i32, u8, u16, u32);

impl IntoValue for f32 {
    #[inline]
    fn into_value(self) -> Option<f64> {
        f64::from(self).into_value()
    }
}

macro_rules! wide {
    ($($t:ty),+) => {
        $(impl IntoValue for $t {
            #[inline]
            #[allow(clippy::cast_precision_loss)]
            fn into_value(self) -> Option<f64> {
                // Past 2^53 a count loses its last digits, which no chart can show anyway.
                Some(self as f64)
            }
        })+
    };
}
wide!(i64, u64, isize, usize, i128, u128);

impl IntoValue for f64 {
    #[inline]
    fn into_value(self) -> Option<f64> {
        self.is_finite().then_some(self)
    }
}

impl<T: IntoValue> IntoValue for Option<T> {
    #[inline]
    fn into_value(self) -> Option<f64> {
        self.and_then(IntoValue::into_value)
    }
}

impl<T: IntoValue + Copy> IntoValue for &T {
    #[inline]
    fn into_value(self) -> Option<f64> {
        (*self).into_value()
    }
}

/// `value`, when it is finite.
#[inline]
pub(crate) fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite())
}

/// The values, cleaned.
pub(crate) fn clean<V: IntoValue>(values: impl IntoIterator<Item = V>) -> Vec<Option<f64>> {
    values.into_iter().map(|v| finite(v.into_value())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_come_from_nothing_and_from_numbers_that_are_not() {
        assert_eq!(
            clean([1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.5]),
            [Some(1.0), None, None, None, Some(-0.5)]
        );
        assert_eq!(clean([Some(3_u32), None]), [Some(3.0), None]);
        assert_eq!(clean([u64::MAX]), [Some(u64::MAX as f64)]);
        assert_eq!(clean([f32::NAN]), [None]);
    }
}
