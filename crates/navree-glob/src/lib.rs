//! # navree-glob
//!
//! The glob matcher behind the patterns apps and Navree configure as plain
//! strings: the URLs a browser test waits for, the hosts an outgoing client
//! may reach inside the private ranges, the stubs of the fake HTTP server,
//! the routes the Logbook never records and the paths the CSRF check skips.
//!
//! A pattern is a string with two wildcards and nothing else:
//!
//! - `*` matches any run of characters, including none, and crosses `/` and
//!   `.`;
//! - `?` matches exactly one character.
//!
//! Every other character matches itself, so `[`, `{`, `\` and `.` are
//! literal and there is no escape. A pattern has to match the whole input,
//! matching is case-sensitive (lower-case both sides to ignore case) and
//! characters are Unicode scalar values, so `?` matches `é` as one character.
//!
//! ```
//! use navree_glob::Glob;
//!
//! assert!(Glob::new("/webhooks/*").matches("/webhooks/stripe/events"));
//! assert!(Glob::new("db-?.internal").matches("db-1.internal"));
//! assert!(!Glob::new("db-?.internal").matches("db-12.internal"));
//! assert!(!Glob::new("*.internal").matches("internal"));
//! ```
//!
//! ## Cost
//!
//! Matching remembers one place to resume from, the last `*` it passed, and
//! never tries two ways through the same pattern. It takes at most the
//! pattern's length times the input's length steps, however many `*` the
//! pattern has: a pattern like `*a*a*a*a*b` against a long run of `a` is
//! quadratic at worst, never exponential. Matching allocates nothing.
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

use std::fmt;

/// The multi-character wildcard.
const ANY: char = '*';
/// The single-character wildcard.
const ONE: char = '?';

/// A glob pattern, compiled once and matched against many inputs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Glob {
    /// The pattern's characters, with each run of `*` folded into one.
    pattern: Box<[char]>,
}

impl Glob {
    /// Compiles `pattern`. Any string is a pattern.
    pub fn new(pattern: &str) -> Self {
        let mut chars: Vec<char> = Vec::with_capacity(pattern.len());
        for c in pattern.chars() {
            if c == ANY && chars.last() == Some(&ANY) {
                continue;
            }
            chars.push(c);
        }
        Self {
            pattern: chars.into_boxed_slice(),
        }
    }

    /// Whether the pattern matches all of `input`.
    pub fn matches(&self, input: &str) -> bool {
        let pattern = &*self.pattern;
        // Where in the pattern and the input we are; the input position is a
        // byte offset that is always on a character boundary.
        let mut at_pattern = 0;
        let mut at_input = 0;
        // Where to resume after a mismatch: the pattern after the last `*`
        // passed, and the input position that `*` has consumed up to.
        let mut resume: Option<(usize, usize)> = None;
        loop {
            let next = input.get(at_input..).and_then(|rest| rest.chars().next());
            match (pattern.get(at_pattern), next) {
                (Some(&ANY), _) => {
                    at_pattern += 1;
                    resume = Some((at_pattern, at_input));
                    continue;
                }
                (None, None) => return true,
                (Some(&expected), Some(c)) if expected == ONE || expected == c => {
                    at_pattern += 1;
                    at_input += c.len_utf8();
                    continue;
                }
                _ => {}
            }
            // A mismatch: let the last `*` take one more character and try
            // the rest of the pattern from there, or give up without one.
            let Some((after_star, consumed)) = resume else {
                return false;
            };
            let Some(skipped) = input.get(consumed..).and_then(|rest| rest.chars().next()) else {
                return false;
            };
            at_input = consumed + skipped.len_utf8();
            at_pattern = after_star;
            resume = Some((after_star, at_input));
        }
    }

    /// The pattern as written, with each run of `*` folded into one.
    pub fn pattern(&self) -> String {
        self.pattern.iter().collect()
    }
}

impl fmt::Display for Glob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use fmt::Write;
        self.pattern.iter().try_for_each(|c| f.write_char(*c))
    }
}
