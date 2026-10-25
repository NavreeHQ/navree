<img src="assets/icon.svg" width="64" height="64" alt="">

# keel

Created in [T3 Code](https://t3.codes).

Keel is a Laravel-class, NestJS-structured application framework on Rust and
Tokio, built as a Cargo workspace of small module crates. `apps/crm-api` (a
Keel API) and `apps/crm-web` (Next.js) are the reference CRM built on top.

- Product requirements: [`docs/prd/keel-prd.md`](docs/prd/keel-prd.md)
- Build design (crate hierarchy, decisions, CRM API contract):
  [`docs/superpowers/specs/2026-10-25-keel-framework-design.md`](docs/superpowers/specs/2026-10-25-keel-framework-design.md)
- Implementation plan:
  [`docs/superpowers/plans/2026-10-25-keel-framework.md`](docs/superpowers/plans/2026-10-25-keel-framework.md)

## Layout

| Path | What |
|---|---|
| `crates/keel` | Facade: re-exports every module crate (one Cargo feature each, `full` by default) and the macros |
| `crates/keel-kernel`, `keel-contracts`, `keel-config` | Module graph, DI container, lifecycle; shared contracts; configuration |
| `crates/keel-*` | One crate per module (http, db, auth, access, queue, ...) |
| `crates/keel-macros` | `#[module]`, `#[injectable]`, `#[controller]` and route attributes |
| `crates/keel-cli` | The `keel` binary (`keel new`, `keel make:*`, `keel dev`) |
| `apps/crm-api` | The CRM API, a Keel application |
| `apps/crm-web` | The CRM front end (Next.js) |

Crates may only depend on crates in lower layers of the hierarchy (spec §3).
`crates/keel/tests/dependency_graph.rs` enforces the allowed edges on every
test run.

## Building

Requirements: Rust 1.95 or newer (edition 2024; the build machine uses
1.99.1), the MSVC build tools on Windows (SQLite is compiled from source via
the bundled `libsqlite3-sys`), and [sccache](https://github.com/mozilla/sccache)
(`.cargo/config.toml` sets `rustc-wrapper = "sccache"`). No OpenSSL, CMake or
NASM is needed: TLS is rustls with the `ring` provider everywhere.

```sh
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

### Target directory convention

Several builds (people, CI jobs or agents) often run at the same time. Cargo
locks the target directory, so each one uses its own and shares compiled
dependencies through sccache:

```sh
CARGO_TARGET_DIR=target/agents/<label> cargo test -p <crate>
```

For example `CARGO_TARGET_DIR=target/agents/http cargo test -p keel-http`.
Use `-p <crate>` while working on one crate, and keep `CARGO_BUILD_JOBS`
moderate when memory is tight.

### Features and drivers

- Databases: SQLite (bundled) by default; enable PostgreSQL with the
  `postgres` feature of `keel` (or `keel-db`).
- External versions live only in the root `[workspace.dependencies]`; crates
  use `dep = { workspace = true }`.
