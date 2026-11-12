# Changelog

Every change to Navree is listed here, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Navree follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Every crate and
every `@navree/*` package carries the same version.

## [Unreleased]

### Security

- Navigator's `query_database` and Lighthouse's tools could be made to run more than the one
  read-only statement they checked, and so to change the development database or write files:
  through hex strings (`X'..'`), custom type modifiers and, on SQLite, variables (`$a::..`), which
  the guard and the database read differently; on PostgreSQL through functions called in `FROM`
  (`pg_read_file`, `dblink`, `pg_terminate_backend`) and functions that run SQL text of their own
  (`ts_stat`, `query_to_xml`); and on a PostgreSQL server with `standard_conforming_strings` off,
  through a backslash before a quote. The database engine now holds the line. On SQLite the file
  is opened read-only, SQLite itself prepares the text and refuses anything after the first
  statement, refuses a statement it says writes (which covers `VACUUM`), and an authorizer allows
  only reading (no `PRAGMA` but the `data_version` FTS5 reads, no `ATTACH`, no transactions). On
  PostgreSQL each query is a single prepared statement in a read-only transaction, in a session
  that pins `standard_conforming_strings = on`, checks it, and pins it again inside the
  transaction (behind a pooler in transaction mode another client's `SET` could otherwise reach
  it). The statement guard also refuses those literals, types, parameters and functions, sends a
  statement only when it reads back unchanged, and refuses statements over 64 KB or nested over
  1,000 expressions deep, which could crash Navigator.
