//! [`AppMessages`]: the app's validation messages, rendered as the extractors
//! render them, for code that checks input on its own path (Navree UI's forms).

use std::fmt;
use std::sync::Arc;

use navree_kernel::AppContext;

use crate::errors::ValidationErrors;
use crate::extract;
use crate::failure;
use crate::{MessageTranslator, Messages, Violation, Violations};

/// The app's validation messages: its [`Messages`] (built-in templates plus the
/// `validation.messages` and `validation.attributes` of `ValidationModule`) and, when
/// the app has one, its [`MessageTranslator`] (navree-i18n's `I18nModule` registers
/// one, which renders in the request's language).
///
/// It renders a failed rule exactly as [`Valid`](crate::Valid) and
/// [`ValidAsync`](crate::ValidAsync) do, so a check that runs on another path says the
/// same thing in the same language. Navree UI's forms read their messages through it.
///
/// ```
/// use navree_validation::{AppMessages, Messages, Violation};
///
/// let messages = AppMessages::new(
///     std::sync::Arc::new(Messages::default().with_message("email", "Check :attribute")),
///     None,
/// )
/// .with_attributes([("email", "Work email")]);
/// assert_eq!(
///     messages.render_violation("email", &Violation::rule("email")),
///     "Check Work email"
/// );
/// ```
#[derive(Clone)]
pub struct AppMessages {
    messages: Arc<Messages>,
    translator: Option<Arc<dyn MessageTranslator>>,
}

impl AppMessages {
    /// `messages`, rendered through `translator` when there is one.
    pub fn new(messages: Arc<Messages>, translator: Option<Arc<dyn MessageTranslator>>) -> Self {
        Self {
            messages,
            translator,
        }
    }

    /// The messages of the app behind `ctx`: `ValidationModule`'s table (the built-in
    /// templates without it) and the app's `dyn MessageTranslator`, if it has one.
    /// Without an app, the built-in English templates.
    pub async fn of(ctx: Option<&AppContext>) -> Self {
        Self {
            messages: extract::messages(ctx).await,
            translator: extract::translator(ctx).await,
        }
    }

    /// The message table.
    pub fn messages(&self) -> &Messages {
        &self.messages
    }

    /// The translator, when the app has one.
    pub fn translator(&self) -> Option<&dyn MessageTranslator> {
        self.translator.as_deref()
    }

    /// The same messages with these display names for fields (`("email", "Work
    /// email")`), in place of the app's `validation.attributes` for the same fields. A
    /// translator still names a field its own way first (navree-i18n's `field-<name>`
    /// message). The app's shared table is not changed.
    pub fn with_attributes<I, F, N>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = (F, N)>,
        F: Into<String>,
        N: Into<String>,
    {
        let mut messages = Messages::clone(&self.messages);
        for (field, name) in names {
            messages = messages.with_attribute(field, name);
        }
        self.messages = Arc::new(messages);
        self
    }

    /// The same messages with these templates for rule keys the app has not set a
    /// message for (`("form.number", ":attribute: enter a number.")`): a key with an
    /// override in `validation.messages`, or a built-in template, keeps it. A
    /// translator still renders the key in the request's language first when it has
    /// it. The app's shared table is not changed.
    pub fn with_default_messages<I, K, T>(mut self, templates: I) -> Self
    where
        I: IntoIterator<Item = (K, T)>,
        K: Into<String>,
        T: Into<String>,
    {
        let mut messages = Messages::clone(&self.messages);
        for (key, template) in templates {
            messages = messages.with_default_message(key, template);
        }
        self.messages = Arc::new(messages);
        self
    }

    /// The message for `rule` (a key such as `required` or `min.string`) failing on
    /// `field`, with the rule's `params`: through the translator when the app has one,
    /// else from the table.
    pub fn render(&self, field: &str, rule: &str, params: &[(&str, String)]) -> String {
        match &self.translator {
            Some(translator) => translator.message(&self.messages, field, rule, params),
            None => self.messages.render(field, rule, params),
        }
    }

    /// The display name of `field`: the translator's, else the table's.
    pub fn attribute(&self, field: &str) -> String {
        match &self.translator {
            Some(translator) => translator.attribute(&self.messages, field),
            None => self.messages.attribute(field),
        }
    }

