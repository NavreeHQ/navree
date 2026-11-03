# Mistakes in old products, and their Navree form

Old products repeat the same mistakes, whatever their stack. Each entry
says what the mistake looks like, how to find it in the old code, data or
running copy, and what the new product does instead. When a spec row
fixes one, write the entry's number in the row and add the change to the
module's "Deliberate changes" table, so the owner approves it and the
parity test expects the new behaviour.

The search commands use `rg` (ripgrep) from the old code's root; adjust
the file types to the stack. A hit is a lead, not a finding: open the file
and confirm before writing the row.

The Rust below belongs to the invoices module of the worked example in the
templates. Its two models:

```rust
use chrono::{DateTime, NaiveDate, Utc};
use navree::orm::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stored as its serde name: "draft", "sent", ...
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceStatus {
    #[default]
    Draft,
    Sent,
    Overdue,
    Paid,
    Void,
}

#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(workspace, timestamps, subject = "Invoice", permission_prefix = "invoices")]
pub struct Invoice {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub customer_id: Uuid,
    pub legacy_id: Option<i64>,
    pub number: String,
    #[model(cast = "string_enum")]
    pub status: InvoiceStatus,
    pub total_cents: i64,           // money in minor units, never a float
    pub currency: String,           // ISO 4217, beside every amount
    #[model(cast = "decimal(4)")]
    pub tax_rate: Decimal,          // an exact decimal where minor units don't fit
    pub issued_on: NaiveDate,       // a calendar day: no time zone
    pub due_on: NaiveDate,
    pub sent_at: Option<DateTime<Utc>>, // an instant: stored in UTC
    pub void_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[relations]
impl Invoice {
    pub fn customer(&self) -> BelongsTo<Invoice, Customer> {
        self.belongs_to(Invoice::CUSTOMER_ID)
    }
}

#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(workspace, timestamps, soft_deletes, subject = "Customer", permission_prefix = "customers")]
pub struct Customer {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub billing_email: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}
```

## 1. Tables that page, sort and filter in the browser

**Looks like:** one request returns every row; a grid library (DataTables,
ag-Grid, a Vue or React table) pages, sorts and searches in the browser.
Slow as data grows, and every row reaches the browser, including rows and
columns the user should not see.

**Detect:**

```sh
rg -n "DataTable\(|\.DataTable\(|serverSide:\s*false|ag-grid|useTable\(|v-data-table" --type-add 'web:*.{js,ts,jsx,tsx,vue,php,html,erb,cshtml}' -t web
rg -n "\.(filter|sort|slice)\(" -g "*.{js,ts,jsx,tsx,vue}"
```

On the running copy, open the screen with the network panel: one large
response and no request when you page or sort means client side. Record the
size (`CAP-`).

**Navree form:** the list endpoint takes the page, the filters and the sort
in the query string, applies the user's access and the workspace, and
returns one page with a total (`spec-list.md`). Pages rendered on the
server draw it with navree-ui's `#[derive(TableRow)]` and `DataTable`,
whose one definition drives the query, the URL and the drawing
(the `navree-design-to-ui` skill). Front ends draw it with
`@navree/next`'s `DataTable` and `useListQuery`, `@navree/nuxt`'s
`<NavreeDataTable>`, or a Helm resource for back-office lists.

