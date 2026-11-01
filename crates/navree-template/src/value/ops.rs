//! Arithmetic, concatenation, containment and slicing of values.
//!
//! Error messages name the kinds of the operands, never their values.

use super::{Coerced, Repr, SeqKind, Value, as_f64, coerce};
use crate::error::Error;

fn impossible(op: &str, lhs: &Value, rhs: &Value) -> Error {
    Error::invalid(format!(
        "tried to use {op} operator on unsupported types {} and {}",
        lhs.kind(),
        rhs.kind()
    ))
}

fn failed(op: &str) -> Error {
    Error::invalid(format!("unable to calculate the result of {op}"))
}

/// `a + b`: numbers add, strings and sequences concatenate.
pub(crate) fn add(lhs: &Value, rhs: &Value) -> Result<Value, Error> {
    if lhs.is_tuple() || rhs.is_tuple() {
        if let (Repr::Seq(a, ..), Repr::Seq(b, ..)) = (&lhs.0, &rhs.0)
            && lhs.is_tuple()
            && rhs.is_tuple()
        {
            return Ok(Value::tuple(concat_items(a, b)));
        }
        return Err(impossible("+", lhs, rhs));
    }
    if let (Repr::Seq(a, ..), Repr::Seq(b, ..)) = (&lhs.0, &rhs.0) {
        return Ok(Value::iterable(concat_items(a, b)));
    }
    match coerce(lhs, rhs, true) {
        Some(Coerced::Int(a, b)) => a.checked_add(b).map(Value::from).ok_or_else(|| failed("+")),
        Some(Coerced::Float(a, b)) => Ok(Value::from(a + b)),
        Some(Coerced::Str(a, b)) => Ok(Value::from([a, b].concat())),
        None => Err(impossible("+", lhs, rhs)),
    }
}

fn concat_items(a: &[Value], b: &[Value]) -> Vec<Value> {
    let mut items = Vec::with_capacity(a.len() + b.len());
    items.extend_from_slice(a);
    items.extend_from_slice(b);
    items
}

/// `a - b`.
pub(crate) fn sub(lhs: &Value, rhs: &Value) -> Result<Value, Error> {
    match coerce(lhs, rhs, true) {
        Some(Coerced::Int(a, b)) => a.checked_sub(b).map(Value::from).ok_or_else(|| failed("-")),
        Some(Coerced::Float(a, b)) => Ok(Value::from(a - b)),
        _ => Err(impossible("-", lhs, rhs)),
    }
}

/// `a * b`: numbers multiply; a string or sequence repeats, up to
/// `max_bytes` bytes or `max_items` items.
pub(crate) fn mul(
    lhs: &Value,
    rhs: &Value,
    max_bytes: usize,
    max_items: usize,
) -> Result<Value, Error> {
    if let Some((text, count)) = lhs
        .as_str()
        .map(|s| (s, rhs))
        .or_else(|| rhs.as_str().map(|s| (s, lhs)))
    {
        let count = count
            .as_usize()
            .ok_or_else(|| Error::invalid("strings can only be multiplied with integers"))?;
        if !matches!(text.len().checked_mul(count), Some(len) if len <= max_bytes) {
            return Err(Error::limit("repeated string is too large"));
        }
        return Ok(Value::from(text.repeat(count)));
    }
    let sequence = match (&lhs.0, &rhs.0) {
        (Repr::Seq(values, kind, _), _) => Some((values, *kind, rhs)),
        (_, Repr::Seq(values, kind, _)) => Some((values, *kind, lhs)),
        _ => None,
    };
    if let Some((values, kind, count)) = sequence {
        let count = count.as_usize().ok_or_else(|| {
            Error::invalid("sequences and iterables can only be multiplied with integers")
        })?;
        if !matches!(values.len().checked_mul(count), Some(len) if len <= max_items) {
            return Err(Error::limit("repeated sequence is too large"));
        }
        let mut items = Vec::with_capacity(values.len() * count);
        for _ in 0..count {
            items.extend_from_slice(values);
        }
        return Ok(match kind {
            SeqKind::Tuple => Value::tuple(items),
            _ if items.is_empty() => Value::list(items),
            _ => Value::iterable(items),
        });
    }
    match coerce(lhs, rhs, true) {
        Some(Coerced::Int(a, b)) => a.checked_mul(b).map(Value::from).ok_or_else(|| failed("*")),
        Some(Coerced::Float(a, b)) => Ok(Value::from(a * b)),
        _ => Err(impossible("*", lhs, rhs)),
    }
}

