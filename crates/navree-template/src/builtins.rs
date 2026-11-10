//! The built-in filters, tests and global functions.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::environment::Environment;
use crate::error::{Error, ErrorKind};
use crate::escape::{JsonStyle, Mode, Quote, write_display, write_escaped, write_json};
use crate::functions::{ArgType, Callable, FunctionArgs, MAX_STRING_ARG};
use crate::value::ops;
use crate::value::{Coerced, Kwargs, Repr, Safety, Value, ValueKind, ValueMap, coerce};
use crate::vm::State;

/// A built-in: a plain function of the render state and the arguments.
type Builtin = for<'s, 'v> fn(&'s State<'_>, &'v [Value]) -> Result<Value, Error>;

/// Adds the built-ins to `env`.
pub(crate) fn register(env: &mut Environment) {
    let filters: [(&'static str, Builtin); 25] = [
        ("safe", safe),
        ("escape", escape),
        ("e", escape),
        ("lower", lower),
        ("upper", upper),
        ("capitalize", capitalize),
        ("replace", replace),
        ("length", length),
        ("count", length),
        ("trim", trim),
        ("join", join),
        ("split", split),
        ("default", default),
        ("d", default),
        ("round", round),
        ("int", int),
        ("first", first),
        ("last", last),
        ("min", min),
        ("list", list),
        ("string", string),
        ("items", items),
        ("tojson", tojson),
        ("urlencode", urlencode),
        ("abs", abs),
    ];
    env.filters = filters
        .into_iter()
        .map(|(name, filter)| (Cow::Borrowed(name), Arc::new(filter) as Arc<Callable>))
        .collect();

    let tests: [(&'static str, Builtin); 37] = [
        ("undefined", is_undefined),
        ("defined", is_defined),
        ("none", is_none),
        ("safe", is_safe),
        ("escaped", is_safe),
        ("boolean", is_boolean),
        ("odd", is_odd),
        ("even", is_even),
        ("divisibleby", is_divisibleby),
        ("number", is_number),
        ("integer", is_integer),
        ("int", is_integer),
        ("float", is_float),
        ("string", is_string),
        ("sequence", is_sequence),
        ("iterable", is_iterable),
        ("mapping", is_mapping),
        ("startingwith", is_startingwith),
        ("endingwith", is_endingwith),
        ("lower", is_lower),
        ("upper", is_upper),
        ("sameas", is_sameas),
        ("eq", is_eq),
        ("equalto", is_eq),
        ("==", is_eq),
        ("ne", is_ne),
        ("!=", is_ne),
        ("lt", is_lt),
        ("lessthan", is_lt),
        ("le", is_le),
        ("gt", is_gt),
        ("greaterthan", is_gt),
        ("ge", is_ge),
        ("in", is_in),
        ("true", is_true),
        ("false", is_false),
        ("filter", is_filter),
    ];
    env.tests = tests
        .into_iter()
        .map(|(name, test)| (Cow::Borrowed(name), Arc::new(test) as Arc<Callable>))
        .collect();
    env.tests
        .insert(Cow::Borrowed("test"), Arc::new(is_test as Builtin));

    env.globals = BTreeMap::new();
    for (name, function) in [("range", range as Builtin), ("dict", dict as Builtin)] {
        env.globals.insert(
            Cow::Borrowed(name),
            Value::from_function(name, Arc::new(function)),
        );
    }
}

/// Converts the arguments of a built-in, like those of a registered function.
fn args<'a, T: FunctionArgs<'a>>(
    state: &State<'_>,
    values: &'a [Value],
) -> Result<T::Output, Error> {
    T::from_values(values, state.strict_undefined())
}

/// A string argument: strings as they are, other values as they print,
/// undefined values an error where those are. It remembers what the value
/// was safe for.
struct StringInput<'a> {
    text: Cow<'a, str>,
    safety: Safety,
}

impl<'a> ArgType<'a> for StringInput<'_> {
    type Output = StringInput<'a>;

    fn from_value(value: Option<&'a Value>, strict: bool) -> Result<StringInput<'a>, Error> {
        let Some(value) = value else {
            return Err(Error::from(ErrorKind::MissingArgument));
        };
        if value.is_kwargs() {
            return Err(Error::invalid("cannot convert kwargs to string"));
        }
        if strict && value.is_undefined() && !value.is_silent_undefined() {
            return Err(Error::undefined());
        }
        Ok(StringInput {
            text: match value.as_str() {
                Some(text) => Cow::Borrowed(text),
                None => Cow::Owned(value.to_string_capped(MAX_STRING_ARG)?),
            },
            safety: value.safety(),
        })
    }
}

impl StringInput<'_> {
    fn as_str(&self) -> &str {
        &self.text
    }

    /// `text` with the mark of this input.
    fn keep_safety(&self, text: &str) -> Value {
        Value::marked(Arc::from(text), self.safety)
    }

    /// Whether the input needs no more escaping in `mode`.
    fn is_safe_for(&self, mode: Mode) -> bool {
        self.safety != Safety::Plain && self.safety.fits(mode)
    }

    /// The text, escaped for the current mode unless it needs no escaping
    /// there. `|tojson` text gets its quotes settled, as anywhere it is
    /// combined with other text.
    fn formatted(&self, state: &State<'_>) -> Result<Cow<'_, str>, Error> {
        if self.safety == Safety::Trusted
            || (self.is_safe_for(state.mode()) && self.safety != Safety::Tojson)
        {
            return Ok(Cow::Borrowed(self.as_str()));
        }
        let mode = state.mode();
        state.charge_copy(self.as_str().len())?;
        let value = Value::marked(Arc::from(self.as_str()), self.safety);
        let mut out = state.string_builder(self.as_str().len())?;
        write_escaped(&mut out, mode, &value)?;
        Ok(Cow::Owned(out.into_string()))
    }
}

/// An item's attribute when `attribute` is given (`a.b.0`), else the item.
/// Each step of the path looks a key up.
fn map_attribute(state: &State<'_>, item: Value, attribute: Option<&str>) -> Result<Value, Error> {
    let Some(path) = attribute else {
        return Ok(item);
    };
    state.charge(1 + (path.len() / 8) as u64)?;
    Ok(get_path(&item, path)
        .ok()
        .filter(|value| !value.is_undefined())
        .unwrap_or_default())
}

/// Looks up a dotted path; numeric parts are indexes.
fn get_path(value: &Value, path: &str) -> Result<Value, Error> {
    let mut current = value.clone();
    for part in path.split('.') {
        current = match part.parse::<usize>() {
            Ok(index) => current.get_item(&Value::from(index))?,
            Err(_) => current.get_attr(part)?,
        };
    }
    Ok(current)
}

/// The items a filter goes through, charged as work; undefined values are
/// an error where those are.
fn checked_items(state: &State<'_>, value: &Value) -> Result<Vec<Value>, Error> {
    state.assert_not_undefined(value)?;
    state.iter_items(value)
}

// Filters ---------------------------------------------------------------

fn safe(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    // A string keeps its text: only the mark changes. `|tojson` output stays
    // what it is, since its quotes depend on where it lands.
    if let [Value(Repr::Str(text, safety))] = values {
        let safety = match safety {
            Safety::Tojson => Safety::Tojson,
            _ => Safety::Trusted,
        };
        return Ok(Value::marked(text.clone(), safety));
    }
    state.charge_text_args(values)?;
    let (text,) = args::<(String,)>(state, values)?;
    state.charge_copy(text.len())?;
    Ok(Value::from_safe_string(text))
}

fn escape_value(state: &State<'_>, value: &Value) -> Result<Value, Error> {
    let mode = state.escape_mode();
    if value.is_safe() && value.safety().fits(mode) {
        return Ok(value.clone());
    }
    state.charge(value.shape().weight)?;
    let mut out = state.string_builder(0)?;
    write_escaped(&mut out, mode, value)?;
    out.finish(built_safety(mode))
}

/// What text built in a string's own buffer in `mode` is marked: JSON that is
/// also fit for HTML keeps its quotes until it lands, where they are written
/// for the place they land in.
fn built_safety(mode: Mode) -> Safety {
    match mode {
        Mode::Both => Safety::Tojson,
        _ => mode.safety(),
    }
}

fn escape(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    escape_value(state, value)
}

/// What `text` comes to with `map` applied to each character.
fn mapped_size<I: Iterator<Item = char>>(text: &str, map: impl Fn(char) -> I) -> usize {
    text.chars()
        .map(|ch| map(ch).map(char::len_utf8).sum::<usize>())
        .sum()
}

fn lower(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    state.charge_text_args(values)?;
    let (input,) = args::<(StringInput<'_>,)>(state, values)?;
    let text = input.as_str();
    state.charge_scan(text.len())?;
    // Case mapping can make text longer; the final sigma is no exception.
    state.check_growing_size(text.len(), mapped_size(text, char::to_lowercase))?;
    Ok(input.keep_safety(&text.to_lowercase()))
}

fn upper(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    state.charge_text_args(values)?;
    let (input,) = args::<(StringInput<'_>,)>(state, values)?;
    let text = input.as_str();
    state.charge_scan(text.len())?;
    state.check_growing_size(text.len(), mapped_size(text, char::to_uppercase))?;
    Ok(input.keep_safety(&text.to_uppercase()))
}

fn capitalize(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    state.charge_text_args(values)?;
    let (input,) = args::<(StringInput<'_>,)>(state, values)?;
    let text = input.as_str();
    state.charge_scan(text.len())?;
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return Ok(input.keep_safety(""));
    };
    let rest = chars.as_str();
    let size = first.to_uppercase().map(char::len_utf8).sum::<usize>()
        + mapped_size(rest, char::to_lowercase);
    state.check_growing_size(text.len(), size)?;
    let output = first.to_uppercase().collect::<String>() + &rest.to_lowercase();
    Ok(input.keep_safety(&output))
}

fn replace(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    state.charge_text_args(values)?;
    let (value, from, to) =
        args::<(StringInput<'_>, StringInput<'_>, StringInput<'_>)>(state, values)?;
    let mode = state.mode();
    let safety_aware = mode != Mode::None
        && (value.is_safe_for(mode) || from.is_safe_for(mode) || to.is_safe_for(mode));
    let (text, replacement) = if safety_aware {
        (value.formatted(state)?, to.formatted(state)?)
    } else {
        (Cow::Borrowed(value.as_str()), Cow::Borrowed(to.as_str()))
    };
    let out = replace_capped(state, &text, from.as_str(), &replacement)?;
    out.finish(if safety_aware {
        built_safety(mode)
    } else {
        Safety::Plain
    })
}

/// `str::replace`, stopping at the output limit: a short pattern and a long
/// replacement can multiply a string's size.
fn replace_capped(
    state: &State<'_>,
    text: &str,
    from: &str,
    to: &str,
) -> Result<crate::output::Output, Error> {
    state.charge_scan(text.len())?;
    let matches = text.matches(from).count();
    let size = text.len().saturating_add(matches.saturating_mul(to.len()));
    let mut out = state.string_builder(size)?;
    let mut last = 0;
    for (start, found) in text.match_indices(from) {
        out.write_str(text.get(last..start).unwrap_or(""))?;
        out.write_str(to)?;
        last = start + found.len();
    }
    out.write_str(text.get(last..).unwrap_or(""))?;
    Ok(out)
}

fn length(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    if let Some(text) = value.as_str() {
        state.charge_scan(text.len())?;
    }
    value.len().map(Value::from).ok_or_else(|| {
        Error::invalid(format!(
            "cannot calculate length of value of type {}",
            value.kind()
        ))
    })
}

fn trim(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    state.charge_text_args(values)?;
    let (input, chars) = args::<(StringInput<'_>, Option<StringInput<'_>>)>(state, values)?;
    let text = input.as_str();
    state.charge_scan(text.len())?;
    let output = match &chars {
        Some(chars) => {
            state.charge_scan(chars.as_str().len())?;
            state.charge_allocation((chars.as_str().len() as u64).saturating_mul(4))?;
            let mut set: Vec<char> = chars.as_str().chars().collect();
            set.sort_unstable();
            set.dedup();
            text.trim_matches(|ch: char| set.binary_search(&ch).is_ok())
        }
        None => text.trim(),
    };
    state.charge_copy(output.len())?;
    Ok(input.keep_safety(output))
}

fn join(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    state.charge_text_args(values.get(1..).unwrap_or(&[]))?;
    let (value, joiner, attribute, kwargs) =
        args::<(&Value, Option<StringInput<'_>>, Option<&str>, Kwargs)>(state, values)?;
    let joiner = match joiner {
        Some(joiner) => Some(joiner),
        None => kwargs
            .values()
            .get(&Value::from("d"))
            .map(|value| StringInput::from_value(Some(value), false))
            .transpose()?
            .map(owned_input),
    };
    let attribute: Option<String> = match attribute {
        Some(attribute) => Some(attribute.to_owned()),
        None => kwargs.get::<Option<&str>>("attribute")?.map(str::to_owned),
    };
    if kwargs.has("d") {
        let _: Option<&Value> = kwargs.get("d")?;
    }
    kwargs.assert_all_used()?;
    let joiner_text = joiner.as_ref().map(StringInput::as_str).unwrap_or_default();
    let items = state
        .iter_items(value)
        .map_err(|error| match error.kind() {
            ErrorKind::LimitExceeded => error,
            _ => Error::invalid(format!("cannot join value of type {}", value.kind())),
        })?;
    let items: Vec<Value> = items
        .into_iter()
        .map(|item| map_attribute(state, item, attribute.as_deref()))
        .collect::<Result<_, _>>()?;
    let mode = state.mode();
    if mode == Mode::None {
        return join_plain(state, &items, joiner_text)?.finish(Safety::Plain);
    }
    if joiner
        .as_ref()
        .is_some_and(|joiner| joiner.is_safe_for(mode))
    {
        return join_safe(state, &items, joiner_text)?.finish(built_safety(mode));
    }
    // A plain joiner only becomes safe if at least one item is safe.
    if items
        .iter()
        .any(|item| item.is_safe() && item.safety().fits(mode))
    {
        let joiner = match &joiner {
            Some(joiner) => joiner.formatted(state)?,
            None => Cow::Borrowed(""),
        };
        join_safe(state, &items, &joiner)?.finish(built_safety(mode))
    } else {
        join_plain(state, &items, joiner_text)?.finish(Safety::Plain)
    }
}

fn owned_input(input: StringInput<'_>) -> StringInput<'static> {
    StringInput {
        text: Cow::Owned(input.text.into_owned()),
        safety: input.safety,
    }
}

fn join_plain(
    state: &State<'_>,
    items: &[Value],
    joiner: &str,
) -> Result<crate::output::Output, Error> {
    let mut out = state.string_builder(0)?;
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.write_str(joiner)?;
        }
        state.charge(item.shape().weight)?;
        write_display(&mut out, item)?;
    }
    Ok(out)
}

fn join_safe(
    state: &State<'_>,
    items: &[Value],
    joiner: &str,
) -> Result<crate::output::Output, Error> {
    let mut out = state.string_builder(0)?;
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.write_str(joiner)?;
        }
        state.assert_not_undefined(item)?;
        state.charge(item.shape().weight)?;
        write_escaped(&mut out, state.mode(), item)?;
    }
    Ok(out)
}

fn split(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, separator, max_splits) =
        args::<(&Value, Option<&str>, Option<i64>)>(state, values)?;
    let Some(text) = value.as_str() else {
        return Err(Error::invalid("value is not a string"));
    };
    let parts_limit = max_splits
        .and_then(|n| usize::try_from(n).ok())
        .map(|n| n.saturating_add(1));
    // Counted first, so the whole result is charged before any of it exists.
    state.charge_scan(text.len())?;
    let count = match (separator, parts_limit) {
        (None, None) => text.split_whitespace().count(),
        (Some(separator), None) => text.split(separator).count(),
        (None, Some(limit)) => text.split_whitespace().count().min(limit),
        (Some(separator), Some(limit)) => text.splitn(limit, separator).count(),
    };
    state.charge_strings(count, text.len())?;
    state.charge_items(count)?;
    let safety = value.safety();
    let wrap = |part: &str| Value::marked(Arc::from(part), safety);
    let parts: Vec<Value> = match (separator, parts_limit) {
        (None, None) => text.split_whitespace().map(wrap).collect(),
        (Some(separator), None) => text.split(separator).map(wrap).collect(),
        (None, Some(limit)) => splitn_whitespace(text, limit)
            .into_iter()
            .map(wrap)
            .collect(),
        (Some(separator), Some(limit)) => text.splitn(limit, separator).map(wrap).collect(),
    };
    Ok(Value::list(parts))
}

/// Splits on runs of whitespace into at most `limit` parts; the last part
/// keeps the rest as it is.
fn splitn_whitespace(text: &str, limit: usize) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut part_start = None;
    let mut last_end = 0;
    let mut skip_whitespace = true;
    for (index, ch) in text.char_indices() {
        if parts.len() + 1 >= limit && !skip_whitespace {
            continue;
        }
        if ch.is_whitespace() {
            if let Some(start) = part_start.take() {
                parts.push(text.get(start..index).unwrap_or(""));
                last_end = index;
                skip_whitespace = true;
            }
        } else {
            skip_whitespace = false;
            if part_start.is_none() {
                part_start = Some(index);
                last_end = index;
            }
        }
    }
    let rest = text.get(last_end..).unwrap_or("");
    if !rest.is_empty() {
        parts.push(rest);
    }
    parts
}

fn default(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, rest) = match values.split_first() {
        Some(split) => split,
        None => return Err(Error::from(ErrorKind::MissingArgument)),
    };
    if rest.len() > 2 {
        return Err(Error::from(ErrorKind::TooManyArguments));
    }
    let (_, lax) = args::<(Option<&Value>, Option<&Value>)>(state, rest)?;
    let lax = match lax {
        Some(lax) => {
            if state.undefined_behavior() == crate::UndefinedBehavior::Strict && lax.is_undefined()
            {
                return Err(Error::undefined());
            }
            lax.is_true()
        }
        None => false,
    };
    if value.is_undefined() || (lax && !value.is_true()) {
        // The first argument as given: `none` stays `none`.
        return Ok(rest.first().cloned().unwrap_or_else(|| Value::from("")));
    }
    Ok(value.clone())
}

