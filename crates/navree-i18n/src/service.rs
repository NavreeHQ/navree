//! The [`I18n`] service and the state its locales share.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use chrono::Utc;
use navree_db::{Db, Record as _, Uow, record};
use uuid::Uuid;

use crate::catalog::{self, Catalog, Lookup, Report};
use crate::format::Formats;
use crate::locale::Locale;
use crate::migration::LocalePreferenceRow;
use crate::options::{I18nOptions, Missing};
use crate::{Args, I18nError, tag};

/// Whose saved locale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    User,
    Workspace,
}

impl Owner {
    fn as_str(self) -> &'static str {
        match self {
            Owner::User => "user",
            Owner::Workspace => "workspace",
        }
    }
}

/// What the locales of one app share: its options, catalog, formatters and
/// database.
pub(crate) struct Shared {
    pub(crate) options: Arc<I18nOptions>,
    catalog: RwLock<Arc<Catalog>>,
    db: Option<Arc<Db>>,
    /// `(locale, id)` pairs already warned about.
    warned: Mutex<HashSet<(String, String)>>,
    formats: Mutex<HashMap<String, Arc<Formats>>>,
}

impl Shared {
    pub(crate) fn catalog(&self) -> Arc<Catalog> {
        self.catalog
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn formats(&self, tag: &str) -> Arc<Formats> {
        let mut formats = self.formats.lock().unwrap_or_else(PoisonError::into_inner);
        formats
            .entry(tag.to_owned())
            .or_insert_with(|| Arc::new(Formats::new(&tag::parse(tag).unwrap_or_default())))
            .clone()
    }

    /// The default locale.
    pub(crate) fn default_locale(self: &Arc<Self>) -> Locale {
        let catalog = self.catalog();
        Locale::of(self, &catalog.default)
    }

    /// The supported locale `requested` matches, else the default one.
    pub(crate) fn locale(self: &Arc<Self>, requested: &str) -> Locale {
        let catalog = self.catalog();
        match catalog.match_tag(requested) {
            Some(tag) => Locale::of(self, tag),
            None => Locale::of(self, &catalog.default),
        }
    }

    /// The supported locale `requested` matches.
    pub(crate) fn matching(self: &Arc<Self>, requested: &str) -> Option<Locale> {
        let catalog = self.catalog();
        catalog
            .match_tag(requested)
            .map(|tag| Locale::of(self, tag))
    }

    /// `id` in `tag` (a supported tag), with `i18n.missing` applied.
    pub(crate) fn translate(&self, tag: &str, id: &str, args: &Args) -> Result<String, I18nError> {
        let catalog = self.catalog();
        match catalog.format(tag, id, args.fluent()) {
            Lookup::Found { text, errors } if errors.is_empty() => Ok(text),
            Lookup::Found { text, errors } => match self.options.missing {
                Missing::Error => Err(I18nError::Format {
                    locale: tag.to_owned(),
                    id: id.to_owned(),
                    reason: errors.join("; "),
                }),
                Missing::Warn | Missing::Id => {
                    if self.first_warning(tag, id) {
                        tracing::warn!(
                            target: "navree::i18n",
                            locale = tag,
                            id,
                            errors = ?errors,
                            "message formatted with errors"
                        );
                    }
                    Ok(text)
                }
            },
            Lookup::Missing => self.missing(tag, id),
        }
    }

    /// What a message no locale defines gives.
    pub(crate) fn missing(&self, tag: &str, id: &str) -> Result<String, I18nError> {
        match self.options.missing {
            Missing::Error => Err(I18nError::MissingMessage {
                locale: tag.to_owned(),
                id: id.to_owned(),
            }),
            Missing::Id => Ok(id.to_owned()),
            Missing::Warn => {
                if self.first_warning(tag, id) {
                    tracing::warn!(
                        target: "navree::i18n",
                        locale = tag,
                        id,
                        "missing message: showing its id"
                    );
                }
                Ok(id.to_owned())
            }
        }
    }

    /// Whether this is the first warning about `id` in `tag`.
    fn first_warning(&self, tag: &str, id: &str) -> bool {
        self.warned
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert((tag.to_owned(), id.to_owned()))
    }

    /// The saved locale of `owner` `id`, if it is still supported.
    pub(crate) async fn saved(
        self: &Arc<Self>,
        owner: Owner,
        id: Uuid,
    ) -> Result<Option<Locale>, I18nError> {
        let Some(db) = &self.db else {
            return Ok(None);
        };
        let row = LocalePreferenceRow::select()
            .eq("scope_type", owner.as_str())
            .eq("scope_id", id)
            .one(&db.read_executor().await)
            .await?;
        Ok(row.and_then(|row| self.matching(&row.locale)))
    }

    async fn save(
        &self,
        uow: &Uow,
        owner: Owner,
        id: Uuid,
        locale: &str,
    ) -> Result<String, I18nError> {
        if self.db.is_none() {
            return Err(I18nError::NoDatabase);
        }
        let tag = self.supported_tag(locale)?;
        let row = LocalePreferenceRow {
            scope_type: owner.as_str().to_owned(),
            scope_id: id,
            locale: tag.clone(),
            updated_at: Utc::now(),
        };
        record::upsert(
            uow,
            &row,
            &["scope_type", "scope_id"],
            &["locale", "updated_at"],
        )
        .await?;
        Ok(tag)
    }

    /// The supported tag `locale` matches, or why there is none.
    pub(crate) fn supported_tag(&self, locale: &str) -> Result<String, I18nError> {
        tag::parse(locale).map_err(|_| I18nError::InvalidLocale(locale.to_owned()))?;
        let catalog = self.catalog();
        catalog
            .match_tag(locale)
            .map(str::to_owned)
            .ok_or_else(|| I18nError::UnsupportedLocale {
                locale: locale.to_owned(),
                supported: catalog.supported.join(", "),
            })
    }
}

/// Translations in an explicit locale, the supported locales, and saved
/// user and workspace locales.
///
/// `I18nModule` provides it (`Arc<I18n>`); [`I18n::new`] builds one
/// without an app.
///
/// ```
/// use navree_i18n::{I18n, I18nOptions, args};
///
/// let i18n = I18n::new(I18nOptions {
///     locales: vec!["en".into(), "fr".into()],
///     dir: "tests/fixtures/locales".into(),
///     ..I18nOptions::default()
/// })
/// .unwrap();
/// assert_eq!(i18n.t("fr", "deal-created", args!(count = 3)).unwrap(), "3 affaires créées");
/// assert_eq!(i18n.locales(), ["en", "fr"]);
/// assert!(i18n.has("fr", "pipeline-empty"));
/// ```
#[derive(Clone)]
pub struct I18n {
    pub(crate) shared: Arc<Shared>,
}

impl I18n {
    /// Loads the Fluent files of `options` (from `options.dir`, or the
    /// embedded `options.files`). Fails on invalid options and on any
    /// problem in the files (a syntax error, a message defined twice),
    /// naming the file and line.
    pub fn new(options: I18nOptions) -> Result<I18n, I18nError> {
        I18n::load(Arc::new(options), None, false)
    }

