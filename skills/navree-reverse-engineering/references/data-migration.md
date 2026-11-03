# Old and dirty data

How to move years of an old product's data into the new one without
losing rows, inventing values or hiding problems. In order: profile the
data, write cleaning rules from the profile, decide what happens to each
kind of bad row, build an import that can run again safely, then reconcile
counts and totals until they match. The spec that holds the decisions is
`spec-migration.md`.

## Ground rules

- Work on a restored copy, through a database user that can only read.
  Never connect the import to the live database.
- Never drop a row silently. Every row ends up imported, merged into
  another (with both old ids kept), or in the rejects table with a reason.
- Keep every old id. The new rows get new keys; the old key goes into a
  `legacy_id` column or an id map, so you can trace, re-run and compare.
- Decisions that change meaning (merging duplicates, guessing a currency,
  dropping orphans) are the owner's: put them in `QUESTIONS.md`.
- Rehearse the full import on a fresh copy until it reconciles, and time
  it; the cut-over plan depends on how long it takes.

## 1. Profile first

Run each query on the copy, save it as `specs/02-base/queries/SQL-<n>.sql`
with its output in a comment below it, and write the finding in
`DATA_PROFILE.md`. The queries are PostgreSQL; MySQL differences are noted.

Size and emptiness of every column of a table:

```sql
-- SQL-001: rows, nulls and empty strings per column (one query per column; generate them from information_schema)
SELECT 'customers.email' AS col,
       COUNT(*)                                         AS row_count,
       COUNT(*) FILTER (WHERE email IS NULL)            AS nulls,
       COUNT(*) FILTER (WHERE TRIM(email) = '')         AS blanks,
       COUNT(DISTINCT LOWER(TRIM(email)))               AS distinct_values
FROM customers;
-- MySQL: SUM(email IS NULL) and SUM(TRIM(email) = '') instead of FILTER.
```

Enums and lookups hiding in text:

```sql
-- SQL-017: every spelling of a status, with counts
SELECT status, COUNT(*) AS n FROM orders GROUP BY status ORDER BY n DESC;

-- SQL-018: free text that should be a lookup (few values, many rows)
SELECT LOWER(TRIM(city)) AS value, COUNT(*) AS n
FROM customers GROUP BY 1 ORDER BY n DESC LIMIT 50;
```

Duplicates by the natural key people use:

```sql
-- SQL-040: customers sharing an email inside a branch
SELECT branch_id, LOWER(TRIM(email)) AS email, COUNT(*) AS n, MIN(id) AS keep, MAX(id) AS other
FROM customers
WHERE TRIM(COALESCE(email, '')) <> ''
GROUP BY branch_id, LOWER(TRIM(email))
HAVING COUNT(*) > 1
ORDER BY n DESC;
```

Orphans (references the database never enforced):

```sql
-- SQL-024: orders whose customer does not exist
SELECT COUNT(*) FROM orders o
LEFT JOIN customers c ON c.id = o.cust
WHERE o.cust IS NOT NULL AND c.id IS NULL;
```

Mixed formats (dates, phones, codes stored as text):

```sql
-- SQL-019: date formats in a text column
SELECT CASE
         WHEN odate ~ '^\d{4}-\d{2}-\d{2}$'       THEN 'Y-m-d'
         WHEN odate ~ '^\d{1,2}/\d{1,2}/\d{4}$'   THEN 'd/m/Y or m/d/Y'
         WHEN odate IS NULL OR TRIM(odate) = ''    THEN 'empty'
         ELSE 'other'
       END AS format,
       COUNT(*) AS n
FROM orders GROUP BY 1 ORDER BY n DESC;
-- MySQL: odate REGEXP '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'.
-- For 'd/m/Y or m/d/Y', split by branch or user: the first part above 12 settles it.
```

Money stored as floating point, or totals that don't add up:

```sql
-- SQL-022: orders whose stored total differs from the sum of their lines
SELECT o.id, o.total, SUM(l.qty * l.unit_price) AS lines_total
FROM orders o JOIN order_lines l ON l.order_id = o.id
GROUP BY o.id, o.total
HAVING ABS(o.total - SUM(l.qty * l.unit_price)) > 0.005;
```

Rows that should not come at all, and the old server's clock:

