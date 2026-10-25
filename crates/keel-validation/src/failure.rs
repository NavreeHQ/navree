//! Raw validation failures (a field path plus a rule key and parameters),
//! collected from garde reports, serde errors and async rules, and rendered
//! into [`ValidationErrors`] with a [`Messages`] table at the end.
//!
//! garde's built-in rules report through its `I18n` hook; inside
//! [`scope`] that hook (and keel's own rules, see [`rule_error`]) encode the
//! rule key and parameters into the garde message, so the template is picked
//! only when rendering: once the app's [`Messages`] are known and with the
//! JSON input at hand (`min`/`max` on an array use the `array` messages).

use std::borrow::Cow;
use std::cell::Cell;
use std::fmt::Display;

use garde::i18n::{I18n, InvalidCreditCard, InvalidEmail, InvalidPhoneNumber, InvalidUrl, IpKind};
use serde_json::Value;

use crate::{Messages, ValidationErrors};

/// Starts an encoded rule message: `MARK key (SEP name=value)*`.
const MARK: &str = "\u{1f}keel\u{1f}";
const SEP: char = '\u{1f}';

/// One failed rule on one field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Failure {
    /// The dotted field path (`items.0.sku`); empty for the input as a whole.
    pub(crate) path: String,
    pub(crate) rule: Rule,
}

/// What failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Rule {
    /// A rule from the message table, with its parameters.
    Keyed {
        key: String,
        params: Vec<(String, String)>,
    },
    /// A ready message (a custom garde rule with its own text).
    Text(String),
}

impl Failure {
    pub(crate) fn keyed(path: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            rule: Rule::Keyed {
                key: key.into(),
                params: Vec::new(),
            },
        }
    }
}

thread_local! {
    /// Whether keel's i18n hook is installed on this thread.
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f` (synchronous garde validation) with keel's message encoding.
pub(crate) fn scope<R>(f: impl FnOnce() -> R) -> R {
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE.with(|active| active.set(self.0));
        }
    }
    let _reset = Reset(ACTIVE.with(|active| active.replace(true)));
    garde::with_i18n(KeelI18n, f)
}

/// A garde error for `rule` with `params`, rendered from the app's message
/// table (so it can be overridden like any built-in rule). Use it in your
/// own garde `custom` rules:
///
/// ```
/// fn even(value: &u32, _: &()) -> garde::Result {
///     if value.is_multiple_of(2) {
///         Ok(())
///     } else {
///         // Override with `ValidationOptions { messages: {"even": "..."} }`.
///         Err(keel_validation::rule_error("even", &[]))
///     }
/// }
/// ```
///
/// A rule key without a template (default or override) renders the
/// `invalid` template. Custom rules may also return plain text with an
/// `:attribute` placeholder (`garde::Error::new("The :attribute is odd.")`).
///
/// Outside keel's extractors the message is rendered at once from the
/// default table, for a field called `value`.
pub fn rule_error(rule: &str, params: &[(&str, &dyn Display)]) -> garde::Error {
    if ACTIVE.with(Cell::get) {
        garde::Error::new(encode(rule, params))
    } else {
        let params: Vec<(&str, String)> = params
            .iter()
            .map(|(name, value)| (*name, value.to_string()))
            .collect();
        garde::Error::new(Messages::default().render("value", resolve_key(rule, None), &params))
    }
}

fn encode(rule: &str, params: &[(&str, &dyn Display)]) -> String {
    let mut out = format!("{MARK}{rule}");
    for (name, value) in params {
        out.push(SEP);
        out.push_str(name);
        out.push('=');
        out.push_str(&value.to_string());
    }
    out
}

fn decode(message: &str) -> Rule {
    let Some(rest) = message.strip_prefix(MARK) else {
        return Rule::Text(message.to_owned());
    };
    let mut parts = rest.split(SEP);
    let key = parts.next().unwrap_or_default().to_owned();
    let params = parts
        .map(|part| match part.split_once('=') {
            Some((name, value)) => (name.to_owned(), value.to_owned()),
            None => (part.to_owned(), String::new()),
        })
        .collect();
    Rule::Keyed { key, params }
}

/// Encoded messages for garde's built-in rules.
struct KeelI18n;

fn keyed(rule: &str, params: &[(&str, &dyn Display)]) -> Cow<'static, str> {
    Cow::Owned(encode(rule, params))
}

