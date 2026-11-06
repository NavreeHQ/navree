//! Navree UI in the reader's language (the `ui` feature): the words of every label struct
//! of the toolkit, built from the locale's Fluent messages; money in the locale's
//! grouping and symbol; dates named by the locale; and the [`Localizer`] that lets a form
//! say its date, time and slider messages in the request's language.
//!
//! The words ship with this crate (`resources/<tag>/ui.ftl`: English and Hindi), so a page
//! needs no Fluent file of its own to be translated. A word an app defines under the same
//! id in its own `locales/<tag>/*.ftl` wins over the shipped one. A locale with no shipped
//! words gets the toolkit's English, word by word, never an id.
//!
//! ```
//! use navree_i18n::ui::FromLocale;
//! use navree_i18n::{I18n, I18nOptions};
//! use navree_ui::TableLabels;
//!
//! let i18n = I18n::new(I18nOptions {
//!     locales: vec!["en".into(), "hi".into()],
//!     ..I18nOptions::default()
//! })
//! .unwrap();
//! let labels = TableLabels::from_locale(&i18n.locale("hi"));
//! assert_eq!(labels.search, "खोजें");
//! assert_eq!(TableLabels::from_locale(&i18n.locale("en")).search, "Search");
//! ```

use std::borrow::Cow;

#[cfg(feature = "phone")]
use navree_ui::PhoneLabels;
use navree_ui::charts::{ChartLabels, Grouping};
use navree_ui::components::{CarouselLabels, DisplayLabels, LazyLabels};
use navree_ui::dates::DateLocale;
use navree_ui::flow::{SliderLabels, StepperLabels, ToolbarLabels, TreeLabels, WizardLabels};
use navree_ui::localize::{Localizer, ReaderWords};
use navree_ui::pages::{StateKind, StateLabels};
use navree_ui::{MarkdownLabels, Money, SelectLabels, SortLabels, TableLabels, UploadLabels};

use crate::{Args, Locale};

/// The blanks the toolkit's templates have (`{n}`, `{label}`, ...). A message gets each as
/// its own text, so the template comes out with its blanks and the toolkit fills them in
/// when it draws the page.
const BLANKS: &[&str] = &[
    "count",
    "country",
    "date",
    "first",
    "first_name",
    "first_x",
    "from",
    "gap",
    "item",
    "label",
    "last",
    "last_name",
    "last_x",
    "limit",
    "list",
    "marks",
    "max",
    "max_x",
    "min",
    "min_x",
    "n",
    "name",
    "position",
    "q",
    "rate",
    "series",
    "share",
    "shown",
    "size",
    "step",
    "time",
    "title",
    "to",
    "total",
    "types",
    "value",
    "what",
    "x",
    "zone",
];

/// A label struct of the toolkit, in a locale.
pub trait FromLocale: Sized {
    /// The words in `locale`. A word the locale has no message for stays English.
    fn from_locale(locale: &Locale) -> Self;
}

/// A text a label struct keeps.
trait Slot {
    fn current(&self) -> &str;
    fn put(&mut self, text: String);
}

impl Slot for String {
    fn current(&self) -> &str {
        self
    }

    fn put(&mut self, text: String) {
        *self = text;
    }
}

impl Slot for Cow<'static, str> {
    fn current(&self) -> &str {
        self
    }

    fn put(&mut self, text: String) {
        *self = Cow::Owned(text);
    }
}

/// The arguments that make a message come out as a template: every blank is its own
/// `{name}`.
fn blanks() -> Args {
    let mut args = Args::new();
    for name in BLANKS {
        args.set(name, format!("{{{name}}}"));
    }
    args
}

/// Puts the message `id` in `slot`, if the locale has it.
fn say(locale: &Locale, blanks: &Args, id: &str, slot: &mut impl Slot) {
    if !locale.has(id) {
        return;
    }
    let Ok(text) = locale.translate(id, blanks) else {
        return;
    };
    let text = clean(slot.current(), text);
    slot.put(text);
}

