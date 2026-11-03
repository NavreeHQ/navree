---
name: navree-reverse-engineering
description: "Reverse engineering an old product into Navree: turns an existing product in any stack (a web app in PHP, Laravel, Rails, Django, Node, .NET or Java, a desktop or Access back office) with old, messy data into written specs, a task list and test cases, then rebuilds it in Navree from those specs and verifies it against the specs and the old product. Phases that each write files to a spec folder (intake, map, base layer and data profile, modules, per-module CRUD, list, screen, service, worker, workflow, reporting and integration specs, roles and permissions, tenancy, data migration, reconcile, task list and tests, build, verify), one subagent brief per role with a refuter for every spec, owner stop points, a catalog of old-code mistakes and their Navree form, and the mapping from spec items to Navree features. Use when asked to reverse engineer a product, convert or port an old app to Navree, write specs from a codebase, migrate or rewrite a legacy app, rebuild a system whose data can't be converted directly, or plan the data migration of an old database into a Navree app."
---

# Reverse engineering an old product into Navree

Old products hide their rules. Business logic sits in the browser, tables
sort and filter on the client, options are typed into the code, and the data
holds years of nulls, duplicates and free text. You can't convert such a
product directly. You read it, write down what it does as specs with
evidence, plan and test from the specs, then build it again in Navree, layer
by layer and module by module, and prove the new product matches.

This skill is the method. Each phase writes files to one spec folder, so a
long job survives restarts and every claim points at its source. Each job
has a written brief for the agent that does it, and every spec meets a
second agent whose only job is to prove it wrong.

## The rules

1. **Files, not memory.** Every phase writes its files before it ends. A
   phase with no files did not happen. `specs/STATUS.md` says where to
   resume; the agent running the whole job keeps it, roles don't edit it.
2. **Evidence on every row.** Each spec row says where it comes from (a
   `file:line`, a `table.column`, a query, a captured request), how it is
   known (`code`, `data`, `running`, `inferred`) and how sure you are
   (`high`, `medium`, `low`). Every `inferred` row is also a question for
   the owner.
3. **Read-only on the old product.** Work on a copy of its code and a copy
   of its database. Never the live database, never outbound mail, payments
   or webhooks. Intake records how.
4. **One owner per file.** One role owns each file; another role may fill
   only the columns its brief names (the module planner fills `MAP.md`'s
   Owner column). Another role refutes it. The agent that builds never
   grades its own work.
5. **Done means evidence.** A row closes only with the test, command or
   query that proves it, quoted. "Looks done" is not a status.
6. **Keep the behaviour, fix the mistakes.** The new product does what the
   old one does. Where the old code is wrong (`references/mistakes.md`), the
   spec says so, gives the Navree form, and the owner approves the change.
7. **Only real Navree.** Map spec items to features that exist
   (`references/navree-mapping.md`). Anything else is a task to build, not
   a feature to assume.

## The spec folder

Choose its place at intake and write it in `INTAKE.md`. A good default is
one repository with `specs/` now and the Navree app beside it later
(`navree new` needs a new or empty directory).

```text
specs/
  STATUS.md                     phase, state, owner role, files, gate, sign-off
  00-intake/INTAKE.md           the product, stacks, how to run it safely, what can't run
  01-map/MAP.md                 routes, screens, endpoints, tables, jobs, schedules, integrations, roles...
  02-base/SCHEMA.md             tables, columns, keys, implied relations, enums, lookups
  02-base/DATA_PROFILE.md       nulls, duplicates, orphans, mixed formats, bad money and dates
  02-base/queries/SQL-*.sql     every profiling query, with its output beside it
  03-modules/MODULES.md         modules, what each owns, dependency order
  04-specs/<module>/MODULE.md   the module's index: layers, status, open questions
  04-specs/<module>/crud.md     entities, fields, validation, control per field, option sources
  04-specs/<module>/lists.md    server-side tables: columns, filters, sort, search, paging, bulk, export
  04-specs/<module>/screens.md  pages, what each shows, actions, empty, loading and error states
  04-specs/<module>/service.md  business rules and computations
  04-specs/<module>/workers.md  jobs and schedules
  04-specs/<module>/workflows.md states, transitions, who may move them, timers
  04-specs/<module>/reports.md  metrics, dimensions, sources, time grain
  04-specs/<module>/integrations.md  other systems: direction, auth, payloads, failure
  05-cross/ROLES.md             role x action x record scope
  05-cross/TENANCY.md           who owns which rows, how they stay apart
  05-cross/CROSS_CUTTING.md     auth, audit, notifications, settings, files, search, i18n, time zones
  06-migration/MIGRATION.md     old to new, field by field; order; checks
  06-migration/CLEANING.md      cleaning rules and what happens to bad rows
  07-reconcile/refutations/     one verdict file per spec file
  07-reconcile/RECONCILE.md     contradictions, unmapped items, guesses
  07-reconcile/QUESTIONS.md     what only the owner can answer
  08-plan/TASKS.md              the task list, in build order
  08-plan/TESTS.md              test cases, each tied to spec rows
  09-build/BUILD_LOG.md         per task: commit, tests, gate
  10-verify/VERIFY.md           every spec row and test case, closed with evidence
  10-verify/PARITY.md           old against new, per parity test
  10-verify/MIGRATION_RUN.md    rehearsals: counts, totals, rejected rows
  10-verify/logs/               the output of every command the verifier ran
  evidence/                     captures (CAP-*), screenshots (SHOT-*), query outputs
```

