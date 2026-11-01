//! Raw validation failures (a field path plus a rule key and parameters),
//! collected from [`Violations`], serde errors and async rules, and
//! rendered into [`ValidationErrors`] with a [`Messages`] table at the end.
//!
//! Rules report a [`Violation`]: a rule key with its parameters, or a text.
//! The template is picked only when rendering, once the app's [`Messages`]
//! are known and with the JSON input at hand (a custom rule's generic
//! `min.length` uses the `array` messages on an array).
//!
//! serde errors are text, so inside the extractors ([`encoding`]) the
//! international types encode their rule key into the error message
//! ([`serde_error`]), and [`from_serde`] decodes it.

use std::cell::Cell;
use std::fmt::Display;

use serde_json::Value;

use crate::{MessageTranslator, Messages, ValidationErrors, Violation, Violations};

/// Starts an encoded rule message: `MARK key (SEP name=value)*`.
const MARK: &str = "\u{1f}navree\u{1f}";
const SEP: char = '\u{1f}';
/// Ends an encoded rule inside a serde error, whose message may go on
/// (serde_json appends the position).
const END: char = '\u{1e}';

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
    /// A ready message (a custom rule with its own text).
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
    /// Whether serde errors encode their rule key on this thread.
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f` (deserializing input) with navree's message encoding, so the
/// international types report their rule key (see [`serde_error`]).
pub(crate) fn encoding<R>(f: impl FnOnce() -> R) -> R {
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE.with(|active| active.set(self.0));
        }
    }
    let _reset = Reset(ACTIVE.with(|active| active.replace(true)));
    f()
}

/// A serde error for a value that fails `rule` while deserializing (a
/// phone number, a country code): inside the extractors it carries the rule
/// key, so the failure renders from the app's message table at the field's
/// path; elsewhere it is `plain`.
pub(crate) fn serde_error<E: serde::de::Error>(rule: &str, plain: impl Display) -> E {
    serde_error_with(rule, &[], plain)
}

/// [`serde_error`] for a rule with parameters.
pub(crate) fn serde_error_with<E: serde::de::Error>(
    rule: &str,
    params: &[(&str, &dyn Display)],
    plain: impl Display,
) -> E {
    if ACTIVE.with(Cell::get) {
        E::custom(format_args!("{}{END}", encode(rule, params)))
    } else {
        E::custom(plain)
    }
}

/// A [`Violation`] of `rule` with `params`, rendered from the app's
/// message table (so it can be overridden like any built-in rule). Use it
/// in your own `custom` rules:
///
/// ```
/// use navree_validation::Violation;
///
/// fn even(value: &u32) -> Result<(), Violation> {
///     if value.is_multiple_of(2) {
///         Ok(())
///     } else {
///         // Override with `ValidationOptions { messages: {"even": "..."} }`.
///         Err(navree_validation::rule_error("even", &[]))
///     }
/// }
/// ```
///
/// A rule key without a template (default or override) renders the
/// `invalid` template. Custom rules may also return plain text with an
/// `:attribute` placeholder (`Err(":attribute must be odd")`).
///
/// It is `Violation::rule(rule)` with each parameter added by
/// [`with`](Violation::with).
pub fn rule_error(rule: &str, params: &[(&str, &dyn Display)]) -> Violation {
    params.iter().fold(
        Violation::rule(rule.to_owned()),
        |violation, (name, value)| violation.with((*name).to_owned(), value),
    )
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

/// The failures of a value's [`Violations`].
pub(crate) fn from_violations(violations: Violations) -> Vec<Failure> {
    violations
        .into_iter()
        .map(|(path, violation)| Failure {
            path,
            rule: match violation.as_text() {
                Some(text) => Rule::Text(text.to_owned()),
                None => Rule::Keyed {
                    key: violation.key().unwrap_or_default().to_owned(),
                    params: violation
                        .params()
                        .map(|(name, value)| (name.to_owned(), value.to_owned()))
                        .collect(),
                },
            },
        })
        .collect()
}

/// The failure for a serde error `message` at `path`: a missing field is
/// `required` and an unknown one `prohibited` (both at the field's own
/// path), a `null` is `required`, a wrong type names the expected type
/// (`string`, `integer`, `numeric`, `boolean`, `array`, `object`), an
/// unknown enum variant is `in` (listing the variants as `:values`), a bad
/// UUID `uuid`, a bad date `date`; a type that reports its own rule
/// ([`serde_error`]) gets that rule; anything else is `invalid`.
pub(crate) fn from_serde(path: &serde_path_to_error::Path, message: &str) -> Failure {
    let base = serde_path(path);
    if let Some(start) = message.find(MARK) {
        let encoded = &message[start..];
        let encoded = encoded.split(END).next().unwrap_or(encoded);
        return Failure {
            path: base,
            rule: decode(encoded),
        };
    }
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
    if message.starts_with("unknown variant `") {
        return Failure {
            path: base,
            rule: Rule::Keyed {
                key: "in".to_owned(),
                params: vec![("values".to_owned(), expected_variants(message))],
            },
        };
    }
    Failure::keyed(base, classify(message))
}

/// The variants serde expected, as `a, b, c`: from "unknown variant `x`,
/// expected `a` or `b`" or "..., expected one of `a`, `b`, `c`". The list
/// comes last; the rejected input before it may contain anything.
fn expected_variants(message: &str) -> String {
    let Some((_, expected)) = message.rsplit_once(", expected ") else {
        return String::new();
    };
    expected
        .split('`')
        .skip(1)
        .step_by(2)
        .collect::<Vec<_>>()
        .join(", ")
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
/// (from a custom rule that doesn't know what it measured) become the
/// `array` variant when the input at the path is an array or an object,
/// else the `string` variant. The built-in `length` reports its variant.
pub(crate) fn resolve_key<'a>(key: &'a str, input: Option<&Value>) -> &'a str {
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

/// Renders `failures` with `messages`, through `translator` when the app
/// has one. `input` is the JSON body, when there is one; `root` names
/// errors about the input as a whole.
pub(crate) fn render(
    failures: &[Failure],
    messages: &Messages,
    translator: Option<&dyn MessageTranslator>,
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
                let attribute = match translator {
                    Some(translator) => translator.attribute(messages, field),
                    None => messages.attribute(field),
                };
                text.replace(":attribute", &attribute)
            }
            Rule::Keyed { key, params } => {
                let at = input.and_then(|input| lookup(input, &failure.path));
                let params: Vec<(&str, String)> = params
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.clone()))
                    .collect();
                let rule = resolve_key(key, at);
                match translator {
                    Some(translator) => translator.message(messages, field, rule, &params),
                    None => messages.render(field, rule, &params),
                }
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
    fn violations_become_failures() {
        let mut violations = Violations::new();
        violations.add("items.0.sku", Violation::rule("min.string").with("min", 1));
        violations.add("name", Violation::text(":attribute must be odd"));
        assert_eq!(
            from_violations(violations),
            [
                Failure {
                    path: "items.0.sku".into(),
                    rule: Rule::Keyed {
                        key: "min.string".into(),
                        params: vec![("min".into(), "1".into())],
                    },
                },
                Failure {
                    path: "name".into(),
                    rule: Rule::Text(":attribute must be odd".into()),
                },
            ]
        );
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
    fn unknown_variants_list_the_allowed_values() {
        #[derive(Debug, serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        #[allow(dead_code)]
        enum Stage {
            Lead,
            Won,
            Lost,
        }
        let error =
            serde_path_to_error::deserialize::<_, Stage>(serde_json::json!("closed")).unwrap_err();
        assert_eq!(
            from_serde(error.path(), &error.inner().to_string()).rule,
            Rule::Keyed {
                key: "in".into(),
                params: vec![("values".into(), "lead, won, lost".into())],
            }
        );
        assert_eq!(
            expected_variants("unknown variant `x`, expected `lead` or `won`"),
            "lead, won"
        );
        assert_eq!(
            expected_variants("unknown variant `x`, expected `lead`"),
            "lead"
        );
        // The rejected input is echoed first; only serde's list counts.
        let error =
            serde_path_to_error::deserialize::<_, Stage>(serde_json::json!("a, expected `admin`"))
                .unwrap_err();
        assert_eq!(
            from_serde(error.path(), &error.inner().to_string()).rule,
            Rule::Keyed {
                key: "in".into(),
                params: vec![("values".into(), "lead, won, lost".into())],
            }
        );
    }

    #[test]
    fn serde_errors_carry_their_rule_inside_the_extractors() {
        fn fail<'de, D: serde::Deserializer<'de>>(_: D) -> Result<(), D::Error> {
            Err(serde_error("phone", "not a phone number"))
        }
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Contact {
            #[serde(deserialize_with = "fail")]
            phone: (),
        }
        let input = r#"{"phone": "x"}"#;

        // From text, serde_json appends the position after the encoding.
        let error = encoding(|| {
            let mut json = serde_json::Deserializer::from_str(input);
            serde_path_to_error::deserialize::<_, Contact>(&mut json).unwrap_err()
        });
        assert!(error.inner().to_string().contains(" at line 1"));
        assert_eq!(
            from_serde(error.path(), &error.inner().to_string()),
            Failure::keyed("phone", "phone")
        );

        let error = serde_json::from_str::<Contact>(input).unwrap_err();
        assert!(
            error.to_string().starts_with("not a phone number"),
            "{error}"
        );
    }

    #[test]
    fn lookup_follows_objects_and_arrays() {
        let input = serde_json::json!({"items": [{"sku": "a"}]});
        assert_eq!(lookup(&input, "items.0.sku"), Some(&serde_json::json!("a")));
        assert_eq!(lookup(&input, "items.9"), None);
        assert_eq!(lookup(&input, ""), Some(&input));
    }

    #[test]
    fn rule_error_is_a_keyed_violation() {
        let error = rule_error("uuid", &[]);
        assert_eq!(error.message(), "value must be a UUID");
        let error = rule_error("date_format", &[("format", &"YYYY-MM-DD")]);
        assert_eq!(
            error,
            Violation::rule("date_format").with("format", "YYYY-MM-DD")
        );
    }
}
