# Template: CRUD

`04-specs/<module>/crud.md` describes each record the module keeps: what it
holds, what is valid, which control edits each field, where a field's
options come from, and who may do what to it. Old products leave most of
this implicit: a text box where a date belongs, a select filled from a
hard-coded array, validation only in JavaScript. Write what the old product
does, then what the new one does, and link every change to its catalog
entry in `mistakes.md`.

## Format, one section per entity

```text
## <Entity> (old table <table>, map <TB-id>)

| Property | Value | Source | Known by | Conf. | Status |
| New model | <Model> in module <module>, table <table> | | | | |
| Key | <old key> -> UUID v7 `id`; old key kept as `legacy_id` | | | | |
| Natural key | <columns people use to find it; unique per tenant?> | | | | |
| Tenant | workspace-owned / shared / library | | | | |
| Deletes | hard / soft (kept, hidden) / never (archived by status) | | | | |
| Audit | change events through the unit of work: yes / no | | | | |

### Fields

| ID | Field | Old column | Type | Required | Default | Rules | Control | Options from | Shown in | Edited by | Source | Known by | Conf. | Status |

### Operations

| ID | Operation | Who | Rules beyond the fields | Side effects | Old behaviour | Source | Known by | Conf. | Status |

### Integrity

| ID | Rule | Kind | Source | Known by | Conf. | Status |
```

## The columns

- **Field**: the new name, snake case. **Old column**: `table.column`, or
  `computed`, or `none` for a new field.
- **Type**: the new type in plain words: `text(200)`, `long text`,
  `integer`, `money (minor units, currency field <x>)` (minor units are
  whole cents or pence, so sums are exact), `decimal(10,4)`,
  `date`, `date-time (UTC)`, `boolean`, `enum <Name>`, `reference to
  <Entity>`, `file`, `json`. Never `float` for money or quantities that must
  add up.
- **Required**: `yes`, `no`, or `when <condition>` (then the rule goes to
  the service spec). Base it on the data profile: a column that is empty in
  12% of old rows can't be required on import without a cleaning rule.
- **Rules**: one per line, from this list, so the builder can map them to
  `#[validate]`: `length(min, max)`, `range(min, max)`, `email`, `url`,
  `phone (E.164)`, `one of <values>`, `pattern <regex>`, `unique (<scope>)`,
  `exists <Entity> (<scope>)`, `date after <field or today>`,
  `custom: <name>` (describe it under Integrity). A rule that spans two
  records or needs the database beyond `unique`/`exists` is a service rule:
  write `see <module>.service.<n>`.
- **Control**: what edits it on a form. Choose by type and size:

| Field | Control |
|---|---|
| short text, name, code | text input (max length from the rule) |
| long text, notes | textarea; rich text only when the old product kept formatting |
| email, phone, URL | email, phone (with country), URL inputs; never plain text |
| integer, decimal | number input with step and range |
| money | money input in the currency's units, stored as minor units |
| date, date-time, time | date, date-time or time picker; date-times show the viewer's zone |
| boolean | checkbox, or a switch when it takes effect at once |
| enum, up to 4 values | radio group |
| enum, 5 or more values, fixed | select |
| reference to a small table that rarely grows (under ~50 rows) | select, options loaded from the server |
| reference to a large or growing table | combobox that searches on the server as the user types |
| several values from a fixed set | checkboxes or a multi-select |
| file, image | file input with accepted types and a size limit |

- **Options from**: where a select, radio group or combobox gets its
  choices: `enum <Name>` (fixed in code), `lookup <table>` (a small
  table, loaded from an endpoint), `search <endpoint>` (server-searched,
  paged), `setting <key>` (changes per tenant), `depends on <field>`
  (cascading: the endpoint takes the parent value). Never a list typed into
  a template or script; if the old product did that, record the old list
  and the catalog entry.
- **Shown in**: `list`, `detail`, `form`, `export`, `hidden` (never sent to
  the browser: secrets, internal flags).
- **Edited by**: the roles that may change this field when it differs from
  the record's update rule (a credit limit only a manager edits). This feeds
  `ROLES.md` and becomes a field rule in the abilities.
