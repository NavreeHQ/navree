---
name: navree-module-conformity
description: "Audits a Navree module or app for conformity and tenant isolation: `navree check` (module graph, then `conformity:check`: `api.docs` with `api.require_docs` and `api.undocumented`, `workspace.entities`, `workspace.repo` with the `navree:allow(workspace-repo)` marker, `workspace.tables`, `i18n.translations`, `db.migrations`), `TestApp::isolation_suite::<M>()` cross-workspace tests from a module's route table and factories (`IsolationSuiteExt::factory`, `.record`, `.body`, `.skip`, `Tenancy`), custom checks with `ConformityCheck`, and `navree gate` (fmt, clippy, tests, check and the front end under a machine lock). Use when finishing or reviewing a module, before a push or release, when adding a workspace-owned entity or route, when `navree check` or an isolation suite fails, or when data might leak between workspaces."
---

# Module conformity

A module is done when it compiles, its tests pass, and it follows the
rules that live between modules: every route documented, every table owned
by a workspace or shared, workspace data reached only through
`WorkspaceRepo`, and no route that reads, changes or deletes another
workspace's records. Three tools prove it:

- **`navree check`**: the module graph, then the conformity checks the
  app's modules contribute.
- **`app.isolation_suite::<M>()`**: cross-workspace tests for every route of
  module `M`, generated from its route table.
- **`navree gate`**: fmt, clippy, the tests, `navree check` and the front
  end, in one command.

## Steps: audit a module

1. Classify every entity in the module's `register`:

   ```rust
   m.workspace_scoped::<Deal>()        // workspace entity and access subject (record checks by id)
       .workspace_entity::<DealNote>()  // workspace entity, not an access subject
       .global_entity::<Currency>();    // one set of rows for every workspace
   ```

   Workspace entities implement `WorkspaceScoped` (column `workspace_id`
   unless `WORKSPACE_COLUMN` says otherwise).
2. Reach workspace entities only through `WorkspaceRepo::<E>` (`select`,
   `find`, `find_or_fail`, `insert`, `update`, `delete`) or
   `CurrentWorkspace::scope(select)`. A deliberate cross-workspace query
   (an admin report, a maintenance command) gets
   `// navree:allow(workspace-repo)` on its line or the line above.
3. Document every route with `#[api(summary = "..", tag = "..")]` (or
   `.api(ApiMeta { .. })` on a `RouteDef`). Routes that stay undocumented
   on purpose go in `api.undocumented` (`"METHOD /path"`, `"/path"`, a
   trailing `*`).
4. Give the workspace table an index that starts with `workspace_id` and,
   on PostgreSQL, `RlsPolicy::new("deals").create_sql()` in its migration.
5. Write the module's isolation suite (below) and run it with `cargo test`.
6. Run `navree check`, fix every error, read every warning.
7. Run `navree gate` before pushing.

## navree check

```sh
navree check                          # graph, then every conformity check
navree check --only workspace         # a section: workspace.entities, .repo, .tables
navree check --only api.docs --json   # one check, as JSON (status, checked, findings)
navree check --graph                  # print the module tree and stop
cargo run -- conformity:check --root . --json   # the app command itself (CI, deploys)
```

`conformity:check` boots the app once without serving, so it needs the
app's config and a migrated database (`navree migrate` first). Errors exit
1; warnings only report.

| Check | From | Fails on (error) / reports (warning) |
|---|---|---|
| `api.docs` | `ApiModule` | error: a route without `#[api]` (not in `api.undocumented`) |
| `workspace.entities` | `WorkspaceModule` | error: an entity in `src/` not classified, or classified both ways |
| `workspace.repo` | `WorkspaceModule` | error: `Entity::find*`, `insert*`, `update*`, `delete*` or `Repo::<E>` on a workspace entity |
| `workspace.tables` | `WorkspaceModule` | error: no workspace column; warning: no index starting with it, row-level security off (PostgreSQL), table missing |
| `i18n.translations` | built in | runs the app's `i18n:check` when a module registers it, else skipped |
| `db.migrations` | built in | runs the app's `migrate:lint` when a module registers it, else skipped |

The `workspace.*` checks are skipped under `schema` and `database`
isolation. `api.require_docs = true` makes an undocumented route stop the
boot itself (turn it on in development).

The `workspace.repo` scan reads source as text: it follows `use` trees,
renames, globs and `type` aliases, and skips `tests/`, `benches/`,
`examples/` and `#[cfg(test)]`. It cannot see through generics or macros;
the isolation suite covers what reaches the routes.

## Isolation suites

