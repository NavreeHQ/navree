//! Case-insensitive text matching that also folds non-ASCII letters:
//! [`contains_ignore_case`] and [`eq_ignore_case`], with the same results on
//! SQLite, PostgreSQL and MySQL: case is ignored, accents are not (`équipe`
//! finds `Équipe` but not `Equipe`). Each returns a condition as a portable
//! [`Statement`] fragment (SQL with `?` placeholders and their values) for
//! statements built by hand; navree-orm's columns have the same matching
//! (`Deal::TITLE.contains_ignore_case(..)`).
//!
//! The usual `LOWER(column) LIKE '%term%'` falls short on every database:
//!
//! - SQLite's `LOWER`, `UPPER`, `LIKE` and `NOCASE` fold ASCII letters
//!   only: `LOWER('Équipe')` stays `'Équipe'`, so neither `équipe` nor
//!   `Équipe` (lower-cased in Rust to `équipe`) finds it. On SQLite these
//!   helpers use a case-sensitive `GLOB` whose pattern lists every case
//!   variant of each letter (`*[éÉ][qQ][uU][iI][pP][eE]*`).
//! - MySQL's comparisons follow the column's collation: the default
//!   `utf8mb4_0900_ai_ci` also ignores accents (`'equipe' = 'Équipe'`),
//!   and a `_bin` or `_cs` collation does not ignore case. On MySQL and
//!   MariaDB these helpers compare the lower-cased column byte for byte
//!   (`INSTR(CAST(LOWER(name) AS BINARY), CAST(? AS BINARY)) > 0`), which
//!   ignores case under every collation and never ignores accents. The
//!   column must be `utf8mb4`, the connection's character set.
//! - PostgreSQL's `LOWER` and `ILIKE` follow the database's character
//!   type: a database created with the `C` locale (`initdb --locale=C`, a
//!   common production setting) folds ASCII letters only, like SQLite. A
//!   text whose cased letters are all ASCII is compared with `LOWER(..)`,
//!   which gives the same result under every locale (and can use an index
//!   on `LOWER(column)`); a text with other cased letters becomes a
//!   case-sensitive regular expression whose bracket expressions list
//!   every case variant of each letter (`"name" ~ '[Éé][qQ][uU]..'`).
//!
//! The term is lower-cased letter by letter, as the databases' `LOWER`
//! does (`ΟΔΟΣ` → `οδοσ`, where `str::to_lowercase` would end the word
//! with `ς`).

use crate::{Backend, Statement};

/// `expr` contains `term`, ignoring case (simple Unicode case mapping), as
/// a portable condition: `expr` is SQL (a quoted column, an expression;
/// a `?` in it is a placeholder), the term is bound. Every character of
/// `term` matches itself: `%`, `_`, `*`, `?`, `[`, `\` and the
/// regular-expression characters are not wildcards. Like any "contains"
/// search, it reads every row: no index helps.
///
/// ```
/// use navree_db::{Backend, Value, contains_ignore_case};
///
/// let backend = Backend::MySql;
/// let condition = contains_ignore_case(backend, &backend.quote_identifier("name"), "Équipe");
/// assert_eq!(condition.sql(), "INSTR(CAST(LOWER(`name`) AS BINARY), CAST(? AS BINARY)) > 0");
/// assert_eq!(condition.values(), [Value::from("équipe")]);
/// ```
///
/// In a statement of your own, append the fragment's SQL and its values:
///
/// ```
/// use navree_db::{Backend, Statement, contains_ignore_case};
///
/// let condition = contains_ignore_case(Backend::Sqlite, "\"name\"", "Øl");
/// let select = Statement::portable(
///     format!("SELECT id FROM companies WHERE {}", condition.sql()),
///     condition.values().to_vec(),
/// );
/// assert_eq!(select.sql(), r#"SELECT id FROM companies WHERE "name" GLOB ?"#);
/// ```
pub fn contains_ignore_case(backend: Backend, expr: &str, term: &str) -> Statement {
    match backend {
        Backend::Sqlite => Statement::portable(
            format!("{expr} GLOB ?"),
            [format!("*{}*", glob_pattern(term)).into()],
        ),
        Backend::MySql => Statement::portable(
            format!("INSTR(CAST(LOWER({expr}) AS BINARY), CAST(? AS BINARY)) > 0"),
            [fold_case(term).into()],
        ),
        Backend::Postgres if folds_beyond_ascii(term) => {
            Statement::portable(format!("{expr} ~ ?"), [regex_pattern(term).into()])
        }
        Backend::Postgres => Statement::portable(
            format!("LOWER({expr}) LIKE ? ESCAPE '\\'"),
            [like_contains(term).into()],
        ),
    }
}

