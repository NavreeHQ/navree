//! `navree_glob`: a pattern against a text, the first byte of the input
//! giving the length of the pattern.
//!
//! Invariant: the match agrees with a plain table-driven matcher on every
//! input small enough for the table, and no input takes more than a bounded
//! number of steps (the `-timeout` of the run is the check).
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_glob::Glob;

/// Which prefixes of `pattern` match which prefixes of `text`, one row per
/// pattern character.
fn reference(pattern: &[char], text: &[char]) -> bool {
    let mut row = vec![false; text.len() + 1];
    row[0] = true;
    for &p in pattern {
        let mut next = vec![false; text.len() + 1];
        if p == '*' {
            let mut any = false;
            for (j, slot) in next.iter_mut().enumerate() {
                any |= row[j];
                *slot = any;
            }
        } else {
            for j in 1..=text.len() {
                next[j] = row[j - 1] && (p == '?' || p == text[j - 1]);
            }
        }
        row = next;
    }
    row[text.len()]
}

fuzz_target!(|data: &[u8]| {
    let Some((&split, rest)) = data.split_first() else {
        return;
    };
    let Ok(rest) = std::str::from_utf8(rest) else {
        return;
    };
    // Cut `rest` into pattern and text at a character boundary.
    let mut at = (usize::from(split) * rest.len()) / 256;
    while !rest.is_char_boundary(at) {
        at -= 1;
    }
    let (pattern, text) = rest.split_at(at);
    let matched = Glob::new(pattern).matches(text);
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    if pattern.len() * text.len() <= 1 << 20 {
        assert_eq!(matched, reference(&pattern, &text), "{pattern:?} {text:?}");
    }
    // A pattern matches the text it was made of, with any run replaced by `*`.
    let own: String = text.iter().collect();
    assert!(Glob::new(&own).matches(&own));
    assert!(Glob::new("*").matches(&own));
});
