# Template: lists

`04-specs/<module>/lists.md` describes every table of records a user sees:
what it shows, how it filters, sorts, searches and pages, what can be done
to several rows at once, and how it exports. Old products often load the
whole table into the browser and filter, sort and page there; that breaks
as data grows, leaks rows the user may not see, and makes exports differ
from the screen. In the new product every list runs on the server: one
query applies the user's access, the tenant, the filters, the sort and the
page.

## Format, one section per list

```text
## <List name> (screen <SC-id>, old endpoint <EP-id>)

| Property | Value | Source | Known by | Conf. | Status |
| Rows | <entity>, scope: <whose rows: all, own, team, branch> | | | | |
| Old behaviour | server / client paging; rows loaded; size today | | | | |
| Default sort | <column, direction>, then a unique column | | | | |
| Page | size <n>, choices <n, n>, offset or cursor | | | | |
| Search | columns searched, match rule (contains, starts with, exact) | | | | |
| Empty state | text and action when there are no rows / no matches | | | | |

### Columns

| ID | Column | Field or computation | Sortable | Format | Hidden by default | Visible to | Source | Known by | Conf. | Status |

### Filters

| ID | Filter | Field | Kind | Options from | Default | Source | Known by | Conf. | Status |

### Bulk actions

| ID | Action | Who | Rule per row | Confirm | Runs | Source | Known by | Conf. | Status |

### Export

| ID | Format | Columns | Rows | Limit | Who | Source | Known by | Conf. | Status |
```

## The columns

- **Rows**: the entity and the record scope. The scope must match
  `ROLES.md`: a sales rep's list holds only the rows the rep may open.
- **Old behaviour**: `client` when the old screen fetched all rows (cite
  the request and its size, `CAP-` id) and paged, sorted or filtered in the
  browser. Each such list gets a deliberate-change row in `MODULE.md`.
- **Default sort** ends with a unique column (the key), so pages never
  repeat or skip rows.
- **Page**: offset paging (`page`, `per_page`, a total) for screens people
  page through; cursor paging for feeds and very large tables where a
  total is too costly.
- **Column / Field or computation**: a field from the CRUD spec, a related
  field (`customer.name`, loaded with the rows, never one query per row),
  or a computation with its service row id. Counts and sums come from the
  database (`load_count`, a grouped query), not from the browser.
- **Format**: money with currency, date in the viewer's zone, enum label,
  badge, link.
- **Visible to**: roles that may see this column when not everyone may
  (cost price, margin). Hidden means not sent, not just not drawn.
- **Filter kind**: `select` (one value), `multi` (several), `range`
  (numbers, money), `date range` (with presets), `boolean`, `text`,
  `reference` (a combobox searched on the server). **Options from** follows
  the CRUD rules: an enum, a lookup endpoint, or a search endpoint.
- **Bulk actions**: **Rule per row** says which selected rows the action
  may touch (access and state); the server re-checks every row and reports
  the rows it skipped. **Runs**: `in the request` for small sets, `queued`
  with progress for large ones.
- **Export**: the same query as the screen (same filters, same access),
  `csv` or `xlsx`, **Limit** in rows; above it, queued with a download link.

## Worked example

```text
## Invoices (screen SC-040, old endpoint EP-051 /invoices/list.php)

| Property | Value | Source | Known by | Conf. | Status |
| Rows | Invoice; accountant and manager: all of the branch; sales rep: invoices of own customers | ROLES.md roles.12, roles.14 | code | high | confirmed |
| Old behaviour | client: list.php returns all 61 204 invoices of the branch as JSON (4.8 MB); DataTables pages and sorts in the browser | CAP-021, js/invoices.js:20 | running | high | confirmed |
| Default sort | issued_on descending, then id | js/invoices.js:24 (order: [[2,'desc']]) | code | high | confirmed |
| Page | 25; choices 25, 50, 100; offset with total | js/invoices.js:22 | code | high | confirmed |
| Search | number (starts with), customer name and code (contains) | js/invoices.js:30 (searches every column, cost price included) | code | high | confirmed |
| Empty state | "No invoices yet" with New invoice; with filters: "No invoices match" with Clear filters | SHOT-040 shows a blank table | running | high | confirmed |

### Columns
| invoices.list.1 | Number | number | yes | link to detail | no | all | SHOT-040 | running | high | confirmed |
| invoices.list.2 | Customer | customer.name (eager loaded) | yes | link to customer | no | all | js/invoices.js:40 | code | high | confirmed |
| invoices.list.3 | Total | total_cents | yes | money, currency | no | all | SHOT-040 | running | high | confirmed |
| invoices.list.4 | Margin | invoices.service.6 | yes | percent | yes | manager | js/invoices.js:55 (computed in the browser, sent to everyone) | code | high | confirmed |

### Filters
| invoices.list.10 | Status | status | multi | enum InvoiceStatus | all but void | SHOT-041 | running | high | confirmed |
| invoices.list.11 | Customer | customer_id | reference | search GET /api/v1/customers?search= | | js/invoices.js:70 (select of 9 811 names) | code | high | confirmed |
| invoices.list.12 | Issued | issued_on | date range | presets: this month, last month, this year | | SHOT-041 | running | high | confirmed |

### Bulk actions
| invoices.list.20 | Send reminders | accountant | status is overdue | yes, with count | queued, progress shown | js/invoices.js:90 | code | high | confirmed |

### Export
| invoices.list.30 | csv, xlsx | the visible columns | the filtered rows | 10 000 in the request, more queued | accountant, manager | list.php?export=1 (all rows, ignores filters) | code | high | confirmed |
```

## Checks before the file is done

- No list keeps paging, sorting or filtering in the browser.
- Every list states its record scope, and the scope matches `ROLES.md`.
- Every reference filter searches on the server.
- Every related column is loaded with the rows, not one query per row.
- Every computed column cites its service row.
- Export uses the same filters and access as the screen.
- The file ends with `## Questions`, `## Deliberate changes` and
  `## New map rows` (formats in `spec-module.md`); an empty one says
  "none".