/// `expr` equals `value`, ignoring case (simple Unicode case mapping), as
/// a portable condition (see [`contains_ignore_case`]).
///
/// ```
/// use navree_db::{Backend, Value, eq_ignore_case};
///
/// let condition = eq_ignore_case(Backend::Postgres, "\"email\"", "Ann@Example.com");
/// assert_eq!(condition.sql(), r#"LOWER("email") = ?"#);
/// assert_eq!(condition.values(), [Value::from("ann@example.com")]);
/// ```
pub fn eq_ignore_case(backend: Backend, expr: &str, value: &str) -> Statement {
    match backend {
        Backend::Sqlite => {
            Statement::portable(format!("{expr} GLOB ?"), [glob_pattern(value).into()])
        }
        Backend::MySql => Statement::portable(
            format!("CAST(LOWER({expr}) AS BINARY) = CAST(? AS BINARY)"),
            [fold_case(value).into()],
        ),
        Backend::Postgres if folds_beyond_ascii(value) => Statement::portable(
            format!("{expr} ~ ?"),
            [format!("^{}$", regex_pattern(value)).into()],
        ),
        Backend::Postgres => {
            Statement::portable(format!("LOWER({expr}) = ?"), [fold_case(value).into()])
        }
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
    use crate::Value;

    fn text(statement: &Statement) -> (&str, Vec<Value>) {
        (statement.sql(), statement.values().to_vec())
    }

    #[test]
    fn case_folds_letter_by_letter() {
        assert_eq!(fold_case("Équipe STRAßE"), "équipe straße");
        // `str::to_lowercase` would end the word with `ς`; databases fold
        // each letter on its own, to `σ`.
        assert_eq!(fold_case("ΟΔΟΣ"), "οδοσ");
        assert_eq!(fold_case("50%_Off"), "50%_off");
    }

    #[test]
    fn mysql_compares_lower_cased_bytes() {
        assert_eq!(
            text(&contains_ignore_case(
                Backend::MySql,
                "`name`",
                "Équipe 50%_*"
            )),
            (
                "INSTR(CAST(LOWER(`name`) AS BINARY), CAST(? AS BINARY)) > 0",
                vec![Value::from("équipe 50%_*")]
            )
        );
        assert_eq!(
            text(&eq_ignore_case(Backend::MySql, "`name`", "ΟΔΟΣ")),
            (
                "CAST(LOWER(`name`) AS BINARY) = CAST(? AS BINARY)",
                vec![Value::from("οδοσ")]
            )
        );
    }

    #[test]
    fn postgres_matches_letters_beyond_ascii_with_a_regular_expression() {
        assert_eq!(
            text(&eq_ignore_case(Backend::Postgres, "\"name\"", "ΟΔΟΣ")),
            (r#""name" ~ ?"#, vec![Value::from("^[Οο][Δδ][Οο][Σσ]$")])
        );
        assert_eq!(
            text(&contains_ignore_case(
                Backend::Postgres,
                "\"name\"",
                "Øl (v2.0)"
            )),
            (r#""name" ~ ?"#, vec![Value::from(r"[Øø][lL] \([vV]2\.0\)")])
        );
    }

    #[test]
    fn regular_expressions_escape_punctuation() {
        assert_eq!(regex_pattern("Ö.b*c"), r"[Öö]\.[bB]\*[cC]");
        assert_eq!(regex_pattern("ß 50%_"), r"ß 50\%\_");
        assert_eq!(regex_pattern("a|b^c$"), r"[aA]\|[bB]\^[cC]\$");
        // Only letters outside ASCII need the expression on PostgreSQL.
        assert!(folds_beyond_ascii("Équipe"));
        assert!(!folds_beyond_ascii("Equipe 50% ß"));
    }

    #[test]
    fn letters_become_classes_of_their_case_variants() {
        assert_eq!(glob_pattern("Équipe"), "[Éé][qQ][uU][iI][pP][eE]");
        assert_eq!(glob_pattern("øresund"), "[øØ][rR][eE][sS][uU][nN][dD]");
        assert_eq!(glob_pattern("ς"), "[ςΣσ]");
        // No single-character upper case: left as it is.
        assert_eq!(glob_pattern("ß"), "ß");
        assert_eq!(glob_pattern("42 - "), "42 - ");
    }

    #[test]
    fn glob_wildcards_match_themselves() {
        assert_eq!(glob_pattern("a*b?c[d]"), "[aA][*][bB][?][cC][[][dD]]");
        assert_eq!(glob_pattern("100%_\\"), "100%_\\");
    }

    #[test]
    fn sqlite_uses_glob_and_postgres_lower_like() {
        assert_eq!(
            text(&contains_ignore_case(Backend::Sqlite, "\"name\"", "Øl")),
            (r#""name" GLOB ?"#, vec![Value::from("*[Øø][lL]*")])
        );
        assert_eq!(
            text(&eq_ignore_case(Backend::Sqlite, "\"name\"", "Énergie")),
            (
                r#""name" GLOB ?"#,
                vec![Value::from("[Éé][nN][eE][rR][gG][iI][eE]")]
            )
        );
        assert_eq!(
            text(&contains_ignore_case(
                Backend::Postgres,
                "\"name\"",
                "50%_Off"
            )),
            (
                r#"LOWER("name") LIKE ? ESCAPE '\'"#,
                vec![Value::from(r"%50\%\_off%")]
            )
        );
        assert_eq!(
            text(&eq_ignore_case(
                Backend::Postgres,
                "\"name\"",
                "Ann@Example.com"
            )),
            (r#"LOWER("name") = ?"#, vec![Value::from("ann@example.com")])
        );
    }
}
