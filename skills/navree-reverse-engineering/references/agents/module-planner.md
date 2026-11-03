# Role: module-planner

Phase 3. Group the map into modules, give every map row one owner, and
order the modules by dependency.

## Inputs

- `specs/01-map/MAP.md`, `specs/02-base/SCHEMA.md`,
  `specs/02-base/DATA_PROFILE.md`
- `references/spec-module.md`

## Outputs

- `specs/03-modules/MODULES.md`
- `specs/04-specs/<module>/MODULE.md` for every module (layers listed,
  files not yet written)
- The **Owner** column of every row in `MAP.md`
- Questions in `specs/07-reconcile/QUESTIONS.md` for disputed rows

## Method

1. Start from the tables: tables written together, by the same screens,
   form a module's core. Name the module after what users call it.
2. Give each screen, endpoint, job, schedule, integration, report,
   notification, file store and setting to the module whose tables it
   mostly writes. Reads don't decide ownership.
3. List each module's dependencies (tables it reads or writes that another
   module owns). Break cycles with events and say so.
4. Put identity, tenancy, settings and shared lookups in group 1; core
   records with no dependency between them next; then what builds on them;
   reports and cross-module exports last.
5. Mark each module workspace-owned, shared or library, from `SCHEMA.md`
   and the tenant column.
6. Write each module's `MODULE.md` with every layer and a first guess of
   which it has; writers in phase 4 confirm or change it.

## Evidence rules

- Ownership cites the code that writes the table (`file:line`) or the
  screens that do.
- A row two modules could own is a question, not a coin toss.

## Refuter

Check `Map coverage`: no open rows. Pick ten map rows at random and check
that their owner writes their tables. Walk the dependency list: no module
depends on one in a later group.

## Return

Modules and groups, dependencies, disputed rows, questions added.