- **Operations**: create, read, update, delete, restore, duplicate,
  archive, import, export, and any custom action the record has (`send`,
  `void`). **Who** names roles and record scope ("sales rep: own customers
  only"). **Side effects**: what else changes (a stock movement, a
  notification, an audit entry, a job).
- **Integrity**: uniqueness across fields, cross-field rules ("due date not
  before issue date"), invariants ("an invoice total equals the sum of its
  lines"), and what must never happen ("a paid invoice is never edited").
  **Kind**: `database` (a unique index, a foreign key, a check), `rule`
  (validation), `service` (needs the service spec).

## Worked example

```text
## Invoice (old table invoices, map TB-018)

| Property | Value | Source | Known by | Conf. | Status |
| New model | Invoice in module invoices, table invoices | | | | confirmed |
| Key | invoices.id int -> UUID v7 id; old id kept as legacy_id | SCHEMA.md schema.61 | code | high | confirmed |
| Natural key | number, unique per branch (old: unique overall, but branches reuse numbers after 2019) | SQL-031 | data | high | confirmed |
| Tenant | workspace-owned (a branch is a workspace) | MODULES.md | inferred | medium | question Q-001 |
| Deletes | never; void by status. Old app hard-deleted drafts | invoices/delete.php:12 | code | high | confirmed |

### Fields
| ID | Field | Old column | Type | Required | Default | Rules | Control | Options from | Shown in | Edited by | Source | Known by | Conf. | Status |
| invoices.crud.3 | customer_id | invoices.cust | reference to Customer | yes | | exists Customer (same workspace) | combobox, server search by name, code, email | search GET /api/v1/customers?search= | list, detail, form | | invoices/create.php:40 (a <select> of all 9 811 customers) | code | high | confirmed |
| invoices.crud.4 | issued_on | invoices.idate (varchar) | date | yes | today in the workspace's zone | date | date picker | | list, detail, form, export | | invoices/create.php:58 (text box, no check) | code | high | confirmed |
| invoices.crud.5 | due_on | invoices.due | date | yes | issued_on + customer's terms | date after issued_on | date picker | | list, detail, form | | js/invoice.js:112 (computed only in the browser) | code | high | confirmed |
| invoices.crud.6 | total_cents | invoices.total (float) | money (minor units, currency field currency) | yes | computed | range(min 0) | read-only, computed from lines | | list, detail, export | nobody | js/invoice.js:140 (sum in the browser) | code | high | confirmed |
| invoices.crud.7 | status | invoices.status | enum InvoiceStatus: draft, sent, paid, overdue, void | yes | draft | one of the enum | none on the form; changed by actions | enum InvoiceStatus | list, detail, export | see invoices.workflow.* | SQL-033 (7 spellings) | data | high | confirmed |
| invoices.crud.8 | payment_method | invoices.paymeth | enum PaymentMethod: bank_transfer, card, cash, cheque | no | | one of the enum | radio group | enum PaymentMethod | detail, form | accountant | invoices/edit.php:77 (hard-coded <option>s, 'cheque' missing in edit) | code | high | confirmed |
| invoices.crud.9 | notes | invoices.notes | long text | no | | length(max 5000) | textarea | | detail, form | | invoices/edit.php:90 | code | high | confirmed |

### Operations
| invoices.crud.20 | void | accountant, manager | only when status is sent or overdue; reason required | status void, credit note created, audit entry | old: any user, any status, via ?action=void | invoices/actions.php:30 | code | high | confirmed |
```

## Checks before the file is done

- Every column of the module's tables in `SCHEMA.md` has a field row or a
  `dropped` row with a reason.
- Every field has a control, and every select, radio or combobox an
  **Options from** that is not a hard-coded list.
- Every money field is minor units or decimal with a currency; no `float`.
- Every required field was checked against the data profile's null rate.
- Every client-side rule found in the old code appears as a server rule or
  a service rule.
- Every operation names roles and record scope, and every one of those
  appears in `ROLES.md`.
- The file ends with `## Questions`, `## Deliberate changes` and
  `## New map rows` (formats in `spec-module.md`); an empty one says
  "none".