impl I18n for KeelI18n {
    fn length_lower_than(&self, min: usize) -> Cow<'static, str> {
        keyed("min.length", &[("min", &min)])
    }

    fn length_greater_than(&self, max: usize) -> Cow<'static, str> {
        keyed("max.length", &[("max", &max)])
    }

    fn range_lower_than(&self, min: &dyn Display) -> Cow<'static, str> {
        keyed("min.numeric", &[("min", min)])
    }

    fn range_greater_than(&self, max: &dyn Display) -> Cow<'static, str> {
        keyed("max.numeric", &[("max", max)])
    }

    fn credit_card_invalid(&self, _reason: InvalidCreditCard) -> Cow<'static, str> {
        keyed("credit_card", &[])
    }

    fn pattern_no_match(&self, pattern: &dyn Display) -> Cow<'static, str> {
        keyed("regex", &[("pattern", pattern)])
    }

    fn contains_missing(&self, pattern: &dyn Display) -> Cow<'static, str> {
        keyed("contains", &[("value", pattern)])
    }

    fn url_invalid(&self, _reason: InvalidUrl) -> Cow<'static, str> {
        keyed("url", &[])
    }

    fn prefix_missing(&self, pattern: &dyn Display) -> Cow<'static, str> {
        keyed("starts_with", &[("values", pattern)])
    }

    fn suffix_missing(&self, pattern: &dyn Display) -> Cow<'static, str> {
        keyed("ends_with", &[("values", pattern)])
    }

    fn phone_number_invalid(&self, _reason: InvalidPhoneNumber) -> Cow<'static, str> {
        keyed("phone", &[])
    }

    fn ip_invalid(&self, kind: IpKind) -> Cow<'static, str> {
        let rule = match kind {
            IpKind::Any => "ip",
            IpKind::V4 => "ipv4",
            IpKind::V6 => "ipv6",
        };
        keyed(rule, &[])
    }

    fn matches_field_mismatch(&self, field: &dyn Display) -> Cow<'static, str> {
        keyed("same", &[("other", field)])
    }

    fn email_invalid(&self, _reason: InvalidEmail) -> Cow<'static, str> {
        keyed("email", &[])
    }

    fn ascii_invalid(&self) -> Cow<'static, str> {
        keyed("ascii", &[])
    }

    fn alphanumeric_invalid(&self) -> Cow<'static, str> {
        keyed("alpha_num", &[])
    }

    fn required_not_set(&self) -> Cow<'static, str> {
        keyed("required", &[])
    }
}

/// The failures of a garde report.
pub(crate) fn from_report(report: garde::Report) -> Vec<Failure> {
    report
        .into_inner()
        .into_iter()
        .map(|(path, error)| Failure {
            path: dotted(&path),
            rule: decode(error.message()),
        })
        .collect()
}