    /// Loads the files; `lenient` keeps going past problems in them (for
    /// the `i18n:*` commands).
    pub(crate) fn load(
        options: Arc<I18nOptions>,
        db: Option<Arc<Db>>,
        lenient: bool,
    ) -> Result<I18n, I18nError> {
        options.check().map_err(I18nError::Files)?;
        let catalog = load_catalog(&options, lenient)?;
        Ok(I18n {
            shared: Arc::new(Shared {
                options,
                catalog: RwLock::new(Arc::new(catalog)),
                db,
                warned: Mutex::new(HashSet::new()),
                formats: Mutex::new(HashMap::new()),
            }),
        })
    }

    /// The module's options.
    pub fn options(&self) -> &I18nOptions {
        &self.shared.options
    }

    /// Message `id` (or `id.attribute`) in `locale`, with `args`: the
    /// locale's message, else its parent's (`fr-CA` → `fr`), else the
    /// default locale's. A `locale` that matches no supported locale
    /// translates in the default one; one that is not a language tag is
    /// [`I18nError::InvalidLocale`]. A message no locale defines gives the
    /// id (`i18n.missing` `warn` or `id`) or
    /// [`I18nError::MissingMessage`] (`error`).
    pub fn t(&self, locale: &str, id: &str, args: Args) -> Result<String, I18nError> {
        tag::parse(locale).map_err(|_| I18nError::InvalidLocale(locale.to_owned()))?;
        self.locale(locale).translate(id, &args)
    }

