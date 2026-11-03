# Template: modules and the module index

Phase 3 writes `03-modules/MODULES.md` (the whole product) and one
`04-specs/<module>/MODULE.md` per module (its index). A module here becomes
a Navree module later (`navree make:module <name>`), so name it the way
Navree names modules: lower case, plural, one word or snake case
(`invoices`, `price_lists`).

## MODULES.md

```text
# Modules of <product>

## Modules

| Module | Purpose | Owns (map ids) | Depends on | Group | Workspace-owned | Source | Known by | Conf. | Status |

## Build order

Group 1: <modules>    identity, tenancy, settings, lookups shared by everyone
Group 2: <modules>    core records with no dependency between them
Group 3: <modules>    records and processes built on group 2
Group 4: <modules>    reports, exports, integrations that read everything

## Map coverage

| Map section | Rows | With an owner | Without an owner | Dropped |

## Disputed rows

| Map id | Candidates | Why | Question |
```

- **Owns**: map ids (`RT`, `SC`, `EP`, `TB`, `JB`, `SH`, `IN`, `RP`, `NT`,
  `FL`, `ST`). Every map row has exactly one owner. A table two modules
  write is owned by one and listed under **Depends on** by the other.
- **Depends on**: a module that this one reads or writes through. Cycles
  are a finding: break them with an event (one module announces, the other
  listens) and say so.
- **Workspace-owned**: `yes` when its rows belong to one tenant in the new
  product, `shared` when every tenant sees the same rows (a currency list),
  `library` when there are global rows plus a tenant's own. This feeds the
  tenancy spec and, later, `navree check`.
- **Map coverage** must show no row without an owner before stop 2.
  (A row becomes `owned` later, when confirmed spec rows cover it.)

## MODULE.md

The module planner writes it with a first guess of the layers; after
the refuters, the reconciler fills it from the layer files. Layer writers
don't edit it: they end their own file with the sections below, and the
reconciler copies them here.

```text
# Module <name>

Purpose: <one sentence>
Owns: <map ids>
Depends on: <modules>

## Layers

| Layer | File | Status | Rows | Open questions | Why none |
| crud | crud.md | | | | |
| lists | lists.md | | | | |
| screens | screens.md | | | | |
| service | service.md | | | | |
| workers | workers.md | | | | |
| workflows | workflows.md | | | | |
| reports | reports.md | | | | |
| integrations | integrations.md | | | | |

## Deliberate changes from the old product

| Spec row | Old behaviour | New behaviour | Why (mistakes catalog entry or owner answer) | Approved |
```

A layer the module lacks gets `none` and a reason with a source ("no job,
cron entry or queue touches the `price_lists` table: grep of jobs/ and
crontab, see MAP.md JB-*"). An empty layer without a reason is unfinished.

## The closing sections of every spec file

Every spec file in `02-base`, `04-specs`, `05-cross` and `06-migration`
ends with these, so writers running side by side never edit a shared
file:

```text
## Questions
| Row | Question | Options | Recommendation |

## Deliberate changes
| Row | Old behaviour | New behaviour | Why (mistakes entry or owner answer) |

## New map rows
| Kind (RT, SC, EP, TB, ...) | What | Where found |
```

## Worked example

```text
| Module | Purpose | Owns | Depends on | Group | Workspace-owned | Source | Known by | Conf. | Status |
| branches | the company's branches, the tenant | TB-002, SC-003, RL-001..RL-005 | | 1 | no (it is the tenant) | MAP.md | code | high | confirmed |
| customers | wholesale customers and contacts | TB-005, TB-006, SC-010..SC-014, EP-020..EP-029 | branches | 2 | yes | MAP.md | code | high | confirmed |
| products | catalogue and price lists | TB-008..TB-011, SC-020..SC-026 | branches | 2 | library | MAP.md; Q-002 | inferred | medium | question |
| orders | orders and their lines | TB-014, TB-015, SC-030..SC-036, JB-002 | customers, products | 3 | yes | MAP.md | code | high | confirmed |
| invoices | invoices, payments, reminders | TB-018..TB-020, SC-040..SC-046, JB-003, SH-001, IN-002 | orders | 3 | yes | MAP.md | code | high | confirmed |
| reports | monthly sales and ageing | RP-001..RP-006 | invoices, orders | 4 | yes | MAP.md | code | high | confirmed |

## Disputed rows
| TB-021 payment_terms | customers, invoices | both write it (customers/edit.php:44, invoices/create.php:120) | Q-003 |
```

## Checks before the files are done

- Every map row has exactly one owner module; `Map coverage` shows no
  row without one.
- The build order respects every dependency; no module depends on a later
  group.
- Every module has a `MODULE.md` with every layer filled in or `none` with
  a reason.