fn abs(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(Value,)>(state, values)?;
    match value.0 {
        Repr::Int(integer) => integer
            .checked_abs()
            .map(Value::from)
            .ok_or_else(|| Error::invalid("overflow on abs")),
        Repr::Float(float) => Ok(Value::from(float.abs())),
        _ => Err(Error::invalid("cannot get absolute value")),
    }
}

fn round_too_large() -> Error {
    Error::invalid("rounded value is too large to represent")
}

fn round_int(value: i128, precision: i32, method: &str) -> Result<i128, Error> {
    let Some(factor) = 10_i128.checked_pow(precision.unsigned_abs()) else {
        // The factor is larger than any integer: only zero is in reach.
        return match method {
            "ceil" if value > 0 => Err(round_too_large()),
            "floor" if value < 0 => Err(round_too_large()),
            _ => Ok(0),
        };
    };
    let quotient = value.div_euclid(factor);
    let remainder = value.rem_euclid(factor);
    let round_up = match method {
        "ceil" => remainder > 0,
        "floor" => false,
        // Half to even, like Python's round().
        _ => {
            remainder > factor - remainder || (remainder == factor - remainder && quotient % 2 != 0)
        }
    };
    quotient
        .checked_add(i128::from(round_up))
        .and_then(|quotient| quotient.checked_mul(factor))
        .ok_or_else(round_too_large)
}

