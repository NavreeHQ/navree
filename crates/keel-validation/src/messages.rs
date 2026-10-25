//! The message table: Laravel's English validation messages, keyed by rule,
//! and [`Messages`], the table an app actually uses (defaults plus
//! overrides from [`ValidationOptions`](crate::ValidationOptions)).
//!
//! Templates use Laravel's placeholders: `:attribute` (the field's display
//! name, `:Attribute` capitalised) and rule parameters such as `:min`,
//! `:max`, `:other`, `:values` and `:format`.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};

/// The built-in templates, by rule key. `min.*` and `max.*` pick the variant
/// for the field's type (`string`, `array` or `numeric`).
pub const DEFAULT_MESSAGES: &[(&str, &str)] = &[
    (
        "alpha_num",
        "The :attribute field must only contain letters and numbers.",
    ),
    ("array", "The :attribute field must be an array."),
    (
        "ascii",
        "The :attribute field must only contain single-byte alphanumeric characters and symbols.",
    ),
    ("boolean", "The :attribute field must be true or false."),
    (
        "confirmed",
        "The :attribute field confirmation does not match.",
    ),
    ("contains", "The :attribute field must contain :value."),
    (
        "credit_card",
        "The :attribute field must be a valid credit card number.",
    ),
    ("date", "The :attribute field must be a valid date."),
    (
        "date_format",
        "The :attribute field must match the format :format.",
    ),
    (
        "email",
        "The :attribute field must be a valid email address.",
    ),
    (
        "ends_with",
        "The :attribute field must end with one of the following: :values.",
    ),
    ("exists", "The selected :attribute is invalid."),
    ("in", "The selected :attribute is invalid."),
    ("integer", "The :attribute field must be an integer."),
    ("invalid", "The :attribute field is invalid."),
    ("ip", "The :attribute field must be a valid IP address."),
    ("ipv4", "The :attribute field must be a valid IPv4 address."),
    ("ipv6", "The :attribute field must be a valid IPv6 address."),
    ("json", "The :attribute field must be a valid JSON string."),
    (
        "max.array",
        "The :attribute field must not have more than :max items.",
    ),
    (
        "max.numeric",
        "The :attribute field must not be greater than :max.",
    ),
    (
        "max.string",
        "The :attribute field must not be greater than :max characters.",
    ),
    (
        "min.array",
        "The :attribute field must have at least :min items.",
    ),
    ("min.numeric", "The :attribute field must be at least :min."),
    (
        "min.string",
        "The :attribute field must be at least :min characters.",
    ),
    ("numeric", "The :attribute field must be a number."),
    ("object", "The :attribute field must be an object."),
    (
        "phone",
        "The :attribute field must be a valid phone number.",
    ),
    ("prohibited", "The :attribute field is prohibited."),
    ("regex", "The :attribute field format is invalid."),
    ("required", "The :attribute field is required."),
    ("same", "The :attribute field must match :other."),
    (
        "starts_with",
        "The :attribute field must start with one of the following: :values.",
    ),
    ("string", "The :attribute field must be a string."),
    ("unique", "The :attribute has already been taken."),
    ("url", "The :attribute field must be a valid URL."),
    ("uuid", "The :attribute field must be a valid UUID."),
];

/// The rule whose template is used when a rule key has none.
const FALLBACK_RULE: &str = "invalid";

/// The validation messages of an app: [`DEFAULT_MESSAGES`] plus overrides.
///
/// - Message overrides are keyed by rule (`"required"`, `"min.string"`) or,
///   for one field only, by `"<field>.<rule>"` (`"email.unique"`), like
///   Laravel's custom messages.
/// - Attribute names replace a field's display name (`"first_name"` shows
///   as `first name` unless `attributes` says otherwise).
///
/// `ValidationModule` provides it; without the module the extractors use
/// the defaults.
///
/// ```
/// use keel_validation::Messages;
///
/// let messages = Messages::default()
///     .with_message("email.required", "We need your :attribute.")
///     .with_attribute("email", "e-mail address");
/// assert_eq!(
///     messages.render("email", "required", &[]),
///     "We need your e-mail address."
/// );
/// assert_eq!(
///     messages.render("first_name", "min.string", &[("min", "2".into())]),
///     "The first name field must be at least 2 characters."
/// );
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Messages {
    overrides: BTreeMap<String, String>,
    attributes: BTreeMap<String, String>,
}

impl Messages {
    /// The defaults with `messages` and `attributes` on top.
    pub fn new(messages: BTreeMap<String, String>, attributes: BTreeMap<String, String>) -> Self {
        Self {
            overrides: messages,
            attributes,
        }
    }

    /// Overrides the template of `key` (a rule, or `"<field>.<rule>"`).
    pub fn with_message(mut self, key: impl Into<String>, template: impl Into<String>) -> Self {
        self.overrides.insert(key.into(), template.into());
        self
    }

    /// Sets the display name of `field` (a dotted path such as `address.city`).
    pub fn with_attribute(mut self, field: impl Into<String>, name: impl Into<String>) -> Self {
        self.attributes.insert(field.into(), name.into());
        self
    }

