//! [`Dialect`], [`Version`] and [`Target`]: the database statements are
//! compiled for.

use std::fmt;
use std::str::FromStr;

use sea_orm::DbBackend;

/// A database family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dialect {
    /// PostgreSQL, with or without the TimescaleDB extension.
    Postgres,
    /// MySQL or MariaDB.
    MySql,
    /// SQLite.
    Sqlite,
}

impl Dialect {
    /// The dialect of a SeaORM backend (`None` for a backend this crate does
    /// not know).
    pub fn of(backend: DbBackend) -> Option<Dialect> {
        match backend {
            DbBackend::Postgres => Some(Dialect::Postgres),
            DbBackend::MySql => Some(Dialect::MySql),
            DbBackend::Sqlite => Some(Dialect::Sqlite),
            _ => None,
        }
    }

    /// The SeaORM backend of this dialect.
    pub fn backend(self) -> DbBackend {
        match self {
            Dialect::Postgres => DbBackend::Postgres,
            Dialect::MySql => DbBackend::MySql,
            Dialect::Sqlite => DbBackend::Sqlite,
        }
    }

    /// `postgres`, `mysql` or `sqlite`: the driver name used in file names
    /// such as `database/schema/postgres-schema.sql`.
    pub fn driver(self) -> &'static str {
        match self {
            Dialect::Postgres => "postgres",
            Dialect::MySql => "mysql",
            Dialect::Sqlite => "sqlite",
        }
    }
}

impl fmt::Display for Dialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Dialect::Postgres => "PostgreSQL",
            Dialect::MySql => "MySQL",
            Dialect::Sqlite => "SQLite",
        })
    }
}

/// A version number such as `17.2` or `3.46.1`, compared numerically.
///
/// ```
/// use navree_schema::Version;
///
/// let version = Version::parse("PostgreSQL 16.2 (Debian 16.2-1.pgdg120+2)").unwrap();
/// assert_eq!(version, Version::new(16, 2, 0));
/// assert!(version >= Version::from(14));
/// assert_eq!(Version::parse("11.4.2-MariaDB-ubu2404").unwrap().to_string(), "11.4.2");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// The major version.
    pub major: u32,
    /// The minor version.
    pub minor: u32,
    /// The patch version.
    pub patch: u32,
}

impl Version {
    /// `major.minor.patch`.
    pub const fn new(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    /// The first `a[.b[.c]]` number in `text` (version strings carry
    /// prefixes and suffixes: `PostgreSQL 16.2 on ...`, `8.0.36-0ubuntu`).
    pub fn parse(text: &str) -> Option<Version> {
        let start = text.find(|c: char| c.is_ascii_digit())?;
        let mut parts = [0u32; 3];
        let mut count = 0;
        for piece in text[start..].split('.') {
            let digits: String = piece.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() || count == 3 {
                break;
            }
            parts[count] = digits.parse().ok()?;
            count += 1;
            if digits.len() != piece.len() {
                break;
            }
        }
        (count > 0).then(|| Version::new(parts[0], parts[1], parts[2]))
    }
}

impl From<u32> for Version {
    fn from(major: u32) -> Version {
        Version::new(major, 0, 0)
    }
}

impl From<(u32, u32)> for Version {
    fn from((major, minor): (u32, u32)) -> Version {
        Version::new(major, minor, 0)
    }
}

impl From<(u32, u32, u32)> for Version {
    fn from((major, minor, patch): (u32, u32, u32)) -> Version {
        Version::new(major, minor, patch)
    }
}

impl FromStr for Version {
    type Err = String;

    fn from_str(text: &str) -> Result<Version, String> {
        Version::parse(text).ok_or_else(|| format!("`{text}` is not a version number"))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The database statements are compiled for: its dialect, its server
/// version, whether it is MariaDB, and the TimescaleDB extension's version
/// when it is installed.
///
/// A connected [`Schema`](crate::Schema) detects its target; an offline one
/// is given it:
///
/// ```
/// use navree_schema::{Dialect, Target, Version};
///
/// let target = Target::postgres(17).with_timescaledb((2, 17));
/// assert_eq!(target.dialect(), Dialect::Postgres);
/// assert_eq!(target.timescaledb(), Some(Version::new(2, 17, 0)));
/// assert!(Target::mariadb((11, 4)).is_mariadb());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    dialect: Dialect,
    version: Version,
    mariadb: bool,
    timescaledb: Option<Version>,
    timescaledb_installable: Option<Version>,
}

impl Target {
    fn new(dialect: Dialect, version: Version) -> Target {
        Target {
            dialect,
            version,
            mariadb: false,
            timescaledb: None,
            timescaledb_installable: None,
        }
    }

