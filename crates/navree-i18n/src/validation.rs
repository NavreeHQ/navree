//! Validation messages from Fluent: the `MessageTranslator` navree-i18n
//! registers for navree-validation's extractors.

use std::sync::Arc;

use navree_validation::{DEFAULT_MESSAGES, MessageTranslator, Messages};

use crate::catalog::{Catalog, LocaleBundle, Lookup, format_in};
use crate::locale::Locale;
use crate::{Args, I18n};

/// Renders validation messages in the current locale. For rule `min.string`
/// on field `first_name` it tries, in order:
///
/// 1. the app's message `validation-min-string` in the locale (or a
///    parent of it);
/// 2. in the default locale's language, the `validation.messages`
///    override;
/// 3. the message navree-i18n ships for the locale's language;
/// 4. the `validation.messages` override (other languages);
/// 5. the default locale's message (the app's, then the shipped one);
/// 6. the built-in English template.
///
/// The field's display name is the app's message `field-first_name` (a
/// dotted path's dots become hyphens: `field-address-city`), else
/// `validation.attributes`, else the path. Variables: `$attribute`,
/// `$Attribute` (capitalised) and the rule's own (`$min`, `$max`,
/// `$other` (a display name), `$values`, `$value`, `$format`,
/// `$pattern`); numeric ones are numbers, so they select plural variants.
pub(crate) struct Translator {
    pub(crate) i18n: Arc<I18n>,
}

impl Translator {
    /// The current locale, as a locale of this app.
    fn locale(&self) -> Locale {
        let current = Locale::current();
        if current.belongs_to(&self.i18n.shared) {
            current
        } else {
            self.i18n.locale(current.tag())
        }
    }

    fn display_name(
        &self,
        catalog: &Catalog,
        tag: &str,
        messages: &Messages,
        field: &str,
    ) -> String {
        let id = format!("field-{}", field.replace('.', "-"));
        let (own, fallback) = catalog.split_chain(tag);
        own.into_iter()
            .chain(fallback)
            .find(|bundle| bundle.messages.contains_key(&id))
            .and_then(|bundle| found(format_in(bundle, &id, None)))
            .unwrap_or_else(|| messages.attribute(field))
    }
}

impl MessageTranslator for Translator {
    fn message(
        &self,
        messages: &Messages,
        field: &str,
        rule: &str,
        params: &[(&str, String)],
    ) -> String {
        let locale = self.locale();
        let catalog = self.i18n.shared.catalog();
        let tag = locale.tag();
        let attribute = self.display_name(&catalog, tag, messages, field);
        let mut values: Vec<(&str, String)> = vec![
            ("Attribute", upper_first(&attribute)),
            ("attribute", attribute),
        ];
        for (name, value) in params {
            let value = if *name == "other" {
                self.display_name(&catalog, tag, messages, value)
            } else {
                value.clone()
            };
            values.push((name, value));
        }
        let mut args = Args::new();
        for (name, value) in &values {
            match value.parse::<f64>() {
                Ok(number) if number.is_finite() && !value.starts_with('+') => {
                    args.set(name, number)
                }
                _ => args.set(name, value.as_str()),
            };
        }

        let id = format!("validation-{}", rule.replace('.', "-"));
        let (own, fallback) = catalog.split_chain(tag);
        let app = |bundles: &[&LocaleBundle]| {
            bundles
                .iter()
                .find(|bundle| bundle.messages.contains_key(&id))
                .and_then(|bundle| found(format_in(bundle, &id, args.fluent())))
        };
        let shipped = |bundles: &[&LocaleBundle]| {
            bundles
                .iter()
                .find(|bundle| !bundle.messages.contains_key(&id) && bundle.bundle.has_message(&id))
                .and_then(|bundle| found(format_in(bundle, &id, args.fluent())))
        };
        let overridden =
            || overridden(messages, field, rule).map(|template| Messages::fill(template, &values));
        let default_language = catalog
            .default
            .split('-')
            .next()
            .is_some_and(|language| language == locale.language());

        if let Some(text) = app(&own) {
            return text;
        }
        if default_language && let Some(text) = overridden() {
            return text;
        }
        if let Some(text) = shipped(&own) {
            return text;
        }
        if let Some(text) = overridden() {
            return text;
        }
        if let Some(text) = app(&fallback).or_else(|| shipped(&fallback)) {
            return text;
        }
        if rule != INVALID && messages.lookup(field, rule).is_none() {
            // A rule with no message anywhere reads as `invalid`.
            return self.message(messages, field, INVALID, params);
        }
        Messages::fill(messages.template(field, rule), &values)
    }

    fn attribute(&self, messages: &Messages, field: &str) -> String {
        let locale = self.locale();
        let catalog = self.i18n.shared.catalog();
        self.display_name(&catalog, locale.tag(), messages, field)
    }
}

/// The rule whose message stands for rules that have none.
const INVALID: &str = "invalid";

/// The text of a lookup that found a message.
fn found(lookup: Option<Lookup>) -> Option<String> {
    match lookup? {
        Lookup::Found { text, .. } => Some(text),
        Lookup::Missing => None,
    }
}