fn round_directed(value: f64, precision: i32, ceil: bool) -> Result<f64, Error> {
    if !value.is_finite() {
        return Ok(value);
    }
    let factor: f64 = format!("1e{precision}").parse().unwrap_or(f64::INFINITY);
    let scaled = value * factor;
    if scaled.is_infinite() {
        // The value has no digits at this precision.
        return Ok(value);
    }
    let rounded = if ceil { scaled.ceil() } else { scaled.floor() };
    // Adding 0.0 turns -0.0 into 0.0, like Python's integer result does.
    let result = (rounded + 0.0) / factor;
    if !result.is_finite() {
        return Err(round_too_large());
    }
    Ok(result)
}

fn round_with_negative_precision(value: f64, precision: i32) -> Result<f64, Error> {
    if !value.is_finite() {
        return Ok(value);
    }
    let decimal_place = -i64::from(precision);
    if decimal_place >= 309 {
        return Ok(0.0_f64.copysign(value));
    }
    // Enough decimal digits to represent any f64 exactly: rounding them
    // avoids an approximate division by 10^n.
    let formatted = format!("{:.1074e}", value.abs());
    let Some((mantissa, exponent)) = formatted.split_once('e') else {
        return Err(Error::invalid("unable to round value"));
    };
    let exponent: i64 = exponent
        .parse()
        .map_err(|_| Error::invalid("unable to round value"))?;
    let retained_len = exponent - decimal_place + 1;
    let Ok(retained_len) = usize::try_from(retained_len) else {
        return Ok(0.0_f64.copysign(value));
    };
    let digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    let (Some(discarded), Some(kept)) = (digits.get(retained_len..), digits.get(..retained_len))
    else {
        return Ok(value);
    };
    let last_retained = kept.last().map_or(0, |digit| digit - b'0');
    let first_discarded = discarded.first().copied().unwrap_or(b'0');
    let round_up = first_discarded > b'5'
        || (first_discarded == b'5'
            && (discarded.iter().skip(1).any(|&digit| digit != b'0') || last_retained % 2 != 0));
    let mut retained = kept.to_vec();
    if round_up {
        let mut carry = true;
        for digit in retained.iter_mut().rev() {
            if *digit == b'9' {
                *digit = b'0';
            } else {
                *digit += 1;
                carry = false;
                break;
            }
        }
        if carry {
            retained.insert(0, b'1');
        }
    }
    if retained.iter().all(|&digit| digit == b'0') {
        return Ok(0.0_f64.copysign(value));
    }
    let coefficient = String::from_utf8_lossy(&retained);
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let rounded: f64 = format!("{sign}{coefficient}e{decimal_place}")
        .parse()
        .map_err(|_| Error::invalid("unable to round value"))?;
    if rounded.is_infinite() {
        return Err(round_too_large());
    }
    Ok(rounded)
}