    /// PostgreSQL `version` (without TimescaleDB).
    pub fn postgres(version: impl Into<Version>) -> Target {
        Target::new(Dialect::Postgres, version.into())
    }

    /// MySQL `version`.
    pub fn mysql(version: impl Into<Version>) -> Target {
        Target::new(Dialect::MySql, version.into())
    }

    /// MariaDB `version`.
    pub fn mariadb(version: impl Into<Version>) -> Target {
        Target {
            mariadb: true,
            ..Target::new(Dialect::MySql, version.into())
        }
    }

    /// SQLite `version`.
    pub fn sqlite(version: impl Into<Version>) -> Target {
        Target::new(Dialect::Sqlite, version.into())
    }

    /// The same PostgreSQL target with TimescaleDB `version` installed.
    pub fn with_timescaledb(mut self, version: impl Into<Version>) -> Target {
        if self.dialect == Dialect::Postgres {
            self.timescaledb = Some(version.into());
        }
        self
    }

    /// The same PostgreSQL target where TimescaleDB `version` is not
    /// installed but can be (`CREATE EXTENSION` would succeed: the server
    /// has the extension, preloads its library, and the role may create
    /// it).
    pub fn with_timescaledb_installable(mut self, version: impl Into<Version>) -> Target {
        if self.dialect == Dialect::Postgres {
            self.timescaledb_installable = Some(version.into());
        }
        self
    }

    /// The database family.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// The server version.
    pub fn version(&self) -> Version {
        self.version
    }

    /// Whether the server is MariaDB (dialect [`Dialect::MySql`]).
    pub fn is_mariadb(&self) -> bool {
        self.mariadb
    }

    /// The installed TimescaleDB version, if any.
    pub fn timescaledb(&self) -> Option<Version> {
        self.timescaledb
    }

    /// The TimescaleDB version `CREATE EXTENSION` would install, when the
    /// extension is not installed yet but can be.
    pub fn timescaledb_installable(&self) -> Option<Version> {
        self.timescaledb_installable
    }

    /// The server's name and version, for messages: `PostgreSQL 17.0.0`,
    /// `MariaDB 11.4.0`.
    pub fn describe(&self) -> String {
        let name = match (self.dialect, self.mariadb) {
            (Dialect::MySql, true) => "MariaDB".to_owned(),
            (dialect, _) => dialect.to_string(),
        };
        format!("{name} {}", self.version)
    }

    pub(crate) fn at_least(&self, version: impl Into<Version>) -> bool {
        self.version >= version.into()
    }

    pub(crate) fn mark_timescaledb_installed(&mut self, version: Version) {
        self.timescaledb = Some(version);
        self.timescaledb_installable = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_from_server_strings() {
        assert_eq!(Version::parse("17"), Some(Version::new(17, 0, 0)));
        assert_eq!(Version::parse("3.46.1"), Some(Version::new(3, 46, 1)));
        assert_eq!(
            Version::parse("8.0.36-0ubuntu0.22.04.1"),
            Some(Version::new(8, 0, 36))
        );
        assert_eq!(
            Version::parse("PostgreSQL 16.2 on x86_64-pc-linux-gnu"),
            Some(Version::new(16, 2, 0))
        );
        assert_eq!(Version::parse("2.17.2"), Some(Version::new(2, 17, 2)));
        assert_eq!(Version::parse("no digits"), None);
        assert!(Version::new(8, 0, 13) > Version::new(8, 0, 3));
        assert!(Version::from((10, 5)) < Version::new(10, 5, 2));
    }

    #[test]
    fn targets_describe_themselves() {
        assert_eq!(Target::postgres(17).describe(), "PostgreSQL 17.0.0");
        assert_eq!(Target::mariadb((11, 4)).describe(), "MariaDB 11.4.0");
        assert_eq!(Target::sqlite((3, 46, 1)).describe(), "SQLite 3.46.1");
        assert_eq!(Target::mysql(8).with_timescaledb(2).timescaledb(), None);
    }
}
