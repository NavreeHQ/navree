# Role: metrics-writer

Phase 4, the reporting layer, one module per dispatch (usually the module
that owns the reports, plus any module with a dashboard). Pin down every
number people read, so the new product computes it once and the parity
test can check it.

## Inputs

- Scope: one module.
- The module's `RP` rows in `MAP.md`; `MODULE.md`; the CRUD and service
  specs of the modules whose data the reports read.
- The old reporting code (pages, scripts, SQL, spreadsheet exports) and
  the running copy.
- `references/spec-reporting.md`, `references/mistakes.md` (entries 9, 11)

## Outputs

- `specs/04-specs/<module>/reports.md`
- The reports line of `MODULE.md`

## Method

1. For each report, find where it computes: SQL, server code or the
   browser. Write each metric's exact definition, including what it
   leaves out.
2. Find its dimensions, time grain and the zone its days fall in. A
   report grouped by the server's or the database's day is a deliberate
   change to the tenant's zone, with the owner's approval.
3. Find who sees it and whose rows they see.
4. Read known old numbers: run the report on the copy for closed periods
   (a past month, a past year) and record the figures with a screenshot or
   capture. Prefer periods that span a clock change and a month end.
5. Note reports that disagree with each other for the same metric, and
   reports that disagree with the data (recompute one with SQL): each is
   a question.
6. Estimate cost: rows read per run at today's volume; mark reports that
   need a summary table or a TimescaleDB continuous aggregate.

## Evidence rules

- Every metric cites the code that computes it.
- Every known old number has its parameters and its evidence file.
- A disagreement cites both numbers and both sources.

## Refuter

Recompute one known old number with SQL on the copy from the written
definition. Check every report states its zone and its scope per role.
Check every metric used by two reports is one row.

## Return

Metrics and reports written, known old numbers captured, disagreements,
reports that need summaries, questions added.
