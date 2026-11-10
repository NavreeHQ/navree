//! [`I18nOptions`]: the `i18n` configuration section.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use navree_config::ConfigSection;
use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};

use crate::tag;

/// Where the locale of a request comes from. The sources run in the order
/// of [`I18nOptions::negotiation`]; the first that names a supported
/// locale wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Source {
    /// The query parameter [`I18nOptions::query_param`] (`?locale=fr`).
    Query,
    /// The signed-in user's saved locale ([`I18n::set_user_locale`](crate::I18n::set_user_locale),
    /// `PUT /api/me/locale`). Needs `DatabaseModule`.
    User,
    /// The saved locale of the workspace the request names
    /// ([`I18n::set_workspace_locale`](crate::I18n::set_workspace_locale)),
    /// when the signed-in user is a member of it. Needs `WorkspaceModule`.
    Workspace,
    /// The cookie [`I18nOptions::cookie`].
    Cookie,
    /// `Accept-Language`, by quality.
    Header,
}

impl Source {
    /// `query`, `user`, `workspace`, `cookie` or `header`.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Query => "query",
            Source::User => "user",
            Source::Workspace => "workspace",
            Source::Cookie => "cookie",
            Source::Header => "header",
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Source {
    type Err = String;

    /// The name [`as_str`](Source::as_str) gives.
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "query" => Ok(Source::Query),
            "user" => Ok(Source::User),
            "workspace" => Ok(Source::Workspace),
            "cookie" => Ok(Source::Cookie),
            "header" => Ok(Source::Header),
            _ => Err("expected query, user, workspace, cookie or header".to_owned()),
        }
    }
}

/// What a lookup of a message that no locale defines gives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Missing {
    /// The message id, with a warning logged once per locale and id.
    #[default]
    Warn,
    /// An error: [`I18n::t`](crate::I18n::t) returns
    /// [`I18nError::MissingMessage`](crate::I18nError::MissingMessage) and
    /// `t!` panics with it. For tests.
    Error,
    /// The message id, silently.
    Id,
}

/// Whether variables are wrapped in Unicode isolation marks (U+2068 and
/// U+2069), which keep a left-to-right name readable inside a
/// right-to-left sentence and the other way round.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BidiIsolation {
    /// Only in right-to-left locales (Arabic, Hebrew, Persian, Urdu, ...).
    #[default]
    Auto,
    /// In every locale (also `true`).
    Always,
    /// Never (also `false`).
    Never,
}

impl<'de> Deserialize<'de> for BidiIsolation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BidiVisitor;

        impl Visitor<'_> for BidiVisitor {
            type Value = BidiIsolation;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("\"auto\", \"always\", \"never\" or a boolean")
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(if value {
                    BidiIsolation::Always
                } else {
                    BidiIsolation::Never
                })
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                match value.trim() {
                    "auto" => Ok(BidiIsolation::Auto),
                    "always" | "true" => Ok(BidiIsolation::Always),
                    "never" | "false" => Ok(BidiIsolation::Never),
                    other => Err(E::invalid_value(de::Unexpected::Str(other), &self)),
                }
            }
        }

        deserializer.deserialize_any(BidiVisitor)
    }
}

/// Fluent files embedded in the binary with
/// [`embed_locales!`](crate::embed_locales): `(locale, path, source)`.
/// The default, no files, reads [`I18nOptions::dir`] at boot instead.
#[derive(Clone, Copy, Default)]
pub struct LocaleFiles(Option<&'static [(&'static str, &'static str, &'static str)]>);

impl LocaleFiles {
    /// The embedded files, or `None` when the files are read from the
    /// directory.
    pub fn embedded(&self) -> Option<&'static [(&'static str, &'static str, &'static str)]> {
        self.0
    }
}

impl From<&'static [(&'static str, &'static str, &'static str)]> for LocaleFiles {
    fn from(files: &'static [(&'static str, &'static str, &'static str)]) -> Self {
        LocaleFiles(Some(files))
    }
}

impl fmt::Debug for LocaleFiles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(files) => write!(f, "LocaleFiles(embedded, {} file(s))", files.len()),
            None => f.write_str("LocaleFiles(from the directory)"),
        }
    }
}

