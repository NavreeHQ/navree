//! Text compared ignoring letter case beyond ASCII ([`Where::IgnoreCase`]),
//! with the same results on every database: case is ignored, accents are
//! not, the text is literal, NULLs never match. The rules are navree-db's
//! `contains_ignore_case` and `eq_ignore_case`:
//!
//! - SQLite's `LOWER` and `LIKE` fold ASCII only: a `GLOB` whose pattern
//!   lists every case variant of each letter (`*[éÉ][qQ]..*`).
//! - MySQL compares the lower-cased column byte for byte
//!   (`INSTR(CAST(LOWER(x) AS BINARY), CAST(? AS BINARY)) > 0`), whatever
//!   the column's collation.
//! - PostgreSQL folds ASCII only under the `C` locale: text whose cased
//!   letters are all ASCII is compared with `LOWER(..)`, other text with a
//!   case-sensitive regular expression of case variants.
//!
//! [`Where::IgnoreCase`]: crate::ir::Where::IgnoreCase

use std::borrow::Cow;

use super::Lowerer;
use crate::error::Result;
use crate::ir::{Expr, IgnoreCaseMode};
use crate::render::ast::{self, BinaryOp, LikeCase, SqlType};
use crate::sql::Dialect;
use crate::value::Value;

impl Lowerer {
    pub(crate) fn ignore_case(
        &self,
        expr: &Expr,
        text: &str,
        mode: IgnoreCaseMode,
    ) -> Result<ast::Expr> {
        let expr = self.expr(expr)?;
        let contains = matches!(mode, IgnoreCaseMode::Contains);
        Ok(match self.dialect {
            Dialect::Sqlite => {
                let pattern = if contains {
                    format!("*{}*", glob_pattern(text))
                } else {
                    glob_pattern(text)
                };
                expr.binary(
                    BinaryOp::Custom(Cow::Borrowed("GLOB")),
                    ast::Expr::Value(Value::from(pattern)),
                )
            }
            Dialect::MySql => {
                let bytes = |expr: ast::Expr| expr.cast(SqlType::Custom(Cow::Borrowed("BINARY")));
                let lowered = bytes(ast::Expr::func("LOWER", vec![expr]));
                let term = bytes(ast::Expr::Value(Value::from(fold_case(text))));
                if contains {
                    ast::Expr::func("INSTR", vec![lowered, term]).gt(ast::Expr::int(0))
                } else {
                    lowered.eq(term)
                }
            }
            Dialect::Postgres if folds_beyond_ascii(text) => {
                let pattern = if contains {
                    regex_pattern(text)
                } else {
                    format!("^{}$", regex_pattern(text))
                };
                expr.binary(
                    BinaryOp::Custom(Cow::Borrowed("~")),
                    ast::Expr::Value(Value::from(pattern)),
                )
            }
            Dialect::Postgres => {
                let lowered = ast::Expr::func("LOWER", vec![expr]);
                if contains {
                    ast::Expr::Like {
                        expr: Box::new(lowered),
                        pattern: Box::new(ast::Expr::Value(Value::from(like_contains(text)))),
                        escape: Some('\\'),
                        negated: false,
                        case: LikeCase::Database,
                    }
                } else {
                    lowered.eq(ast::Expr::Value(Value::from(fold_case(text))))
                }
            }
        })
    }
}

/// `text` lower-cased letter by letter, as the databases' `LOWER` does: no
/// context rules (a word-final `Σ` becomes `σ`, not `ς`).
fn fold_case(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

/// Whether `text` has a letter outside ASCII with case variants: a letter
/// PostgreSQL's `LOWER` folds under some locales only (not under `C`).
fn folds_beyond_ascii(text: &str) -> bool {
    text.chars()
        .any(|c| !c.is_ascii() && case_variants(c).len() > 1)
}

/// A PostgreSQL regular expression matching exactly `text` in any letter
/// case: a letter with case variants becomes a bracket expression of them
/// (`[éÉ]`), and ASCII punctuation is escaped (`\.`, `\%`), so every
/// character matches itself.
fn regex_pattern(text: &str) -> String {
    let mut pattern = String::with_capacity(text.len() * 4);
    for c in text.chars() {
        let variants = case_variants(c);
        if variants.len() > 1 {
            pattern.push('[');
            pattern.extend(variants);
            pattern.push(']');
        } else {
            if c.is_ascii_punctuation() {
                pattern.push('\\');
            }
            pattern.push(c);
        }
    }
    pattern
}

/// `%term%`, lower-cased, with `%`, `_` and `\` escaped (escape `\`).
fn like_contains(term: &str) -> String {
    let mut pattern = String::with_capacity(term.len() + 2);
    pattern.push('%');
    for c in fold_case(term).chars() {
        if matches!(c, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

/// A `GLOB` pattern matching exactly `text` in any letter case: a letter
/// with case variants becomes a character class of them (`[éÉ]`), and the
/// `GLOB` wildcards `*`, `?` and `[` become one-character classes.
fn glob_pattern(text: &str) -> String {
    let mut pattern = String::with_capacity(text.len() * 4);
    for c in text.chars() {
        let variants = case_variants(c);
        if variants.len() > 1 {
            pattern.push('[');
            pattern.extend(variants);
            pattern.push(']');
        } else if matches!(c, '*' | '?' | '[') {
            pattern.push('[');
            pattern.push(c);
            pattern.push(']');
        } else {
            pattern.push(c);
        }
    }
    pattern
}

/// `c` and its single-character lower- and upper-case forms (and theirs:
/// `ς` → `Σ` → `σ`), without duplicates, `c` first.
fn case_variants(c: char) -> Vec<char> {
    let mut variants = vec![c];
    let mut add = |candidate: char| {
        if !variants.contains(&candidate) {
            variants.push(candidate);
        }
    };
    for form in [single(c.to_lowercase()), single(c.to_uppercase())]
        .into_iter()
        .flatten()
    {
        add(form);
        for other in [single(form.to_lowercase()), single(form.to_uppercase())]
            .into_iter()
            .flatten()
        {
            add(other);
        }
    }
    variants
}

/// The only character of `chars`, if it has exactly one.
fn single(mut chars: impl Iterator<Item = char>) -> Option<char> {
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_folds_letter_by_letter() {
        assert_eq!(fold_case("Équipe STRAßE"), "équipe straße");
        assert_eq!(fold_case("ΟΔΟΣ"), "οδοσ");
    }

    #[test]
    fn patterns_list_case_variants() {
        assert_eq!(glob_pattern("Équipe"), "[Éé][qQ][uU][iI][pP][eE]");
        assert_eq!(glob_pattern("a*b?c[d]"), "[aA][*][bB][?][cC][[][dD]]");
        assert_eq!(glob_pattern("ς"), "[ςΣσ]");
        assert_eq!(regex_pattern("Ö.b*c"), r"[Öö]\.[bB]\*[cC]");
        assert_eq!(like_contains("50%_Off"), r"%50\%\_off%");
        assert!(folds_beyond_ascii("Équipe"));
        assert!(!folds_beyond_ascii("Equipe 50% ß"));
    }
}