/// A message as a template: no bidi isolates around the blanks (a right-to-left locale's
/// bundle adds them; the toolkit fills the blanks itself), and the spaces at the ends of
/// the English word kept, which Fluent trims (` (opens in a new tab)`).
fn clean(english: &str, mut text: String) -> String {
    if text.contains('\u{2068}') {
        for name in BLANKS {
            text = text.replace(
                &format!("\u{2068}{{{name}}}\u{2069}"),
                &format!("{{{name}}}"),
            );
        }
    }
    let lead = &english[..english.len() - english.trim_start().len()];
    let tail = &english[english.trim_end().len()..];
    format!("{lead}{text}{tail}")
}

/// Fills the fields of a label struct from the messages `ui-<group>-<id>`.
macro_rules! say_all {
    ($locale:ident, $blanks:ident, $labels:ident, $group:literal { $($field:ident = $id:literal),* $(,)? }) => {
        $( say($locale, &$blanks, concat!("ui-", $group, "-", $id), &mut $labels.$field); )*
    };
}

/// How `locale` groups the digits of a number: by thousands (`1,234,567`) or in lakhs and
/// crores (`12,34,567`). Read from the locale's own number formatting, so it follows its data.
fn grouping_of(locale: &Locale) -> Grouping {
    let sample = locale.format_number(1_000_000.0);
    let sizes: Vec<usize> = sample
        .split(|c: char| !c.is_ascii_digit())
        .filter(|group| !group.is_empty())
        .map(str::len)
        .collect();
    // `10,00,000` has a group of two after the first; `1,000,000` has groups of three.
    if sizes.len() == 3 && sizes[1] == 2 {
        Grouping::Indian
    } else {
        Grouping::Thousands
    }
}

/// A table groups its counts as the locale groups numbers.
fn table_conventions(labels: &mut TableLabels, locale: &Locale) {
    labels.grouping = grouping_of(locale);
}

macro_rules! labels {
    ($($ty:ty, $group:literal { $($field:ident = $id:literal),* $(,)? } $(=> $conventions:ident)?)*) => {$(
        impl FromLocale for $ty {
            fn from_locale(locale: &Locale) -> Self {
                let blanks = blanks();
                let mut labels = <$ty>::default();
                say_all!(locale, blanks, labels, $group { $($field = $id),* });
                $($conventions(&mut labels, locale);)?
                labels
            }
        }
    )*};
}

