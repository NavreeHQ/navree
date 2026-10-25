//! Case-insensitive text matching that also folds non-ASCII letters:
//! [`contains_ignore_case`] and [`eq_ignore_case`].
//!
//! The usual `LOWER(column) LIKE '%term%'` misses non-ASCII text on SQLite,
//! whose `LOWER`, `UPPER`, `LIKE` and `NOCASE` fold ASCII letters only:
//! `LOWER('Équipe')` stays `'Équipe'`, so neither `équipe` nor `Équipe`
//! (lower-cased in Rust to `équipe`) finds it. On SQLite these helpers use
//! a case-sensitive `GLOB` whose pattern lists every case variant of each
//! letter (`*[éÉ][qQ][uU][iI][pP][eE]*`); on PostgreSQL, whose `LOWER`
//! follows Unicode, they keep `LOWER(..) LIKE`.

use sea_orm::DbBackend;
use sea_orm::sea_query::{BinOper, Expr, ExprTrait, Func, LikeExpr};

/// `expr` contains `term`, ignoring case (simple Unicode case mapping).
/// `%`, `_`, `*`, `?`, `[` and `\` in `term` match themselves.
///
/// ```ignore
/// let backend = uow.db().backend();
/// select = select.filter(contains_ignore_case(backend, Expr::col(company::Column::Name), "équipe"));
/// ```
pub fn contains_ignore_case(backend: DbBackend, expr: impl Into<Expr>, term: &str) -> Expr {
    match backend {
        DbBackend::Sqlite => glob(expr, format!("*{}*", glob_pattern(term))),
        _ => lower(expr).like(like_contains(term)),
    }
}

/// `expr` equals `value`, ignoring case (simple Unicode case mapping).
pub fn eq_ignore_case(backend: DbBackend, expr: impl Into<Expr>, value: &str) -> Expr {
    match backend {
        DbBackend::Sqlite => glob(expr, glob_pattern(value)),
        _ => lower(expr).eq(value.to_lowercase()),
    }
}

fn lower(expr: impl Into<Expr>) -> Expr {
    Expr::expr(Func::lower(expr.into()))
}

fn glob(expr: impl Into<Expr>, pattern: String) -> Expr {
    expr.into()
        .binary(BinOper::Custom("GLOB"), Expr::val(pattern))
}

/// `%term%`, lower-cased, with `%`, `_` and `\` escaped (escape `\`).
fn like_contains(term: &str) -> LikeExpr {
    let mut pattern = String::with_capacity(term.len() + 2);
    pattern.push('%');
    for c in term.to_lowercase().chars() {
        if matches!(c, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    LikeExpr::new(pattern).escape('\\')
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
    use sea_orm::sea_query::{Alias, PostgresQueryBuilder, Query, SqliteQueryBuilder};

    use super::*;

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
        let sql = |backend: DbBackend, condition: Expr| {
            let mut query = Query::select();
            query
                .column(Alias::new("name"))
                .from(Alias::new("t"))
                .and_where(condition);
            match backend {
                DbBackend::Sqlite => query.to_string(SqliteQueryBuilder),
                _ => query.to_string(PostgresQueryBuilder),
            }
        };
        let col = || Expr::col(Alias::new("name"));
        let sqlite = sql(
            DbBackend::Sqlite,
            contains_ignore_case(DbBackend::Sqlite, col(), "Øl"),
        );
        assert!(sqlite.contains(r#""name" GLOB '*[Øø][lL]*'"#), "{sqlite}");
        let sqlite = sql(
            DbBackend::Sqlite,
            eq_ignore_case(DbBackend::Sqlite, col(), "Énergie"),
        );
        assert!(
            sqlite.contains(r#""name" GLOB '[Éé][nN][eE][rR][gG][iI][eE]'"#),
            "{sqlite}"
        );
        let postgres = sql(
            DbBackend::Postgres,
            contains_ignore_case(DbBackend::Postgres, col(), "50%_Off"),
        );
        assert!(
            postgres.contains(r#"LOWER("name") LIKE E'%50\\%\\_off%' ESCAPE E'\\'"#)
                || postgres.contains(r#"LOWER("name") LIKE '%50\%\_off%' ESCAPE '\'"#),
            "{postgres}"
        );
        let postgres = sql(
            DbBackend::Postgres,
            eq_ignore_case(DbBackend::Postgres, col(), "Énergie"),
        );
        assert!(
            postgres.contains(r#"LOWER("name") = 'énergie'"#),
            "{postgres}"
        );
    }
}