```rust
use chrono::NaiveDate;
use navree::access::{Accessible, Read};
use navree::orm::access::AccessibleQuery as _;
use navree::prelude::*;
use navree::validation::rules;
use serde::Deserialize;
use uuid::Uuid;

/// `?search=&status=&customer_id=&issued_from=&issued_to=&sort=&page=&per_page=`
#[derive(Debug, Deserialize, Validate)]
pub struct InvoiceFilters {
    #[validate(length(max = 50))]
    pub search: Option<String>,
    #[validate(skip)]
    pub status: Option<InvoiceStatus>,
    #[validate(skip)]
    pub customer_id: Option<Uuid>,
    #[validate(skip)]
    pub issued_from: Option<NaiveDate>,
    #[validate(skip)]
    pub issued_to: Option<NaiveDate>,
    #[validate(custom(rules::one_of(&["issued_on", "-issued_on", "total", "-total"])))]
    pub sort: Option<String>,
}

#[injectable]
pub struct InvoiceService;

impl InvoiceService {
    pub async fn list(
        &self,
        uow: &Uow,
        scope: Accessible<Invoice, Read>,
        filters: InvoiceFilters,
        page: PageQuery,
    ) -> Result<Page<Record<Invoice>>> {
        let query = scope
            .query() // only the rows this user may read, in the current workspace
            .when_some(filters.status, |q, status| q.where_(Invoice::STATUS.eq(status)))
            .when_some(filters.customer_id, |q, id| q.where_(Invoice::CUSTOMER_ID.eq(id)))
            .when_some(filters.issued_from, |q, day| q.where_(Invoice::ISSUED_ON.gte(day)))
            .when_some(filters.issued_to, |q, day| q.where_(Invoice::ISSUED_ON.lte(day)))
            .when_some(filters.search, |q, term| q.where_(Invoice::NUMBER.like(format!("{term}%"))));
        let query = match filters.sort.as_deref() {
            Some("issued_on") => query.order_by(Invoice::ISSUED_ON),
            Some("total") => query.order_by(Invoice::TOTAL_CENTS),
            Some("-total") => query.order_by_desc(Invoice::TOTAL_CENTS),
            _ => query.order_by_desc(Invoice::ISSUED_ON),
        };
        Ok(query
            .order_by_desc(Invoice::ID) // a unique column last: stable pages
            .with("customer")          // one query for every row's customer
            .paginate(uow, page)
            .await?)
    }
}

#[injectable]
pub struct InvoicesController {
    invoices: Arc<InvoiceService>,
}

#[controller("/invoices", version = "1")]
impl InvoicesController {
    /// A page of invoices: `{data, meta: {current_page, per_page, total, last_page}}`.
    #[get("/")]
    #[can(Read, Invoice)]
    async fn index(
        &self,
        uow: Uow,
        scope: Accessible<Invoice, Read>,
        Valid(Query(filters)): Valid<Query<InvoiceFilters>>,
        page: PageQuery,
    ) -> Result<Page<Record<Invoice>>> {
        self.invoices.list(&uow, scope, filters, page).await
    }
}
```

Skills: `navree-controllers-and-routing`, `navree-access-control`,
`navree-orm`, `navree-frontend-next` or `navree-frontend-nuxt`,
`navree-helm`.

## 2. A select filled with every row of a large table

**Looks like:** `<select>` with thousands of customers or products,
rendered on the server or filled by one request. Slow, unusable, and it
shows every record to everyone.

**Detect:**

```sh
rg -n "<select" -g "*.{php,html,erb,cshtml,blade.php,twig,jsx,tsx,vue}"
rg -n 'foreach\s*\(\s*\$\w+\s+as|\.map\(.*<option|ng-options|v-for=.*option' -g "*.{php,html,blade.php,twig,jsx,tsx,vue}"
```

For each select, find its source query and count the rows on the copy.

**Navree form:** by size and growth (`spec-crud.md`, Control):

- a small fixed set (an enum): a select, or a radio group up to 4 values,
  from the enum;
- a small table that rarely grows: a select whose options come from an
  endpoint;
- a large or growing table: a combobox that searches on the server, a page
  of matches at a time.

```rust
#[derive(Debug, Deserialize, Validate)]
pub struct OptionsQuery {
    #[validate(length(max = 100))]
    pub search: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CustomerOption {
    pub id: Uuid,
    pub label: String,
}

#[injectable]
pub struct CustomerOptionsController;

#[controller("/customers", version = "1")]
impl CustomerOptionsController {
    /// At most 20 customers whose name contains `search`, for a combobox.
    #[get("/options")]
    #[can(Read, Customer)]
    async fn options(
        &self,
        uow: Uow,
        scope: Accessible<Customer, Read>,
        Valid(Query(input)): Valid<Query<OptionsQuery>>,
    ) -> Result<Json<Vec<CustomerOption>>> {
        let customers = scope
            .query()
            .when_some(input.search, |q, term| q.where_(Customer::NAME.like(format!("%{term}%"))))
            .order_by(Customer::NAME)
            .limit(20)
            .get(&uow)
            .await?;
        Ok(Json(
            customers
                .into_models()
                .into_iter()
                .map(|c| CustomerOption { id: c.id, label: c.name })
                .collect(),
        ))
    }
}
```