labels! {
    TableLabels, "table" {
        search = "search",
        filters = "filters",
        live = "live",
        refresh = "refresh",
        columns = "columns",
        view = "view",
        views = "views",
        export = "export",
        apply = "apply",
        clear_filters = "clear-filters",
        remove_filter = "remove-filter",
        remove_sort = "remove-sort",
        sorted_by = "sorted-by",
        rows_per_page = "rows-per-page",
        showing = "showing",
        showing_open = "showing-open",
        one_result = "one-result",
        count_all = "count-all",
        first = "first",
        previous = "previous",
        next = "next",
        empty = "empty",
        empty_filtered = "empty-filtered",
        empty_filtered_hint = "empty-filtered-hint",
        failed = "failed",
        retry = "retry",
        lost_access = "lost-access",
        loading = "loading",
        select_row = "select-row",
        select_page = "select-page",
        selected = "selected",
        page_selected = "page-selected",
        select_all = "select-all",
        all_selected = "all-selected",
        some_selected = "some-selected",
        off_page = "off-page",
        clear_selection = "clear-selection",
        selection_ended = "selection-ended",
        confirm_title = "confirm-title",
        confirm = "confirm",
        cancel = "cancel",
        sort_asc = "sort-asc",
        sort_desc = "sort-desc",
        sort_add = "sort-add",
        sort_clear = "sort-clear",
        hide_column = "hide-column",
        column_options = "column-options",
        move_up = "move-up",
        move_down = "move-down",
        resize = "resize",
        reset_columns = "reset-columns",
        empty_value = "empty-value",
        any = "any",
        yes = "yes",
        no = "no",
        min = "min",
        max = "max",
        from = "from",
        to = "to",
        find_option = "find-option",
        show_details = "show-details",
        hide_details = "hide-details",
        actions = "actions",
        row_actions = "row-actions",
        density = "density",
        compact = "compact",
        default_density = "default-density",
        spacious = "spacious",
        save_view = "save-view",
        view_name = "view-name",
        delete_view = "delete-view",
        make_default = "make-default",
        default_view = "default-view",
    } => table_conventions
    ChartLabels, "chart" {
        show_table = "show-table",
        chart = "chart",
        keys = "keys",
        loading = "loading",
        failed = "failed",
        retry = "retry",
        empty = "empty",
        empty_text = "empty-text",
        other = "other",
        total = "total",
        share = "share",
        category = "category",
        time = "time",
        value = "value",
        no_value = "no-value",
        of_first = "of-first",
        of_previous = "of-previous",
        reduced = "reduced",
        summary_series = "summary-series",
        summary_one = "summary-one",
        summary_bars = "summary-bars",
        summary_parts = "summary-parts",
        summary_funnel = "summary-funnel",
        summary_none = "summary-none",
        summary_marks = "summary-marks",
        one_point = "one-point",
        week_of = "week-of",
        again = "again",
        stages_cut = "stages-cut",
    }
    CarouselLabels, "carousel" {
        previous = "previous",
        next = "next",
        pause = "pause",
        play = "play",
        slides = "slides",
        slide_of = "slide-of",
        picker = "picker",
        pickers = "pickers",
    }
    LazyLabels, "lazy" {
        start = "start",
        end = "end",
        loaded = "loaded",
        loading = "loading",
        failed = "failed",
    }
    DisplayLabels, "display" {
        hidden_one = "hidden-one",
        hidden_many = "hidden-many",
        show_hidden = "show-hidden",
        hide_hidden = "hide-hidden",
        dismiss = "dismiss",
        connecting = "connecting",
        live = "live",
        reconnecting = "reconnecting",
        paused = "paused",
        closed = "closed",
        times_in = "times-in",
        timeline_empty = "timeline-empty",
        feed_empty = "feed-empty",
        failed = "failed",
        retry = "retry",
        apply = "apply",
        loading = "loading",
        upcoming = "upcoming",
        gap_under_a_second = "gap-under-a-second",
        gap_after = "gap-after",
        second = "second",
        minute = "minute",
        hour = "hour",
        day = "day",
        outcome_yes = "outcome-yes",
        outcome_no = "outcome-no",
        new_one = "new-one",
        new_many = "new-many",
    }
    SliderLabels, "slider" {
        minimum = "minimum",
        maximum = "maximum",
        range_to = "range-to",
        missing = "missing",
        duplicate = "duplicate",
        not_a_number = "not-a-number",
        too_low = "too-low",
        too_high = "too-high",
        off_step = "off-step",
        low_above_high = "low-above-high",
        too_close = "too-close",
        decimal_point = "decimal-point",
    }
    StepperLabels, "stepper" {
        done = "done",
        current = "current",
        error = "error",
        disabled = "disabled",
        optional = "optional",
        step_of = "step-of",
    }
    ToolbarLabels, "toolbar" {
        more = "more",
    }
    TreeLabels, "tree" {
        expand = "expand",
        collapse = "collapse",
        more = "more",
        loading = "loading",
        failed = "failed",
        retry = "retry",
        loaded = "loaded",
    }
    WizardLabels, "wizard" {
        next = "next",
        back = "back",
        finish = "finish",
        save = "save",
        edit = "edit",
        review = "review",
        review_hint = "review-hint",
        needs_attention = "needs-attention",
        nothing = "nothing",
        yes = "yes",
        no = "no",
        stale = "stale",
        stepper = "stepper",
    }
    MarkdownLabels, "markdown" {
        new_tab = "new-tab",
        image_removed = "image-removed",
        table = "table",
        task_done = "task-done",
        task_open = "task-open",
        copy_code = "copy-code",
        cut = "cut",
    }
    SelectLabels, "select" {
        empty = "empty",
        none = "none",
        searching = "searching",
        failed = "failed",
        retry = "retry",
        lost = "lost",
        clear = "clear",
        remove = "remove",
        focused = "focused",
        added = "added",
        removed = "removed",
        cleared = "cleared",
        count = "count",
        result = "result",
        results = "results",
        create = "create",
        creating = "creating",
        create_failed = "create-failed",
        limit = "limit",
        more = "more",
        shown = "shown",
        min = "min",
        select_all = "select-all",
        deselect_all = "deselect-all",
        toggle = "toggle",
        recent = "recent",
        list = "list",
        required = "required",
        palette = "palette",
    }
    SortLabels, "sort" {
        help = "help",
        handle = "handle",
        picked = "picked",
        moved = "moved",
        dropped = "dropped",
        cancelled = "cancelled",
        returned = "returned",
        settled = "settled",
        gone = "gone",
        failed = "failed",
        offline = "offline",
        full = "full",
        locked = "locked",
        count = "count",
        count_limit = "count-limit",
        count_limit_text = "count-limit-text",
        empty = "empty",
        move_summary = "move-summary",
        move_to = "move-to",
        move_position = "move-position",
        move_submit = "move-submit",
        full_option = "full-option",
        more = "more",
    }
    UploadLabels, "upload" {
        choose = "choose",
        choose_one = "choose-one",
        drop = "drop",
        drop_one = "drop-one",
        send = "send",
        remove = "remove",
        cancel = "cancel",
        retry = "retry",
        files = "files",
        sending = "sending",
        done = "done",
        failed = "failed",
        offline = "offline",
        too_big = "too-big",
        wrong_type = "wrong-type",
        empty = "empty",
        too_many = "too-many",
        removed = "removed",
        expired = "expired",
        sent_one = "sent-one",
        sent_many = "sent-many",
        nothing = "nothing",
        up_to = "up-to",
        up_to_files = "up-to-files",
        or = "or",
        images = "images",
        videos = "videos",
        audio = "audio",
        text = "text",
    }
}