    /// The shared default table.
    pub(crate) fn shared_default() -> Arc<Messages> {
        static DEFAULT: LazyLock<Arc<Messages>> = LazyLock::new(|| Arc::new(Messages::default()));
        DEFAULT.clone()
    }

    /// The template for `rule` on `field`: a field-specific override, a rule
    /// override, the default, or the `invalid` template.
    pub fn template(&self, field: &str, rule: &str) -> &str {
        let specific = (!field.is_empty())
            .then(|| self.overrides.get(&format!("{field}.{rule}")))
            .flatten();
        specific
            .or_else(|| self.overrides.get(rule))
            .map(String::as_str)
            .or_else(|| default_template(rule))
            .or_else(|| self.overrides.get(FALLBACK_RULE).map(String::as_str))
            .or_else(|| default_template(FALLBACK_RULE))
            .unwrap_or("The :attribute field is invalid.")
    }

    /// The display name of `field`: its configured attribute name, else the
    /// field in snake case with underscores as spaces (`firstName` and
    /// `first_name` both show as `first name`).
    pub fn attribute(&self, field: &str) -> String {
        match self.attributes.get(field) {
            Some(name) => name.clone(),
            None => snake_case(field).replace('_', " "),
        }
    }

    /// The message for `rule` failing on `field`, with `params` filled in
    /// (`:other` names a field, so it shows as that field's display name).
    pub fn render(&self, field: &str, rule: &str, params: &[(&str, String)]) -> String {
        let attribute = self.attribute(field);
        let mut values: Vec<(&str, String)> = Vec::with_capacity(params.len() + 2);
        values.push(("Attribute", upper_first(&attribute)));
        values.push(("attribute", attribute));
        for (name, value) in params {
            let value = if *name == "other" {
                self.attribute(value)
            } else {
                value.clone()
            };
            values.push((name, value));
        }
        interpolate(self.template(field, rule), &values)
    }
}

/// The built-in template of `rule`.
pub(crate) fn default_template(rule: &str) -> Option<&'static str> {
    DEFAULT_MESSAGES
        .binary_search_by(|(key, _)| (*key).cmp(rule))
        .ok()
        .map(|index| DEFAULT_MESSAGES[index].1)
}

/// Replaces every `:name` placeholder that has a value; leaves the others.
fn interpolate(template: &str, values: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(colon) = rest.find(':') {
        out.push_str(&rest[..colon]);
        let after = &rest[colon + 1..];
        let len = after
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
        let name = &after[..len];
        match values.iter().find(|(key, _)| *key == name) {
            Some((_, value)) if len > 0 => out.push_str(value),
            _ => {
                out.push(':');
                out.push_str(name);
            }
        }
        rest = &after[len..];
    }
    out.push_str(rest);
    out
}

/// `firstName` → `first_name` (ASCII letters only; other characters kept).
fn snake_case(field: &str) -> String {
    let mut out = String::with_capacity(field.len() + 4);
    let mut previous_lower = false;
    for c in field.chars() {
        if c.is_ascii_uppercase() {
            if previous_lower {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            previous_lower = false;
        } else {
            out.push(c);
            previous_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        }
    }
    out
}

fn upper_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_table_is_sorted_for_lookup() {
        assert!(
            DEFAULT_MESSAGES.windows(2).all(|w| w[0].0 < w[1].0),
            "DEFAULT_MESSAGES must stay sorted by key"
        );
        assert_eq!(
            default_template("email"),
            Some("The :attribute field must be a valid email address.")
        );
        assert_eq!(default_template("nope"), None);
    }

    #[test]
    fn placeholders_are_filled_and_unknown_ones_kept() {
        let values = [("attribute", "name".to_owned()), ("min", "2".to_owned())];
        assert_eq!(
            interpolate("The :attribute needs :min (:unknown at 10:30):", &values),
            "The name needs 2 (:unknown at 10:30):"
        );
    }

    #[test]
    fn attributes_are_humanised() {
        let messages = Messages::default();
        assert_eq!(messages.attribute("first_name"), "first name");
        assert_eq!(messages.attribute("firstName"), "first name");
        assert_eq!(
            messages.attribute("items.0.unitPrice"),
            "items.0.unit price"
        );
        assert_eq!(messages.attribute("HTTPCode"), "httpcode");
        let messages = messages.with_attribute("email", "e-mail");
        assert_eq!(messages.attribute("email"), "e-mail");
    }

    #[test]
    fn overrides_win_field_specific_first() {
        let messages = Messages::default()
            .with_message("required", "Need :attribute.")
            .with_message("email.required", "Email please.");
        assert_eq!(messages.render("email", "required", &[]), "Email please.");
        assert_eq!(messages.render("name", "required", &[]), "Need name.");
        assert_eq!(
            messages.render("name", "no_such_rule", &[]),
            "The name field is invalid."
        );
    }

    #[test]
    fn capitalised_attribute_and_other_field() {
        let messages = Messages::default().with_message("same", ":Attribute must match :other.");
        assert_eq!(
            messages.render(
                "password",
                "same",
                &[("other", "password_confirmation".into())]
            ),
            "Password must match password confirmation."
        );
    }
}