    /// The message of `violation` on `field`, as the extractors render it.
    pub fn render_violation(&self, field: &str, violation: &Violation) -> String {
        let failures = [failure::of(field, violation)];
        let errors = failure::render(
            &failures,
            &self.messages,
            self.translator.as_deref(),
            None,
            field,
        );
        errors
            .get(field)
            .and_then(|messages| messages.first())
            .cloned()
            .unwrap_or_default()
    }

    /// Every message of `violations`, by field, as the extractors render them. A
    /// violation of the value as a whole (an empty path) is reported under `root`.
    pub fn render_violations(&self, violations: &Violations, root: &str) -> ValidationErrors {
        let failures: Vec<failure::Failure> = violations
            .iter()
            .map(|(path, violation)| failure::of(path, violation))
            .collect();
        failure::render(
            &failures,
            &self.messages,
            self.translator.as_deref(),
            None,
            root,
        )
    }
}

impl Default for AppMessages {
    /// The built-in English templates, with no translator.
    fn default() -> Self {
        Self {
            messages: Messages::shared_default(),
            translator: None,
        }
    }
}

impl fmt::Debug for AppMessages {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppMessages")
            .field("messages", &self.messages)
            .field("translator", &self.translator.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shouts every message, to show it went through the translator.
    struct Loud;

    impl MessageTranslator for Loud {
        fn message(
            &self,
            messages: &Messages,
            field: &str,
            rule: &str,
            params: &[(&str, String)],
        ) -> String {
            messages.render(field, rule, params).to_uppercase()
        }

        fn attribute(&self, messages: &Messages, field: &str) -> String {
            messages.attribute(field).to_uppercase()
        }
    }

    // MOD-005: the same words as the extractors, with names and fallbacks added on top.
    #[test]
    fn renders_as_the_extractors_do_with_names_and_defaults_on_top() {
        let app = Messages::default()
            .with_message("form.number", "App: :attribute")
            .with_attribute("email", "e-mail");
        let messages = AppMessages::new(Arc::new(app), None)
            .with_attributes([("email", "Work email")])
            .with_default_messages([
                ("form.number", ":attribute: enter a number."),
                ("form.date", ":attribute: enter a date."),
                ("required", "never used: a built-in key keeps its template"),
            ]);
        assert_eq!(
            messages.render_violation("email", &Violation::rule("email")),
            "Work email must be a valid email address",
            "a form's name wins over validation.attributes"
        );
        assert_eq!(
            messages.render("amount", "form.number", &[]),
            "App: amount",
            "an app's override wins over a default"
        );
        assert_eq!(
            messages.render("due", "form.date", &[]),
            "due: enter a date."
        );
        assert_eq!(
            messages.render("name", "required", &[]),
            "name should not be empty"
        );
        assert_eq!(
            messages.render_violation("odd", &Violation::text(":attribute is odd")),
            "odd is odd"
        );
        let violations = {
            let mut violations = Violations::new();
            violations.add("email", Violation::rule("unique"));
            violations.add("", Violation::rule("invalid"));
            violations
        };
        let errors = messages.render_violations(&violations, "_form");
        assert_eq!(
            errors.get("email"),
            Some(&["Work email has already been taken".to_owned()][..])
        );
        assert_eq!(
            errors.get("_form"),
            Some(&["_form is invalid".to_owned()][..])
        );
    }

    // MOD-005: a translator renders every message, the field's name included.
    #[test]
    fn a_translator_renders_every_message() {
        let messages = AppMessages::new(Arc::new(Messages::default()), Some(Arc::new(Loud)))
            .with_attributes([("email", "Work email")]);
        assert!(messages.translator().is_some());
        assert_eq!(
            messages.render_violation("email", &Violation::rule("unique")),
            "WORK EMAIL HAS ALREADY BEEN TAKEN"
        );
        assert_eq!(messages.attribute("email"), "WORK EMAIL");
        assert_eq!(
            messages.render_violation("email", &Violation::text(":attribute is taken")),
            "WORK EMAIL is taken"
        );
        assert!(format!("{messages:?}").contains("translator: true"));
        assert!(AppMessages::default().translator().is_none());
    }
}
