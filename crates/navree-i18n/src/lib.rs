//! # navree-i18n
//!
//! `I18nModule`: one app answering in each user's language.
//!
//! - **Fluent files.** Messages live in `locales/<locale>/*.ftl`
//!   ([Fluent](https://projectfluent.org) syntax: variables, plurals with
//!   each language's rules, selectors, terms, attributes, `NUMBER()` and
//!   `DATETIME()`), read at boot from `i18n.dir` or embedded with
//!   [`embed_locales!`]. A message missing from `fr-CA` falls back to
//!   `fr`, then to `i18n.default_locale`. A file with a syntax error, or a
//!   message defined twice, fails the boot naming the file and line.
//! - **Translating.** [`t!`] translates in the current locale
//!   ([`Locale::current`]); [`I18n::t`] in an explicit one, with
//!   [`args!`].
//! - **The request's locale** is negotiated from the sources of
//!   `i18n.negotiation`, first match wins: the `query` parameter, the
//!   signed-in `user`'s saved locale, the `workspace`'s saved locale, a
//!   `cookie` and the `Accept-Language` `header`. Handlers take
//!   [`Locale`]; responses carry `Content-Language`.
//! - **Formatting.** [`Locale::format_number`], [`Locale::format_money`]
//!   and [`Locale::format_date`] follow the locale's conventions (ICU4X).
//! - **Everything else follows.** Validation messages (navree-validation)
//!   come from Fluent, with shipped translations in seven languages;
//!   templates get `t()` and locale-aware `money` and `date` filters
//!   (navree-view); notifications render in the recipient's saved locale
//!   (navree-notify).
//! - **Tooling.** `i18n:check`, `i18n:missing <locale>` and
//!   `i18n:publish validation`.
//!
//! ```
//! use navree_i18n::{I18n, I18nOptions, Locale, t};
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() {
//! let i18n = I18n::new(I18nOptions {
//!     locales: vec!["en".into(), "fr".into()],
//!     dir: "tests/fixtures/locales".into(),
//!     ..I18nOptions::default()
//! })
//! .unwrap();
//! let text = Locale::scope(i18n.locale("fr"), async { t!("deal-created", count = 3) }).await;
//! assert_eq!(text, "3 affaires créées");
//! # }
//! ```
#![forbid(unsafe_code)]

pub mod migration;

mod args;
mod catalog;
mod commands;
mod controller;
mod error;
mod format;
mod locale;
mod module;
mod negotiate;
mod notify;
mod options;
mod service;
mod shipped;
mod tag;
mod validation;
mod views;

pub use args::{Arg, Args, Money};
pub use controller::{I18nController, LocaleInput, LocaleListResource, LocaleResource};
pub use error::I18nError;
pub use format::DateStyle;
pub use locale::{IntoLocale, IntoNumber, Locale};
pub use migration::I18N_MIGRATIONS;
pub use module::I18nModule;
/// Embeds a directory of Fluent files in the binary (`embed_locales!("locales")`),
/// for [`I18nOptions::files`]. The path is relative to the crate's
/// `Cargo.toml`; each subdirectory is a locale. Every file is parsed when
/// the crate builds, so a syntax error is a compile error naming the file
/// and line.
pub use navree_i18n_macros::embed_locales;
pub use options::{BidiIsolation, I18nOptions, LocaleFiles, Missing, Source};
pub use service::I18n;

/// Message arguments: `args!(count = 3, owner = "Ada")` is an [`Args`];
/// `args!()` is empty. Values are anything [`Arg`] converts from.
///
/// ```
/// use navree_i18n::args;
///
/// let args = args!(count = 2, title = "Lovelace renewal");
/// assert_eq!(args.len(), 2);
/// assert!(args!().is_empty());
/// ```
#[macro_export]
macro_rules! args {
    () => {
        $crate::Args::new()
    };
    ($($name:ident = $value:expr),+ $(,)?) => {{
        let mut args = $crate::Args::new();
        $( args.set(::core::stringify!($name), $value); )+
        args
    }};
}

/// Translates message `id` (or `id.attribute`) in the current locale
/// ([`Locale::current`]) with `name = value` arguments, and returns the
/// text. A message no locale defines gives its id (`i18n.missing` `warn`,
/// which also logs it once, or `id`); with `i18n.missing = "error"` it
/// panics, so tests catch it.
///
/// ```
/// use navree_i18n::{I18n, I18nOptions, Locale, t};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let i18n = I18n::new(I18nOptions {
///     locales: vec!["en".into(), "fr".into()],
///     dir: "tests/fixtures/locales".into(),
///     ..I18nOptions::default()
/// })
/// .unwrap();
/// Locale::scope(i18n.locale("fr"), async {
///     assert_eq!(t!("deal-created", count = 1), "Une affaire créée");
///     assert_eq!(t!("deal-won", owner = "Ada", title = "Lovelace"), "Ada a gagné Lovelace.");
///     assert_eq!(t!("pipeline-empty"), "Votre pipeline est vide. Créez une affaire pour commencer.");
/// })
/// .await;
/// # }
/// ```
#[macro_export]
macro_rules! t {
    ($id:expr $(,)?) => {
        $crate::__private::translate($id, $crate::Args::new())
    };
    ($id:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::__private::translate($id, $crate::args!($($name = $value),+))
    };
}

#[doc(hidden)]
pub mod __private {
    use crate::{Args, Locale};

    /// What `t!` expands to.
    pub fn translate(id: &str, args: Args) -> String {
        match Locale::current().translate(id, &args) {
            Ok(text) => text,
            Err(error) => panic!("t!: {error}"),
        }
    }
}
