//! The message table: English validation messages (the field path, then
//! the rule, in lower case without a full stop), keyed by rule, and [`Messages`], the table an
//! app actually uses (defaults plus overrides from
//! [`ValidationOptions`](crate::ValidationOptions)).
//!
//! Messages name the field by its property path and have no trailing period
//! (`"email must be a valid email address"`, `"items.1.quantity must not be
//! less than 1"`). Templates use placeholders: `:attribute` (the field's
//! path or configured display name, `:Attribute` capitalised) and rule
//! parameters such as `:min`, `:max`, `:other`, `:values` and `:format`.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};

/// The built-in templates, by rule key. `min.*` and `max.*` pick the variant
/// for the field's type (`string`, `array` or `numeric`).
pub const DEFAULT_MESSAGES: &[(&str, &str)] = &[
    (
        "alpha_num",
        ":attribute must contain only letters and numbers",
    ),
    ("array", ":attribute must be an array"),
    ("ascii", ":attribute must contain only ASCII characters"),
    ("boolean", ":attribute must be a boolean value"),
    ("confirmed", ":attribute confirmation does not match"),
    ("contains", ":attribute must contain a :value string"),
    ("credit_card", ":attribute must be a credit card"),
    ("date", ":attribute must be a valid ISO 8601 date string"),
    (
        "date_format",
        ":attribute must be a valid date in the format :format",
    ),
    ("email", ":attribute must be a valid email address"),
    ("ends_with", ":attribute must end with :values"),
    ("exists", ":attribute does not exist"),
    (
        "in",
        ":attribute must be one of the following values: :values",
    ),
    ("integer", ":attribute must be an integer number"),
    ("invalid", ":attribute is invalid"),
    ("ip", ":attribute must be an ip address"),
    ("ipv4", ":attribute must be an IPv4 address"),
    ("ipv6", ":attribute must be an IPv6 address"),
    ("json", ":attribute must be a valid JSON string"),
    (
        "max.array",
        ":attribute must contain no more than :max elements",
    ),
    ("max.numeric", ":attribute must not be greater than :max"),
    (
        "max.string",
        ":attribute must be shorter than or equal to :max characters",
    ),
    (
        "min.array",
        ":attribute must contain at least :min elements",
    ),
    ("min.numeric", ":attribute must not be less than :min"),
    (
        "min.string",
        ":attribute must be longer than or equal to :min characters",
    ),
    ("numeric", ":attribute must be a number"),
    ("object", ":attribute must be an object"),
    ("phone", ":attribute must be a valid phone number"),
    ("prohibited", "property :attribute should not exist"),
    ("regex", ":attribute must match :pattern regular expression"),
    ("required", ":attribute should not be empty"),
    ("same", ":attribute must match :other"),
    ("starts_with", ":attribute must start with :values"),
    ("string", ":attribute must be a string"),
    ("unique", ":attribute has already been taken"),
    ("url", ":attribute must be a URL address"),
    ("uuid", ":attribute must be a UUID"),
];

/// The rule whose template is used when a rule key has none.
const FALLBACK_RULE: &str = "invalid";

/// The validation messages of an app: [`DEFAULT_MESSAGES`] plus overrides.
///
/// - Message overrides are keyed by rule (`"required"`, `"min.string"`) or,
///   for one field only, by `"<field>.<rule>"` (`"email.unique"`).
/// - Attribute names replace a field's display name (a field shows as its
///   path, `first_name` or `items.0.sku`, unless `attributes` says
///   otherwise).
///
/// `ValidationModule` provides it; without the module the extractors use
/// the defaults.
///
/// ```
/// use navree_validation::Messages;
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
///     "first_name must be longer than or equal to 2 characters"
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
            .unwrap_or(":attribute is invalid")
    }

    /// The display name of `field`: its configured attribute name, else the
    /// field path itself (`first_name`, `items.0.sku`).
    pub fn attribute(&self, field: &str) -> String {
        match self.attributes.get(field) {
            Some(name) => name.clone(),
            None => field.to_owned(),
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
            Some(":attribute must be a valid email address")
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
    fn attributes_are_property_paths() {
        let messages = Messages::default();
        assert_eq!(messages.attribute("first_name"), "first_name");
        assert_eq!(messages.attribute("firstName"), "firstName");
        assert_eq!(messages.attribute("items.0.unitPrice"), "items.0.unitPrice");
        let messages = messages.with_attribute("email", "e-mail");
        assert_eq!(messages.attribute("email"), "e-mail");
    }

    #[test]
    fn defaults_read_like_class_validator() {
        let messages = Messages::default();
        let min = [("min", "2".to_owned())];
        let max = [("max", "255".to_owned())];
        assert_eq!(
            messages.render("email", "email", &[]),
            "email must be a valid email address"
        );
        assert_eq!(
            messages.render("name", "required", &[]),
            "name should not be empty"
        );
        assert_eq!(
            messages.render("name", "max.string", &max),
            "name must be shorter than or equal to 255 characters"
        );
        assert_eq!(
            messages.render("name", "min.string", &min),
            "name must be longer than or equal to 2 characters"
        );
        assert_eq!(
            messages.render("tags", "min.array", &min),
            "tags must contain at least 2 elements"
        );
        assert_eq!(
            messages.render("age", "min.numeric", &min),
            "age must not be less than 2"
        );
        assert_eq!(
            messages.render("extra", "prohibited", &[]),
            "property extra should not exist"
        );
        for (rule, template) in DEFAULT_MESSAGES {
            assert!(
                !template.starts_with("The ") && !template.ends_with('.'),
                "{rule}: {template}"
            );
        }
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
            "name is invalid"
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
            "Password must match password_confirmation."
        );
    }
}
