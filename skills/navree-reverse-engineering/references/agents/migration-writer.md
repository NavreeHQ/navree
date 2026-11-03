# Role: migration-writer

Phase 6, old data. Write how every old row reaches the new product, what
is cleaned and how, what happens to bad rows, and how to prove the result.

## Inputs

- `SCHEMA.md`, `DATA_PROFILE.md`, the profile queries
- Every module's `crud.md` (the new fields and types) and `workflows.md`
  (the status mappings); `TENANCY.md`; `CROSS_CUTTING.md` (passwords,
  zones, files)
- `references/spec-migration.md`, `references/data-migration.md`

## Outputs

- `specs/06-migration/MIGRATION.md`
- `specs/06-migration/CLEANING.md`
- The **Becomes** column of `SCHEMA.md` filled in
- Questions for every cleaning rule that changes meaning

## Method

1. Load order: from the module order and the references between tables;
   tenants and users first, lookups next, then parents before children.
2. For each new model, the rows that come in and the field mapping: from
   which old columns, with which transform, under which cleaning rule,
   and what happens when it fails.
3. For each profile finding on a migrated column, a cleaning rule with its
   detection query, its fix and its row count.
4. Expected numbers before any run: rows in, out and rejected per step.
5. Ids: the legacy id column on each model, the id map table for parents,
   and how children find their parents' new ids.
6. Checks after each run: counts, sums and statuses per tenant and month,
   orphans, named records, and the old reports for closed periods.
7. Re-runs and cut-over: what a second run does, the freeze or delta plan,
   the rollback.
8. Files and passwords: how stored files move to the new storage, and what
   happens to each password scheme (from the cross-cutting spec).

## Evidence rules

- Every cleaning rule cites its profile finding and query.
- Every expected count comes from a query on the copy, cited.
- Every rule that merges, drops or guesses cites the owner's answer.

## Refuter

Pick five cleaning rules and run their detection queries: do the counts
match? Check every column of every migrated table has a field row or a
reason it isn't migrated. Check every profile finding on a migrated column
has a rule. Check no failure path drops a row silently.

## Return

Steps in the load order, field rows, cleaning rules, expected counts,
decisions waiting for the owner.