fn round(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, precision, method, kwargs) =
        args::<(Value, Option<i32>, Option<&str>, Kwargs)>(state, values)?;
    let precision = match precision {
        Some(precision) => precision,
        None => kwargs.get::<Option<i32>>("precision")?.unwrap_or(0),
    };
    let method = match method {
        Some(method) => method,
        None => kwargs.get::<Option<&str>>("method")?.unwrap_or("common"),
    };
    kwargs.assert_all_used()?;
    if !matches!(method, "common" | "ceil" | "floor") {
        return Err(Error::invalid("method must be common, ceil or floor"));
    }
    match value.0 {
        Repr::Int(integer) => {
            if precision >= 0 {
                return Ok(value);
            }
            round_int(integer, precision, method).map(Value::from)
        }
        Repr::Float(float) if method != "common" => {
            round_directed(float, precision, method == "ceil").map(Value::from)
        }
        Repr::Float(float) => {
            if precision < 0 {
                return round_with_negative_precision(float, precision).map(Value::from);
            }
            // Formatting rounds half to even and keeps decimal behavior.
            if precision > 1074 {
                return Ok(Value::from(float));
            }
            let places = usize::try_from(precision).unwrap_or(0);
            let text = format!("{float:.places$}");
            text.parse::<f64>()
                .map(Value::from)
                .map_err(|_| Error::invalid("unable to round value"))
        }
        _ => Err(Error::invalid(format!(
            "cannot round value ({})",
            value.kind()
        ))),
    }
}