    /// Whether `locale` (or a parent of it) defines message `id`: the
    /// default locale's messages do not count for other locales.
    pub fn has(&self, locale: &str, id: &str) -> bool {
        self.locale(locale).has(id)
    }

    /// The supported locales (`i18n.locales`, in their canonical form).
    pub fn locales(&self) -> Vec<String> {
        self.shared.catalog().supported.clone()
    }

    /// The default locale's tag.
    pub fn default_locale(&self) -> String {
        self.shared.catalog().default.clone()
    }

    /// The supported locale `tag` matches (`fr-CA` → `fr`, `pt` →
    /// `pt-BR`), else the default locale.
    pub fn locale(&self, tag: &str) -> Locale {
        self.shared.locale(tag)
    }

    /// The supported locale `tag` matches, if any.
    pub fn matching(&self, tag: &str) -> Option<Locale> {
        self.shared.matching(tag)
    }

    /// Runs `future` in `locale` of this app (see [`Locale::scope`]).
    pub fn scope<F: Future>(&self, locale: &str, future: F) -> impl Future<Output = F::Output> {
        Locale::scope(self.locale(locale), future)
    }

    /// The user's saved locale, else the default one.
    pub async fn user_locale(&self, user_id: Uuid) -> Result<Locale, I18nError> {
        Ok(self
            .shared
            .saved(Owner::User, user_id)
            .await?
            .unwrap_or_else(|| self.shared.default_locale()))
    }

    /// Saves the user's locale (the supported locale `locale` matches) in
    /// `uow`. An unsupported locale is [`I18nError::UnsupportedLocale`];
    /// without `DatabaseModule`, [`I18nError::NoDatabase`].
    pub async fn set_user_locale(
        &self,
        uow: &Uow,
        user_id: Uuid,
        locale: &str,
    ) -> Result<Locale, I18nError> {
        let tag = self.shared.save(uow, Owner::User, user_id, locale).await?;
        Ok(self.locale(&tag))
    }

    /// The workspace's saved locale, else the default one.
    pub async fn workspace_locale(&self, workspace_id: Uuid) -> Result<Locale, I18nError> {
        Ok(self
            .shared
            .saved(Owner::Workspace, workspace_id)
            .await?
            .unwrap_or_else(|| self.shared.default_locale()))
    }

    /// Saves the workspace's default locale in `uow` (as
    /// [`set_user_locale`](I18n::set_user_locale)).
    pub async fn set_workspace_locale(
        &self,
        uow: &Uow,
        workspace_id: Uuid,
        locale: &str,
    ) -> Result<Locale, I18nError> {
        let tag = self
            .shared
            .save(uow, Owner::Workspace, workspace_id, locale)
            .await?;
        Ok(self.locale(&tag))
    }

    /// Reads the Fluent files again (what `i18n.reload` does when a file
    /// changes). On a problem the current messages stay.
    pub fn reload(&self) -> Result<(), I18nError> {
        let catalog = load_catalog(&self.shared.options, false)?;
        *self
            .shared
            .catalog
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Arc::new(catalog);
        Ok(())
    }

    /// The ids of the default locale's messages that `locale` (and its
    /// parents) do not define.
    pub fn missing(&self, locale: &str) -> Result<Vec<String>, I18nError> {
        let tag = self.shared.supported_tag(locale)?;
        Ok(self
            .shared
            .catalog()
            .missing(&tag)
            .into_iter()
            .map(|(id, _)| id)
            .collect())
    }

    /// The `i18n:check` report.
    pub(crate) fn report(&self) -> Report {
        let dir = self.shared.options.dir.to_string_lossy().replace('\\', "/");
        let shown = match self.shared.options.files.embedded() {
            Some(_) => "(embedded)".to_owned(),
            None => dir.trim_end_matches('/').to_owned(),
        };
        self.shared.catalog().report(&shown)
    }
}

impl fmt::Debug for I18n {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let catalog = self.shared.catalog();
        f.debug_struct("I18n")
            .field("default_locale", &catalog.default)
            .field("locales", &catalog.supported)
            .finish_non_exhaustive()
    }
}