A module writes only the layer files it has. `MODULE.md` lists every layer
and says "none" with a reason for the ones it lacks.

Writers that run side by side never share a file. Each spec file in
`02-base`, `04-specs`, `05-cross` and `06-migration` ends with the same
three sections, and the reconciler collects them: `## Questions` (into
`QUESTIONS.md`), `## Deliberate changes` (into the module's `MODULE.md`)
and `## New map rows` (into `MAP.md`, under "Added later").

## Ids and the row format

| Kind | Id | Example |
|---|---|---|
| Map rows | `RT` route, `SC` screen, `EP` endpoint, `TB` table, `JB` job, `SH` schedule, `IN` integration, `RL` role, `RP` report or export, `NT` notification, `FL` file store, `ST` setting | `EP-031` |
| Spec rows | `<module>.<layer>.<n>`; cross-cutting `roles.<n>`, `tenancy.<n>`, `cross.<n>`; base `schema.<n>`, `profile.<n>`; migration `migration.<n>` | `invoices.crud.7` |
| Evidence | `SQL-` query, `CAP-` captured request and response, `SHOT-` screenshot | `CAP-012` |
| Cleaning rules | `C-` | `C-7` |
| Questions, tasks, tests | `Q-`, `TK-`, `T-` | `Q-004` |

Every spec row ends with the same columns:

```text
| ... | Source                         | Known by | Conf.  | Status    |
| ... | app/Http/InvoiceController.php:88, CAP-012 | running  | high   | confirmed |
```

A writer sets its rows to `draft`, or `question` when only the owner can
settle them. Then a row moves `draft` → `confirmed` (the refuter could not
break it) or `refuted` (back to its writer) → `planned` (tasks and tests
exist) → `built` (the builder's task passed its gate) → `verified` (the
verifier quoted the evidence). `dropped` needs the owner's answer in
`QUESTIONS.md`. The worked examples in the templates show rows after
refutation. Base rows (`schema.`, `profile.`) are evidence, not
requirements: they stop at `confirmed`. A map row is `open`, gets an owner
module in phase 3, becomes `owned` when confirmed spec rows cover it, then
`closed` (all of them verified) or `dropped`.

## The phases

| # | Phase | Role (brief in `references/agents/`) | Writes | Template |
|---|---|---|---|---|
| 0 | Intake | `intake` | `00-intake/INTAKE.md`, `STATUS.md` | in the brief |
| 1 | Map | `cartographer` | `01-map/MAP.md` | in the brief |
| | **Stop 1: the owner confirms the map** | | | |
| 2 | Base layer | `schema-reader`, `data-profiler` | `02-base/*` | `spec-base.md` |
| 3 | Modules | `module-planner` | `03-modules/MODULES.md`, `04-specs/*/MODULE.md`, the Owner column of `MAP.md` | `spec-module.md` |
| | **Stop 2: the owner confirms modules and order** | | | |
| 4 | Specs per module and layer | `crud-writer`, `list-writer`, `screen-writer`, `service-writer`, `worker-writer`, `workflow-writer`, `metrics-writer`, `integration-writer` | `04-specs/<module>/*.md` | one per layer, below |
| 5 | Cross-cutting | `access-writer`, `cross-cutting-writer` | `05-cross/*` | `spec-roles.md`, `spec-tenancy.md`, `spec-cross-cutting.md` |
| 6 | Data migration | `migration-writer` | `06-migration/*` | `spec-migration.md` |
| 7 | Reconcile | `refuter` (every spec file, base files included), `reconciler` | `07-reconcile/*`; collects questions, deliberate changes and new map rows | in the briefs |
| | **Stop 3: the owner answers the questions** | | | |
| 8 | Task list and tests | `test-planner` | `08-plan/TASKS.md`, `TESTS.md` | `tests.md` |
| | **Stop 4: the owner approves the plan** | | | |
| 9 | Build | `builder` | the Navree app, `09-build/BUILD_LOG.md` | the other Navree skills |
| 10 | Verify | `verifier` | `10-verify/*` | `tests.md` |
| | **Stop 5: the owner accepts, then cut-over** | | | |

### 0. Intake

Learn what the product is for, who uses it, its stacks and versions, where
the code and data live, and how to run it without harm: a restored copy of
the database (personal data masked when the owner asks), mail to a local
catcher, payments in a sandbox, outbound network blocked, schedulers off
until wanted. Write down what can't run and why: parity tests for those
parts become `N/A` with the owner's acceptance, never silent.

### 1. Map

List everything the product has, as checklists with ids: routes, screens,
endpoints, tables, jobs, schedules, integrations, roles, reports and
exports, notifications, file stores, settings. Derive each list from a
command (a route dump, a grep, `information_schema`), keep the command in
`MAP.md` with its count, and never list from memory. Nothing is judged here.
Every later phase ticks rows off this map; a row nobody owns at the end is a
gap.

### 2. Base layer

`schema-reader` writes the real schema: tables, columns, types, keys, and
the relations the code implies but the database doesn't declare. Enums and
lookups come from check constraints, code constants and `SELECT DISTINCT`.
`data-profiler` runs read-only queries on the copy and writes what the data
really holds: null rates, duplicates by natural key, orphans, mixed
formats, free text that should be a lookup, dates and money stored badly,
soft-deleted and test rows. Every query is saved as `queries/SQL-*.sql` with
its output. `references/data-migration.md` has the queries.

### 3. Modules

Group the map into modules (a module owns its tables, screens, endpoints and
jobs) and order them by dependency: identity, tenancy and lookups first,
then core records, then what builds on them, then reports. Every map row
gets exactly one owning module; a row two modules want is a question.

### 4. Specs per module and layer

For each module, one spec file per layer it has, each by its own writer
from its template in `references/`. These are where old products fail
most, so be exact:

| Layer | Writer | File | Template | What it pins down |
|---|---|---|---|---|
| CRUD | `crud-writer` | `crud.md` | `spec-crud.md` | every field: type, rules, the control that edits it (date, money, phone, email, enum, foreign key, boolean, long text, file), where its options come from |
| Lists | `list-writer` | `lists.md` | `spec-list.md` | columns, filters, sort, search, paging, bulk actions, export, all on the server |
| Screens | `screen-writer` | `screens.md` | `spec-screen.md` | what each page shows, its actions, its empty, loading, error and forbidden states, the journeys |
| Service | `service-writer` | `service.md` | `spec-service.md` | every business rule and calculation, including the ones hidden in the browser |
| Workers | `worker-writer` | `workers.md` | `spec-worker.md` | trigger, input, idempotency (a second run changes nothing), retries, schedule and zone, side effects |
| Workflows | `workflow-writer` | `workflows.md` | `spec-workflow.md` | states, transitions, who may move them, timers |
| Reports | `metrics-writer` | `reports.md` | `spec-reporting.md` | metrics, dimensions, source, time grain, time zone, known old numbers |
| Integrations | `integration-writer` | `integrations.md` | `spec-integration.md` | the other system, direction, auth, payloads, failure, retries, webhooks |

Write CRUD first; lists, screens and workflows point at its rows.

### 5. Cross-cutting

`access-writer` writes the role × action × record-scope matrix
(`spec-roles.md`) and tenancy (`spec-tenancy.md`). `cross-cutting-writer`
covers authentication, audit, notifications, settings, files, search,
localization and time zones (`spec-cross-cutting.md`).

### 6. Data migration

Field by field from old to new: transformations, cleaning rules, what
happens to each kind of bad row, the load order, how the import re-runs
safely, and the counts and totals that must match afterwards
(`spec-migration.md`, `references/data-migration.md`).

### 7. Reconcile

A `refuter` takes each spec file and tries to break every row: it opens the
cited source, looks for contrary evidence, checks the map for rows nobody
covered and the mistakes catalog for old bugs copied as rules. Then the
`reconciler` reads everything and writes contradictions, unmapped items and
guesses to `RECONCILE.md`, and every open point to `QUESTIONS.md`, each
with the options and a recommendation. It also fills each module's
`MODULE.md` from the layer files and adds the new map rows to `MAP.md`.

### 8. Task list and tests

`test-planner` turns confirmed specs into tasks in build order and test
cases: acceptance tests per spec row, parity tests (the same case on the
old and the new product, compared) where the old one runs, a role matrix
test, tenancy isolation, migration counts and totals. `references/tests.md`
has the formats.

### 9. Build

Build in Navree in module order, each module layer by layer (schema and
models, services, CRUD and lists, workers, workflows, integrations,
reports, screens), test first, loading the Navree skill that each task
names. Each task ends green on `navree gate`; its commit goes in
`BUILD_LOG.md`.

### 10. Verify

A `verifier` (never the builder) runs every test case, the parity tests
against the old product, the role matrix and the isolation suites, and a
full migration rehearsal on a fresh copy of the old data. It closes rows in
`VERIFY.md` with quoted evidence and reopens what fails.

## Stop points

Stop, show the owner a short summary with counts, and wait. Record the
answer and the date in `STATUS.md`.

1. **After the map.** What the product is, what can't run, the counts per
   map section, anything surprising. The owner adds what the code doesn't
   show (manual steps, spreadsheets beside the product, reports people
   rely on).
2. **After the modules.** The module list, what each owns, the order.
3. **After reconcile.** Every question in `QUESTIONS.md`, with options and a
   recommendation. Specs are frozen once answered; later changes go
   through a new question.
4. **Before the build.** The task list, the test plan, the list of
   deliberate changes from the old behaviour, and every parity test marked
   `N/A`.
5. **Before cut-over.** `VERIFY.md`, `PARITY.md`, the last migration
   rehearsal and anything still `NOT RUN`.

## Subagents

**With subagents.** Dispatch one role per agent, giving it its brief
(`references/agents/<role>.md`), its scope (a module, a layer, a spec
file), the paths it reads and the files it writes. Agents write files and
return only a short summary and the list of files. Run in parallel:

- phases 1 and 2: the cartographer first, then the schema reader and the
  data profiler side by side;
- phase 4: every module at once after stop 2; inside a module, the CRUD
  writer first, then the other layer writers side by side;
- phases 5 and 6: the access, cross-cutting and migration writers side by
  side, once the module specs exist;
- phase 7: one refuter per spec file, never the agent that wrote it;
- phase 10: verifiers per module, then one for the whole product.

Build one module at a time in dependency order: modules share
`src/app.rs`, `src/abilities.rs` and the migration order. Two independent
modules may build in separate worktrees, each merged only after its own
`navree gate` passes on top of the other.

```text
Role: crud-writer (brief: skills/navree-reverse-engineering/references/agents/crud-writer.md)
Scope: module `invoices`
Read: specs/01-map/MAP.md, specs/02-base/*, specs/03-modules/MODULES.md,
      specs/04-specs/invoices/MODULE.md, the old code at /work/old-app (read only)
Write: specs/04-specs/invoices/crud.md, using references/spec-crud.md
Return: the files written, row counts by status, questions added. No other prose.
```

**Without subagents.** Run the same briefs yourself, in the phase order,
one role at a time. Before each role, read its brief and only the files it
lists, not your memory of earlier phases. Refute after a break: re-read
the spec file cold, as the refuter brief says, before trusting it.

## Done

The product is done when every box holds, each with its evidence in
`VERIFY.md`:

- [ ] Every map row is `closed`, or `dropped` with the owner's answer.
- [ ] Every spec row in `04-specs`, `05-cross` and `06-migration` is
      `verified` and cites the test that proves it; base rows are
      `confirmed`.
- [ ] `QUESTIONS.md` has no open question.
- [ ] Every test case in `TESTS.md` passed on the final commit; parity
      tests passed, or each difference is a deliberate change the owner
      approved, or is `N/A` with the owner's acceptance. Nothing is
      `NOT RUN`: only the owner can turn a case into `N/A`, in writing.
- [ ] The role matrix test covers every role, action and record scope in
      `ROLES.md`.
- [ ] Every workspace-owned module has an isolation suite that passes, and
      `navree check` reports no error.
- [ ] A full migration rehearsal on a fresh copy reconciles: row counts,
      totals and rejected rows match `MIGRATION.md`, and a second run
      changes nothing.
- [ ] `navree gate` passes on the final commit.
- [ ] The owner signed stop 5 in `STATUS.md`.

## Common mistakes

- Converting the old database straight into the new schema. Profile it
  first; the profile decides the cleaning rules and the schema's nulls.
- Copying the old screens' data handling: client-side tables, hard-coded
  options, rules that run only in the browser. Each is a catalog entry
  with its server-side form.
- Specs without sources. A row nobody can trace can't be refuted or
  tested.
- Letting the writer or the builder mark its own work done.
- Answering a question with a guess to keep moving. Ask, and work on
  another module meanwhile.
- Treating the map as finished after the first pass. Every phase adds the
  rows it finds and says so in `MAP.md`.
- Running the old product against live data or live integrations.
- Building several modules at once on one branch, then fighting over
  `src/app.rs` and migrations.
- Testing only as an administrator. The leaks show as the least powerful
  role.

## References

| File | Read when |
|---|---|
| `references/agents/<role>.md` | Before running or dispatching that role (21 briefs, listed below) |
| [`references/spec-base.md`](references/spec-base.md) | Phase 2: schema and data profile |
| [`references/spec-module.md`](references/spec-module.md) | Phase 3: modules and the module index |
| [`spec-crud.md`](references/spec-crud.md), [`spec-list.md`](references/spec-list.md), [`spec-screen.md`](references/spec-screen.md) | Phase 4: records, tables, pages |
| [`spec-service.md`](references/spec-service.md), [`spec-worker.md`](references/spec-worker.md), [`spec-workflow.md`](references/spec-workflow.md) | Phase 4: rules, background work, states |
| [`spec-reporting.md`](references/spec-reporting.md), [`spec-integration.md`](references/spec-integration.md) | Phase 4: reports, other systems |
| [`spec-roles.md`](references/spec-roles.md), [`spec-tenancy.md`](references/spec-tenancy.md), [`spec-cross-cutting.md`](references/spec-cross-cutting.md) | Phase 5 |
| [`spec-migration.md`](references/spec-migration.md), [`data-migration.md`](references/data-migration.md) | Phases 2 and 6, and every rehearsal |
| [`references/mistakes.md`](references/mistakes.md) | Phases 4 to 7: old-code mistakes, how to find them, the Navree form |
| [`references/navree-mapping.md`](references/navree-mapping.md) | Phases 4, 8 and 9: which Navree feature and skill takes each spec item |
| [`references/tests.md`](references/tests.md) | Phases 8 and 10: task list, test cases, parity, role matrix, migration checks |

The briefs, in phase order:
[`intake`](references/agents/intake.md),
[`cartographer`](references/agents/cartographer.md),
[`schema-reader`](references/agents/schema-reader.md),
[`data-profiler`](references/agents/data-profiler.md),
[`module-planner`](references/agents/module-planner.md),
[`crud-writer`](references/agents/crud-writer.md),
[`list-writer`](references/agents/list-writer.md),
[`screen-writer`](references/agents/screen-writer.md),
[`service-writer`](references/agents/service-writer.md),
[`worker-writer`](references/agents/worker-writer.md),
[`workflow-writer`](references/agents/workflow-writer.md),
[`metrics-writer`](references/agents/metrics-writer.md),
[`integration-writer`](references/agents/integration-writer.md),
[`access-writer`](references/agents/access-writer.md),
[`cross-cutting-writer`](references/agents/cross-cutting-writer.md),
[`migration-writer`](references/agents/migration-writer.md),
[`refuter`](references/agents/refuter.md),
[`reconciler`](references/agents/reconciler.md),
[`test-planner`](references/agents/test-planner.md),
[`builder`](references/agents/builder.md),
[`verifier`](references/agents/verifier.md).
Load the `navree` skill before phase 9; each task names the other skill
it needs.

## Docs

- https://navree.com/docs/database/seeding-and-factories/
- https://navree.com/docs/packages/imports-and-exports/
- https://navree.com/docs/frontend/data-tables/
- https://navree.com/docs/basics/access-control/
- https://navree.com/docs/testing/isolation-tests/
- https://navree.com/docs/testing/conformity-checks/