/// Options of the localization module (configuration section `i18n`).
///
/// ```toml
/// [i18n]
/// default_locale = "en"
/// locales = ["en", "fr", "de", "pt-BR"]   # or "en,fr,de,pt-BR"
/// dir = "locales"                         # locales/<locale>/*.ftl
/// negotiation = ["query", "user", "workspace", "cookie", "header"]
/// query_param = "locale"                  # ?locale=fr
/// cookie = "locale"
/// missing = "warn"                        # warn | error | id
/// reload = false                          # development: reload changed files
/// bidi_isolation = "auto"                 # auto | always | never
/// routes = true                           # PUT /api/me/locale, GET /api/i18n/locales
/// ```
///
/// Environment variables map as usual (`APP_I18N__DEFAULT_LOCALE=fr`);
/// lists also accept a comma-separated string (`APP_I18N__LOCALES=en,fr`).
#[derive(Clone, Debug, Deserialize, ConfigSection)]
#[config(key = "i18n", docs = "/docs/packages/localization/", validate = Self::check_values)]
#[serde(default)]
pub struct I18nOptions {
    /// The locale when negotiation finds nothing, and the last fallback of
    /// every lookup. Must be one of [`locales`](Self::locales).
    pub default_locale: String,
    /// The supported locales, as BCP 47 tags (`en`, `fr`, `pt-BR`).
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub locales: Vec<String>,
    /// The directory of the Fluent files (`<dir>/<locale>/*.ftl`), relative
    /// to the working directory; not read when [`files`](Self::files) are
    /// embedded.
    pub dir: PathBuf,
    /// The sources of a request's locale, in order: `query`, `user`,
    /// `workspace`, `cookie` and `header` (default `user, workspace,
    /// header`).
    #[serde(deserialize_with = "navree_config::comma_list_of")]
    #[config(default = "user, workspace, header")]
    pub negotiation: Vec<Source>,
    /// The `query` source's parameter.
    pub query_param: String,
    /// The `cookie` source's cookie.
    pub cookie: String,
    /// What a missing message gives.
    #[config(one_of = "warn, error, id")]
    pub missing: Missing,
    /// Whether changed files are reloaded while the app runs (checked
    /// twice a second; for development). Not with embedded files.
    pub reload: bool,
    /// Isolation marks around variables (`auto`, `always` or `never`; also
    /// `true` and `false`).
    #[config(one_of = "auto, always, never")]
    pub bidi_isolation: BidiIsolation,
    /// Whether the module mounts `GET /api/i18n/locales` and, with
    /// `DatabaseModule` and an authenticator in the app,
    /// `PUT /api/me/locale`.
    pub routes: bool,
    /// Files embedded with [`embed_locales!`](crate::embed_locales); not
    /// configurable from files or the environment.
    #[serde(skip)]
    pub files: LocaleFiles,
}

impl Default for I18nOptions {
    fn default() -> Self {
        Self {
            default_locale: "en".to_owned(),
            locales: vec!["en".to_owned()],
            dir: PathBuf::from("locales"),
            negotiation: vec![Source::User, Source::Workspace, Source::Header],
            query_param: "locale".to_owned(),
            cookie: "locale".to_owned(),
            missing: Missing::Warn,
            reload: false,
            bidi_isolation: BidiIsolation::Auto,
            routes: true,
            files: LocaleFiles::default(),
        }
    }
}