```sql
-- SQL-030: test and soft-deleted rows
SELECT COUNT(*) FILTER (WHERE email LIKE '%@test.%' OR name ILIKE 'test%') AS test_like,
       COUNT(*) FILTER (WHERE deleted = 1)                                AS soft_deleted
FROM customers;

-- SQL-061: the zone the old database wrote local times in
SHOW timezone;                                -- MySQL: SELECT @@global.time_zone, @@system_time_zone;
```

Text that was stored in the wrong encoding shows as `Ã©` for `é` or `â€™`
for `’`. Search for those pairs (`WHERE name LIKE '%Ã%'`) and record the
count; the cleaning rule re-decodes such values once.

## 2. Cleaning rules

Write one rule per finding in `CLEANING.md`. The usual kinds:

| Finding | Usual rule | Needs the owner? |
|---|---|---|
| Spelling variants of an enum | map each variant; unknown values to a flagged default | only for unknown values |
| Blank strings meaning "none" | blank to null | no |
| Values with spaces or case drift | trim; lower case for emails and codes | no |
| Phone numbers in many formats | parse to E.164 with the tenant's country; unparseable kept in a note and flagged | yes, the default country |
| Dates in several formats | parse by a known format per source (branch, user, era); ambiguous ones rejected | yes, when ambiguous |
| Local times without a zone | read in the old server's zone (or the tenant's), store UTC | yes, confirm the zone |
| Money as floating point | multiply to minor units and round half up; recompute totals from lines where lines exist and compare | yes, when stored and computed totals differ |
| Duplicates by natural key | merge into one row, repoint references, keep every old id | yes |
| Orphans | quarantine, or attach to a placeholder parent | yes |
| Free text that should be a lookup | build the lookup from the distinct values; map the text; the rest to "Other" with the original kept | yes, the list |
| Soft-deleted and test rows | leave out or import as archived | yes |
| Values out of range | reject the row, or clamp and flag | yes |

A rule names its detection query, its fix, the rows it touches on the copy
and, when it changes meaning, the owner's answer.

## 3. Rejected rows

Every row that fails a rule that says `reject` or `quarantine` goes into
a rejects table in the new database, with the old table, the old id, the
rule id and the reason. People fix rejects in the old data (on the next
copy) or by hand in the new product, then the next run picks them up.
Report rejects per rule after each run in `MIGRATION_RUN.md`; a rule
rejecting far more rows than the profile predicted is a bug in the rule.

For data that arrives as spreadsheets rather than a database (price lists
kept in Excel, a CSV export from a hosted tool), use `navree-sheets`'
import pipeline instead of a command: it detects encodings, maps headers,
validates each row with the row type's rules, stages rows for a preview,
applies them with a duplicate mode (`skip`, `update`, `fail`) and writes an
error file of the rejected rows (`navree-sheets` skill).

## 4. An import that can run again

The import runs many times: rehearsals, then the real cut-over, sometimes
a final delta. Make every run safe to repeat:

- **Idempotent by old id.** Before creating a row, look up its `legacy_id`;
  if it exists, skip it (or update it, when the rule says the old product
  is still the source of truth until cut-over). A unique index on
  `legacy_id` (per tenant) backs this up.
- **Ordered and batched.** Read the old table ordered by its key, in
  batches (500 to 5 000 rows), each batch in its own transaction. A crash
  loses at most one batch, and the next run resumes past the last key
  imported.
- **In dependency order.** Parents before children (the load order in
  `MIGRATION.md`); children find their parent's new id through the parent's
  `legacy_id` or an id map table.
- **Pure transforms.** The cleaning rules are plain functions from an old
  row to a new value or a rejection, so tests can feed them real old rows.
- **A dry run.** The same run, rolled back at the end of every batch,
  printing what it would create and reject.

### The Navree side

Point a named connection at the copy with a read-only user (the
`navree-database` skill), and write the import as a console command per
table in the module that owns it (`navree make:command invoices
import_legacy`):

```toml
# config/app.toml (or APP_DATABASE__CONNECTIONS__LEGACY__URL in the environment)
[database.connections.legacy]
url = "mysql://reader:secret@127.0.0.1:3307/old_app_copy"
max_connections = 2
```