#[cfg(feature = "phone")]
labels! {
    PhoneLabels, "phone" {
        country = "country",
        choose = "choose",
        preferred = "preferred",
        search = "search",
        empty = "empty",
        country_set = "country-set",
        count = "count",
    }
}

impl FromLocale for StateLabels {
    fn from_locale(locale: &Locale) -> Self {
        let blanks = blanks();
        let mut labels = StateLabels::default();
        for kind in StateKind::ALL {
            let mut words = labels.words(kind).clone();
            let name = kind.name();
            let id = |part: &str| format!("ui-state-kind-{name}-{part}");
            say(locale, &blanks, &id("short"), &mut words.short);
            say(locale, &blanks, &id("title"), &mut words.title);
            say(locale, &blanks, &id("description"), &mut words.description);
            say(locale, &blanks, &id("toast"), &mut words.toast);
            labels = labels.with_words(kind, words);
        }
        say_all!(locale, blanks, labels, "state" {
            home = "home",
            sign_in = "sign-in",
            try_again = "try-again",
            reload = "reload",
            request_access = "request-access",
            reference = "reference",
            copy = "copy",
            wait = "wait",
            wait_one = "wait-one",
            debug = "debug",
            forbidden_what = "forbidden-what",
            not_found_what = "not-found-what",
            signed_out = "signed-out",
            signed_out_help = "signed-out-help",
            sign_in_what = "sign-in-what",
            row_locked = "row-locked",
            row_missing = "row-missing",
            nothing_yet = "nothing-yet",
            nothing_yet_what = "nothing-yet-what",
            no_results = "no-results",
            no_results_help = "no-results-help",
            clear_filters = "clear-filters",
            caught_up = "caught-up",
            caught_up_help = "caught-up-help",
        });
        labels
    }
}

/// The toolkit's conventions and words, as a [`Locale`] has them.
pub trait LocaleUi {
    /// Names of months and weekdays, the order of a typed date, the first day of the week,
    /// the clock and the words on the date and time pickers for this locale (a locale the
    /// toolkit has no table for is English). `DateLocale::of(tag)`.
    fn date_locale(&self) -> DateLocale;

    /// The words of the label struct `T` in this locale.
    fn labels<T: FromLocale>(&self) -> T;

    /// What a form says on its own in this locale: the date, time and slider words, the
    /// heading of a refused form and the submit button's text.
    fn reader_words(&self) -> ReaderWords;
}