- On PostgreSQL, `query_database` and Lighthouse's tools now refuse to run as a superuser, as a
  role that can become one (a member of a superuser role, or a superuser session with a lesser
  `role`), or as a member of `pg_read_server_files`, `pg_write_server_files`,
  `pg_execute_server_program`, `pg_signal_backend` or `pg_signal_autovacuum_worker`, since such a
  role reaches the server's files, programs and other sessions from a query. They also refuse a
  role that may execute, whatever its privilege on the schema, a `SECURITY DEFINER` function of
  the database's own (not an extension's), a function in an untrusted language (`plpython3u`,
  `plperlu`, `pltclu`, or C outside an extension), or a function of an extension that reaches
  files, programs, other servers or the network (any extension with a foreign-data wrapper,
  `dblink`, `adminpack`, `http`, `pg_net`, `pg_background`, `pg_cron`, `pgagent`, `plproxy`,
  `orafce`, `aws_s3`, `aws_lambda`, `aws_commons`, `azure_storage`, `azure_ai`,
  `google_ml_integration`, `pgml`); the support functions of an aggregate it may execute count
  too (PostgreSQL checks `EXECUTE` on them against the aggregate's owner), and so does a foreign
  table it may `SELECT` from, directly or through views. The refusal names what it found. What
  an ordinary role can still reach is what the other extensions installed in the database give
  it (their C functions, and their own `SECURITY DEFINER` functions: PostGIS, pgcrypto,
  TimescaleDB, pgvector), a table, including a foreign table, read by a view it may read (the
  view reads its tables with its owner's rights), and any extension Navigator doesn't know that
  adds a function reaching the network or a file.
- A query could make Navigator hold gigabytes: `printf('%.*c', 400000000, 'x')`, a recursive
  `zeroblob(..)` or `json_group_array` on SQLite, `repeat('x', 900000000)` or an `EXPLAIN` plan
  on PostgreSQL. SQLite now refuses to make a string or blob over 1 MB, and
  `navree mcp` caps SQLite's heap at 256 MB for its process (code that embeds Navigator sets its
  own cap with `database::limit_sqlite_heap`). PostgreSQL cuts each value to 4 KB before sending
  it (its own work ends with the 5 second timeout), and `EXPLAIN` is refused there. A query may
  answer at most 100 columns; every value over 4 KB comes back cut and ending with `… [cut]`,
  and an answer stops at about 1 MB of JSON, with `truncated` set (a single row that large keeps
  its first values and cuts the rest). Error messages are cut to 2 KB in what the tools return
  (PostgreSQL still sends the whole message, up to its 1 GB limit, and Navigator reads it once
  before cutting it). Lighthouse's tools
  read their own tables whole, up to 8 MB a value, so histograms and backtraces stay complete.
- `describe_schema` on PostgreSQL could list every table instead of the one asked for, given a
  table name with a backslash and a quote on a server with `standard_conforming_strings` off.
- The pgvector store wrote the field name of a comparison filter (`Filter::gt`, `gte`, `lt`,
  `lte`) into its SQL unescaped. Field names are bound parameters now.
- The UI runtime refuses a `data-ui-form` whose action is on another origin before sending it,
  and applies nothing from an answer that was redirected to another origin. One case is left:
  the browser follows a redirect before the runtime sees it, so on a page served without
  Navree's content security policy, a same-origin action that redirects to another origin
  (an open redirect in the app) sends the request there, with the session's `x-xsrf-token`
  header and, after a 307 or 308, the form's body, if that origin answers the browser's
  preflight. Navree's policy (`connect-src 'self'`) stops that request in the browser.

### Added

- `navree dock up` keeps a read-only role, `navree_reader`, in the PostgreSQL database, for
  Navigator. When Dock creates it, it may connect, use `public` and read its tables, now and
  later; Dock never grants it anything again, so a `REVOKE` stays. On every `up` it is made a
  plain login role whose sessions start read-only with a 5 second statement timeout (role
  defaults a session may change) and may write at most 64 MB of temporary files. Its password is its own, derived from the service's, and Dock
  sends the server only its SCRAM verifier. Like any role, it also has what PostgreSQL gives
  `PUBLIC`: `CONNECT`, `TEMP`, and `EXECUTE` on functions, extension C functions included.
- `NAVREE_MCP_DATABASE_URL`: the database `navree mcp` reads instead of the app's
  `database.url`, such as the same database through a read-only role.
- `NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1`: lets `navree mcp` query as a refused
  PostgreSQL role, with a warning each time it starts.
- `navree_navigator::DatabaseOptions`, `Navigator::with_database` and `run_stdio_with`, which
  carry those two settings for code that embeds Navigator, and
  `navree_navigator::database::limit_sqlite_heap` with `SQLITE_HEAP_LIMIT`, `MAX_VALUE_BYTES`,
  `MAX_ANSWER_BYTES`, `MAX_COLUMNS`, `MAX_MESSAGE_BYTES` and `CUT_MARK`.

### Changed

- Upgrading with Dock: run `navree dock up` once. It creates `navree_reader` in your running
  database (nothing is restarted), and from then on Navigator's `query_database`,
  `describe_schema` and Lighthouse's tools read the Dock database as that role. Until you do,
  they fail to connect and say that `navree dock up` creates the role. Dock's own user,
  `navree`, is a superuser, which `query_database` now refuses.
- Upgrading without Dock: on PostgreSQL, `navree mcp`'s `query_database` and Lighthouse's tools
  refuse a privileged role, and a development database often connects as a superuser. Create a
  read-only role (the refusal prints the SQL; the Navigator docs have it too) and set
  `NAVREE_MCP_DATABASE_URL` to its URL in your agent's MCP configuration, or set
  `NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1` to keep querying as before. A role that may
  execute `SECURITY DEFINER`, untrusted-language or network and file extension functions is
  refused too: revoke `EXECUTE` on the functions the refusal names. `describe_schema` is not
  affected.
- On SQLite, `query_database` no longer reads pragma table functions (`pragma_table_info`):
  `describe_schema` answers what they would. R*Tree tables can't be queried through
  `query_database` either: opening one prepares a write. FTS5 tables can.
- On PostgreSQL, `query_database` refuses `EXPLAIN`: the plan holds the query's constants at any
  size. Run it in `psql`, or ask `describe_schema` about the tables and indexes. SQLite's
  `EXPLAIN QUERY PLAN` still works.
- Dock refuses a `dock.toml` whose PostgreSQL `user` is `navree_reader`.
- Navigator's docs and tool descriptions say that hiding columns with secret-looking names in
  query results is a convenience, not a protection: a query can rename or wrap a column.

## [1.0.1] - 2026-11-11

### Fixed

- `navree dev` on Linux restarted the server again and again with nothing edited: it took a file
  being opened or read (the server reading `.env` as it starts, `cargo build` reading `Cargo.toml`)
  for an edit. Only edits start a rebuild now, and a server busy writing `storage/` no longer delays one.

### Security

- The development install of `@navree/nuxt` takes node-forge 1.4.3, for the four advisories fixed in it.
  Only Nuxt's development HTTPS certificates use it; the published package does not depend on it.
- The fuzzing workspace takes rustix 1.1.6, for RUSTSEC-2026-0486; the main workspace already had it.

### Changed

- Dependencies: the minor and patch updates of the Rust crates and of the front-end packages.

## [1.0.0] - 2026-11-09

First release. Read what it includes in the [release notes](https://navree.com/docs/prologue/release-notes/).

[Unreleased]: https://github.com/NavreeHQ/navree/compare/v1.0.1...HEAD
[1.0.1]: https://github.com/NavreeHQ/navree/releases/tag/v1.0.1
[1.0.0]: https://github.com/NavreeHQ/navree/releases/tag/v1.0.0