In Helm, `Field::belongs_to::<CustomerResource>("customer_id")` and
`Filter::belongs_to` already search on the server
(`/admin/api/resources/{r}/options?search=`). In navree-ui pages, a select
whose choices come from the server is `FormView::options` for a small set;
a combobox that searches on the server is still coming, so a large set is
a gap for the owner (the `navree-design-to-ui` skill).

## 3. The wrong control for the type

**Looks like:** dates typed into text boxes, amounts with a currency
symbol typed in, phone numbers as free text, a status as a text box, a
foreign key as a number box, yes/no as a text field, long notes in a
one-line input, files referenced by path.

**Detect:** list every form input with its `type` and name, and compare
with the column type and the data profile:

```sh
rg -n "<input[^>]*type=\"?(text|number)\"?[^>]*name=\"?\w*(date|_at|_on|amount|price|total|phone|tel|email|status|_id)\b" -g "*.{php,html,blade.php,twig,erb,cshtml,jsx,tsx,vue}"
```

Mixed formats in the profile (`SQL-019`) are the proof that the control
let people type anything.

**Navree form:** the control table in `spec-crud.md`, with the field's
Rust type deciding what the server accepts: `NaiveDate` for a day,
`DateTime<Utc>` for an instant, minor units for money, `Phone` for a phone
number (E.164), an enum for a status, a `Uuid` reference checked with
`exists`. Helm fields follow the same types (`Field::date`, `money`,
`select`, `belongs_to`, `email`, `textarea`, `file`). In navree-ui pages
the same type picks the control: a `#[derive(Validate, Form)]` struct
draws the form, runs its rules in the browser and again on the server, and
shows each error under its field (`navree-design-to-ui`).

## 4. Validation only in the browser

**Looks like:** `required` attributes, jQuery Validate, a form library's
schema, a `maxlength`; the server saves whatever arrives.

**Detect:**

```sh
rg -n "required|maxlength|pattern=|\.validate\(|yup\.|zod|vee-validate|Validators\." -g "*.{html,js,ts,jsx,tsx,vue,php,blade.php}"
```

For each form, find the handler that saves it and look for the same
checks there. Then post the form with the browser's checks removed (or with
`curl`) on the running copy: whatever it accepts is the server's real rule
(`CAP-`).

**Navree form:** the rules live once on the request type, run on the
server for every caller, and come back as field errors (400 with `errors`).

```rust
use navree::validation::{Phone, Validate, rules};

#[derive(Debug, Deserialize, Validate)]
pub struct CreateCustomer {
    #[validate(length(min = 1, max = 200))]
    pub name: String,
    #[validate(email, length(max = 254))]
    pub billing_email: String,
    #[validate(skip)]
    pub phone: Option<Phone>, // parsed to E.164; bad numbers are a 400
    #[validate(range(min = 0, max = 100_000_000))]
    pub credit_limit_cents: i64,
    #[validate(custom(rules::one_of(&["net_14", "net_30", "net_60"])))]
    pub payment_terms: String,
}
```

Rules that need the database are `unique` and `exists` with `ValidAsync`;
rules across records belong to the service (`spec-service.md`). Skill:
`navree-validation`.

## 5. Permissions hidden in the page, not checked on the server

**Looks like:** `if ($user->role == 'admin')` around a button in a
template, a menu built from the role, and a handler that checks nothing. Or
the handler checks the role but not whose record it is, so changing an id
in the URL opens anyone's record.

**Detect:**

```sh
rg -n "role\s*==|role\s*===|isAdmin|is_admin|hasRole|can\?\(|@can|v-if=.*role|\*ngIf=.*role" -g "!*.{min.js,map}"
```

For every route that changes data, find its server-side check. On the
running copy, call each such route as the least powerful role, and with
another user's record id (`CAP-`): what succeeds is a leak.

**Navree form:** every route has an access attribute (it doesn't compile
without one); record checks load the record and apply the rules; lists
filter in SQL; the rules live in `src/abilities.rs`, as permissions on
roles plus conditions on the record's columns.

