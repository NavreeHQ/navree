//! [`ClientCommand`]: the database's own command-line client (`psql`,
//! `mysql`, `mariadb`, `sqlite3`, `mongosh`) for a connection URL, behind
//! `navree db`.

use crate::DbError;

use crate::options::scheme;

/// A database client to run: the program, its arguments, and the
/// environment that carries the password (the arguments of a running
/// program are visible to other users of the machine; its environment is
/// not).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientCommand {
    /// The program, found on `PATH`.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Environment variables to set for it.
    pub env: Vec<(String, String)>,
}

impl ClientCommand {
    /// The client of the database at `url`: `psql` for PostgreSQL (the
    /// password in `PGPASSWORD`, `sslmode` in `PGSSLMODE`), `mysql` for
    /// MySQL and `mariadb` for MariaDB (the password in `MYSQL_PWD`),
    /// `sqlite3` for a SQLite file, `mongosh` for MongoDB (with the URL).
    ///
    /// ```
    /// use navree_db::ClientCommand;
    ///
    /// let client = ClientCommand::for_url("postgres://crm:s%40cret@db.internal:5433/crm?sslmode=require")?;
    /// assert_eq!(client.program, "psql");
    /// assert_eq!(
    ///     client.args,
    ///     ["--host", "db.internal", "--port", "5433", "--username", "crm", "--dbname", "crm"]
    /// );
    /// assert_eq!(
    ///     client.env,
    ///     [("PGPASSWORD".to_owned(), "s@cret".to_owned()), ("PGSSLMODE".to_owned(), "require".to_owned())]
    /// );
    /// # Ok::<(), navree_db::DbError>(())
    /// ```
    pub fn for_url(url: &str) -> Result<ClientCommand, DbError> {
        let unsupported = || {
            DbError::unsupported(format!(
                "no database client is known for `{}`",
                crate::options::redact_url(url)
            ))
        };
        let scheme = scheme(url).ok_or_else(unsupported)?;
        match scheme.as_ref() {
            "sqlite" => sqlite(url),
            "postgres" | "postgresql" => {
                let parts = UrlParts::parse(url);
                let mut args = parts.connection_args("--host", "--port", "--username");
                if let Some(database) = &parts.database {
                    args.extend(["--dbname".to_owned(), database.clone()]);
                }
                let mut env = Vec::new();
                if let Some(password) = &parts.password {
                    env.push(("PGPASSWORD".to_owned(), password.clone()));
                }
                if let Some(mode) = parts.query("sslmode") {
                    env.push(("PGSSLMODE".to_owned(), mode));
                }
                Ok(ClientCommand {
                    program: "psql".to_owned(),
                    args,
                    env,
                })
            }
            "mysql" | "mariadb" => {
                let parts = UrlParts::parse(url);
                let mut args = parts.connection_args("--host", "--port", "--user");
                if let Some(database) = &parts.database {
                    args.extend(["--database".to_owned(), database.clone()]);
                }
                Ok(ClientCommand {
                    program: scheme.into_owned(),
                    args,
                    env: parts
                        .password
                        .map(|password| ("MYSQL_PWD".to_owned(), password))
                        .into_iter()
                        .collect(),
                })
            }
            "mongodb" | "mongodb+srv" => Ok(ClientCommand {
                program: "mongosh".to_owned(),
                args: vec![url.to_owned()],
                env: Vec::new(),
            }),
            _ => Err(unsupported()),
        }
    }
}

/// `sqlite3 <file>` for `sqlite://path?mode=rwc`, `sqlite:path` or
/// `sqlite:///abs/path`.
fn sqlite(url: &str) -> Result<ClientCommand, DbError> {
    let rest = &url["sqlite:".len()..];
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    let path = rest.split(['?', '#']).next().unwrap_or_default();
    if path.is_empty() || path == ":memory:" {
        return Err(DbError::unsupported(
            "an in-memory SQLite database lives in the app's process; there is no client to open"
                .to_owned(),
        ));
    }
    Ok(ClientCommand {
        program: "sqlite3".to_owned(),
        args: vec![percent_decode(path)],
        env: Vec::new(),
    })
}

/// The parts of `scheme://user:password@host:port/database?query`.
#[derive(Debug, Default)]
struct UrlParts {
    user: Option<String>,
    password: Option<String>,
    host: Option<String>,
    port: Option<String>,
    database: Option<String>,
    query: Vec<(String, String)>,
}

