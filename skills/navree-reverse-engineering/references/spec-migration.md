# Template: data migration

Phase 6 writes `06-migration/MIGRATION.md` (what goes where, in which
order, and how to prove it arrived) and `06-migration/CLEANING.md` (every
cleaning rule and what happens to rows that fail it). Both build on the
data profile: a rule with no profile row behind it is a guess. The method,
the profiling queries and the Navree side are in `data-migration.md`.

## MIGRATION.md

```text
# Migration of <product>

Source: <the copy, its date>; the production snapshot at cut-over: <how and when>
Target: <the Navree app, its database>
Tool: <console command(s), import pipeline, or both> (see data-migration.md)

## Load order

| Step | Target model | From | Depends on | Rows in | Rows out (expected) | Rejected (expected) |

## <Target model> (from <old table(s)>)

### Rows

| ID | Rule | Rows | Source | Known by | Conf. | Status |
(which old rows come in: all, not test customers, not soft-deleted older than ...)

### Fields

| ID | New field | From | Transform | Cleaning rule | If it fails | Source | Known by | Conf. | Status |

## Ids

How old ids map to new ones (the legacy id column, the id map table), and how
references between tables are rewired.

## Checks after each run

| ID | Check | Old query | New query | Must match | Tolerance |

## Re-runs and cut-over

What a second run does; the freeze; the final delta; the rollback.
```

- **Rows in / out / rejected**: from the profile, before the first run.
  A rehearsal that differs from these numbers is a finding even when
  nothing failed.
- **From**: `table.column`, several columns, a lookup through another
  table, a constant, or `generated`.
- **Transform**: exact and testable: "trim, lower case", "parse with the
  branch's date format, then the branch's zone to UTC", "FLOAT pounds x 100,
  rounded half up to an integer of pence", "map via lookup
  `order_status_map`".
- **Cleaning rule**: the `CLEANING.md` id.
- **If it fails**: `reject the row`, `set default <x> and flag`, `set null
  and flag`, `quarantine` (load into a holding table for a person to fix),
  or `stop the run`. Never silently drop.
- **Checks after each run**: row counts per table and per tenant, sums of
  money per tenant and per month, counts per status, distinct natural
  keys, orphans (must be zero), and a handful of named records compared
  field by field. **Must match** says exactly or by how much and why.

## CLEANING.md

```text
| ID | Rule | Applies to | Detect | Fix | Rows (profile) | Decision | Source | Known by | Conf. | Status |
```

- **Detect**: the query (or `SQL-` id) that finds the rows.
- **Fix**: what the import does with them, step by step.
- **Rows (profile)**: how many rows it touches on the copy.
- **Decision**: the owner's answer (`Q-` id) when the fix changes meaning
  (merging duplicate customers, dropping orphans, guessing a currency).

## Worked example

```text
## Load order
| 1 | Workspace | branches | | 3 | 3 | 0 |
| 2 | Customer | customers | 1 | 9 811 | 9 642 | 0 (169 merged as duplicates, M-3) |
| 3 | Invoice | invoices | 2 | 61 204 | 61 204 | 0; 311 flagged status-unknown |
| 4 | Payment | payments | 3 | 58 870 | 58 802 | 68 (orphans, M-7) |

## Invoice (from invoices)
| migration.20 | number | invoices.inv_no | trim; prefix branch code when it clashes inside the branch | C-4 | reject the row | SQL-031 | data | high | confirmed |
| migration.21 | issued_on | invoices.idate | parse 'Y-m-d', 'd/m/Y' (North, South) or 'm/d/Y' (West) | C-6 | reject the row | SQL-019 | data | medium | question Q-009 |
| migration.22 | total_cents | invoices.total | round(total x 100), half up | C-8 | stop the run (a total can't be guessed) | SQL-022 | data | high | confirmed |
| migration.23 | status | invoices.status | lower, trim, map via order_status_map; '' -> draft and flag | C-2 | set draft and flag | SQL-033 | data | high | confirmed |

## Checks after each run
| migration.60 | invoices per branch | SELECT branch_id, COUNT(*) FROM invoices GROUP BY branch_id | per workspace count of invoices | exactly | 0 |
| migration.61 | money per branch per month | SUM(ROUND(total*100)) by branch, month of idate | SUM(total_cents) by workspace, month of issued_on | exactly | 0 |
| migration.62 | open receivables | old ageing report, 2025-09-30 | new ageing report, same date | exactly | 0 |

## CLEANING.md
| C-3 | Duplicate customers | customers | same lower(trim(email)) within a branch (SQL-040) | merge into the oldest; repoint orders and invoices; keep both legacy ids | 169 pairs | Q-005: merge | SQL-040 | data | high | confirmed |
| C-7 | Payments without an invoice | payments | invoice_id not in invoices (SQL-044) | quarantine in legacy_rejects with the reason | 68 | Q-006: quarantine, finance reviews | SQL-044 | data | high | confirmed |
```

## Checks before the files are done

- Every column of every migrated table has a field row, or is listed as
  not migrated with a reason.
- Every profile finding that affects a migrated column has a cleaning
  rule.
- Every cleaning rule says what happens to failing rows; none drops rows
  silently.
- Expected counts in, out and rejected are filled in before the first run.
- Every check has an old query and a new query that can run as written.
