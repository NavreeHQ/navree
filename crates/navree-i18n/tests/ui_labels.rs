//! Navree UI in the reader's language: the label structs from Fluent, money by locale, the
//! toolkit's date names against ICU, and the form's own words.
#![cfg(feature = "ui")]

use chrono::NaiveDate;
use navree_i18n::ui::{FromLocale, LocaleUi, MoneyIn};
use navree_i18n::{DateStyle, I18n, I18nOptions, Locale};
use navree_ui::charts::ChartLabels;
use navree_ui::components::{CarouselLabels, DisplayLabels, LazyLabels};
use navree_ui::flow::{SliderLabels, StepperLabels, ToolbarLabels, TreeLabels, WizardLabels};
use navree_ui::form::FORM_MESSAGES;
use navree_ui::pages::{StateKind, StateLabels};
use navree_ui::{MarkdownLabels, Money, SelectLabels, SortLabels, TableLabels, UploadLabels};

/// Every language the toolkit has date names for, English India, and a language with
/// no shipped words.
const TAGS: &[&str] = &[
    "en", "en-IN", "de", "fr", "es", "it", "pt", "nl", "sv", "pl", "ru", "tr", "ja", "zh", "ko",
    "ar", "he", "hi", "id", "ta",
];

fn i18n() -> I18n {
    I18n::new(I18nOptions {
        locales: TAGS.iter().map(|tag| (*tag).to_owned()).collect(),
        ..I18nOptions::default()
    })
    .expect("the locales load")
}

/// The blank names (`{n}`, `{label}`) of a Debug print, sorted and without repeats.
fn blanks_of(debug: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = debug;
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
    found.sort();
    found.dedup();
    found
}

/// Runs `$check` on each label struct with its name.
macro_rules! each_struct {
    ($check:ident) => {
        $check::<TableLabels>("TableLabels");
        $check::<ChartLabels>("ChartLabels");
        $check::<CarouselLabels>("CarouselLabels");
        $check::<LazyLabels>("LazyLabels");
        $check::<DisplayLabels>("DisplayLabels");
        $check::<SliderLabels>("SliderLabels");
        $check::<StepperLabels>("StepperLabels");
        $check::<ToolbarLabels>("ToolbarLabels");
        $check::<TreeLabels>("TreeLabels");
        $check::<WizardLabels>("WizardLabels");
        $check::<MarkdownLabels>("MarkdownLabels");
        $check::<SelectLabels>("SelectLabels");
        $check::<SortLabels>("SortLabels");
        $check::<UploadLabels>("UploadLabels");
        $check::<StateLabels>("StateLabels");
    };
}

fn english_is_the_toolkits_default<T: FromLocale + Default + std::fmt::Debug>(name: &str) {
    let i18n = i18n();
    assert_eq!(
        format!("{:?}", T::from_locale(&i18n.locale("en"))),
        format!("{:?}", T::default()),
        "{name}: the shipped English must be the toolkit's own words"
    );
}

// The English file is the toolkit's default, word for word, for every label struct.
#[test]
fn english_is_the_toolkits_default_word_for_word() {
    each_struct!(english_is_the_toolkits_default);
}

fn hindi_has_its_own_words_and_the_same_blanks<T: FromLocale + Default + std::fmt::Debug>(
    name: &str,
) {
    let i18n = i18n();
    let hindi = format!("{:?}", T::from_locale(&i18n.locale("hi")));
    let english = format!("{:?}", T::default());
    assert_ne!(hindi, english, "{name}: nothing was translated");
    assert_eq!(
        blanks_of(&hindi),
        blanks_of(&english),
        "{name}: a translation lost or invented a blank"
    );
    assert!(
        !hindi.contains('$'),
        "{name}: a message came out raw: {hindi}"
    );
}

#[test]
fn hindi_translates_every_label_struct_and_keeps_the_blanks() {
    each_struct!(hindi_has_its_own_words_and_the_same_blanks);
    let hindi = i18n().locale("hi");
    let table = TableLabels::from_locale(&hindi);
    assert_eq!(table.search, "खोजें");
    assert_eq!(table.showing, "{total} में से {from} से {to} परिणाम दिख रहे हैं");
    let states = StateLabels::from_locale(&hindi);
    assert_eq!(states.words(StateKind::NotFound).short, "पृष्ठ नहीं मिला");
    assert_eq!(
        states.words(StateKind::Forbidden).toast,
        "आपके पास ऐसा करने की अनुमति नहीं है।"
    );
    assert_eq!(states.home, "मुख्य पृष्ठ पर जाएँ");
    // The space before a link's note is the English word's, kept.
    assert_eq!(
        MarkdownLabels::from_locale(&hindi).new_tab,
        " (नए टैब में खुलता है)"
    );
}