fn int(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, default, base, kwargs) =
        args::<(&Value, Option<Value>, Option<u32>, Kwargs)>(state, values)?;
    let default = match default {
        Some(default) => Some(default),
        None => kwargs.get::<Option<Value>>("default")?,
    };
    let base = match base {
        Some(base) => base,
        None => kwargs.get::<Option<u32>>("base")?.unwrap_or(10),
    };
    kwargs.assert_all_used()?;
    let result = match &value.0 {
        Repr::Undefined(_) | Repr::None => {
            state.assert_not_undefined(value)?;
            return Ok(default.unwrap_or_else(|| Value::from(0)));
        }
        Repr::Bool(flag) => return Ok(Value::from(i128::from(*flag))),
        Repr::Int(_) => return Ok(value.clone()),
        #[allow(clippy::cast_possible_truncation)]
        Repr::Float(float) => return Ok(Value::from(*float as i128)),
        Repr::Str(text, _) => {
            state.charge_scan(text.len())?;
            match parse_int(text, base) {
                Some(integer) => return Ok(Value::from(integer)),
                #[allow(clippy::cast_possible_truncation)]
                None => text
                    .parse::<f64>()
                    .map(|float| Value::from(float as i128))
                    .map_err(|error| Error::invalid(error.to_string())),
            }
        }
        _ => Err(Error::invalid(format!(
            "cannot convert {} to integer",
            value.kind()
        ))),
    };
    result.or_else(|error| default.ok_or(error))
}

