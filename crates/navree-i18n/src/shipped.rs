//! The Fluent files navree-i18n ships: validation messages in English,
//! French, German, Spanish, Italian, Dutch and Brazilian Portuguese
//! (`crates/navree-i18n/resources/<tag>/validation.ftl`).

use unic_langid::LanguageIdentifier;

/// The shipped validation messages, by locale.
pub(crate) const VALIDATION: &[(&str, &str)] = &[
    ("en", include_str!("../resources/en/validation.ftl")),
    ("fr", include_str!("../resources/fr/validation.ftl")),
    ("de", include_str!("../resources/de/validation.ftl")),
    ("es", include_str!("../resources/es/validation.ftl")),
    ("it", include_str!("../resources/it/validation.ftl")),
    ("nl", include_str!("../resources/nl/validation.ftl")),
    ("pt-BR", include_str!("../resources/pt-BR/validation.ftl")),
];

/// The words of Navree UI (the `ui` feature) and the form's own checks, by locale.
#[cfg(feature = "ui")]
pub(crate) const UI: &[(&str, &str)] = &[
    ("en", include_str!("../resources/en/ui.ftl")),
    ("hi", include_str!("../resources/hi/ui.ftl")),
];

/// The shipped words of Navree UI for `langid`, if its language has them.
#[cfg(feature = "ui")]
pub(crate) fn ui_for(langid: &LanguageIdentifier) -> Option<&'static str> {
    let language = langid.language.as_str();
    UI.iter()
        .find(|(shipped, _)| *shipped == language)
        .map(|(_, source)| *source)
}

/// The shipped tag for `langid`: the same tag, else the first of the same
/// language (`pt` and `pt-PT` get `pt-BR`, `fr-CA` gets `fr`).
pub(crate) fn validation_tag(langid: &LanguageIdentifier) -> Option<&'static str> {
    let tag = langid.to_string();
    VALIDATION
        .iter()
        .find(|(shipped, _)| *shipped == tag)
        .or_else(|| {
            VALIDATION.iter().find(|(shipped, _)| {
                shipped
                    .split('-')
                    .next()
                    .is_some_and(|language| language == langid.language.as_str())
            })
        })
        .map(|(shipped, _)| *shipped)
}

/// The shipped validation messages for `langid`, if its language has them.
pub(crate) fn validation_for(langid: &LanguageIdentifier) -> Option<&'static str> {
    let tag = validation_tag(langid)?;
    VALIDATION
        .iter()
        .find(|(shipped, _)| *shipped == tag)
        .map(|(_, source)| *source)
}

#[cfg(test)]
mod tests {
    use fluent_bundle::FluentResource;
    use navree_validation::DEFAULT_MESSAGES;

    use super::*;

    #[test]
    fn every_shipped_file_parses_and_covers_every_rule() {
        for (tag, source) in VALIDATION {
            let resource = FluentResource::try_new((*source).to_owned())
                .unwrap_or_else(|(_, errors)| panic!("{tag}: {errors:?}"));
            let ids: Vec<String> = resource
                .entries()
                .filter_map(|entry| match entry {
                    fluent_syntax::ast::Entry::Message(message) => Some(message.id.name.to_owned()),
                    _ => None,
                })
                .collect();
            for (rule, _) in DEFAULT_MESSAGES {
                let id = format!("validation-{}", rule.replace('.', "-"));
                assert!(ids.contains(&id), "{tag} lacks {id}");
            }
            assert_eq!(ids.len(), DEFAULT_MESSAGES.len(), "{tag}");
        }
    }

    /// The ids of the messages in `source`, in order.
    #[cfg(feature = "ui")]
    fn ui_ids(source: &str) -> Vec<String> {
        let resource = FluentResource::try_new(source.to_owned())
            .unwrap_or_else(|(_, errors)| panic!("{errors:?}"));
        resource
            .entries()
            .filter_map(|entry| match entry {
                fluent_syntax::ast::Entry::Message(message) => Some(message.id.name.to_owned()),
                _ => None,
            })
            .collect()
    }

    #[cfg(feature = "ui")]
    #[test]
    fn every_shipped_file_of_navree_ui_parses_and_has_the_same_messages_as_english() {
        let english = ui_ids(UI[0].1);
        assert!(english.len() > 300, "{}", english.len());
        for (tag, source) in UI {
            assert_eq!(ui_ids(source), english, "{tag}");
        }
        let tag = |tag: &str| ui_for(&tag.parse().unwrap()).is_some();
        assert!(tag("en") && tag("en-IN") && tag("hi"));
        assert!(!tag("fr") && !tag("ta"));
    }

    #[test]
    fn languages_share_their_shipped_file() {
        let tag = |tag: &str| validation_tag(&tag.parse().unwrap());
        assert_eq!(tag("fr"), Some("fr"));
        assert_eq!(tag("fr-CA"), Some("fr"));
        assert_eq!(tag("pt"), Some("pt-BR"));
        assert_eq!(tag("pt-PT"), Some("pt-BR"));
        assert_eq!(tag("en-GB"), Some("en"));
        assert_eq!(tag("ja"), None);
    }
}