```rust
use navree::access::{AbilityCtx, ActionMarker, Authorized, Rules, field};

/// The custom action `void`: permission `invoices.void`.
pub struct Void;

impl ActionMarker for Void {
    const NAME: &'static str = "void";
}

pub fn define(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules); // invoices.void, given to the accountant and owner roles
    rules
        .cannot(Void::NAME, "Invoice")
        .when(field("status").is_in(["draft", "paid", "void"]))
        .because("Only sent or overdue invoices can be voided.");
}

#[derive(Debug, Deserialize, Validate)]
pub struct VoidInvoice {
    #[validate(length(min = 3, max = 500))]
    pub reason: String,
}

#[injectable]
pub struct InvoiceActionsController;

#[controller("/invoices", version = "1")]
impl InvoiceActionsController {
    /// 404 when the invoice is not in the user's workspace, 403 when the rules say no.
    #[post("/{id}/void")]
    #[can(Void, Invoice, from = "id")]
    async fn void(
        &self,
        uow: Uow,
        invoice: Authorized<Invoice, Void>,
        Valid(Json(input)): Valid<Json<VoidInvoice>>,
    ) -> Result<Json<Record<Invoice>>> {
        let mut invoice = invoice.into_inner();
        invoice.status = InvoiceStatus::Void;
        invoice.void_reason = Some(input.reason);
        invoice.save(&uow).await?;
        Ok(Json(invoice))
    }
}
```

The front end hides what the user can't do from the same rules
(`GET /api/me/abilities`), but the server's check is the protection.
Skills: `navree-access-control`, `navree-workspaces`.

## 6. Options and lookups typed into the code

**Looks like:** `<option value="1">Cash</option>` in a template, a
JavaScript array of statuses, a PHP array of countries, a different list
on the edit form than on the create form.

**Detect:**

```sh
rg -n "<option\s+value=" -g "*.{php,html,blade.php,twig,erb,cshtml}"
rg -n "(const|var|let)\s+\w*(STATUS|TYPE|OPTION|LIST)\w*\s*=\s*[\[{]" -i
```

Compare each list with the values the data profile found: values in the
data that no list offers mean the lists changed over time (an old value
still in rows), and the migration must map them.

**Navree form:** a fixed set is an enum in the model (`cast =
"string_enum"`), validated on input and sent to front ends from one place;
a set people change is a lookup table with its own CRUD and an options
endpoint (entry 2); a set that differs per tenant is a lookup per
workspace or a setting (`navree-settings`).

## 7. N+1 queries

**Looks like:** a list that runs one query for the rows and one more per
row for a related record (`$invoice->customer->name` in a loop, lazy
loading in a template).

**Detect:** turn on the old database's query log on the copy (`SET GLOBAL
general_log = 'ON'` on MySQL; `log_statement = 'all'` on PostgreSQL), open
the list, and count queries per page. Repeated queries that differ only by
an id are the pattern.

**Navree form:** load relations with the rows (`.with("customer")`, entry
1); count and sum related rows in the query (`load_count`, `load_sum`).
In development the ORM refuses lazy loads, and tests can assert the query
count (`navree-logbook`: `last_request().queries()`,
`duplicate_queries()`).

## 8. Money as floating point

**Looks like:** `FLOAT` or `DOUBLE` columns for prices and totals,
JavaScript numbers summed in the browser, totals that differ from the sum
of their lines by fractions of a cent, rounding done differently in two
places.

**Detect:**

```sh
rg -n "parseFloat|toFixed\(|floatval|\(float\)|round\(" -g "*.{js,ts,php,py,rb}"
```

```sql
SELECT table_name, column_name, data_type
FROM information_schema.columns
WHERE data_type IN ('float', 'double', 'real', 'double precision')
  AND table_schema NOT IN ('information_schema', 'pg_catalog', 'mysql', 'sys', 'performance_schema');
