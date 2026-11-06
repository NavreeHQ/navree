//! [`ReaderWords`] and [`Localizer`]: what Navree UI says in the reader's language on
//! its own, where no page code is there to choose the words.
//!
//! A page chooses its own labels (`TableLabels`, `StateLabels` and the rest are values it
//! passes in). A form that comes back with errors does not: [`Submit`](crate::Submit)
//! decodes the post before any page code runs, and the date, time and slider messages it
//! writes, and the heading of the page it falls back on, belong to the reader. An app
//! that has a [`Localizer`] gets those in the reader's language; without one they are
//! English. navree-i18n's `I18nModule` registers one when its `ui` feature is on, and
//! reads the request's locale.

use std::borrow::Cow;

use navree_kernel::AppContext;

use crate::dates::DateLocale;
use crate::flow::SliderLabels;

/// The words and conventions of the reader's language that a form needs when it is read
/// back or drawn: how dates and times are named and typed, what a slider says of a value it
/// refuses, the submit button's text and the heading of the page a refused form falls back on.
///
/// ```
/// use navree_ui::localize::ReaderWords;
///
/// let words = ReaderWords::english();
/// assert_eq!(words.check_form, "Check this form");
/// assert_eq!(words.dates.month_name(11), "November");
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ReaderWords {
    /// The names of months and weekdays, the order of a typed date and the words on the
    /// date and time pickers.
    pub dates: DateLocale,
    /// What a slider says of a value off its grid.
    pub slider: SliderLabels,
    /// The title and heading of the page a refused form falls back on.
    pub check_form: Cow<'static, str>,
    /// The text of a form's submit button when the page does not set its own.
    pub save: Cow<'static, str>,
}

impl ReaderWords {
    /// English, as every form says it without a [`Localizer`].
    pub fn english() -> Self {
        Self {
            dates: DateLocale::english(),
            slider: SliderLabels::default(),
            check_form: Cow::Borrowed("Check this form"),
            save: Cow::Borrowed("Save"),
        }
    }

    /// The words of the reader behind `ctx`: the app's [`Localizer`] says them in the
    /// request's language; an app without one, or no app, gives English.
    pub async fn of(ctx: Option<&AppContext>) -> Self {
        match ctx {
            Some(ctx) => match ctx.try_get::<dyn Localizer>().await {
                Some(localizer) => localizer.words(),
                None => Self::english(),
            },
            None => Self::english(),
        }
    }
}

impl Default for ReaderWords {
    fn default() -> Self {
        Self::english()
    }
}

/// Says [`ReaderWords`] in the language of the request being served. An app registers
/// one as `Arc<dyn Localizer>` (navree-i18n's `I18nModule` does, with its `ui` feature);
/// the form extractor asks it for each post.
pub trait Localizer: Send + Sync + 'static {
    /// The words of the language the running request is in.
    fn words(&self) -> ReaderWords;
}
