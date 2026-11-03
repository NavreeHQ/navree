# Role: schema-reader

Phase 2, base layer. Write down the database as it really is: tables,
columns, keys, the relations the code relies on, enums and lookups.

## Inputs

- `specs/01-map/MAP.md` (the `TB` rows)
- The database copy (read-only user) and the old code.
- `references/spec-base.md` (the `SCHEMA.md` format)

## Outputs

- `specs/02-base/SCHEMA.md`
- `specs/02-base/queries/SQL-*.sql` for every query you ran
- New `TB` rows in `MAP.md` under "Added later", if any table was missed

## Method

1. Dump the schema: `information_schema.columns`, `table_constraints`,
   `key_column_usage`, indexes, check constraints, and the engine's own
   view (`SHOW CREATE TABLE` on MySQL, `pg_dump --schema-only` on
   PostgreSQL). Save the dump in `evidence/`.
2. For every table and column, write the row in `SCHEMA.md` with its
   declared type, nullability and default.
3. Find relations the database doesn't declare: search the code for joins
   and lookups on `*_id` columns and similar names (`cust`, `parent`),
   then confirm with a query that the values match a key
   (`SELECT COUNT(*) ... WHERE NOT EXISTS ...`): a relation with orphans is
   still a relation, and the orphan count goes to the profiler.
4. Find enums and lookups: MySQL `ENUM` types, check constraints, lookup
   tables, code constants, and columns with few distinct values
   (`SELECT col, COUNT(*) ... GROUP BY col`). For each, record every value
   and where its label comes from.
5. Note the database's own logic: triggers, procedures, views, events
   (`mistakes.md`, entry 16). Each is a map row (`JB` or `EP`) and a lead for
   the service writer.

## Evidence rules

- Every row cites the dump, a query (`SQL-`) or a `file:line`.
- A relation known only by its name is `inferred` until a query confirms
  it.
- Values seen are counts and shapes, never personal data.

## Refuter

Pick five tables at random: compare their rows in `SCHEMA.md` with a fresh
`SHOW CREATE TABLE` or `\d`. Pick five relations marked `code`: open the
cited joins. Search the code for any `JOIN` on a column with no relation
row.

## Return

Tables and columns written, relations by kind (declared, code, naming),
enums found, database logic found, tables added to the map.