```

Then run the "totals that don't add up" query from `data-migration.md`.

**Navree form:** amounts in minor units in an `i64` with a currency column
beside them (`total_cents`, `currency` in the model above), or `Decimal`
with `cast = "decimal(N)"` where minor units don't fit (rates, unit
prices with more places). Rounding happens in one service function, with
the rule from `spec-service.md`. Helm's `Field::money(col, currency_col)`
and `Metric::money` read minor units.

## 9. Time zones

**Looks like:** `DATETIME` written with the server's local time, `NOW()`
in SQL, dates compared in the browser's zone, a nightly job at "midnight"
of the server for customers in other zones, reports grouped by UTC day.

**Detect:**

```sh
rg -n "NOW\(\)|CURRENT_TIMESTAMP|date\(\s*['\"]|new Date\(\)|DateTime\.Now|datetime\.now\(\)|Time\.now" -g "!*.min.js"
```

Find the old server's and database's zone (`data-migration.md`, SQL-061)
and how each date column was written.

**Navree form:** instants in `DateTime<Utc>`, calendar days in
`NaiveDate`; each user and workspace has a zone; reports group by the
local day of the workspace; schedules run at a local time per workspace
(`per_workspace_local`); tests use a fixed clock (`Arc<dyn Clock>`).
Skills: `navree-time-zones`, `navree-queues-and-scheduling`.

## 10. Deletes that lose history, or soft deletes that half work

**Looks like:** hard `DELETE` of records that invoices or reports still
need; or a `deleted` flag that some queries forget, so deleted customers
still appear in a dropdown or a total.

**Detect:**

```sh
rg -n "DELETE\s+FROM" -i
rg -n "deleted\s*=\s*0|is_deleted|deleted_at\s+IS\s+NULL" -i
```

Count the queries on each soft-deleting table that leave out the
condition. In the data, count references to rows that no longer exist
(orphans) and rows flagged deleted that are still referenced.

**Navree form:** `#[model(soft_deletes)]` with `deleted_at` (the
`Customer` model above): queries leave trashed rows out unless they ask
for them (`with_trashed()`, `only_trashed()`). Test that each record route
answers 404 for a trashed row, and check `deleted_at` where a rule loads a
record another way. Records that must never
go (invoices) have no delete at all: a status (`void`) instead, and the
spec says so. Skill: `navree-orm`.

## 11. Reports computed in the browser

**Looks like:** a dashboard that downloads every order and sums in
JavaScript; a chart library fed raw rows; two reports of the same figure
that disagree.

**Detect:**

```sh
rg -n "\.reduce\(|_.sumBy|_.groupBy|d3\.(sum|rollup|group)|Chart\(|new Chart|echarts" -g "*.{js,ts,jsx,tsx,vue}"
```

On the running copy, the report's request returns rows, not figures.

**Navree form:** the server computes each metric once from
`spec-reporting.md`: an aggregate query in SQL, grouped by the workspace's
local day; a TimescaleDB hypertable and continuous aggregate for large
time series; Helm `Metric`s for back-office figures; PDFs with
`navree-documents` and spreadsheets with `navree-sheets` from the same
query. Skills: `navree-database`, `navree-time-zones`, `navree-helm`.

## 12. Workers that are not idempotent

**Looks like:** a cron script that sends reminders to everyone overdue,
every time it runs; a job that charges, mails or posts with no record of
having done it; two servers running the same cron.

**Detect:** for each `JB` and `SH` row, ask "what happens if this runs
twice, or two copies run at once?" and look for the guard (a sent-at
column, a log table, a lock). Old mail logs often show double sends.

**Navree form:** a job whose effects are guarded by a record it checks and
writes in the same transaction, unique while pending, retried with
backoff, scheduled per workspace's local time.

```rust
use navree::contracts::{Backoff, Job, JobContext, JobOptions};
use navree::db::Db;

/// One reminder per customer and ISO week (table `reminder_logs`, unique (customer_id, week)).
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(workspace, timestamps(updated_at = false))]
pub struct ReminderLog {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub customer_id: Uuid,
    pub week: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendOverdueReminders {
    pub workspace_id: Uuid,
    pub week: String, // "2026-W45"
}

impl Job for SendOverdueReminders {
    const NAME: &'static str = "invoices.send_overdue_reminders";

    fn options() -> JobOptions {
        JobOptions {
            queue: "mail".into(),
            max_attempts: 5,
            backoff: Backoff::Fixed { secs: 600 },
            timeout_secs: 300,
            unique_for_secs: Some(3600), // one pending job per workspace and week
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let db = ctx.app.get::<Db>().await?;
        let uow = db.uow(None, None).await;
        let work = navree::orm::workspace::scope(self.workspace_id, uow.scope(async {
            let overdue = Invoice::query()
                .where_(Invoice::STATUS.eq(InvoiceStatus::Overdue))
                .get(&uow)
                .await?;
            for invoice in overdue.into_models() {
                let sent = ReminderLog::query()
                    .where_(ReminderLog::CUSTOMER_ID.eq(invoice.customer_id))
                    .where_(ReminderLog::WEEK.eq(self.week.clone()))
                    .exists(&uow)
                    .await?;
                if sent {
                    continue; // a retry, or another worker, already did this customer
                }
                let log = ReminderLog {
                    customer_id: invoice.customer_id,
                    week: self.week.clone(),
                    ..ReminderLog::default()
                };
                ReminderLog::create(&uow, log).await?;
                // Queue the reminder mail here: queued mail waits for the commit.
            }
            Ok::<_, BoxError>(())
        }))
        .await;
        match work {
            Ok(()) => uow.commit().await?,
            Err(error) => {
                uow.rollback().await?;
                return Err(error);
            }
        }
        Ok(())
    }
}
```