fn load_catalog(options: &I18nOptions, lenient: bool) -> Result<Catalog, I18nError> {
    let files = catalog::load_files(options).map_err(I18nError::Files)?;
    Catalog::build(options, files, lenient).map_err(I18nError::Files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args;

    fn fixtures(missing: Missing) -> I18n {
        I18n::new(I18nOptions {
            locales: vec!["en".into(), "fr".into(), "de".into(), "pt-BR".into()],
            dir: "tests/fixtures/locales".into(),
            missing,
            ..I18nOptions::default()
        })
        .unwrap()
    }

    #[test]
    fn translates_in_an_explicit_locale() {
        let i18n = fixtures(Missing::Warn);
        assert_eq!(
            i18n.t("en", "deal-created", args!(count = 1)).unwrap(),
            "One deal created"
        );
        assert_eq!(
            i18n.t("fr", "deal-created", args!(count = 3)).unwrap(),
            "3 affaires créées"
        );
        assert_eq!(
            i18n.t("pt-BR", "deal-created", args!(count = 2)).unwrap(),
            "2 negócios criados"
        );
        // fr-CA is not supported: fr is its nearest supported locale.
        assert_eq!(
            i18n.t("fr-CA", "deal-created", args!(count = 1)).unwrap(),
            "Une affaire créée"
        );
        // Unsupported languages use the default locale.
        assert_eq!(
            i18n.t("ja", "deal-created", args!(count = 2)).unwrap(),
            "2 deals created"
        );
        assert!(matches!(
            i18n.t("not a tag", "deal-created", Args::new()),
            Err(I18nError::InvalidLocale(_))
        ));
    }

    #[test]
    fn missing_messages_follow_the_setting() {
        assert_eq!(
            fixtures(Missing::Warn)
                .t("fr", "nope", Args::new())
                .unwrap(),
            "nope"
        );
        assert_eq!(
            fixtures(Missing::Id).t("fr", "nope", Args::new()).unwrap(),
            "nope"
        );
        assert!(matches!(
            fixtures(Missing::Error).t("fr", "nope", Args::new()),
            Err(I18nError::MissingMessage { .. })
        ));
        // A variable the caller forgot is a formatting error.
        assert!(matches!(
            fixtures(Missing::Error).t("en", "deal-won", args!(owner = "Anu")),
            Err(I18nError::Format { .. })
        ));
        assert_eq!(
            fixtures(Missing::Warn)
                .t("en", "deal-won", args!(owner = "Anu"))
                .unwrap(),
            "Anu won {$title}."
        );
    }

    #[test]
    fn has_locales_and_missing() {
        let i18n = fixtures(Missing::Warn);
        assert!(i18n.has("fr", "pipeline-empty"));
        assert!(!i18n.has("de", "pipeline-empty"));
        assert_eq!(i18n.locales(), ["en", "fr", "de", "pt-BR"]);
        assert_eq!(i18n.default_locale(), "en");
        assert_eq!(i18n.locale("pt").tag(), "pt-BR");
        assert!(i18n.matching("ja").is_none());
        assert!(
            i18n.missing("de")
                .unwrap()
                .contains(&"pipeline-empty".to_owned())
        );
        assert!(matches!(
            i18n.missing("ja"),
            Err(I18nError::UnsupportedLocale { .. })
        ));
    }

    #[test]
    fn invalid_options_and_files_fail() {
        let error = I18n::new(I18nOptions {
            default_locale: "de".into(),
            ..I18nOptions::default()
        })
        .unwrap_err();
        assert!(error.to_string().contains("i18n.default_locale"), "{error}");
    }

    #[tokio::test]
    async fn scopes_set_the_current_locale() {
        let i18n = fixtures(Missing::Warn);
        let text = i18n
            .scope("de", async { crate::t!("deal-created", count = 2) })
            .await;
        assert_eq!(text, "2 Geschäfte angelegt");
    }
}