/// `a / b`: always a float.
pub(crate) fn div(lhs: &Value, rhs: &Value) -> Result<Value, Error> {
    let (Some(a), Some(b)) = (as_f64(lhs, true), as_f64(rhs, true)) else {
        return Err(impossible("/", lhs, rhs));
    };
    if b == 0.0 {
        return Err(failed("/"));
    }
    Ok(Value::from(a / b))
}

fn int_div_rem_floor(a: i128, b: i128) -> Option<(i128, i128)> {
    let mut quotient = a.checked_div(b)?;
    let mut remainder = a.checked_rem(b)?;
    if remainder != 0 && (remainder < 0) != (b < 0) {
        quotient = quotient.checked_sub(1)?;
        remainder = remainder.checked_add(b)?;
    }
    Some((quotient, remainder))
}

fn float_div_rem_floor(a: f64, b: f64) -> (f64, f64) {
    let mut remainder = a % b;
    let mut quotient = (a - remainder) / b;
    if remainder != 0.0 {
        if (remainder < 0.0) != (b < 0.0) {
            remainder += b;
            quotient -= 1.0;
        }
    } else {
        remainder = 0.0_f64.copysign(b);
    }
    if quotient != 0.0 {
        let mut floored = quotient.floor();
        if quotient - floored > 0.5 {
            floored += 1.0;
        }
        quotient = floored;
    } else {
        quotient = 0.0_f64.copysign(a / b);
    }
    (quotient, remainder)
}

/// `a // b`: floored division.
pub(crate) fn int_div(lhs: &Value, rhs: &Value) -> Result<Value, Error> {
    match coerce(lhs, rhs, true) {
        Some(Coerced::Int(a, b)) if b != 0 => int_div_rem_floor(a, b)
            .map(|(quotient, _)| Value::from(quotient))
            .ok_or_else(|| failed("//")),
        Some(Coerced::Float(a, b)) if b != 0.0 => Ok(Value::from(float_div_rem_floor(a, b).0)),
        Some(Coerced::Int(..) | Coerced::Float(..)) => Err(failed("//")),
        _ => Err(impossible("//", lhs, rhs)),
    }
}

/// `a % b`: the remainder with the sign of `b`.
pub(crate) fn rem(lhs: &Value, rhs: &Value) -> Result<Value, Error> {
    match coerce(lhs, rhs, true) {
        Some(Coerced::Int(a, b)) if b != 0 => {
            if a == i128::MIN && b == -1 {
                return Ok(Value::from(0));
            }
            int_div_rem_floor(a, b)
                .map(|(_, remainder)| Value::from(remainder))
                .ok_or_else(|| failed("%"))
        }
        Some(Coerced::Float(a, b)) if b != 0.0 => Ok(Value::from(float_div_rem_floor(a, b).1)),
        Some(Coerced::Int(..) | Coerced::Float(..)) => Err(failed("%")),
        _ => Err(impossible("%", lhs, rhs)),
    }
}

