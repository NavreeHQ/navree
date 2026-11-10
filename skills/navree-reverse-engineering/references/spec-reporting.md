# Template: reporting

`04-specs/<module>/reports.md` describes every number people read to run
the business: dashboards, report pages, monthly PDFs, spreadsheets
exported for the accountant. Old products often compute these in the
browser from a full download, or in a script whose rules nobody wrote
down, and two reports of the same thing disagree. The spec pins down each
metric so the new product computes it once, on the server, and the parity
test can compare it with the old number.

## Format

```text
## Metrics

| ID | Metric | Definition | Source rows | Filter | Aggregation | Unit and rounding | Source | Known by | Conf. | Status |

## Reports

| ID | Report | Metrics | Dimensions | Time grain | Time zone | Period | Who | Format | Refresh | Old form | Source | Known by | Conf. | Status |

## Known old numbers

| ID | Report | Parameters | Old value | Taken on | Evidence |
```

## The columns

- **Definition**: in words, then exactly ("revenue: sum of invoice totals
  excluding tax, invoices in sent, overdue or paid, by issue date"). Say
  what is excluded: void, test customers, internal transfers.
- **Source rows**: tables and the spec rows of the fields used.
- **Aggregation**: count, distinct count, sum, average, minimum, maximum,
  ratio of two metrics (say which is computed first), running total.
- **Dimensions**: what the report breaks down by (branch, customer, sales
  rep, product group). Each must be a field in a CRUD spec.
- **Time grain**: day, week (which day starts it), month, quarter, year.
  **Time zone**: whose day it is: the branch's, the user's, UTC. A report
  by day in UTC shows evening sales on the wrong day.
- **Period**: presets and custom ranges; whether the current period is
  partial.
- **Who**: roles and scope; a sales rep's report holds only the rep's
  rows, the same scope as the lists.
- **Format**: screen (table, chart, figure), PDF, CSV or Excel.
- **Refresh**: live on each request, cached for a time, or a summary
  table refreshed by a worker. Choose by cost: a report over years of rows
  should read a summary, not the raw table.
- **Known old numbers**: values read from the old product for fixed
  parameters (a closed month). They become parity tests with exact
  expected values, so take them from a period that no longer changes.

## Worked example

```text
## Metrics
| reports.report.1 | Revenue | sum of invoice totals excluding tax; status sent, overdue or paid; by issued_on | invoices (invoices.crud.6, .7, .4) | not test customers (customers.is_test, Q-016) | sum | cents, shown with currency | reports/monthly.php:40 | code | high | confirmed |
| reports.report.2 | Days sales outstanding | open receivables / revenue of the last 90 days x 90 | invoices, payments | | ratio | days, 1 decimal | js/dashboard.js:88 (computed in the browser from all invoices) | code | high | confirmed |

## Reports
| reports.report.10 | Monthly sales by customer | revenue, invoice count | customer, month | month | branch zone (old: server, UTC) | last 12 months; any month | manager, accountant; sales rep: own customers | screen, xlsx | live; summary table if over 2 s on the profile volume | page + spreadsheet export | reports/monthly.php | code | high | confirmed |

## Known old numbers
| reports.report.30 | Monthly sales by customer | branch North, 2025-09 | revenue 1 284 331.20 GBP, 2 114 invoices | 2026-10-30 | SHOT-090, CAP-091 |
```

## Checks before the file is done

- Every `RP` map row owned by the module has a report row.
- Every metric has an exact definition with exclusions.
- Every report states its time zone and its scope per role.
- Two reports that show the same metric use the same metric row.
- Every report has at least one known old number from a closed period, or
  a reason it can't (the old report is broken: then a question).
- The file ends with `## Questions`, `## Deliberate changes` and
  `## New map rows` (formats in `spec-module.md`); an empty one says
  "none".
