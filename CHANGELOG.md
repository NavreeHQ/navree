# Changelog

Every change to Navree is listed here, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Navree follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Every crate and
every `@navree/*` package carries the same version.

## [Unreleased]

### Security

- Navigator's `query_database` and Lighthouse's tools could be made to run more than the one
  read-only statement they checked, and so to change the development database: on SQLite and
  PostgreSQL through hex strings (`X'..'`) and custom type modifiers, which were sent back written
  differently from how they were read; on PostgreSQL through functions called in `FROM`
  (`pg_read_file`, `dblink`, `pg_terminate_backend`) and functions that run SQL text of their own
  (`ts_stat`, `query_to_xml`); and on a PostgreSQL server with `standard_conforming_strings` off,
  through a backslash before a quote. The database now holds the line: SQLite files are opened
  read-only, and on PostgreSQL each query is a single prepared statement in a read-only
  transaction, in a session that pins `standard_conforming_strings = on` and checks it. The
  statement guard also refuses those literals, types and functions, and sends a statement only
  when it reads back unchanged.
- On PostgreSQL, `query_database` and Lighthouse's tools now refuse to run as a superuser or as a
  member of `pg_read_server_files`, `pg_write_server_files`, `pg_execute_server_program` or
  `pg_signal_backend`, since such a role reaches the server's files, programs and other sessions
  from a query. The development database usually connects as `postgres`, a superuser: the
  message shows how to create a read-only role, and the new `navree mcp --database-url` points
  Navigator at it. `navree mcp --allow-privileged-database-role` keeps the previous behaviour,
  with a warning each time Navigator starts.
- `describe_schema` on PostgreSQL could list every table instead of the one asked for, given a
  table name with a backslash and a quote on a server with `standard_conforming_strings` off.
- The pgvector store wrote the field name of a comparison filter (`Filter::gt`, `gte`, `lt`,
  `lte`) into its SQL unescaped. Field names are bound parameters now.
- The UI runtime refuses a `data-ui-form` whose action is on another origin before sending it
  with the session's token, and applies nothing from an answer that was redirected to another
  origin. Navree's default content security policy already stopped the first on Navree pages.

### Added

- `navree mcp --database-url <url>`: the database Navigator's database tools read instead of the
  app's `database.url`, such as the same database through a read-only role.
- `navree mcp --allow-privileged-database-role`.

### Changed

- `NavigatorOptions` has two new fields, `database_url` and `allow_privileged_database_role`.
  Code that builds it with a struct literal adds them, or ends with `..Default::default()`.
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