// A locale with no shipped words has the toolkit's English, never an id.
#[test]
fn a_locale_without_shipped_words_keeps_english() {
    let i18n = i18n();
    for tag in ["fr", "ta", "ja"] {
        let locale = i18n.locale(tag);
        assert_eq!(
            format!("{:?}", TableLabels::from_locale(&locale)),
            format!("{:?}", TableLabels::default()),
            "{tag}"
        );
        assert_eq!(
            format!("{:?}", StateLabels::from_locale(&locale)),
            format!("{:?}", StateLabels::default()),
            "{tag}"
        );
    }
}

// `locale.labels::<T>()` is `T::from_locale(&locale)`.
#[test]
fn labels_by_the_locale_are_the_same_words() {
    let hindi = i18n().locale("hi");
    assert_eq!(
        hindi.labels::<UploadLabels>(),
        UploadLabels::from_locale(&hindi)
    );
    assert_eq!(hindi.labels::<UploadLabels>().send, "भेजें");
}

// Money prints in the locale's grouping and symbol: lakhs in India.
#[test]
fn money_is_grouped_as_the_locale_groups_it() {
    let i18n = i18n();
    let amount = Money(10_000_000);
    assert_eq!(
        amount.display_in(&i18n.locale("en-IN"), "INR"),
        "₹1,00,000.00"
    );
    assert_eq!(amount.display_in(&i18n.locale("hi"), "INR"), "₹1,00,000.00");
    assert_eq!(amount.display_in(&i18n.locale("en"), "INR"), "₹100,000.00");
    assert_eq!(
        amount.display_in(&i18n.locale("de"), "EUR"),
        "100.000,00\u{a0}€"
    );
    assert_eq!(
        Money(-350).display_in(&i18n.locale("en-IN"), "INR"),
        "-₹3.50"
    );
}

// The toolkit keeps its own month names (a page needs no ICU); they must agree with ICU's
// for every language it has. A date written out in full holds the month's name as it reads
// inside a date. The languages that write months as numbers (Japanese, Chinese, Korean)
// have no name to compare.
#[test]
fn the_toolkits_month_names_are_icus() {
    let i18n = i18n();
    let mut disagreements = Vec::new();
    for tag in TAGS {
        if matches!(*tag, "ja" | "zh" | "ko") {
            continue;
        }
        let locale = i18n.locale(tag);
        // The tag the locale answers to, so a language the app does not list is not silently
        // compared in English.
        if locale.tag() != *tag {
            continue;
        }
        let dates = locale.date_locale();
        for month in 1..=12u32 {
            let day = NaiveDate::from_ymd_opt(2026, month, 15).unwrap();
            let written = locale.format_date_with(day, DateStyle::Long).to_lowercase();
            let name = dates.month_in_date(month).to_lowercase();
            if !written.contains(&name) {
                disagreements.push(format!(
                    "{tag} month {month}: `{name}` is not in `{written}`"
                ));
            }
        }
    }
    assert!(
        disagreements.is_empty(),
        "the toolkit and ICU name months differently:\n{}",
        disagreements.join("\n")
    );
}

// The locale gives the date picker its names, and English India its week and clock.
#[test]
fn the_date_locale_follows_the_tag() {
    let i18n = i18n();
    assert_eq!(i18n.locale("hi").date_locale().month_name(10), "अक्तूबर");
    let india = i18n.locale("en-IN").date_locale();
    assert_eq!(india.first_day, chrono::Weekday::Sun);
    assert_eq!(india.month_name(3), "March");
}

// What a form says on its own, in the locale.
#[test]
fn a_form_says_its_words_in_the_locale() {
    let i18n = i18n();
    let hindi = i18n.locale("hi").reader_words();
    assert_eq!(hindi.check_form, "इस फ़ॉर्म को जाँचें");
    assert_eq!(hindi.save, "सहेजें");
    assert_eq!(hindi.slider.too_high, "{max} या उससे कम चुनें।");
    assert_eq!(hindi.dates.tag, "hi");
    let english = i18n.locale("en").reader_words();
    assert_eq!(english.check_form, "Check this form");
    assert_eq!(english.dates.tag, "en");
    // A language with no words of its own keeps the toolkit's, with its own dates.
    let french = i18n.locale("fr").reader_words();
    assert_eq!(french.check_form, "Check this form");
    assert_eq!(french.dates.month_name(2), "février");
}

// The form's own checks ship in English and Hindi under the keys the translator reads.
#[test]
fn the_forms_own_checks_are_shipped() {
    let hindi = i18n().locale("hi");
    for (key, _) in FORM_MESSAGES {
        let id = format!("validation-{}", key.replace('.', "-"));
        assert!(hindi.has(&id), "{id} is not shipped in Hindi");
    }
}
