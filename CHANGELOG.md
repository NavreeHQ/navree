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
  statement or anything that writes, and an authorizer allows only reading (no `PRAGMA`,
  `ATTACH`, `VACUUM` or transactions). On PostgreSQL each query is a single prepared statement in
  a read-only transaction, in a session that pins `standard_conforming_strings = on` and checks
  it. The statement guard also refuses those literals, types, parameters and functions, sends a
  statement only when it reads back unchanged, and refuses statements over 64 KB or nested over
  1,000 expressions deep, which could crash Navigator.
- On PostgreSQL, `query_database` and Lighthouse's tools now refuse to run as a superuser, as a
  role that can become one (a member of a superuser role, or a superuser session with a lesser
  `role`), or as a member of `pg_read_server_files`, `pg_write_server_files`,
  `pg_execute_server_program`, `pg_signal_backend` or `pg_signal_autovacuum_worker`, since such a
  role reaches the server's files, programs and other sessions from a query.
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

- `NAVREE_MCP_DATABASE_URL`: the database `navree mcp` reads instead of the app's
  `database.url`, such as the same database through a read-only role.
- `NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1`: lets `navree mcp` query as a privileged
  PostgreSQL role, with a warning each time it starts.
- `navree_navigator::DatabaseOptions`, `Navigator::with_database` and `run_stdio_with`, which
  carry those two settings for code that embeds Navigator.

### Changed

- Upgrading: on PostgreSQL, `navree mcp`'s `query_database` and Lighthouse's tools refuse a
  privileged role, and the development database usually connects as `postgres`, a superuser.
  Create a read-only role (the refusal prints the SQL; the Navigator docs have it too) and set
  `NAVREE_MCP_DATABASE_URL` to its URL in your agent's MCP configuration, or set
  `NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1` to keep querying as before. A plain role can
  still call any `SECURITY DEFINER` function that `PUBLIC` may execute, with its owner's
  privileges; check the ones your database defines. `describe_schema` is not affected.
- On SQLite, `query_database` no longer reads pragma table functions (`pragma_table_info`):
  `describe_schema` answers what they would.
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