fn parse_int(text: &str, base: u32) -> Option<i128> {
    let prefix = match base {
        2 => "0b",
        8 => "0o",
        16 => "0x",
        3..=36 => "",
        _ => return None,
    };
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let digits = match digits.get(..prefix.len()) {
        Some(found) if !prefix.is_empty() && found.eq_ignore_ascii_case(prefix) => {
            digits.get(prefix.len()..).unwrap_or("")
        }
        _ => digits,
    };
    if digits.starts_with(['+', '-']) {
        return None;
    }
    let parsed = i128::from_str_radix(digits, base).ok()?;
    Some(if negative {
        parsed.checked_neg()?
    } else {
        parsed
    })
}

fn first(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    match &value.0 {
        Repr::Str(text, _) => Ok(text.chars().next().map_or(Value::UNDEFINED, Value::from)),
        Repr::Seq(items, ..) => Ok(items.first().cloned().unwrap_or_default()),
        Repr::Map(..) | Repr::Kwargs(..) | Repr::Module(_) => Ok(value
            .as_map()
            .and_then(|map| map.keys().next().cloned())
            .unwrap_or_default()),
        _ => Err(Error::invalid("cannot get first item from value")),
    }
}

fn last(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    match &value.0 {
        Repr::Str(text, safety) => Ok(text.chars().next_back().map_or(Value::UNDEFINED, |ch| {
            Value::marked(Arc::from(ch.to_string()), *safety)
        })),
        Repr::Seq(items, ..) => Ok(items.last().cloned().unwrap_or_default()),
        _ => Err(Error::invalid("cannot get last item from value")),
    }
}

/// Compares two sort keys; strings ignore ASCII case unless `case_sensitive`.
fn cmp_keys(a: &Value, b: &Value, case_sensitive: bool) -> Ordering {
    if !case_sensitive && let (Some(a), Some(b)) = (a.as_str(), b.as_str()) {
        return a
            .bytes()
            .map(|byte| byte.to_ascii_lowercase())
            .cmp(b.bytes().map(|byte| byte.to_ascii_lowercase()));
    }
    a.cmp(b)
}

/// The error for what can't be gone through; a limit stays what it is.
fn not_a_list(error: Error) -> Error {
    match error.kind() {
        ErrorKind::LimitExceeded => error,
        _ => Error::invalid("cannot convert value to list"),
    }
}

fn min(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, kwargs) = args::<(&Value, Kwargs)>(state, values)?;
    let items = checked_items(state, value).map_err(not_a_list)?;
    let case_sensitive = kwargs
        .get::<Option<bool>>("case_sensitive")?
        .unwrap_or(false);
    let attribute = kwargs.get::<Option<&str>>("attribute")?;
    let mut best: Option<(Value, Value)> = None;
    for item in items {
        let key = map_attribute(state, item.clone(), attribute)?;
        state.charge(key.shape().weight)?;
        if let Some((best_key, _)) = &best {
            state.charge(ops::compare_cost(&key, best_key))?;
        }
        let replaces = best
            .as_ref()
            .is_none_or(|(best_key, _)| cmp_keys(&key, best_key, case_sensitive) == Ordering::Less);
        if replaces {
            best = Some((key, item));
        }
    }
    kwargs.assert_all_used()?;
    Ok(best.map_or(Value::UNDEFINED, |(_, item)| item))
}

fn list(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    Ok(Value::list(
        checked_items(state, value).map_err(not_a_list)?,
    ))
}

fn string(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    state.assert_not_undefined(value)?;
    if value.kind() == ValueKind::String {
        return Ok(value.clone());
    }
    state.charge(value.shape().weight)?;
    let mut out = state.string_builder(0)?;
    write_display(&mut out, value)?;
    out.finish(Safety::Plain)
}

fn items(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    let Some(map) = value.as_map() else {
        return Err(Error::invalid("cannot convert value into pairs"));
    };
    // Each pair is a tuple of its own.
    state.charge_items(map.len().saturating_mul(6))?;
    Ok(Value::iterable(
        map.iter()
            .map(|(key, item)| Value::tuple(vec![key.clone(), item.clone()]))
            .collect(),
    ))
}

fn tojson(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, indent, kwargs) = args::<(&Value, Option<Value>, Kwargs)>(state, values)?;
    let indent = match indent {
        Some(indent) => Some(indent),
        None => kwargs.get::<Option<Value>>("indent")?,
    };
    let indent = match indent {
        None => None,
        Some(value) => match bool::try_from(value.clone()) {
            Ok(true) => Some(2),
            Ok(false) => None,
            Err(_) => Some(usize::try_from(value)?),
        },
    };
    kwargs.assert_all_used()?;
    let style = match indent {
        Some(indent) => JsonStyle::Pretty(indent),
        None => JsonStyle::Spaced,
    };
    state.charge(value.shape().weight)?;
    let mut out = state.string_builder(0)?;
    write_json(&mut out, value, style, true, Quote::Raw).map_err(|error| {
        if error.kind() == ErrorKind::LimitExceeded {
            error
        } else {
            Error::invalid("cannot serialize to JSON").with_source(error)
        }
    })?;
    out.finish(Safety::Tojson)
}