impl UrlParts {
    fn parse(url: &str) -> UrlParts {
        let Some((_, rest)) = url.split_once("://") else {
            return UrlParts::default();
        };
        let rest = rest.split('#').next().unwrap_or_default();
        let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let (userinfo, hostport) = match authority.rsplit_once('@') {
            Some((userinfo, hostport)) => (Some(userinfo), hostport),
            None => (None, authority),
        };
        let (user, password) = match userinfo {
            Some(userinfo) => match userinfo.split_once(':') {
                Some((user, password)) => (Some(user), Some(password)),
                None => (Some(userinfo), None),
            },
            None => (None, None),
        };
        let (host, port) = if let Some(bracketed) = hostport.strip_prefix('[') {
            // An IPv6 address: `[::1]:5432`.
            match bracketed.split_once(']') {
                Some((host, rest)) => (host, rest.strip_prefix(':')),
                None => (bracketed, None),
            }
        } else {
            match hostport.rsplit_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (hostport, None),
            }
        };
        let some = |value: &str| (!value.is_empty()).then(|| percent_decode(value));
        UrlParts {
            user: user.and_then(some),
            password: password.and_then(some),
            host: some(host),
            port: port.and_then(some),
            database: path.split('/').next().and_then(some),
            query: query
                .split('&')
                .filter(|pair| !pair.is_empty())
                .map(|pair| {
                    let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
                    (percent_decode(name), percent_decode(value))
                })
                .collect(),
        }
    }

    /// The host, port and user as the client's options.
    fn connection_args(&self, host: &str, port: &str, user: &str) -> Vec<String> {
        let mut args = Vec::new();
        for (flag, value) in [(host, &self.host), (port, &self.port), (user, &self.user)] {
            if let Some(value) = value {
                args.extend([flag.to_owned(), value.clone()]);
            }
        }
        args
    }

    /// The value of query parameter `name`.
    fn query(&self, name: &str) -> Option<String> {
        self.query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    }
}

/// `%40` → `@`; invalid escapes stay as they are.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && let Some(hex) = text.get(at + 1..at + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            at += 3;
            continue;
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(url: &str) -> ClientCommand {
        ClientCommand::for_url(url).unwrap()
    }

    #[test]
    fn mysql_and_mariadb_take_the_password_from_the_environment() {
        let mysql = client("mysql://root:p%3Ass@127.0.0.1:3306/crm");
        assert_eq!(mysql.program, "mysql");
        assert_eq!(
            mysql.args,
            [
                "--host",
                "127.0.0.1",
                "--port",
                "3306",
                "--user",
                "root",
                "--database",
                "crm"
            ]
        );
        assert_eq!(mysql.env, [("MYSQL_PWD".to_owned(), "p:ss".to_owned())]);
        assert!(!mysql.args.iter().any(|arg| arg.contains("p:ss")));
        let mariadb = client("mariadb://app@db/crm");
        assert_eq!(mariadb.program, "mariadb");
        assert_eq!(
            mariadb.args,
            ["--host", "db", "--user", "app", "--database", "crm"]
        );
        assert!(mariadb.env.is_empty());
    }

    #[test]
    fn postgres_hosts_may_be_ipv6_and_options_are_left_out() {
        let pg = client("postgresql://app:secret@[::1]:5432/crm?application_name=x");
        assert_eq!(pg.program, "psql");
        assert_eq!(
            pg.args,
            [
                "--host",
                "::1",
                "--port",
                "5432",
                "--username",
                "app",
                "--dbname",
                "crm"
            ]
        );
        assert_eq!(pg.env, [("PGPASSWORD".to_owned(), "secret".to_owned())]);
    }

    #[test]
    fn sqlite_files_and_document_stores() {
        assert_eq!(
            client("sqlite://storage/app.db?mode=rwc").args,
            ["storage/app.db"]
        );
        assert_eq!(client("sqlite:data.db").args, ["data.db"]);
        assert_eq!(
            client("sqlite:///var/app%20data.db").args,
            ["/var/app data.db"]
        );
        assert!(ClientCommand::for_url("sqlite::memory:").is_err());
        let mongo = client("mongodb://localhost:27017/crm");
        assert_eq!(mongo.program, "mongosh");
        assert_eq!(mongo.args, ["mongodb://localhost:27017/crm"]);
        let unknown = ClientCommand::for_url("redis://:secret@localhost").unwrap_err();
        assert!(!unknown.to_string().contains("secret"), "{unknown}");
    }
}