/// `items[0].sku` as `items.0.sku`, from the path's serialized components
/// (`[["key", "items"], ["index", "0"], ...]`; unnamed components skipped).
fn dotted(path: &garde::Path) -> String {
    let Ok(Value::Array(components)) = serde_json::to_value(path) else {
        return path.to_string();
    };
    components
        .iter()
        .filter_map(|component| match component {
            Value::Array(pair) if pair.first().and_then(Value::as_str) != Some("none") => {
                pair.get(1).and_then(Value::as_str)
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// The failure for a serde error `message` at `path`: a missing field is
/// `required` and an unknown one `prohibited` (both at the field's own
/// path), a `null` is `required`, a wrong type names the expected type
/// (`string`, `integer`, `numeric`, `boolean`, `array`, `object`), an
/// unknown enum variant is `in`, a bad UUID `uuid`, a bad date `date`;
/// anything else is `invalid`.
pub(crate) fn from_serde(path: &serde_path_to_error::Path, message: &str) -> Failure {
    let base = serde_path(path);
    if let Some(field) = backticked(message, "missing field `") {
        return Failure::keyed(join(&base, field), "required");
    }
    if let Some(field) = backticked(message, "unknown field `") {
        // serde_path_to_error has already recorded the unknown key.
        let path = if base == field || base.ends_with(&format!(".{field}")) {
            base
        } else {
            join(&base, field)
        };
        return Failure::keyed(path, "prohibited");
    }
    Failure::keyed(base, classify(message))
}

/// The dotted path of a serde error (`items.0.quantity`).
fn serde_path(path: &serde_path_to_error::Path) -> String {
    use serde_path_to_error::Segment;
    path.iter()
        .filter_map(|segment| match segment {
            Segment::Seq { index } => Some(index.to_string()),
            Segment::Map { key } => Some(key.clone()),
            Segment::Enum { variant } => Some(variant.clone()),
            Segment::Unknown => None,
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn join(base: &str, field: &str) -> String {
    if base.is_empty() {
        field.to_owned()
    } else {
        format!("{base}.{field}")
    }
}

/// The text between `prefix` and the next backtick.
fn backticked<'a>(message: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = message.strip_prefix(prefix)?;
    rest.split_once('`').map(|(field, _)| field)
}

/// Messages of `FromStr` errors behind query and form values.
const INTEGER_ERRORS: &[&str] = &[
    "invalid digit found in string",
    "cannot parse integer from empty string",
    "number too large to fit in target type",
    "number too small to fit in target type",
];
const NUMBER_ERRORS: &[&str] = &[
    "invalid float literal",
    "cannot parse float from empty string",
];
const DATE_ERRORS: &[&str] = &[
    "input contains invalid characters",
    "premature end of input",
    "trailing input",
    "input is out of range",
    "input is not enough for unique date and time",
    "no possible date and time matching input",
];

fn classify(message: &str) -> &'static str {
    if message.starts_with("invalid type: null")
        || message.starts_with("invalid type: unit value")
        || message.starts_with("invalid type: Option value")
    {
        return "required";
    }
    if message.starts_with("unknown variant `") {
        return "in";
    }
    if message.contains("UUID parsing failed") {
        return "uuid";
    }
    if (message.starts_with("invalid type: ") || message.starts_with("invalid value: "))
        && let Some((_, expected)) = message.split_once(", expected ")
    {
        return expected_rule(expected);
    }
    if INTEGER_ERRORS.iter().any(|e| message.contains(e)) {
        "integer"
    } else if NUMBER_ERRORS.iter().any(|e| message.contains(e)) {
        "numeric"
    } else if message.contains("provided string was not `true` or `false`") {
        "boolean"
    } else if DATE_ERRORS.iter().any(|e| message.contains(e)) {
        "date"
    } else {
        "invalid"
    }
}

/// The rule for serde's `expected ...` text.
fn expected_rule(expected: &str) -> &'static str {
    const INTEGERS: &[&str] = &[
        "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
    ];
    let expected = expected.trim();
    let first_word = expected.split_whitespace().next().unwrap_or_default();
    if INTEGERS.contains(&first_word) || expected.contains("integer") {
        "integer"
    } else if first_word == "f32" || first_word == "f64" || expected.contains("number") {
        "numeric"
    } else if expected.contains("UUID") {
        "uuid"
    } else if expected.contains("date") || expected.contains("time") {
        "date"
    } else if expected.contains("string") || expected.contains("character") {
        "string"
    } else if expected.contains("boolean") {
        "boolean"
    } else if expected.contains("sequence")
        || expected.contains("tuple")
        || expected.contains("array")
    {
        "array"
    } else if expected.starts_with("struct ") || expected.contains("map") {
        "object"
    } else if expected.contains("variant") || expected.starts_with("enum ") {
        "in"
    } else {
        "invalid"
    }
}

/// The rule whose template renders `key`: `min.length` and `max.length`
/// become the `array` variant when the input at the path is an array or an
/// object, else the `string` variant.
fn resolve_key<'a>(key: &'a str, input: Option<&Value>) -> &'a str {
    let collection = matches!(input, Some(Value::Array(_) | Value::Object(_)));
    match (key, collection) {
        ("min.length", true) => "min.array",
        ("min.length", false) => "min.string",
        ("max.length", true) => "max.array",
        ("max.length", false) => "max.string",
        _ => key,
    }
}

/// The JSON value at a dotted path.
fn lookup<'a>(input: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(input);
    }
    path.split('.')
        .try_fold(input, |value, segment| match value {
            Value::Object(map) => map.get(segment),
            Value::Array(items) => segment.parse::<usize>().ok().and_then(|i| items.get(i)),
            _ => None,
        })
}