```rust
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use navree::db::{Db, Executor as _, Row, Statement};
use navree::kernel::{AppContext, BoxError, CommandSpec};
use navree::orm::Model;
use serde::Serialize;
use uuid::Uuid;

use super::models::invoice::{Invoice, InvoiceStatus};

/// A row the import refused, kept for people to fix (table `legacy_rejects`).
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "legacy_rejects", timestamps(updated_at = false))]
pub struct LegacyReject {
    pub id: Uuid,
    pub source_table: String,
    pub legacy_id: i64,
    pub rule: String,
    pub reason: String,
    pub created_at: DateTime<Utc>,
}

/// Old ids to new ones, for parents that children look up (table `legacy_ids`).
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "legacy_ids")]
pub struct LegacyId {
    pub id: Uuid,
    pub source_table: String,
    pub legacy_id: i64,
    pub new_id: Uuid,
}

/// `legacy:import-invoices [--dry-run]`.
pub fn import_invoices_command() -> CommandSpec {
    CommandSpec::new(
        "legacy:import-invoices",
        "Copy invoices from the legacy database ([--dry-run])",
        import_invoices,
    )
}

async fn import_invoices(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    if let Some(unknown) = args.iter().find(|arg| *arg != "--dry-run") {
        return Err(format!("unknown option `{unknown}` (expected --dry-run)").into());
    }
    let db = ctx.get::<Db>().await?;
    let legacy = db.connection("legacy").await?;
    // Branches became workspaces in an earlier step, which filled legacy_ids.
    let branches: HashMap<i64, Uuid> = LegacyId::query()
        .where_(LegacyId::SOURCE_TABLE.eq("branches"))
        .get(&db)
        .await?
        .into_models()
        .into_iter()
        .map(|row| (row.legacy_id, row.new_id))
        .collect();
    for (branch, workspace) in branches {
        // Workspace-owned models (`#[model(workspace)]`) read and write in this workspace.
        let (created, rejected) =
            navree::orm::workspace::scope(workspace, copy_branch(&db, &legacy, branch, dry_run))
                .await?;
        println!("branch {branch}: {created} created, {rejected} rejected");
    }
    Ok(())
}

async fn copy_branch(db: &Db, legacy: &Db, branch: i64, dry_run: bool) -> Result<(u64, u64), BoxError> {
    let (mut after, mut created, mut rejected) = (0_i64, 0_u64, 0_u64);
    loop {
        let rows = legacy
            .fetch_all(&Statement::portable(
                "SELECT id, inv_no, total, status FROM invoices WHERE branch_id = ? AND id > ? ORDER BY id LIMIT 500",
                [branch.into(), after.into()],
            ))
            .await?;
        let Some(last) = rows.last() else { break };
        after = last.get::<i64>("id")?;
        let uow = db.uow(None, None).await; // one transaction per batch
        let batch = uow
            .scope(async {
                let (mut made, mut refused) = (0_u64, 0_u64);
                for row in &rows {
                    let legacy_id = row.get::<i64>("id")?;
                    if Invoice::query().where_(Invoice::LEGACY_ID.eq(legacy_id)).exists(&uow).await? {
                        continue; // imported by an earlier run
                    }
                    match invoice_from(row) {
                        Ok(invoice) => {
                            Invoice::create(&uow, invoice).await?;
                            made += 1;
                        }
                        Err((rule, reason)) => {
                            let reject = LegacyReject {
                                source_table: "invoices".into(),
                                legacy_id,
                                rule: rule.into(),
                                reason,
                                ..LegacyReject::default()
                            };
                            LegacyReject::create(&uow, reject).await?;
                            refused += 1;
                        }
                    }
                }
                Ok::<_, BoxError>((made, refused))
            })
            .await;
        match batch {
            Ok((made, refused)) => {
                if dry_run {
                    uow.rollback().await?;
                } else {
                    uow.commit().await?;
                }
                created += made;
                rejected += refused;
            }
            Err(error) => {
                uow.rollback().await?;
                return Err(error);
            }
        }
    }
    Ok((created, rejected))
}

