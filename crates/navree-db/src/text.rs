//! Case-insensitive text matching that also folds non-ASCII letters:
//! [`contains_ignore_case`] and [`eq_ignore_case`], with the same results on
//! SQLite, PostgreSQL and MySQL: case is ignored, accents are not (`équipe`
//! finds `Équipe` but not `Equipe`).
//!
//! The usual `LOWER(column) LIKE '%term%'` falls short on two databases:
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
//!
//! On PostgreSQL, whose `LOWER` follows Unicode, they keep `LOWER(..)
//! LIKE`. The term is lower-cased letter by letter, as the databases'
//! `LOWER` does (`ΟΔΟΣ` → `οδοσ`, where `str::to_lowercase` would end the
//! word with `ς`).

use sea_orm::DbBackend;
use sea_orm::sea_query::{BinOper, Expr, ExprTrait, Func, LikeExpr};

/// `expr` contains `term`, ignoring case (simple Unicode case mapping).
/// `%`, `_`, `*`, `?`, `[` and `\` in `term` match themselves. Like any
/// "contains" search, it reads every row: no index helps.
///
/// ```
/// use navree_db::contains_ignore_case;
/// use sea_orm::DbBackend;
/// use sea_orm::sea_query::{Alias, Expr, MysqlQueryBuilder, Query};
///
/// let sql = Query::select()
///     .column(Alias::new("name"))
///     .from(Alias::new("companies"))
///     .and_where(contains_ignore_case(DbBackend::MySql, Expr::col(Alias::new("name")), "Équipe"))
///     .to_string(MysqlQueryBuilder);
/// assert!(sql.ends_with("WHERE INSTR(CAST(LOWER(`name`) AS BINARY), CAST('équipe' AS BINARY)) > 0"));
/// ```
///
/// With an entity, pass the backend of the connection the query runs on:
///
/// ```ignore
/// let backend = uow.db().backend();
/// select = select.filter(contains_ignore_case(backend, Expr::col(company::Column::Name), "équipe"));
/// ```
pub fn contains_ignore_case(backend: DbBackend, expr: impl Into<Expr>, term: &str) -> Expr {
    match backend {
        DbBackend::Sqlite => glob(expr, format!("*{}*", glob_pattern(term))),
        DbBackend::MySql => Expr::expr(
            Func::cust("INSTR")
                .arg(bytes(lower(expr)))
                .arg(bytes(Expr::val(fold_case(term)))),
        )
        .gt(0),
        _ => lower(expr).like(like_contains(term)),
    }
}

/// `expr` equals `value`, ignoring case (simple Unicode case mapping).
///
/// ```
/// use navree_db::eq_ignore_case;
/// use sea_orm::DbBackend;
/// use sea_orm::sea_query::{Alias, Expr, PostgresQueryBuilder, Query};
///
/// let sql = Query::select()
///     .column(Alias::new("id"))
///     .from(Alias::new("users"))
///     .and_where(eq_ignore_case(DbBackend::Postgres, Expr::col(Alias::new("email")), "Ann@Example.com"))
///     .to_string(PostgresQueryBuilder);
/// assert!(sql.ends_with(r#"WHERE LOWER("email") = 'ann@example.com'"#));
/// ```
pub fn eq_ignore_case(backend: DbBackend, expr: impl Into<Expr>, value: &str) -> Expr {
    match backend {
        DbBackend::Sqlite => glob(expr, glob_pattern(value)),
        DbBackend::MySql => bytes(lower(expr)).eq(bytes(Expr::val(fold_case(value)))),
        _ => lower(expr).eq(fold_case(value)),
    }
}

/// `text` lower-cased letter by letter, as the databases' `LOWER` does: no
/// context rules (a word-final `Σ` becomes `σ`, not `ς`).
fn fold_case(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

fn lower(expr: impl Into<Expr>) -> Expr {
    Expr::expr(Func::lower(expr.into()))
}

/// MySQL's `CAST(expr AS BINARY)`: compared byte for byte, whatever the
/// collation.
fn bytes(expr: impl Into<Expr>) -> Expr {
    Expr::expr(Func::cast_as(expr.into(), "BINARY"))
}

fn glob(expr: impl Into<Expr>, pattern: String) -> Expr {
    expr.into()
        .binary(BinOper::Custom("GLOB"), Expr::val(pattern))
}

/// `%term%`, lower-cased, with `%`, `_` and `\` escaped (escape `\`).
fn like_contains(term: &str) -> LikeExpr {
    let mut pattern = String::with_capacity(term.len() + 2);
    pattern.push('%');
    for c in fold_case(term).chars() {
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
    use sea_orm::sea_query::{
        Alias, MysqlQueryBuilder, PostgresQueryBuilder, Query, SqliteQueryBuilder,
    };

    use super::*;

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
        let sql = |condition: Expr| {
            Query::select()
                .column(Alias::new("name"))
                .from(Alias::new("t"))
                .and_where(condition)
                .to_string(MysqlQueryBuilder)
        };
        let col = || Expr::col(Alias::new("name"));
        let contains = sql(contains_ignore_case(DbBackend::MySql, col(), "Équipe 50%_*"));
        assert!(
            contains.contains(
                "INSTR(CAST(LOWER(`name`) AS BINARY), CAST('équipe 50%_*' AS BINARY)) > 0"
            ),
            "{contains}"
        );
        let equals = sql(eq_ignore_case(DbBackend::MySql, col(), "ΟΔΟΣ"));
        assert!(
            equals.contains("CAST(LOWER(`name`) AS BINARY) = CAST('οδοσ' AS BINARY)"),
            "{equals}"
        );
    }

    #[test]
    fn postgres_folds_the_term_letter_by_letter() {
        let sql = Query::select()
            .column(Alias::new("name"))
            .from(Alias::new("t"))
            .and_where(eq_ignore_case(
                DbBackend::Postgres,
                Expr::col(Alias::new("name")),
                "ΟΔΟΣ",
            ))
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"LOWER("name") = 'οδοσ'"#), "{sql}");
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
