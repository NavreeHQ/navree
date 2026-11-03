# Template: base layer (schema and data profile)

Phase 2 writes two files. `SCHEMA.md` says what the database is.
`DATA_PROFILE.md` says what the data in it really looks like. Both are
facts about the old product, not plans for the new one; the new schema
comes later, in the CRUD specs and the migration spec.

The worked example throughout these templates is an old PHP order and
invoicing app on MySQL 5.7, used by a wholesaler with three branches.

## SCHEMA.md

```text
# Schema of <product>

Database: <engine and version>, copy restored from <backup name and date>
Commands: <how the lists below were produced, with counts>

## Tables

| ID | Table | Rows | Purpose | Key | Owner module | Source | Known by | Conf. | Status |

## Columns, one section per table

### <table>

| ID | Column | Type | Null | Default | Meaning | Values seen | Becomes | Source | Known by | Conf. | Status |

## Relations

| ID | From | To | Declared | Cardinality | On delete | Source | Known by | Conf. | Status |

## Enums and lookups

| ID | Name | Where it lives | Values | Labels | Source | Known by | Conf. | Status |
```

What each column means:

- **Rows**: `SELECT COUNT(*)` on the copy, with the query id.
- **Key**: the primary key, and the natural key people use (an invoice
  number, an email). The natural key drives duplicate checks and re-runnable
  imports.
- **Type** is the declared type. If the data disagrees (a `VARCHAR` holding
  dates), say so in **Meaning** and point at the profile row.
- **Values seen**: for small sets, the distinct values; else a sample
  range. Never paste personal data.
- **Becomes**: left empty here. The CRUD spec fills it with the new field
  or `dropped`.
- **Declared**: `fk` when the database has a foreign key; `code` when only
  the code joins on it (cite the join); `naming` when only the name says so
  (`customer_id`), which is `inferred` until a query shows the values match.
- **Where it lives** for an enum: a check constraint, a MySQL `ENUM`, a code
  constant, a lookup table, or free text with a few repeated values.
- **Labels**: what the screens show for each value, with the source of the
  label (a translation file, a `switch` in a template).

## DATA_PROFILE.md

```text
# Data profile of <product>

Copy: <backup name and date>. Rows profiled: <total>. Queries: queries/SQL-001.sql ...

## Per table

| ID | Table.column | Finding | Measure | Query | Effect on the new product | Known by | Conf. | Status |

## Findings by kind

nulls | duplicates | orphans | mixed formats | free text that should be a lookup |
dates stored badly | money stored badly | time zones | soft-deleted and test rows |
encodings | outliers
```

- **Finding**: one fact, in a sentence ("12% of `customers.email` are
  empty strings, 3% hold two addresses separated by `;`").
- **Measure**: the number, with the total it is out of.
- **Query**: the `SQL-` id; the file in `queries/` holds the query and its
  output.
- **Effect on the new product**: what the specs must decide (a nullable
  column, a cleaning rule, a lookup table, a question). The migration writer
  turns these into rules.

## Worked example

```text
### orders (TB-014)

| ID | Column | Type | Null | Default | Meaning | Values seen | Becomes | Source | Known by | Conf. | Status |
| schema.41 | id | int unsigned | no | auto | key | 1..218 455 | | SHOW CREATE TABLE orders | code | high | confirmed |
| schema.42 | cust | int | yes | NULL | the customer; no foreign key | 1..9 811 | | app/models/Order.php:31 joins customers.id | code | high | confirmed |
| schema.43 | status | varchar(20) | yes | 'new' | order status | new, NEW, open, shipped, Shipped, done, cancel, cancelled, '' | | SQL-017 | data | high | confirmed |
| schema.44 | total | float | yes | 0 | order total incl. tax | -12.5 .. 98 211.07 | | SQL-021 | data | high | confirmed |
| schema.45 | odate | varchar(30) | yes | NULL | order date | '2014-03-02', '02/03/2014', '2 Mar 14' | | SQL-019 | data | high | confirmed |

## Relations
| schema.88 | orders.cust | customers.id | code | many to one | none (rows stay) | app/models/Order.php:31 | code | high | confirmed |

## Data profile
| profile.12 | orders.cust | 1 204 orders point at no customer | 1 204 of 218 455 | SQL-024 | orphan rule needed; question Q-007 | data | high | confirmed |
| profile.13 | orders.status | 9 spellings of 5 states | 218 455 rows | SQL-017 | lookup with a mapping; '' is 311 rows | data | high | confirmed |
| profile.14 | orders.total | stored as FLOAT; 0.3% differ from the sum of lines by more than 0.01 | 655 of 218 455 | SQL-022 | money in minor units; recompute or keep? Q-008 | data | high | confirmed |
| profile.15 | orders.odate | 3 formats; day and month order differ by branch | 3 formats | SQL-019 | parse per branch; question Q-009 | data | medium | question |
```

## Checks before the files are done

- Every table in `MAP.md` has a section, and every column a row.
- Every relation the code joins on is listed, declared or not.
- Every enum lists all values seen, including bad spellings and blanks.
- Every finding has a measure and a query id, and its query is saved with
  its output.
- No personal data copied into the spec: counts and shapes, not values.