/// `a ** b`.
pub(crate) fn pow(lhs: &Value, rhs: &Value) -> Result<Value, Error> {
    match coerce(lhs, rhs, true) {
        Some(Coerced::Int(a, b)) if b < 0 => {
            if a == 0 {
                return Err(failed("**"));
            }
            #[allow(clippy::cast_precision_loss)]
            Ok(Value::from((a as f64).powf(b as f64)))
        }
        Some(Coerced::Int(a, b)) => u32::try_from(b)
            .ok()
            .and_then(|b| a.checked_pow(b))
            .map(Value::from)
            .ok_or_else(|| failed("**")),
        Some(Coerced::Float(a, b)) if a == 0.0 && b < 0.0 => Err(failed("**")),
        Some(Coerced::Float(a, b)) => Ok(Value::from(a.powf(b))),
        _ => Err(impossible("**", lhs, rhs)),
    }
}

/// `-a`.
pub(crate) fn neg(value: &Value) -> Result<Value, Error> {
    match value.0 {
        Repr::Float(float) => Ok(Value::from(-float)),
        Repr::Int(integer) => integer
            .checked_neg()
            .map(Value::from)
            .ok_or_else(|| Error::invalid("overflow")),
        _ => Err(Error::from(crate::error::ErrorKind::InvalidOperation)),
    }
}

/// `value in container`.
pub(crate) fn contains(container: &Value, value: &Value) -> Result<bool, Error> {
    Ok(match &container.0 {
        Repr::Undefined(_) => false,
        Repr::Str(text, _) => match value.as_str() {
            Some(needle) => text.contains(needle),
            None => text.contains(&value.to_string()),
        },
        Repr::Seq(items, ..) => items.iter().any(|item| item == value),
        Repr::Map(..) | Repr::Kwargs(_) | Repr::Module(_) => {
            container.get_item_opt(value).is_some()
        }
        Repr::Function(_) | Repr::Macro(_) | Repr::Loop(_) => false,
        _ => {
            return Err(Error::invalid(
                "cannot perform a containment check on this value",
            ));
        }
    })
}

fn offset_and_len(start: Option<i64>, stop: Option<i64>, end: usize) -> (usize, usize) {
    let end_signed = i64::try_from(end).unwrap_or(i64::MAX);
    let from_end = |x: i64| usize::try_from(end_signed.saturating_add(x).max(0)).unwrap_or(0);
    let start_at = match start.unwrap_or(0) {
        at if at < 0 => from_end(at),
        at => usize::try_from(at).unwrap_or(usize::MAX),
    };
    let stop_at = match stop {
        None => end,
        Some(at) if at < 0 => from_end(at),
        Some(at) => usize::try_from(at).unwrap_or(usize::MAX),
    };
    (start_at, stop_at.saturating_sub(start_at))
}

fn step_back_indices(start: Option<i64>, stop: Option<i64>, step: usize, end: usize) -> Vec<usize> {
    let end_signed = i64::try_from(end).unwrap_or(i64::MAX);
    let start = match start {
        None => end.saturating_sub(1),
        Some(at) if at >= end_signed => end.saturating_sub(1),
        Some(at) if at >= 0 => usize::try_from(at).unwrap_or(0),
        Some(at) => usize::try_from(end_signed.saturating_add(at).max(0)).unwrap_or(0),
    };
    let stop = match stop {
        None => 0,
        Some(at) if at < 0 => usize::try_from(end_signed.saturating_add(at).max(0)).unwrap_or(0),
        Some(at) => usize::try_from(at).unwrap_or(usize::MAX),
    };
    if start < stop {
        return Vec::new();
    }
    let length = if stop == 0 {
        start.saturating_add(step) / step
    } else {
        (start - stop).saturating_add(step - 1) / step
    };
    (stop..=start).rev().step_by(step).take(length).collect()
}