/// The cleaning rules of CLEANING.md for one old row: a new invoice, or the rule it broke.
fn invoice_from(row: &Row) -> Result<Invoice, (&'static str, String)> {
    let legacy_id = row.get::<i64>("id").map_err(|e| ("C-0", e.to_string()))?;
    let number = row
        .get::<Option<String>>("inv_no")
        .map_err(|e| ("C-4", e.to_string()))?
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .ok_or(("C-4", "no invoice number".to_owned()))?;
    let total = row
        .get::<Option<f64>>("total")
        .map_err(|e| ("C-8", e.to_string()))?
        .ok_or(("C-8", "no total".to_owned()))?;
    let status = match row.get::<Option<String>>("status").ok().flatten().unwrap_or_default().trim().to_lowercase().as_str() {
        "" | "new" => InvoiceStatus::Draft,
        "sent" | "open" => InvoiceStatus::Sent,
        "paid" | "done" => InvoiceStatus::Paid,
        "cancel" | "cancelled" => InvoiceStatus::Void,
        other => return Err(("C-2", format!("unknown status `{other}`"))),
    };
    Ok(Invoice {
        legacy_id: Some(legacy_id),
        number,
        total_cents: (total * 100.0).round() as i64, // C-8: half away from zero, to pence
        currency: "GBP".into(),
        status,
        ..Invoice::default()
    })
}
```

- Register the command in the module's `wire` (`m.command(import_invoices_command())`)
  and the two tables' models with their migrations. `legacy_rejects` and
  `legacy_ids` belong to no workspace: classify them with
  `m.global_entity::<LegacyReject>()` and `m.global_entity::<LegacyId>()`,
  or `navree check` reports them (`navree-module-conformity`).
- `navree::orm::workspace::scope` makes `#[model(workspace)]` models read
  and write in that workspace: the lookup by `legacy_id` stays inside it and
  `create` fills `workspace_id`.
- The import writes through the unit of work, so the audit trail records
  the created rows with no actor (`db.uow(None, None)`); pass a system
  user's id instead if the owner wants the import named.
- Test `invoice_from` with real old rows from the profile (ids and shapes,
  not personal data), one test per cleaning rule, and test the command
  twice on the same data: the second run must create nothing.
- Lookups and reference data that every tenant shares (currencies,
  countries) are better as seeders (`navree make:seeder`), which also run
  in tests.

## 5. Reconcile

After every run, write a section in `10-verify/MIGRATION_RUN.md`:

```text
## Run 4, 2026-11-20 14:05, copy of 2026-11-19, commit a1b2c3d, 38 min

| Check (MIGRATION.md) | Old | New | Rejected | Result |
| migration.60 invoices, branch North | 24 118 | 24 118 | 0 | PASS |
| migration.61 money, North, 2025-09 | 128 433 120 | 128 433 120 | | PASS |
| migration.62 open receivables 2025-09-30 | 2 914 006 | 2 913 941 | | FAIL: 65 pence, C-8 rounding on 13 invoices |

Rejects per rule: C-2 0 (expected 0), C-4 3 (expected 3), C-7 68 (expected 68)
Second run on the same copy: 0 created, 0 rejected, 0 changed (PASS)
```

- Counts per table and per tenant, money sums per tenant and per month,
  counts per status, orphans (zero), and named records compared field by
  field. Compute the old side with SQL on the copy and the new side with
  SQL on the new database (Navigator's read-only SQL tool, the `navree db`
  client, or a test).
- A difference is never "close enough": explain it (a rule, with its rows)
  or fix the import. Tolerances in `MIGRATION.md` exist only where the
  owner approved them.
- The old product's own reports for a closed period (`spec-reporting.md`,
  known old numbers) must match the new product's reports on the imported
  data.

## 6. Rehearsals and cut-over

1. Rehearse on a fresh copy after every change to the import, until a run
   reconciles with no unexplained difference. Record the duration.
2. Agree the cut-over with the owner (stop 5): when the old product goes
   read-only, the final copy, how long the import takes, who checks the
   reconciliation, and when the new product opens.
3. If the old product must keep running during a long import, import
   everything up to a timestamp first, then a final delta of rows changed
   since (the old tables need a reliable "changed at" column for that; if
   they lack one, plan a freeze instead).
4. Rollback: the old product stays intact and read-only until the owner
   accepts the new one; switching back means pointing users at it again.
