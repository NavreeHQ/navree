---
name: navree-navigator
description: "Uses Navigator, Navree's MCP server for coding agents (`navree mcp`), and installs the Navree agent skills (`navree skills:install`). Covers connecting Claude Code, Cursor, Codex or VS Code; the twelve read-only tools (`app_info`, `list_routes`, `module_graph`, `list_providers`, `list_commands`, `describe_schema`, `query_database`, `read_logs`, `last_error`, `read_config`, `search_docs`, `read_doc`); the options `--path`, `--no-build`, `--allow-remote-database` and `--docs-offline`; the log file the log tools need; and the security limits. Use when setting up an agent on a Navree app, when a Navigator tool returns an error (a build error, a refused query, no log file, a remote database), before adding a route, wiring a provider or writing a migration, or when asked why a request failed."
---

# Navigator

Navigator answers questions about a Navree app from the app itself: the
route table after every module is wired, the module graph, the database as
it is, the last error in the log, the configuration, and the docs of the
Navree version in `Cargo.lock`. It is an MCP server built into the `navree`
CLI (crate `navree-navigator`), run over stdio in the app's directory. Every
tool is read-only.

## Steps: connect an agent

1. Install the CLI (`cargo install --path crates/navree-cli` from the Navree
   repository, or as the docs say) so `navree` is on `PATH`.
2. Register the server from the app's directory:
   - Claude Code: `claude mcp add navree -- navree mcp` (team-wide:
     `claude mcp add --scope project navree -- navree mcp`, which writes
     `.mcp.json` to commit).
   - Any other stdio client: command `navree`, arguments `["mcp"]`. If the
     client doesn't start servers in the project directory, add
     `"--path", "/path/to/app"`.
3. Turn on the log file in `config/app.toml`, or `read_logs` and
   `last_error` have nothing to read:

   ```toml
   [log]
   file = { path = "storage/logs/navree.log", rotation = "daily", keep_days = 7 }
   ```

4. Install the skills into the project and commit them:
   `navree skills:install` (writes `.claude/skills/` and `.agents/skills/`,
   replaces the `navree` and `navree-*` folders, leaves other skills alone).
5. Check: ask the agent which Navigator tools it has (`/mcp` in Claude
   Code lists them).

## The tools

| Tool | Arguments | Answers |
|---|---|---|
| `app_info` | none | `package`, `path`, `navree` (from Cargo.lock), `rust`, `features` (the facade features Cargo.toml turns on), `modules` (boot order), `database` (`driver`, redacted `url`), `config_files` |
| `list_routes` | `path` (prefix), `method`, `module` | `[{method, path, name, access, module, handler, ...}]` from `<app> routes --json` |
| `module_graph` | `module` | `{modules: [{name, global, root, imports, exports, providers, commands}]}` or one module |
| `list_providers` | `module` | `[{type, scope, module, exported, multi, dependencies}]` |
| `list_commands` | none | `[{name, about, module}]`, built-ins first |
| `describe_schema` | `table` | `{driver, tables: [{name, columns, indexes, foreign_keys}], migrations: {applied, pending}}` |
| `query_database` | `sql` (required) | `{columns, rows, truncated, duration_ms}`, at most 100 rows |
| `read_logs` | `level`, `target`, `request_id`, `since`, `limit` | `{file, lines: [{timestamp, level, target, message, request_id, fields, spans}]}` |
| `last_error` | none | `{error, request_id, request: {method, route, status}, lines}` |
| `read_config` | `key` | the redacted configuration tree, or one key |
| `search_docs` | `query` (required), `limit` | `{version, source, note?, results: [{slug, title, section, url, excerpt, score}]}` |
| `read_doc` | `slug` (required), `section` | the page (or section) as Markdown |

How the app tools work: Navigator runs `cargo build --message-format=json
--bin <app>` (debug profile, your target directory), then the binary with
`introspect --json` (graph, providers, commands; nothing constructed),
`routes --json` (boots without serving) or `migrate:status --json`, with
`APP_DATABASE__MIGRATE_ON_BOOT=false` and `APP_LOG__LEVEL=error` added to
the environment. Answers are cached while the binary is unchanged.

## Use it before changing code

- Before adding a route: `list_routes` with the `path` prefix, to avoid a
  duplicate and copy the neighbors' access rule.
- Before wiring a service: `list_providers` / `module_graph` to find the
  owning module and whether it exports the type; import that module.
- Before a migration or a query: `describe_schema` (pending migrations
  explain "no such column").
- When a request fails: `last_error`, then `read_logs` with its
  `request_id`.
- For API questions: `search_docs`, then `read_doc` with the slug.

## Options

| Option | Effect |
|---|---|
| `--path <dir>` | The app to serve instead of the current directory |
| `--no-build` | Never run `cargo build`; use `target/debug/<app>` if it exists |
| `--allow-remote-database` | Allow a `database.url` that isn't on this machine |
| `--docs-offline` | Never download docs; use the cache or the built-in docs |

`NAVREE_HOME` moves the docs cache (default `~/.navree`, so
`~/.navree/docs/<version>/docs.json`).

## Checks

```sh
navree mcp --help
navree skills:install
cargo test -p navree-navigator
```

## Common mistakes

- **"The app doesn't build"** from `list_routes` and friends: that is the
  compiler's first error; fix it. `describe_schema`, `read_logs`,
  `read_config` and the docs tools keep working meanwhile.
- **Refused queries.** Only one `SELECT`, `WITH`, `EXPLAIN` or `SHOW`
  statement passes; writes, `PRAGMA`, `ATTACH`, `SET`, transaction control,
  `SELECT ... INTO`, `FOR UPDATE`, data-changing CTEs and functions such as
  `pg_sleep`, `pg_read_file` or `load_extension` are refused before
  sending. Change data with migrations, seeders or the app, never through
  Navigator.
- **"isn't on this machine"**: `database.url` points at a remote server
  (often a forgotten `.env`). Fix the URL, or start with
  `--allow-remote-database` on purpose.
- **"doesn't exist yet"** for SQLite: Navigator never creates the file; run
  `navree migrate`.
- **No log file**: set `log.file` (above) and restart `navree dev`.
- **MySQL** URLs are not supported (SQLite and PostgreSQL are).
- Values of columns whose names contain `password`, `secret`, `token`,
  `api_key` or `private_key` come back as `[redacted]`; that is expected.

## Docs

- https://navree.com/docs/ai/navigator/
- https://navree.com/docs/ai/agent-skills/