impl LocaleUi for Locale {
    fn date_locale(&self) -> DateLocale {
        DateLocale::of(self.tag())
    }

    fn labels<T: FromLocale>(&self) -> T {
        T::from_locale(self)
    }

    fn reader_words(&self) -> ReaderWords {
        let blanks = blanks();
        let mut words = ReaderWords::english();
        words.dates = self.date_locale();
        words.slider = SliderLabels::from_locale(self);
        say(self, &blanks, "ui-form-check", &mut words.check_form);
        say(self, &blanks, "ui-form-save", &mut words.save);
        words
    }
}

/// An amount of money in a locale.
pub trait MoneyIn {
    /// The amount in `currency` (an ISO 4217 code) with the locale's grouping, separators
    /// and symbol. A `Money` is in the currency's minor units: `Money(10_000_000)` of `INR`
    /// is `₹1,00,000.00` in `en-IN` and `₹100,000.00` in `en`.
    fn display_in(&self, locale: &Locale, currency: &str) -> String;
}

impl MoneyIn for Money {
    fn display_in(&self, locale: &Locale, currency: &str) -> String {
        locale.format_money(self.0, currency)
    }
}

/// What `I18nModule` registers (with the `ui` feature) so Navree UI's forms say their
/// date, time and slider messages in the language of the request being served.
pub(crate) struct UiLocalizer;

impl Localizer for UiLocalizer {
    fn words(&self) -> ReaderWords {
        Locale::current().reader_words()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The blank names in `text`: every `{name}` of word characters.
    fn blank_names(text: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('}') else {
                break;
            };
            let name = &after[..close];
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                found.push(name.to_owned());
                rest = &after[close + 1..];
            } else {
                rest = after;
            }
        }
        found
    }

    #[test]
    fn every_blank_the_toolkit_writes_is_one_the_messages_know() {
        let defaults = [
            format!("{:?}", TableLabels::default()),
            format!("{:?}", ChartLabels::default()),
            format!("{:?}", CarouselLabels::default()),
            format!("{:?}", LazyLabels::default()),
            format!("{:?}", DisplayLabels::default()),
            format!("{:?}", SliderLabels::default()),
            format!("{:?}", StepperLabels::default()),
            format!("{:?}", ToolbarLabels::default()),
            format!("{:?}", TreeLabels::default()),
            format!("{:?}", WizardLabels::default()),
            format!("{:?}", MarkdownLabels::default()),
            format!("{:?}", SelectLabels::default()),
            format!("{:?}", SortLabels::default()),
            format!("{:?}", UploadLabels::default()),
            format!("{:?}", StateLabels::default()),
        ];
        for debug in defaults {
            for name in blank_names(&debug) {
                assert!(
                    BLANKS.contains(&name.as_str()),
                    "`{{{name}}}` is not a blank in BLANKS"
                );
            }
        }
    }

    #[test]
    fn isolates_around_a_blank_are_dropped_and_the_ends_of_the_english_word_are_kept() {
        let isolated = "\u{2068}{n}\u{2069} of \u{2068}{total}\u{2069}".to_owned();
        assert_eq!(clean("{n} of {total}", isolated), "{n} of {total}");
        assert_eq!(
            clean(" (opens in a new tab)", "(नए टैब में खुलता है)".to_owned()),
            " (नए टैब में खुलता है)"
        );
        assert_eq!(clean("Save", "सहेजें".to_owned()), "सहेजें");
    }

    #[tokio::test]
    async fn the_localizer_reads_the_locale_the_request_is_in() {
        let i18n = crate::I18n::new(crate::I18nOptions {
            locales: vec!["en".into(), "hi".into()],
            ..crate::I18nOptions::default()
        })
        .unwrap();
        let hindi = Locale::scope(i18n.locale("hi"), async { UiLocalizer.words() }).await;
        assert_eq!(hindi.dates.tag, "hi");
        assert_eq!(hindi.check_form, "इस फ़ॉर्म को जाँचें");
        let english = Locale::scope(i18n.locale("en"), async { UiLocalizer.words() }).await;
        assert_eq!(english.dates.tag, "en");
        assert_eq!(english.check_form, "Check this form");
    }
}