Skills: `navree-queues-and-scheduling`, `navree-mail-and-notifications`.

## 13. Integrations without retries, timeouts or signatures

**Looks like:** `curl_exec` or `requests.post` with no timeout and no
retry; an incoming webhook endpoint that trusts any caller; a failure that
is logged and forgotten.

**Detect:**

```sh
rg -n "curl_init|curl_exec|file_get_contents\(\s*['\"]https?|Http::|Guzzle|requests\.(get|post)|fetch\(|axios\.|HttpClient|RestTemplate|WebClient" -g "!*.min.js"
rg -n "webhook|callback|notify_url|ipn" -i
```

For each incoming endpoint, look for a signature or token check before the
body is used.

**Navree form:** outgoing calls through `HttpClient`: timeouts; retries of
methods that are safe to repeat (GET, PUT, DELETE and the like) on
connection errors, 5xx and 429, never of POST or PATCH and never after a
time-out; a guard that refuses calls to private and internal addresses;
a fake for tests. A call that changes something on the other side (a POST)
runs in a job, whose retries resend it with an idempotency key the other
system accepts. Incoming webhooks through `VerifiedWebhook<S>`, with
the provider's signature checked, and `mark_processed` in the same unit of
work as the handler's writes so a repeated event is handled once. Customers'
accounts on other systems through `navree-integrations` connections.
Skills: `navree-cache-and-http-client`, `navree-webhooks`,
`navree-integrations`.

## 14. Secrets in the code

**Looks like:** API keys, database passwords and SMTP passwords in config
files committed with the code, in the database's settings table as plain
text, or in front-end code.

**Detect:**

```sh
rg -n -i "(api[_-]?key|secret|password|passwd|token|private[_-]?key)\s*[=:]\s*['\"][^'\"]{8,}" -g "!*.lock"
rg -n -e "sk_live_|sk_test_|AKIA[0-9A-Z]{16}|xox[bp]-|-----BEGIN (RSA |EC )?PRIVATE KEY"
git log -p -S "password" --oneline | head -50
```

Never copy a found secret into the specs. Record where it is, and tell the
owner at stop 1: it should be rotated whatever happens to the rebuild.

**Navree form:** secrets come from the environment or `.env` (never
committed), the app key encrypts what must be stored (`cast =
"encrypted"`), and tenants' credentials for other systems live in
integration connections. Skills: `navree-config`,
`navree-security-checklist`.

## 15. The tenant condition forgotten

**Looks like:** most queries say `WHERE company_id = ?`, a few don't: a
search, an export, a report, a lookup by id.

**Detect:** for each tenant-owned table (`TENANCY.md`), list every query
that touches it and check for the condition:

```sh
rg -n -i "from\s+invoices|join\s+invoices|Invoice::(where|find|all)|invoices\." | rg -v -i "branch_id|company_id|tenant"
```

On the running copy, sign in as one tenant and request another's ids.

**Navree form:** workspace models scope every query to the current
workspace, fill `workspace_id` on create and answer 404 for another
workspace's record; `navree check` finds queries that leave the scope;
an isolation suite per module proves it. Skills: `navree-workspaces`,
`navree-module-conformity`.

## 16. Rules hidden in the database

**Looks like:** stored procedures, triggers and views that compute totals,
move statuses or write logs; the application code never mentions them.

**Detect:**

```sql
-- PostgreSQL
SELECT event_object_table, trigger_name, action_statement FROM information_schema.triggers;
SELECT routine_name, routine_type FROM information_schema.routines WHERE routine_schema NOT IN ('pg_catalog', 'information_schema');
-- MySQL
SHOW TRIGGERS; SHOW PROCEDURE STATUS WHERE Db = DATABASE(); SHOW FUNCTION STATUS WHERE Db = DATABASE();
```