impl I18nOptions {
    /// Every problem with the options, naming the configuration keys.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.locales.is_empty() {
            errors.push("i18n.locales must name at least one locale".to_owned());
        }
        let mut seen = Vec::new();
        for locale in &self.locales {
            match tag::canonical(locale) {
                Ok(canonical) if seen.contains(&canonical) => {
                    errors.push(format!("i18n.locales lists `{canonical}` twice"));
                }
                Ok(canonical) => seen.push(canonical),
                Err(_) => errors.push(format!(
                    "i18n.locales: `{locale}` is not a valid BCP 47 language tag"
                )),
            }
        }
        match tag::canonical(&self.default_locale) {
            Ok(default) if !seen.is_empty() && !seen.contains(&default) => errors.push(format!(
                "i18n.default_locale `{default}` must be one of i18n.locales ({})",
                seen.join(", ")
            )),
            Ok(_) => {}
            Err(_) => errors.push(format!(
                "i18n.default_locale: `{}` is not a valid BCP 47 language tag",
                self.default_locale
            )),
        }
        for (index, source) in self.negotiation.iter().enumerate() {
            if self.negotiation[..index].contains(source) {
                errors.push(format!("i18n.negotiation lists `{source}` twice"));
            }
        }
        if self.query_param.trim().is_empty() {
            errors.push("i18n.query_param must not be empty".to_owned());
        }
        if self.cookie.trim().is_empty()
            || !self
                .cookie
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            errors.push(
                "i18n.cookie must be a cookie name (letters, digits, `-`, `_` or `.`)".to_owned(),
            );
        }
        if self.reload && self.files.embedded().is_some() {
            errors.push("i18n.reload needs files read from i18n.dir, not embedded ones".to_owned());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// [`check`](I18nOptions::check) with the problems joined (the boot
    /// calls this).
    fn check_values(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use navree_config::FieldType;
    use serde_json::json;

    use super::*;

    fn parse(value: serde_json::Value) -> I18nOptions {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn defaults_match_the_documentation() {
        let options = I18nOptions::default();
        assert_eq!(options.default_locale, "en");
        assert_eq!(options.locales, ["en"]);
        assert_eq!(options.dir, PathBuf::from("locales"));
        assert_eq!(
            options.negotiation,
            [Source::User, Source::Workspace, Source::Header]
        );
        assert_eq!(options.query_param, "locale");
        assert_eq!(options.cookie, "locale");
        assert_eq!(options.missing, Missing::Warn);
        assert!(!options.reload);
        assert_eq!(options.bidi_isolation, BidiIsolation::Auto);
        assert!(options.routes);
        assert!(options.files.embedded().is_none());
        assert!(options.check().is_ok());
    }

    #[test]
    fn lists_accept_comma_separated_strings() {
        let options = parse(json!({
            "locales": "en, fr,de",
            "negotiation": "query,header",
        }));
        assert_eq!(options.locales, ["en", "fr", "de"]);
        assert_eq!(options.negotiation, [Source::Query, Source::Header]);
    }

    #[test]
    fn bidi_isolation_accepts_words_and_booleans() {
        for (value, expected) in [
            (json!("auto"), BidiIsolation::Auto),
            (json!("always"), BidiIsolation::Always),
            (json!("never"), BidiIsolation::Never),
            (json!(true), BidiIsolation::Always),
            (json!(false), BidiIsolation::Never),
            (json!("false"), BidiIsolation::Never),
        ] {
            assert_eq!(
                parse(json!({ "bidi_isolation": value })).bidi_isolation,
                expected
            );
        }
        assert!(
            serde_json::from_value::<I18nOptions>(json!({ "bidi_isolation": "maybe" })).is_err()
        );
    }

    #[test]
    fn check_names_every_problem() {
        let options = I18nOptions {
            default_locale: "de".to_owned(),
            locales: vec!["en".to_owned(), "EN".to_owned(), "not a tag!".to_owned()],
            negotiation: vec![Source::Header, Source::Header],
            cookie: "bad cookie".to_owned(),
            ..I18nOptions::default()
        };
        let errors = options.check().unwrap_err();
        assert_eq!(errors.len(), 5, "{errors:?}");
        assert!(errors[0].contains("lists `en` twice"), "{errors:?}");
        assert!(errors[1].contains("`not a tag!`"), "{errors:?}");
        assert!(
            errors[2].contains("i18n.default_locale `de` must be one of"),
            "{errors:?}"
        );
        assert!(errors[3].contains("lists `header` twice"), "{errors:?}");
        assert!(errors[4].contains("i18n.cookie"), "{errors:?}");
    }

    #[test]
    fn reload_needs_a_directory() {
        static FILES: &[(&str, &str, &str)] = &[("en", "locales/en/main.ftl", "a = b")];
        let options = I18nOptions {
            reload: true,
            files: FILES.into(),
            ..I18nOptions::default()
        };
        assert!(options.check().unwrap_err()[0].contains("i18n.reload"));
    }

    #[test]
    fn a_source_has_the_name_it_prints() {
        for source in [
            Source::Query,
            Source::User,
            Source::Workspace,
            Source::Cookie,
            Source::Header,
        ] {
            assert_eq!(source.to_string().parse::<Source>(), Ok(source));
        }
        assert!("carrier".parse::<Source>().is_err());
        assert!(
            serde_json::from_value::<I18nOptions>(json!({ "negotiation": "query,carrier" }))
                .is_err()
        );
    }

    #[test]
    fn the_section_describes_itself() {
        let schema = I18nOptions::schema().expect("schema");
        assert_eq!(schema.key, "i18n");
        assert_eq!(schema.docs.as_deref(), Some("/docs/packages/localization/"));
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|field| field.name == name)
                .expect(name)
        };
        assert_eq!(field("locales").ty, FieldType::list(FieldType::String));
        assert_eq!(
            field("missing").ty,
            FieldType::choice(&["warn", "error", "id"])
        );
        assert_eq!(
            field("bidi_isolation").ty,
            FieldType::choice(&["auto", "always", "never"])
        );
        // The embedded files are not a setting.
        assert!(schema.fields.iter().all(|field| field.name != "files"));
        let defaults = I18nOptions::defaults().expect("defaults");
        assert_eq!(defaults["default_locale"], "en");
        assert_eq!(defaults["locales"][0], "en");
        assert_eq!(defaults["negotiation"], "user, workspace, header");
        assert_eq!(defaults["missing"], "warn");
        assert_eq!(defaults["bidi_isolation"], "auto");
    }
}