/// The app's `validation.messages` template for `rule` on `field`, when it
/// has one (not the built-in template).
fn overridden<'a>(messages: &'a Messages, field: &str, rule: &str) -> Option<&'a str> {
    let template = messages.lookup(field, rule)?;
    let built_in = DEFAULT_MESSAGES
        .iter()
        .find(|(key, _)| *key == rule)
        .map(|(_, template)| *template);
    (built_in != Some(template)).then_some(template)
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
    use crate::I18nOptions;

    fn translator(dir: &str, default: &str) -> Translator {
        Translator {
            i18n: Arc::new(
                I18n::new(I18nOptions {
                    locales: vec!["en".into(), "fr".into(), "de".into(), "ja".into()],
                    default_locale: default.into(),
                    dir: dir.into(),
                    ..I18nOptions::default()
                })
                .unwrap(),
            ),
        }
    }

    fn render(
        translator: &Translator,
        tag: &str,
        messages: &Messages,
        field: &str,
        rule: &str,
        params: &[(&str, String)],
    ) -> String {
        let locale = translator.i18n.locale(tag);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(Locale::scope(locale, async {
            translator.message(messages, field, rule, params)
        }))
    }

    #[test]
    fn english_matches_the_built_in_messages() {
        let translator = translator("tests/fixtures/none", "en");
        let messages = Messages::default();
        for (rule, _) in DEFAULT_MESSAGES {
            let params = [
                ("min", "2".to_owned()),
                ("max", "5".to_owned()),
                ("other", "password".to_owned()),
                ("values", "a, b".to_owned()),
                ("value", "x".to_owned()),
                ("format", "YYYY-MM-DD".to_owned()),
                ("pattern", "^a$".to_owned()),
            ];
            assert_eq!(
                render(&translator, "en", &messages, "name", rule, &params),
                messages.render("name", rule, &params),
                "{rule}"
            );
        }
    }

    #[test]
    fn shipped_translations_with_plurals_and_display_names() {
        let translator = translator("tests/fixtures/locales", "en");
        let messages = Messages::default();
        assert_eq!(
            render(&translator, "fr", &messages, "first_name", "required", &[]),
            "prénom ne doit pas être vide"
        );
        assert_eq!(
            render(&translator, "fr", &messages, "email", "email", &[]),
            "adresse e-mail doit être une adresse e-mail valide"
        );
        assert_eq!(
            render(
                &translator,
                "fr",
                &messages,
                "last_name",
                "min.string",
                &[("min", "1".into())]
            ),
            "nom doit contenir au moins 1 caractère"
        );
        assert_eq!(
            render(
                &translator,
                "fr",
                &messages,
                "last_name",
                "min.string",
                &[("min", "3".into())]
            ),
            "nom doit contenir au moins 3 caractères"
        );
        assert_eq!(
            render(
                &translator,
                "de",
                &messages,
                "tags",
                "min.array",
                &[("min", "2".into())]
            ),
            "tags muss mindestens 2 Elemente enthalten"
        );
        assert_eq!(
            render(
                &translator,
                "fr",
                &messages,
                "password",
                "same",
                &[("other", "first_name".into())]
            ),
            "password doit correspondre à prénom"
        );
    }

    #[test]
    fn app_messages_and_overrides_take_precedence() {
        let translator = translator("tests/fixtures/locales", "en");
        let messages = Messages::default()
            .with_message("unique", ":attribute is taken")
            .with_message("email", ":attribute looks wrong");
        // The app's own French message beats everything.
        assert_eq!(
            render(&translator, "fr", &messages, "email", "unique", &[]),
            "adresse e-mail est déjà prise"
        );
        // In English (the default language) the override beats the shipped message.
        assert_eq!(
            render(&translator, "en", &messages, "name", "email", &[]),
            "name looks wrong"
        );
        // In German the shipped message beats the English override.
        assert_eq!(
            render(&translator, "de", &messages, "name", "email", &[]),
            "name muss eine gültige E-Mail-Adresse sein"
        );
        // Japanese has nothing of its own: the override, then the default locale.
        assert_eq!(
            render(&translator, "ja", &messages, "name", "email", &[]),
            "name looks wrong"
        );
        assert_eq!(
            render(
                &translator,
                "ja",
                &Messages::default(),
                "name",
                "required",
                &[]
            ),
            "name should not be empty"
        );
        // Unknown rules use the `invalid` template.
        assert_eq!(
            render(
                &translator,
                "fr",
                &Messages::default(),
                "name",
                "no_such_rule",
                &[]
            ),
            "name n'est pas valide"
        );
    }

    #[test]
    fn attributes_are_translated() {
        let translator = translator("tests/fixtures/locales", "en");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let name = runtime.block_on(translator.i18n.scope("fr", async {
            translator.attribute(&Messages::default(), "first_name")
        }));
        assert_eq!(name, "prénom");
        let messages = Messages::default().with_attribute("company", "the company");
        let name = runtime.block_on(
            translator
                .i18n
                .scope("fr", async { translator.attribute(&messages, "company") }),
        );
        assert_eq!(name, "the company");
    }
}