```rust
use navree::testing::TestApp;
use navree::workspace::testing::IsolationSuiteExt;
use serde_json::json;

#[tokio::test]
async fn deals_stay_in_their_workspace() {
    let app = app().await; // WorkspaceModule + DealsModule (+ .fake_auth() with AuthModule)
    app.isolation_suite::<DealsModule>()
        .factory(DealFactory::default())                       // binds {id}, uses workspaces
        .body("POST", "/api/v1/deals", json!({ "title": "Renewal" }))
        .run()
        .await;
}
```

The suite creates workspaces A and B with one owner, then sends every route
of `DealsModule` from A:

| Route | Passes when |
|---|---|
| `/deals/{id}` (any method) with B's record | 404; the same `GET` answers 2xx in B first (the control); after a write B's record reads unchanged |
| `GET /deals` (a list) | 2xx without B's record |
| `POST /deals` with `workspace_id` set to B's id | a 4xx, or the record stays in A |

A 2xx, a 403 (it leaks that the record exists), a 400 before lookup (give
a valid `.body`) or a 401 (no test authenticator) fails the route, and
`run` panics with a table of every route. `try_run` returns the
`IsolationReport` instead.

- `.factory_for("deal", DealFactory::default())` binds `{deal}`;
  `.record("note", |cx| async move { .. })` binds `{note}` with an async
  closure (`cx.tenant().id`, `cx.params()["deal"]`, `cx.n()`), returning
  the id as text.
- A parameter without a binding fails its route (`path parameter {note} has
  no record`), so new routes can't slip past.
- `.skip(method, path, reason)` leaves a route out; creates without a
  `.body` are skipped.
- Paths are the mounted templates: prefix and version included
  (`/api/v1/deals/{id}`).
- Records go through `WorkspaceRepo::insert` (row isolation) or into the
  workspace's schema or database, with the workspace column set.
- `navree::workspace::testing` needs the `navree` features `workspace` and
  `testing` (both in the default `full`).
- Another tenant model: implement `navree::testing::Tenancy` (`create`,
  `owner_field`, `noun`) and pass it with `.tenancy(..)`.

## A check of your own

```rust
use navree::contracts::{CheckOutcome, ConformityCheck, Finding, ModuleBuilderConformityExt};

m.conformity_check(ConformityCheck::new(
    "billing.plans",                 // <section>.<what>: `--only billing` selects it
    "Every plan has a price",        // what holds when it passes
    |cx| async move {
        let catalog = cx.app().get::<PlanCatalog>().await?;
        let findings: Vec<Finding> = catalog.plans.iter()
            .filter(|p| p.price_cents.is_none())
            .map(|p| Finding::error(format!("plan `{}`", p.key), "has no price"))
            .collect();
        Ok(CheckOutcome::checked(catalog.plans.len()).findings(findings))
    },
));
```

`Finding::warning` reports without failing, `.hint(..)` says how to fix,
`CheckOutcome::skipped(reason)` reports a check that doesn't apply, and
`cx.root()` is the app's source directory. In a test:
`ConformityReport::run(app.ctx(), &ConformityOptions::new(env!("CARGO_MANIFEST_DIR"))).await`
then `assert!(report.is_ok(), "{report}")`.

## navree gate

```sh
navree gate                       # fmt, clippy, test, check, web:typecheck, web:test, web:build
navree gate --only test,check     # some steps; `web` = every web step
navree gate --skip web            # leave steps out (CI with its own front-end job)
navree gate --keep-going          # every step, even after a failure
navree gate --web frontend        # the front end's directory (default web/)
navree gate --no-lock             # don't wait for other gates
```

Steps stop at the first failure and print their times. `--workspace` is
added when the app's `Cargo.toml` has `[workspace]`; web steps run when
`web/package.json` has the script (pnpm, yarn or npm by lockfile; typecheck
falls back to `tsc --noEmit` with a `tsconfig.json`). One gate runs per
machine: the lock is `navree-gate.lock` in the temp directory, and a second
gate waits and names the holder.

## Common mistakes

- Answering 403 for another workspace's record. It must be 404, so the
  record's existence doesn't show: use `WorkspaceRepo::find_or_fail` or
  `#[can(.., from = "id")]` behind `InWorkspace`.
- Taking `workspace_id` from the request body. Set it from
  `CurrentWorkspace` (`WorkspaceRepo::insert` does).
- Silencing `workspace.repo` with the allow marker on ordinary handlers.
  The marker is for deliberate cross-workspace reads only.
- Forgetting `navree migrate` before `navree check`: `workspace.tables`
  then warns that the tables don't exist.
- An isolation suite with `workspace.resolvers` lacking `header`: the suite
  names the workspace in `X-Workspace`; add `header` in the test app.
- Running two gates (or two `cargo` builds) at once on one machine; let
  the gate's lock queue them.

## Docs

- https://navree.com/docs/testing/conformity-checks/
- https://navree.com/docs/testing/isolation-tests/
- https://navree.com/docs/workspaces/isolation/
- https://navree.com/docs/digging-deeper/openapi/
