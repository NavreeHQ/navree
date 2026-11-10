//! The glob matcher against a plain reference matcher, on every small
//! pattern and input and on random ones, plus the cases the callers rely
//! on and a pathological pattern.

use std::time::{Duration, Instant};

use navree_glob::Glob;
use proptest::prelude::*;

/// The textbook matcher: a table of which pattern prefixes match which
/// input prefixes. Slow, and obviously right.
fn reference(pattern: &str, input: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let input: Vec<char> = input.chars().collect();
    let mut row = vec![false; input.len() + 1];
    row[0] = true;
    for &p in &pattern {
        let mut next = vec![false; input.len() + 1];
        if p == '*' {
            let mut any = false;
            for j in 0..=input.len() {
                any |= row[j];
                next[j] = any;
            }
        } else {
            for j in 1..=input.len() {
                next[j] = row[j - 1] && (p == '?' || p == input[j - 1]);
            }
        }
        row = next;
    }
    row[input.len()]
}

/// Calls `f` with every string of `0..=max_len` characters over `alphabet`.
fn for_all_strings(alphabet: &[char], max_len: usize, f: &mut impl FnMut(&str)) {
    fn walk(alphabet: &[char], left: usize, current: &mut String, f: &mut impl FnMut(&str)) {
        f(current);
        if left == 0 {
            return;
        }
        for &c in alphabet {
            current.push(c);
            walk(alphabet, left - 1, current, f);
            current.pop();
        }
    }
    walk(alphabet, max_len, &mut String::new(), f);
}

#[test]
fn agrees_with_the_reference_on_every_small_pattern_and_input() {
    let mut inputs = Vec::new();
    for_all_strings(&['a', 'b'], 6, &mut |s| inputs.push(s.to_owned()));
    for_all_strings(&['a', 'b', '*', '?'], 5, &mut |pattern| {
        let glob = Glob::new(pattern);
        for input in &inputs {
            assert_eq!(
                glob.matches(input),
                reference(pattern, input),
                "pattern {pattern:?} input {input:?}"
            );
        }
    });
}

#[test]
fn agrees_with_the_reference_on_multibyte_characters() {
    let mut inputs = Vec::new();
    for_all_strings(&['a', '\u{e9}', '\u{4e2d}', '\u{1f600}'], 4, &mut |s| {
        inputs.push(s.to_owned())
    });
    for_all_strings(&['\u{e9}', '\u{1f600}', 'a', '*', '?'], 4, &mut |pattern| {
        let glob = Glob::new(pattern);
        for input in &inputs {
            assert_eq!(
                glob.matches(input),
                reference(pattern, input),
                "pattern {pattern:?} input {input:?}"
            );
        }
    });
}

proptest! {
    #[test]
    fn agrees_with_the_reference_on_random_patterns(
        pattern in "[ab*?/.]{0,24}",
        input in "[ab/.]{0,40}",
    ) {
        prop_assert_eq!(Glob::new(&pattern).matches(&input), reference(&pattern, &input));
    }

    #[test]
    fn a_pattern_built_from_its_input_matches_it(
        input in "[a-c/.\u{e9}]{0,30}",
        holes in proptest::collection::vec((0usize..30, any::<bool>()), 0..8),
    ) {
        // Replace some characters with `?` or `*`: the pattern still matches
        // the input it came from.
        let mut pattern: Vec<String> = input.chars().map(String::from).collect();
        for (at, star) in holes {
            if let Some(slot) = pattern.get_mut(at) {
                *slot = if star { "*".to_owned() } else { "?".to_owned() };
            }
        }
        let pattern = pattern.concat();
        prop_assert!(Glob::new(&pattern).matches(&input), "{:?} vs {:?}", pattern, input);
    }

    #[test]
    fn matching_never_panics(pattern in ".{0,40}", input in ".{0,60}") {
        let _ = Glob::new(&pattern).matches(&input);
    }
}

#[test]
fn star_crosses_separators_and_matches_nothing() {
    assert!(Glob::new("*").matches(""));
    assert!(Glob::new("*").matches("/a/b.c"));
    assert!(Glob::new("/webhooks/*").matches("/webhooks/"));
    assert!(Glob::new("/webhooks/*").matches("/webhooks/a/b/c"));
    assert!(!Glob::new("/webhooks/*").matches("/webhooks"));
    assert!(Glob::new("*.internal.example").matches("a.b.internal.example"));
    assert!(!Glob::new("*.internal.example").matches("internal.example"));
    assert!(Glob::new("a*b*c").matches("abc"));
    assert!(Glob::new("a*b*c").matches("a--b--c"));
    assert!(!Glob::new("a*b*c").matches("a--c--b"));
    assert!(Glob::new("***").matches("x"));
}

#[test]
fn question_mark_is_one_character() {
    assert!(Glob::new("?").matches("x"));
    assert!(!Glob::new("?").matches(""));
    assert!(!Glob::new("?").matches("xy"));
    assert!(Glob::new("?").matches("\u{e9}"));
    assert!(Glob::new("?").matches("\u{1f600}"));
    assert!(!Glob::new("??").matches("\u{1f600}"));
    assert!(Glob::new("db-?.internal").matches("db-1.internal"));
    assert!(!Glob::new("db-?.internal").matches("db-12.internal"));
    assert!(Glob::new("?*").matches("a"));
    assert!(!Glob::new("?*").matches(""));
}

#[test]
fn everything_else_is_literal_and_case_matters() {
    assert!(Glob::new("[ab]").matches("[ab]"));
    assert!(!Glob::new("[ab]").matches("a"));
    assert!(Glob::new("{a,b}").matches("{a,b}"));
    assert!(Glob::new("a.b").matches("a.b"));
    assert!(!Glob::new("a.b").matches("axb"));
    assert!(Glob::new("\\*").matches("\\anything"));
    assert!(!Glob::new("\\*").matches("*"));
    assert!(!Glob::new("Deals").matches("deals"));
    assert!(!Glob::new("K").matches("\u{212a}"));
}

#[test]
fn the_empty_pattern_matches_only_the_empty_string() {
    assert!(Glob::new("").matches(""));
    assert!(!Glob::new("").matches("a"));
    assert!(Glob::default().matches(""));
}

#[test]
fn the_pattern_is_shown_with_runs_of_star_folded() {
    assert_eq!(Glob::new("/a/*/c?").to_string(), "/a/*/c?");
    assert_eq!(Glob::new("/a/**/c").pattern(), "/a/*/c");
    assert_eq!(Glob::new("a***b").pattern(), "a*b");
}

#[test]
fn a_pathological_pattern_takes_polynomial_time() {
    // `*a*a*a*a*a*a*a*a*a*a*b` against a long run of `a`: backtracking
    // matchers try every way to split the input between the stars.
    let pattern = format!("{}b", "*a".repeat(10));
    let glob = Glob::new(&pattern);
    let input = "a".repeat(100_000);
    let started = Instant::now();
    assert!(!glob.matches(&input));
    assert!(glob.matches(&format!("{input}b")));
    let spent = started.elapsed();
    assert!(
        spent < Duration::from_secs(10),
        "matching took {spent:?}, it should be about a million steps"
    );

    // The same with `?` mixed in.
    let glob = Glob::new(&format!("{}x", "*?a".repeat(10)));
    let started = Instant::now();
    assert!(!glob.matches(&input));
    assert!(started.elapsed() < Duration::from_secs(10));
}
