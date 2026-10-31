//! [`Locale`]: the locale of the current request, job or scope, as an
//! extractor and a task-local value.

use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use chrono::Datelike;
use http::request::Parts;
use navree_http::AppError;
use navree_http::extract::FromRequestParts;
use unic_langid::LanguageIdentifier;

use crate::format::{DateStyle, Formats, naive_date};
use crate::service::Shared;
use crate::{Args, I18nError, tag};

tokio::task_local! {
    static CURRENT: Locale;
}

/// The I18n of the most recently booted app, for code that runs outside
/// any request or scope (a command, a test).
static GLOBAL: Mutex<Weak<Shared>> = Mutex::new(Weak::new());

pub(crate) fn set_global(shared: &Arc<Shared>) {
    *GLOBAL.lock().unwrap_or_else(PoisonError::into_inner) = Arc::downgrade(shared);
}

pub(crate) fn clear_global(shared: &Arc<Shared>) {
    let mut global = GLOBAL.lock().unwrap_or_else(PoisonError::into_inner);
    if std::ptr::eq(global.as_ptr(), Arc::as_ptr(shared)) {
        *global = Weak::new();
    }
}

fn global() -> Option<Arc<Shared>> {
    GLOBAL
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .upgrade()
}

/// A supported locale of the app, and what it formats and translates with.
///
/// - **As an extractor**, the locale negotiated for the request (see
///   [`Source`](crate::Source)); every response to the request carries
///   `Content-Language`.
/// - **[`Locale::current`]** is the locale of the running code: the
///   request's while a handler runs (and in jobs and mail it queues, which
///   render before they are queued), the one set by [`Locale::scope`], else
///   `i18n.default_locale`. [`t!`](crate::t) translates in it.
/// - **Formatting** follows the locale's conventions (ICU4X data):
///   [`format_number`](Locale::format_number),
///   [`format_money`](Locale::format_money) and
///   [`format_date`](Locale::format_date).
///
/// ```
/// use navree_i18n::{I18n, I18nOptions, Locale};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let i18n = I18n::new(I18nOptions {
///     locales: vec!["en".into(), "fr".into()],
///     ..I18nOptions::default()
/// })
/// .unwrap();
/// let total = Locale::scope(i18n.locale("fr"), async {
///     let locale = Locale::current();
///     assert_eq!(locale.tag(), "fr");
///     locale.format_money(4_200_000, "EUR")
/// })
/// .await;
/// assert_eq!(total, "42\u{202f}000,00\u{a0}€");
/// # }
/// ```
#[derive(Clone)]
pub struct Locale {
    tag: Arc<str>,
    langid: LanguageIdentifier,
    shared: Option<Arc<Shared>>,
}

impl Locale {
    /// A locale of `shared` (`tag` is one of its supported tags).
    pub(crate) fn of(shared: &Arc<Shared>, tag: &str) -> Locale {
        Locale {
            tag: Arc::from(tag),
            langid: tag::parse(tag).unwrap_or_default(),
            shared: Some(shared.clone()),
        }
    }

    /// A locale outside any app: it formats, but translates nothing.
    fn detached(requested: &str) -> Locale {
        let langid = tag::parse(requested).unwrap_or_else(|_| "en".parse().unwrap_or_default());
        Locale {
            tag: Arc::from(langid.to_string()),
            langid,
            shared: None,
        }
    }

    /// The locale of the running code: the request's or the scope's, else
    /// the default locale of the app (`en` outside any app).
    pub fn current() -> Locale {
        CURRENT
            .try_with(Clone::clone)
            .unwrap_or_else(|_| match global() {
                Some(shared) => shared.default_locale(),
                None => Locale::detached("en"),
            })
    }

    /// Runs `future` with `locale` as the current locale: a [`Locale`], or
    /// a tag that is matched with the supported locales as a request's is
    /// (`fr-CA` → `fr`; the default locale when nothing matches).
    ///
    /// ```
    /// use navree_i18n::Locale;
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() {
    /// let tag = Locale::scope("de", async { Locale::current().tag().to_owned() }).await;
    /// # let _ = tag;
    /// # }
    /// ```
    pub fn scope<F: Future>(locale: impl IntoLocale, future: F) -> impl Future<Output = F::Output> {
        CURRENT.scope(locale.into_locale(), future)
    }

    /// The tag (`fr`, `pt-BR`).
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// The language subtag (`pt` for `pt-BR`).
    pub fn language(&self) -> &str {
        self.langid.language.as_str()
    }

    /// Whether the locale writes right to left (Arabic, Hebrew, ...).
    pub fn is_rtl(&self) -> bool {
        tag::is_rtl(&self.langid)
    }

    /// `rtl` or `ltr`, for the `dir` attribute of HTML.
    pub fn direction(&self) -> &'static str {
        if self.is_rtl() { "rtl" } else { "ltr" }
    }

    /// The locale's name in its own language (`Français`, `Deutsch`).
    pub fn native_name(&self) -> String {
        tag::native_name(&self.langid)
    }

    /// Message `id` (or `id.attribute`) in this locale, falling back to its
    /// parents and the default locale. A message no locale defines gives
    /// what `i18n.missing` says: the id (`warn`, `id`) or an error
    /// (`error`).
    pub fn translate(&self, id: &str, args: &Args) -> Result<String, I18nError> {
        match &self.shared {
            Some(shared) => shared.translate(&self.tag, id, args),
            None => Ok(id.to_owned()),
        }
    }

    /// Whether this locale, or a parent of it, defines message `id`.
    pub fn has(&self, id: &str) -> bool {
        self.shared
            .as_ref()
            .is_some_and(|shared| shared.catalog().has(&self.tag, id))
    }