/// Writes `text` with every byte percent-encoded but ASCII letters, digits and
/// `/._-`.
fn write_percent_encoded(out: &mut crate::output::Output, text: &str) -> Result<(), Error> {
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'-' | b'_') {
            continue;
        }
        out.write_str(text.get(start..index).unwrap_or(""))?;
        out.write_str(&format!("%{byte:02X}"))?;
        start = index + 1;
    }
    out.write_str(text.get(start..).unwrap_or(""))
}

fn urlencode(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    let limit = state.max_output_bytes();
    let mut out = state.string_builder(0)?;
    if value.kind() == ValueKind::Map {
        let mut first = true;
        for (key, item) in value.as_map().into_iter().flat_map(|map| map.iter()) {
            state.charge(1)?;
            if item.is_none() || item.is_undefined() {
                continue;
            }
            if !first {
                out.write_str("&")?;
            }
            first = false;
            state.charge_text_args(std::slice::from_ref(key))?;
            state.charge_text_args(std::slice::from_ref(item))?;
            write_percent_encoded(&mut out, &key.to_string_capped(limit)?)?;
            out.write_str("=")?;
            write_percent_encoded(&mut out, &item.to_string_capped(limit)?)?;
        }
    } else {
        match &value.0 {
            Repr::None | Repr::Undefined(_) => {}
            Repr::Str(text, _) => write_percent_encoded(&mut out, text)?,
            _ => {
                state.charge_text_args(std::slice::from_ref(value))?;
                write_percent_encoded(&mut out, &value.to_string_capped(limit)?)?;
            }
        }
    }
    out.finish(Safety::Plain)
}

// Tests -----------------------------------------------------------------

fn is_undefined(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    Ok(Value::from(value.is_undefined()))
}

fn is_defined(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    Ok(Value::from(!value.is_undefined()))
}

fn is_none(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    Ok(Value::from(value.is_none()))
}

fn is_safe(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    Ok(Value::from(value.is_safe()))
}

fn is_boolean(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    Ok(Value::from(value.kind() == ValueKind::Bool))
}

fn is_odd(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(Value,)>(state, values)?;
    Ok(Value::from(
        i128::try_from(value).is_ok_and(|integer| integer % 2 != 0),
    ))
}

fn is_even(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(Value,)>(state, values)?;
    Ok(Value::from(
        i128::try_from(value).is_ok_and(|integer| integer % 2 == 0),
    ))
}

fn is_divisibleby(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, other) = args::<(&Value, &Value)>(state, values)?;
    Ok(Value::from(match coerce(value, other, false) {
        Some(Coerced::Int(a, b)) => a.checked_rem(b) == Some(0),
        Some(Coerced::Float(a, b)) => (a % b) == 0.0,
        _ => false,
    }))
}

fn is_number(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    Ok(Value::from(matches!(
        value.kind(),
        ValueKind::Number | ValueKind::Bool
    )))
}

fn is_integer(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    Ok(Value::from(value.is_integer()))
}

fn is_float(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    Ok(Value::from(matches!(value.0, Repr::Float(_))))
}

fn is_string(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    Ok(Value::from(value.kind() == ValueKind::String))
}

fn is_sequence(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    Ok(Value::from(
        value.is_undefined() || value.kind() == ValueKind::Seq,
    ))
}

fn is_iterable(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    if let Some(text) = value.as_str() {
        state.charge_scan(text.len())?;
    }
    Ok(Value::from(value.iter_len().is_some()))
}

fn is_mapping(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    Ok(Value::from(value.kind() == ValueKind::Map))
}

fn is_startingwith(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    state.charge_text_args(values)?;
    let (value, other) = args::<(StringInput<'_>, StringInput<'_>)>(state, values)?;
    state.charge_scan(value.as_str().len().min(other.as_str().len()))?;
    Ok(Value::from(value.as_str().starts_with(other.as_str())))
}

fn is_endingwith(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    state.charge_text_args(values)?;
    let (value, other) = args::<(StringInput<'_>, StringInput<'_>)>(state, values)?;
    state.charge_scan(value.as_str().len().min(other.as_str().len()))?;
    Ok(Value::from(value.as_str().ends_with(other.as_str())))
}

/// Whether a character changes case either way.
fn has_case_mapping(ch: char) -> bool {
    ch.to_lowercase().any(|mapped| mapped != ch) || ch.to_uppercase().any(|mapped| mapped != ch)
}

fn is_lower(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    let Some(text) = value.as_str() else {
        return Ok(Value::from(false));
    };
    state.charge_scan(text.len())?;
    let mut has_cased = false;
    for ch in text.chars() {
        if ch.is_uppercase() || (!ch.is_lowercase() && has_case_mapping(ch)) {
            return Ok(Value::from(false));
        }
        has_cased |= ch.is_lowercase();
    }
    Ok(Value::from(has_cased))
}

fn is_upper(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    let Some(text) = value.as_str() else {
        return Ok(Value::from(false));
    };
    state.charge_scan(text.len())?;
    let mut has_cased = false;
    for ch in text.chars() {
        if ch.is_lowercase() || (!ch.is_uppercase() && has_case_mapping(ch)) {
            return Ok(Value::from(false));
        }
        has_cased |= ch.is_uppercase();
    }
    Ok(Value::from(has_cased))
}