/// `value[start:stop:step]`. Slicing undefined or none is an empty list.
pub(crate) fn slice(
    value: &Value,
    start: &Value,
    stop: &Value,
    step: &Value,
) -> Result<Value, Error> {
    let start = if start.is_none() {
        None
    } else {
        Some(i64::try_from(start.clone())?)
    };
    let stop = if stop.is_none() {
        None
    } else {
        Some(i64::try_from(stop.clone())?)
    };
    let step = if step.is_none() {
        1
    } else {
        i64::try_from(step.clone())?
    };
    if step == 0 {
        return Err(Error::invalid("cannot slice by step size of 0"));
    }
    let step_size = usize::try_from(step.unsigned_abs()).unwrap_or(usize::MAX);
    match &value.0 {
        Repr::Str(text, _) => {
            if step > 0 {
                let (from, len) = offset_and_len(start, stop, text.chars().count());
                Ok(Value::from(
                    text.chars()
                        .skip(from)
                        .take(len)
                        .step_by(step_size)
                        .collect::<String>(),
                ))
            } else {
                let chars: Vec<char> = text.chars().collect();
                Ok(Value::from(
                    step_back_indices(start, stop, step_size, chars.len())
                        .into_iter()
                        .filter_map(|index| chars.get(index))
                        .collect::<String>(),
                ))
            }
        }
        Repr::Undefined(_) | Repr::None => Ok(Value::list(Vec::new())),
        Repr::Seq(items, kind, _) => {
            let picked: Vec<Value> = if step > 0 {
                let (from, len) = offset_and_len(start, stop, items.len());
                items
                    .iter()
                    .skip(from)
                    .take(len)
                    .step_by(step_size)
                    .cloned()
                    .collect()
            } else {
                step_back_indices(start, stop, step_size, items.len())
                    .into_iter()
                    .filter_map(|index| items.get(index).cloned())
                    .collect()
            };
            Ok(if *kind == SeqKind::Tuple {
                Value::tuple(picked)
            } else {
                Value::iterable(picked)
            })
        }
        _ => Err(Error::invalid(format!(
            "value of type {} cannot be sliced",
            value.kind()
        ))),
    }
}

/// The work of comparing two values: what the smaller one takes to walk.
pub(crate) fn compare_cost(a: &Value, b: &Value) -> u64 {
    walk_cost(a).min(walk_cost(b))
}

/// The work of `value in container`.
pub(crate) fn contains_cost(container: &Value, value: &Value) -> u64 {
    walk_cost(container).saturating_add(walk_cost(value))
}

fn walk_cost(value: &Value) -> u64 {
    match &value.0 {
        Repr::Str(text, _) => (text.len() / 64) as u64,
        _ => value.shape().weight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floors_like_python() {
        assert_eq!(
            int_div(&Value::from(-7), &Value::from(2)).expect("ok"),
            Value::from(-4)
        );
        assert_eq!(
            rem(&Value::from(-7), &Value::from(2)).expect("ok"),
            Value::from(1)
        );
        assert!(rem(&Value::from(1), &Value::from(0)).is_err());
        assert!(int_div(&Value::from(1), &Value::from(0)).is_err());
    }

    #[test]
    fn errors_name_no_values() {
        let error = add(&Value::from("s3cret"), &Value::from(1)).expect_err("types");
        assert!(!error.to_string().contains("s3cret"), "{error}");
        let error = add(&Value::from(i128::MAX), &Value::from(1)).expect_err("overflow");
        assert!(!error.to_string().contains("1701411"), "{error}");
    }

    #[test]
    fn repeating_is_capped() {
        assert!(mul(&Value::from("ab"), &Value::from(100), 100, 100).is_err());
        assert!(mul(&Value::from("ab"), &Value::from(50), 100, 100).is_ok());
        let list = Value::list(vec![Value::from(1), Value::from(2)]);
        assert!(mul(&list, &Value::from(usize::MAX), 100, 100).is_err());
    }

    #[test]
    fn slices() {
        let text = Value::from("héllo");
        let none = Value::NONE;
        assert_eq!(
            slice(&text, &Value::from(1), &Value::from(3), &none).expect("ok"),
            Value::from("él")
        );
        assert_eq!(
            slice(&text, &none, &none, &Value::from(-1)).expect("ok"),
            Value::from("olléh")
        );
        assert_eq!(
            slice(&text, &Value::from(-2), &none, &none).expect("ok"),
            Value::from("lo")
        );
        assert!(slice(&text, &none, &none, &Value::from(0)).is_err());
    }
}