/// Renders `failures` with `messages`. `input` is the JSON body, when
/// there is one; `root` names errors about the input as a whole.
pub(crate) fn render(
    failures: &[Failure],
    messages: &Messages,
    input: Option<&Value>,
    root: &str,
) -> ValidationErrors {
    let mut errors = ValidationErrors::new();
    for failure in failures {
        let field = if failure.path.is_empty() {
            root
        } else {
            failure.path.as_str()
        };
        let message = match &failure.rule {
            Rule::Text(text) => {
                let attribute = messages.attribute(field);
                text.replace(":attribute", &attribute)
            }
            Rule::Keyed { key, params } => {
                let at = input.and_then(|input| lookup(input, &failure.path));
                let params: Vec<(&str, String)> = params
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.clone()))
                    .collect();
                messages.render(field, resolve_key(key, at), &params)
            }
        };
        errors.add(field, message);
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_messages_round_trip() {
        let encoded = encode("min.numeric", &[("min", &18), ("x", &"a=b")]);
        assert_eq!(
            decode(&encoded),
            Rule::Keyed {
                key: "min.numeric".into(),
                params: vec![("min".into(), "18".into()), ("x".into(), "a=b".into())],
            }
        );
        assert_eq!(decode("plain text"), Rule::Text("plain text".into()));
    }

    #[test]
    fn garde_paths_become_dotted() {
        let path = garde::Path::new("items").join(0).join("sku");
        assert_eq!(dotted(&path), "items.0.sku");
        assert_eq!(dotted(&garde::Path::empty()), "");
    }

    #[test]
    fn serde_messages_are_classified() {
        assert_eq!(
            classify("invalid type: integer `5`, expected a string"),
            "string"
        );
        assert_eq!(
            classify("invalid type: string \"x\", expected u32"),
            "integer"
        );
        assert_eq!(
            classify("invalid value: integer `-1`, expected u32"),
            "integer"
        );
        assert_eq!(
            classify("invalid type: string \"x\", expected f64"),
            "numeric"
        );
        assert_eq!(
            classify("invalid type: null, expected a string"),
            "required"
        );
        assert_eq!(
            classify("invalid type: string \"x\", expected struct Address"),
            "object"
        );
        assert_eq!(
            classify("invalid type: integer `1`, expected a formatted date string"),
            "date"
        );
        assert_eq!(classify("unknown variant `x`, expected `a` or `b`"), "in");
        assert_eq!(classify("invalid digit found in string"), "integer");
        assert_eq!(classify("invalid float literal"), "numeric");
        assert_eq!(
            classify("provided string was not `true` or `false`"),
            "boolean"
        );
        assert_eq!(classify("input contains invalid characters"), "date");
        assert_eq!(classify("something else"), "invalid");
    }

    #[test]
    fn lookup_follows_objects_and_arrays() {
        let input = serde_json::json!({"items": [{"sku": "a"}]});
        assert_eq!(lookup(&input, "items.0.sku"), Some(&serde_json::json!("a")));
        assert_eq!(lookup(&input, "items.9"), None);
        assert_eq!(lookup(&input, ""), Some(&input));
    }

    #[test]
    fn rule_error_outside_scope_renders_defaults() {
        let error = rule_error("uuid", &[]);
        assert_eq!(error.message(), "The value field must be a valid UUID.");
        let inside = scope(|| rule_error("uuid", &[]));
        assert_eq!(
            decode(inside.message()),
            Rule::Keyed {
                key: "uuid".into(),
                params: vec![]
            }
        );
    }
}
