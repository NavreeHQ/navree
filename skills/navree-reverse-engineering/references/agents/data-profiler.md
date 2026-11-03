# Role: data-profiler

Phase 2, base layer. Measure what the old data really holds, so the
specs and the migration are built on numbers, not hopes.

## Inputs

- `specs/02-base/SCHEMA.md` (run after the schema reader, or beside it on
  the tables it has finished)
- The database copy, through the read-only user.
- `references/spec-base.md` (the `DATA_PROFILE.md` format) and
  `references/data-migration.md` (the queries)

## Outputs

- `specs/02-base/DATA_PROFILE.md`
- `specs/02-base/queries/SQL-*.sql`, each with its output pasted below it
  as a comment

## Method

For every table in `SCHEMA.md`:

1. Row count; for each column, nulls, blanks and distinct values.
2. Enum-like columns: every value with its count, including spellings and
   blanks.
3. Natural keys: duplicates (exact, and after trim and lower case).
4. Relations: orphans in each direction.
5. Text columns holding dates, numbers, phones, emails or codes: the
   formats present, with counts.
6. Money and quantities: the type, totals that don't match their lines,
   negative and extreme values.
7. Dates: the range, values in the future or before the company existed,
   and the zone they were written in (`SQL-061`).
8. Free text that repeats (top 50 values): candidates for lookups.
9. Soft-deleted rows, test rows, and rows owned by deleted parents.
10. Encoding damage (`Ã©`, `â€™`) and control characters.

Write one finding per row, with its measure and the effect on the new
product. Big tables: run the queries on the full copy, not a sample; if a
query is too slow, say so and use a sample with its size stated.

## Evidence rules

- Every finding has a query id, and the query file holds its output.
- Measures are numbers with their totals ("1 204 of 218 455").
- No personal data in the profile or in pasted outputs: aggregate, or
  show ids only.

## Refuter

Re-run five queries chosen at random and compare outputs. For every enum
column in `SCHEMA.md`, check the profile has its value counts. For every
relation, check its orphan count. Look for a finding stated without a
measure.

## Return

Findings by kind with their counts, the largest problems for the
migration, and questions added (`Q-`).
