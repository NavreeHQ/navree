//! [`Backend`]: the database server a connection talks to.

use std::borrow::Cow;
use std::fmt;

/// The database server a pool, a transaction or a statement belongs to.
///
/// TimescaleDB is [`Backend::Postgres`] with the extension; MariaDB is
/// [`Backend::MySql`] (it speaks MySQL's protocol). Each backend's driver is
/// a feature of navree-db (`postgres`, `mysql`, `sqlite`);
/// [`is_compiled`](Backend::is_compiled) says whether it is built in.
///
/// ```
/// use navree_db::Backend;
///
/// assert_eq!(Backend::from_url("postgresql://db/app"), Some(Backend::Postgres));
/// assert_eq!(Backend::from_url("mariadb://db/app"), Some(Backend::MySql));
/// assert_eq!(Backend::Postgres.quote_identifier("audit.events"), r#""audit"."events""#);
/// assert_eq!(Backend::MySql.quote_identifier("odd`name"), "`odd``name`");
/// assert_eq!(Backend::Postgres.placeholder(2), "$2");
/// assert_eq!(Backend::Sqlite.placeholder(2), "?");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Backend {
    /// PostgreSQL, with or without TimescaleDB.
    Postgres,
    /// MySQL and MariaDB.
    MySql,
    /// SQLite.
    Sqlite,
}

impl Backend {
    /// Every backend, compiled in or not.
    pub const ALL: [Backend; 3] = [Backend::Postgres, Backend::MySql, Backend::Sqlite];

    /// The backend a connection URL's scheme selects: `postgres:` and
    /// `postgresql:`, `mysql:` and `mariadb:`, `sqlite:`.
    pub fn from_url(url: &str) -> Option<Backend> {
        let (scheme, _) = url.split_once(':')?;
        match scheme.to_ascii_lowercase().as_str() {
            "postgres" | "postgresql" => Some(Backend::Postgres),
            "mysql" | "mariadb" => Some(Backend::MySql),
            "sqlite" => Some(Backend::Sqlite),
            _ => None,
        }
    }

    /// The database's name in messages: `PostgreSQL`, `MySQL`, `SQLite`.
    pub fn name(self) -> &'static str {
        match self {
            Backend::Postgres => "PostgreSQL",
            Backend::MySql => "MySQL",
            Backend::Sqlite => "SQLite",
        }
    }

    /// The navree-db feature that builds this backend's driver.
    pub fn feature(self) -> &'static str {
        match self {
            Backend::Postgres => "postgres",
            Backend::MySql => "mysql",
            Backend::Sqlite => "sqlite",
        }
    }

    /// Whether this backend's driver is compiled in.
    pub fn is_compiled(self) -> bool {
        match self {
            Backend::Postgres => cfg!(feature = "postgres"),
            Backend::MySql => cfg!(feature = "mysql"),
            Backend::Sqlite => cfg!(feature = "sqlite"),
        }
    }

    /// The `n`-th (1-based) bind placeholder: `$n` on PostgreSQL, `?`
    /// elsewhere.
    pub fn placeholder(self, n: usize) -> Cow<'static, str> {
        match self {
            Backend::Postgres => Cow::Owned(format!("${n}")),
            Backend::MySql | Backend::Sqlite => Cow::Borrowed("?"),
        }
    }

    /// The character that quotes identifiers: `` ` `` on MySQL, `"`
    /// elsewhere.
    pub fn identifier_quote(self) -> char {
        match self {
            Backend::MySql => '`',
            Backend::Postgres | Backend::Sqlite => '"',
        }
    }

    /// `name` quoted as one identifier, embedded quote characters doubled.
    pub fn quote_part(self, name: &str) -> String {
        let quote = self.identifier_quote();
        let mut quoted = String::with_capacity(name.len() + 2);
        quoted.push(quote);
        for c in name.chars() {
            if c == quote {
                quoted.push(quote);
            }
            quoted.push(c);
        }
        quoted.push(quote);
        quoted
    }

    /// `name` quoted as an identifier, each part of a schema-qualified name
    /// (`audit.events`) on its own.
    pub fn quote_identifier(self, name: &str) -> String {
        name.split('.')
            .map(|part| self.quote_part(part))
            .collect::<Vec<_>>()
            .join(".")
    }

    /// Whether `INSERT`, `UPDATE` and `DELETE` can return rows
    /// (`RETURNING`): PostgreSQL and SQLite.
    pub fn supports_returning(self) -> bool {
        !matches!(self, Backend::MySql)
    }

    /// Whether rows can be locked (`SELECT .. FOR UPDATE`): PostgreSQL and
    /// MySQL. SQLite serialises writers instead (units of work begin
    /// `IMMEDIATE`).
    pub fn supports_row_locks(self) -> bool {
        !matches!(self, Backend::Sqlite)
    }

    /// The most values one statement can bind: 65,535 on PostgreSQL and
    /// MySQL, 32,766 on SQLite.
    pub fn max_bind_values(self) -> usize {
        match self {
            Backend::Postgres | Backend::MySql => 65_535,
            Backend::Sqlite => 32_766,
        }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemes_pick_the_backend() {
        assert_eq!(
            Backend::from_url("postgres://h/db"),
            Some(Backend::Postgres)
        );
        assert_eq!(
            Backend::from_url("POSTGRESQL://h/db"),
            Some(Backend::Postgres)
        );
        assert_eq!(Backend::from_url("mysql://h/db"), Some(Backend::MySql));
        assert_eq!(Backend::from_url("mariadb://h/db"), Some(Backend::MySql));
        assert_eq!(Backend::from_url("sqlite::memory:"), Some(Backend::Sqlite));
        assert_eq!(
            Backend::from_url("sqlite://a.db?mode=rwc"),
            Some(Backend::Sqlite)
        );
        assert_eq!(Backend::from_url("redis://h"), None);
        assert_eq!(Backend::from_url("no scheme"), None);
    }

    #[test]
    fn identifiers_are_quoted_per_backend() {
        assert_eq!(Backend::Postgres.quote_identifier("deals"), "\"deals\"");
        assert_eq!(Backend::Sqlite.quote_identifier("we\"ird"), "\"we\"\"ird\"");
        assert_eq!(Backend::MySql.quote_identifier("a.b"), "`a`.`b`");
        assert_eq!(Backend::MySql.quote_part("a.b"), "`a.b`");
    }

    #[test]
    fn capabilities() {
        assert!(Backend::Postgres.supports_returning());
        assert!(!Backend::MySql.supports_returning());
        assert!(Backend::Sqlite.supports_returning());
        assert!(!Backend::Sqlite.supports_row_locks());
        assert_eq!(Backend::Sqlite.max_bind_values(), 32_766);
        assert_eq!(Backend::MySql.to_string(), "MySQL");
        assert_eq!(Backend::Sqlite.is_compiled(), cfg!(feature = "sqlite"));
    }
}