fn is_sameas(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, other) = args::<(&Value, &Value)>(state, values)?;
    Ok(Value::from(match (&value.0, &other.0) {
        (Repr::Seq(a, ..), Repr::Seq(b, ..)) => Arc::ptr_eq(a, b),
        (Repr::Map(a, _), Repr::Map(b, _)) => Arc::ptr_eq(a, b),
        (Repr::Function(a), Repr::Function(b)) => Arc::ptr_eq(a, b),
        (Repr::Macro(a), Repr::Macro(b)) => Arc::ptr_eq(a, b),
        (Repr::Module(a), Repr::Module(b)) => Arc::ptr_eq(a, b),
        (Repr::Loop(a), Repr::Loop(b)) => Arc::ptr_eq(a, b),
        (
            Repr::Seq(..)
            | Repr::Map(..)
            | Repr::Function(_)
            | Repr::Macro(_)
            | Repr::Module(_)
            | Repr::Loop(_),
            _,
        )
        | (
            _,
            Repr::Seq(..)
            | Repr::Map(..)
            | Repr::Function(_)
            | Repr::Macro(_)
            | Repr::Module(_)
            | Repr::Loop(_),
        ) => false,
        _ => {
            state.charge(ops::compare_cost(value, other))?;
            value.kind() == other.kind()
                && value.is_integer() == other.is_integer()
                && value == other
        }
    }))
}

fn is_eq(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, other) = args::<(&Value, &Value)>(state, values)?;
    state.charge(ops::compare_cost(value, other))?;
    Ok(Value::from(value == other))
}

fn is_ne(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, other) = args::<(&Value, &Value)>(state, values)?;
    state.charge(ops::compare_cost(value, other))?;
    Ok(Value::from(value != other))
}

fn is_lt(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, other) = args::<(&Value, &Value)>(state, values)?;
    state.charge(ops::compare_cost(value, other))?;
    Ok(Value::from(value < other))
}

fn is_le(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, other) = args::<(&Value, &Value)>(state, values)?;
    state.charge(ops::compare_cost(value, other))?;
    Ok(Value::from(value <= other))
}

fn is_gt(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, other) = args::<(&Value, &Value)>(state, values)?;
    state.charge(ops::compare_cost(value, other))?;
    Ok(Value::from(value > other))
}

fn is_ge(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, other) = args::<(&Value, &Value)>(state, values)?;
    state.charge(ops::compare_cost(value, other))?;
    Ok(Value::from(value >= other))
}

fn is_in(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, other) = args::<(&Value, &Value)>(state, values)?;
    state.assert_not_undefined(other)?;
    state.charge(ops::contains_cost(other, value))?;
    if other.as_str().is_some() {
        state.charge_text_args(std::slice::from_ref(value))?;
    }
    Ok(Value::from(ops::contains(other, value).unwrap_or(false)))
}

fn is_true(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    Ok(Value::from(matches!(value.0, Repr::Bool(true))))
}

fn is_false(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value,) = args::<(&Value,)>(state, values)?;
    Ok(Value::from(matches!(value.0, Repr::Bool(false))))
}

fn is_filter(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (name,) = args::<(&str,)>(state, values)?;
    Ok(Value::from(state.env().filters.contains_key(name)))
}

fn is_test(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (name,) = args::<(&str,)>(state, values)?;
    Ok(Value::from(state.env().tests.contains_key(name)))
}

// Functions -------------------------------------------------------------

/// The most items `range` gives.
const MAX_RANGE: i128 = 100_000;

fn range(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (lower, upper, step) = args::<(i64, Option<i64>, Option<i64>)>(state, values)?;
    let (start, end) = match upper {
        Some(upper) => (i128::from(lower), i128::from(upper)),
        None => (0, i128::from(lower)),
    };
    let step = i128::from(step.unwrap_or(1));
    let count = match step {
        0 => return Err(Error::invalid("cannot create range with step of 0")),
        step if step > 0 => {
            if end > start {
                (end - start + step - 1) / step
            } else {
                0
            }
        }
        step => {
            if start > end {
                (start - end - step - 1) / -step
            } else {
                0
            }
        }
    };
    if count > MAX_RANGE {
        return Err(Error::invalid("range has too many elements"));
    }
    state.charge_items(usize::try_from(count).unwrap_or(usize::MAX))?;
    let items = (0..count)
        .map(|index| Value::from(start + index * step))
        .collect();
    Ok(Value::iterable(items))
}

fn dict(state: &State<'_>, values: &[Value]) -> Result<Value, Error> {
    let (value, kwargs) = args::<(Option<Value>, Kwargs)>(state, values)?;
    let mut map = match value {
        None => ValueMap::new(),
        Some(value) => match value.as_map() {
            Some(pairs) => {
                state.charge_items(pairs.len().saturating_mul(3))?;
                pairs
                    .iter()
                    .map(|(key, item)| (key.clone(), item.clone()))
                    .collect()
            }
            None => return Err(Error::from(ErrorKind::InvalidOperation)),
        },
    };
    state.charge_items(kwargs.values().len().saturating_mul(3))?;
    for (key, item) in kwargs.values().iter() {
        map.insert(key.clone(), item.clone());
    }
    Ok(Value::map(map))
}