**Navree form:** each becomes a service rule (`spec-service.md`) run in
the unit of work, or a model event or observer when it must run on every
save; reports become queries. Each is a map row (`EP` or `JB` with "in the
database").

## 17. Sensitive fields sent and hidden by the page

**Looks like:** a list's JSON carries cost prices, margins, personal data
or internal notes, and the page hides the columns.

**Detect:** capture the JSON of every list and detail screen as the least
powerful role and search it for the sensitive fields in `ROLES.md`
(`CAP-`).

**Navree form:** fields a role may not read are not in the response at
all: `#[model(hidden)]` for what nobody reads; for what some roles may not
read, a `cannot(READ, ..).fields([..])` rule, which Helm (its pages and its
CSV export) applies by itself, and which your own routes apply by
returning a response type with only the fields the screen needs, or by
dropping the fields the user can't read (`ability.permitted_fields(..)`,
`navree::api::mask_fields`). Spreadsheet exports you write yourself must
do the same. Skills: `navree-access-control`,
`navree-errors-and-responses` (API resources).

## 18. Exports that ignore the screen

**Looks like:** "Export" downloads every row of the table whatever the
filters and the user's access; cells starting with `=` run as formulas in
Excel; Arabic or accented text is garbled.

**Detect:** export as the least powerful role with a filter on; compare
the rows with the screen.

**Navree form:** the export runs the list's own query (same filters, same
access), through `navree-sheets`: a byte-order mark, formula escaping, a
download in the request for small files and a queued export with a signed
link for big ones (the caller chooses: `GET` or `POST` on a named export).
Helm's export reads the user's access again for every chunk.

## 19. Status values as free text or magic numbers

**Looks like:** `status = 3`, `type = 'X'`, several spellings of one state,
transitions written from many screens without checks.

**Detect:** the enum queries in `data-migration.md` (SQL-017), and every
write to the status column:

```sh
rg -n -i "status\s*=\s*['\"0-9]|set\s+status\s*=|->status\s*=|\.status\s*="
```

**Navree form:** an enum with `cast = "string_enum"` and a transition
table (`spec-workflow.md`) enforced by service methods and access rules
(entry 5); the migration maps every old spelling.

## 20. Lost updates

**Looks like:** two users open the same record, both save, the second
silently overwrites the first; two requests change a balance at once.

**Detect:** look for read-modify-write of counters and balances without a
lock or an atomic update; ask the owner whether people complain of edits
"disappearing".

**Navree form:** balances change inside the unit of work with the row
locked (`lock_for_update()`) or with a single atomic update. For edit
conflicts on forms there is no built-in check: if the owner wants one,
write a task (the form sends the `updated_at` it loaded, the service
refuses a stale save with a 409 and a code).

## 21. Files in the public web folder

**Looks like:** uploads saved under `public/uploads/` with their original
names, served to anyone with the URL; paths built from user input.

**Detect:**

```sh
rg -n 'move_uploaded_file|\$_FILES|multer|request\.files|IFormFile|upload_to='
```

**Navree form:** files on a storage disk, checked by content, served
through an access-checked route or a signed temporary link; files attached
to models through `navree-media`. Skills: `navree-storage`,
`navree-media`.

## 22. SQL built by joining strings

**Looks like:** `"SELECT ... WHERE name = '" . $_GET['q'] . "'"`.

**Detect:**

```sh
rg -n -i "(SELECT|UPDATE|DELETE|INSERT)[^;]*[\"']\s*(\.|\+)\s*[\$]?\w+"
```

Each hit on user input is also a finding to tell the owner (the old
product may be exploitable today).

**Navree form:** the query builder and `Statement` values bind every
parameter; raw SQL takes values as parameters, never in the text.

## 23. Bulk writes that skip the audit trail

**Looks like:** a mass `UPDATE` from a back-office screen or a script that
leaves no record of who changed what.

**Detect:** every `UPDATE ... WHERE` without a primary key in the old code
and scripts, and the owner's account of "who changed this?" questions they
couldn't answer.

**Navree form:** bulk actions loop over records and save them through the
unit of work, so each change records its event; a mass `query.update(..)`
records none and is kept for data fixes that the spec allows. Skills:
`navree-database`, `navree-orm`.