    /// A number with the locale's separators and at most three fraction
    /// digits: `1234.5` is `1,234.5` in `en` and `1 234,5` in `fr`.
    pub fn format_number(&self, value: impl IntoNumber) -> String {
        self.formats().number(value.into_f64())
    }

    /// An amount in minor units (cents) of `currency` (an ISO 4217 code),
    /// with the currency's decimals and its symbol where the locale puts
    /// it: `format_money(4_200_000, "EUR")` is `€42,000.00` in `en` and
    /// `42 000,00 €` in `fr`.
    pub fn format_money(&self, minor_units: i64, currency: &str) -> String {
        self.formats().money(minor_units, currency)
    }

    /// A date (a chrono `NaiveDate`, `NaiveDateTime` or `DateTime`, in its
    /// own time zone) in the locale's medium style: `Oct 26, 2026` in `en`,
    /// `26 oct. 2026` in `fr`.
    pub fn format_date(&self, date: impl Datelike) -> String {
        self.format_date_with(date, DateStyle::Medium)
    }

    /// A date in `style` (`short`, `medium` or `long`).
    pub fn format_date_with(&self, date: impl Datelike, style: DateStyle) -> String {
        self.formats().date(naive_date(&date), None, style, None)
    }

    fn formats(&self) -> Arc<Formats> {
        match &self.shared {
            Some(shared) => shared.formats(&self.tag),
            None => Arc::new(Formats::new(&self.langid)),
        }
    }

    /// Whether this locale belongs to `shared`.
    pub(crate) fn belongs_to(&self, shared: &Arc<Shared>) -> bool {
        self.shared
            .as_ref()
            .is_some_and(|own| Arc::ptr_eq(own, shared))
    }
}

impl AsRef<str> for Locale {
    fn as_ref(&self) -> &str {
        &self.tag
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.tag)
    }
}

impl fmt::Debug for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Locale").field(&&*self.tag).finish()
    }
}

impl PartialEq for Locale {
    fn eq(&self, other: &Self) -> bool {
        self.tag == other.tag
    }
}

impl Eq for Locale {}

impl PartialEq<&str> for Locale {
    fn eq(&self, other: &&str) -> bool {
        &*self.tag == *other
    }
}

impl FromRequestParts for Locale {
    type Rejection = AppError;

    /// The locale negotiated for the request (a 500 without `I18nModule`).
    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Locale>()
            .cloned()
            .ok_or_else(|| AppError::internal("the Locale extractor needs I18nModule in the app"))
    }
}

/// What [`Locale::scope`] accepts: a [`Locale`], or a tag (`&str`,
/// `String`, or a validated [`navree_validation::Locale`]) matched with the
/// supported locales of the current app.
pub trait IntoLocale {
    /// The locale.
    fn into_locale(self) -> Locale;
}

impl IntoLocale for Locale {
    fn into_locale(self) -> Locale {
        self
    }
}

impl IntoLocale for &Locale {
    fn into_locale(self) -> Locale {
        self.clone()
    }
}

impl IntoLocale for &str {
    fn into_locale(self) -> Locale {
        let shared = CURRENT
            .try_with(|current| current.shared.clone())
            .ok()
            .flatten()
            .or_else(global);
        match shared {
            Some(shared) => shared.locale(self),
            None => Locale::detached(self),
        }
    }
}

impl IntoLocale for String {
    fn into_locale(self) -> Locale {
        self.as_str().into_locale()
    }
}

impl IntoLocale for &String {
    fn into_locale(self) -> Locale {
        self.as_str().into_locale()
    }
}

/// A validated BCP 47 tag from input (a request body's `locale` field) is
/// matched with the supported locales like a tag.
impl IntoLocale for navree_validation::Locale {
    fn into_locale(self) -> Locale {
        self.as_str().into_locale()
    }
}

impl IntoLocale for &navree_validation::Locale {
    fn into_locale(self) -> Locale {
        self.as_str().into_locale()
    }
}

/// A number [`Locale::format_number`] formats: any integer or float type.
pub trait IntoNumber {
    /// The value as an `f64`.
    fn into_f64(self) -> f64;
}

macro_rules! into_number {
    ($($ty:ty),*) => {$(
        impl IntoNumber for $ty {
            #[allow(clippy::cast_precision_loss, clippy::cast_lossless)]
            fn into_f64(self) -> f64 {
                self as f64
            }
        }
    )*};
}

into_number!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64);

#[cfg(test)]
mod tests {
    #[test]
    fn handlers_on_the_locale_are_native() {
        async fn handler(_: super::Locale) {}
        let _ = navree_http::handler::Endpoint::new(handler);
    }

    use chrono::NaiveDate;

    use super::*;

    #[tokio::test]
    async fn detached_locales_format_but_do_not_translate() {
        let locale = Locale::detached("de");
        assert_eq!(locale.tag(), "de");
        assert_eq!(locale.language(), "de");
        assert_eq!(locale.format_number(1234.5), "1.234,5");
        assert_eq!(locale.format_number(3_u8), "3");
        assert_eq!(locale.format_money(1999, "EUR"), "19,99\u{a0}€");
        let day = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
        assert_eq!(locale.format_date(day), "26.10.2026");
        assert_eq!(
            locale.format_date_with(day, DateStyle::Long),
            "26. Oktober 2026"
        );
        assert_eq!(locale.translate("hello", &Args::new()).unwrap(), "hello");
        assert!(!locale.has("hello"));
        assert_eq!(Locale::detached("not a tag").tag(), "en");
        assert_eq!(Locale::detached("ar").direction(), "rtl");
        assert_eq!(Locale::detached("fr").native_name(), "Français");
    }
}
