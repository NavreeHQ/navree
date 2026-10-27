//! Property tests for the filter language: printing and parsing round-trip,
//! and the parser never panics.

use navree_logbook::{Comparison, EntryKind, Filter, StatusMatch, Term};
use proptest::prelude::*;

fn value() -> impl Strategy<Value = String> {
    "[A-Za-z0-9_./{}:-]{1,16}"
}

fn term() -> impl Strategy<Value = Term> {
    prop_oneof![
        "[A-Za-z0-9_./-]{1,12}( [a-z]{1,6})?".prop_map(Term::Text),
        value().prop_map(Term::User),
        value().prop_map(Term::Tag),
        (100u16..=599).prop_map(|code| Term::Status(StatusMatch::Exact(code))),
        (1u8..=5).prop_map(|class| Term::Status(StatusMatch::Class(class))),
        value().prop_map(Term::Route),
        proptest::sample::select(EntryKind::ALL.to_vec()).prop_map(Term::Type),
        Just(Term::Slow),
        Just(Term::Failed),
        (0u32..100_000, any::<bool>()).prop_map(|(ms, longer)| {
            let comparison = if longer {
                Comparison::Longer
            } else {
                Comparison::Shorter
            };
            Term::Duration(comparison, f64::from(ms) / 4.0)
        }),
        value().prop_map(Term::Batch),
        value().prop_map(Term::Trace),
    ]
}

proptest! {
    #[test]
    fn printed_filters_parse_back(terms in proptest::collection::vec(term(), 0..6)) {
        let filter = Filter::of(terms);
        let printed = filter.to_string();
        let parsed = Filter::parse(&printed);
        prop_assert_eq!(parsed, Ok(filter), "{}", printed);
    }

    #[test]
    fn the_parser_never_panics(input in "\\PC{0,64}") {
        let _ = Filter::parse(&input);
    }

    #[test]
    fn parsing_is_stable(input in "[a-z0-9:<>\" ./{}-]{0,48}") {
        if let Ok(filter) = Filter::parse(&input) {
            let again = Filter::parse(&filter.to_string());
            prop_assert_eq!(again, Ok(filter));
        }
    }
}
