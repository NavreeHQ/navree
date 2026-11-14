---
name: navree-helm
description: "Helm, the admin panel of navree-helm: the `helm` facade feature (in `full`), `HelmModule` and the `helm` config section, `navree make:resource <module> <Model>`, the `Resource` trait over a navree-orm model (LABEL, SINGULAR, GROUP, ICON, TITLE, SEARCH, SLUG, PER_PAGE, DEFAULT_SORT, ORDER, READ_ONLY, EXPORT, SIDEBAR_COUNT, COUNT_CAP, WORKSPACES, fields, filters, actions, metrics, query, create, update, delete), `Field`, `Filter`, `Action` / `ActionContext` / `ActionResult`, `Metric`, `HelmPage` and `m.helm_page(..)`, `SaveContext` and `FormData::parse`, file fields on storage disks, the access rules every screen checks (`helm.view`, then read, create, update, delete and action abilities on each record and field), the audit entries, sign-in, CSRF and the strict CSP, and testing through `/admin/api`. Use when adding an admin panel or back office, exposing a model at `/admin`, adding a filter, bulk action, metric card or custom admin page, saving admin edits through a service, or debugging a resource missing from the menu, a 403 or 404 in the panel, a boot error naming a resource, or a field shown read-only."
---

# Helm: the admin panel

`navree::helm` (crate `navree-helm`, facade feature `helm`, part of `full`)
serves an admin panel at `/admin` generated from your navree-orm models.
Each model (an access subject: `#[model(subject = "Deal")]`) gets a
**resource**: a provider implementing `Resource` that says
which fields to show and edit, how to filter and search, which actions and
metrics it has. Helm draws list, detail, create and edit screens on the
server with Navree UI (its app shell, a `DataTable` and forms described at run
time from each resource, no front-end build) and a JSON API under `/admin/api`
with the same checks.

When in doubt, read the code: `crates/navree-helm/src` (start at
`resource.rs`, `field.rs`, `action.rs`, `metric.rs`), its tests in
`crates/navree-helm/tests/helm/` and `crates/navree/tests/helm.rs` (the docs
page's examples through the facade). The page is
`docs/tools/helm.mdx`.

## Steps: add the panel to an app

1. The app root needs `DatabaseModule`, `CryptoModule`, `AccessModule` (every
   check) and the app's browser sign-in: `SessionModule`, `AuthModule`,
   `UiModule` and `AuthPages` (the panel has no sign-in screen of its own; a
   guest is sent to `/sign-in?next=..`). Add Helm last:

```rust,ignore
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(CryptoModule::from_config())
    .module(HttpModule::from_config())
    .module(DatabaseModule::from_config())
    .module(SessionModule::from_config())
    .module(AuthModule::from_config())
    .module(UiModule)
    .module(AuthPages::new().app_name("Agra CRM"))
    .module(AccessModule::for_root(access()))
    // ... feature modules
    .module(HelmModule::from_config()) // section `helm`; or HelmModule::for_root(HelmOptions { .. })
```

   Helm has no tables. `ViewModule` is needed only for `Field::view` and
   custom pages, `StorageModule` only for `Field::file`, `QueueModule` only
   to run `.queued()` actions as jobs (without it they run in the request).

2. Let people in: `view` on the subject `Helm` (permission `helm.view`).

```sh
navree access:grant manager helm.view
```

   Super-admins pass every check. Everything else depends on the user's
   abilities on each resource's model.

3. Generate a resource per model and check what it wrote:

```sh
navree make:model deals Deal --policy    # the model, an access subject (models/deal.rs)
navree make:resource deals Deal          # module, model (Deal or deal); --path src/modules
```

   It reads `src/modules/deals/models/deal.rs` and writes
   `src/modules/deals/resources/deal.rs` (a field per column: money when
   there is a `currency` column, read-only timestamps, a comment suggesting
   `Field::belongs_to` for each `*_id`), declares `mod resources;` and
   registers it at the `// navree:wire` marker with
   `navree::helm::ModuleBuilderHelmExt::resource::<..>(m)`. If the model is
   no access subject, it prints what to add (`subject` in its
   `#[model(..)]`): Helm needs one. Only an ORM model is a resource.

4. Open `http://127.0.0.1:8080/admin` and sign in with an app account. When
   the account has two-factor authentication on, the panel asks for the code
   after the password: the same check as the API (a six-digit
   `navree::crypto::totp` code, 30-second steps with one step of drift, each
   code accepted once, or a recovery code). In a test, compute the code from
   the secret `AuthService::enable_two_factor` returned with
   `Totp::builder(Secret::from_base32(&secret)?).build()?.generate(now)` and
   move a `FixedClock` to the next step between codes.

## Resource template

```rust,ignore
use navree::helm::{Action, ActionResult, Field, Filter, Metric, Resource};
use navree::orm::Builder;
use navree::prelude::*;

use crate::modules::deals::models::deal::Deal;
// #[derive(Model, Clone, Debug, Default, Serialize)]
// #[model(table = "deals", subject = "Deal", permission_prefix = "deals", soft_deletes)]

#[injectable]
pub struct DealResource;

impl Resource for DealResource {
    type Model = Deal;                          // a navree-orm model that is a subject

    const LABEL: &'static str = "Deals";        // plural, menu and headings
    const SINGULAR: &'static str = "Deal";      // defaults to LABEL
    const GROUP: &'static str = "Sales";        // menu group; "" = "Resources"
    const ICON: &'static str = "handshake";     // Lucide name; unknown = folder + boot warning
    const TITLE: &'static str = "title";        // names a record; default "id"
    const SEARCH: &'static [&'static str] = &["title"];
    // SLUG (default: table name with '-'), PER_PAGE (0 = helm.per_page),
    // DEFAULT_SORT ("-amount_cents"; default -created_at or -pk), ORDER (i32),
    // WORKSPACES (Workspaces::Current by default; Every spans workspaces),
    // READ_ONLY (true: no New/Edit/Delete shown, writes refused, and actions
    // with the create, update or delete ability, the default, refused too),
    // EXPORT (true: CSV of the list), SIDEBAR_COUNT (true: a count in
    // the menu; false for a table whose count is not cheap), COUNT_CAP
    // (10_000: lists count rows only up to this, then say `10,000+`).

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("title").required().max_length(200).sortable(),
            Field::money("amount_cents", "currency").label("Amount").min(0).sortable(),
            Field::select("stage").options([("lead", "Lead"), ("won", "Won")]).badge(),
            Field::belongs_to::<CompanyResource>("company_id").label("Company"),
            Field::date("expected_close_date").sortable(),
            Field::computed("probability", |d: &Deal| format!("{}%", d.probability)),
            Field::datetime("created_at").readonly().sortable(),
        ]
    }

    fn filters(&self) -> Vec<Filter<Deal>> {
        vec![
            Filter::select("stage").options([("lead", "Lead"), ("won", "Won")]),
            Filter::date_range("expected_close_date"),
            Filter::boolean("open", "Open deals only", |query, on| {
                if on { query.where_(Deal::CLOSED_AT.is_null()) } else { query }
            }),
        ]
    }

    fn actions(&self) -> Vec<Action<Deal>> {
        vec![Action::new("mark-won", "Mark as won")
            .ability(Move)                       // an ActionMarker; default Update
            .confirm("Mark the selected deals as won?")
            .handle(|ctx, deals: Vec<Record<Deal>>| async move {
                let service = ctx.get::<DealService>().await?;
                let count = deals.len();
                for deal in deals {
                    service.win(ctx.uow(), ctx.user_id(), deal).await?;
                }
                Ok(ActionResult::message(format!("{count} deals marked as won.")))
            })]
    }

    fn metrics(&self) -> Vec<Metric<Deal>> {
        vec![                                    // typed columns or column names
            Metric::value("Open pipeline").sum(Deal::AMOUNT_CENTS)
                .filter(Deal::CLOSED_AT.is_null()).money("USD").on_dashboard(),
            Metric::trend("New deals").count().by_day(Deal::CREATED_AT, 30),
            Metric::partition("Deals by stage").count().group_by(Deal::STAGE),
        ]
    }

    fn query(&self, query: Builder<Deal>) -> Builder<Deal> {
        query.where_(Deal::OWNER_ID.is_not_null()) // never widens access
    }
}
```

Register it in its module's `wire`: `m.resource::<DealResource>();` (the
prelude brings `ModuleBuilderHelmExt`). That also registers the model's
subject with navree-access (`AccessModule::for_feature`). Every screen reads
`Deal::query()` (its global scopes apply: soft-deleted rows are hidden
without a `query`), then `query`, then the user's access filter.

## Fields, filters, actions, metrics (what exists)

- **Fields:** `id`, `text`, `textarea`, `markdown` (no raw HTML), `number`,
  `money(col, currency_col)` (minor units; `USD` when blank), `boolean`,
  `select` (+ `.options(pairs)`, `.badge()`), `date`, `datetime` (shown and typed
  on the app's clock, `CurrentZone` with `TimeModule`, else UTC; stored UTC), `email`, `url`, `file(col, disk)`
  (+ `.accept([..])`, `.max_size(bytes)`, 10 MiB default), `belongs_to::<R>`,
  `has_many::<R>(foreign_key)` (a panel on the detail page; `R` needs a
  `belongs_to` on that key; several may share a key if they list different
  resources), `computed(name, f)`, `view(name, template)`.
  Modifiers: `.label`, `.help`, `.sortable`, `.readonly`, `.hide_on_index`,
  `.hide_on_detail`, `.hide_on_forms`, `.only_on_forms`, `.hide_on_export`, `.required`, `.min`,
  `.max`, `.min_length`, `.max_length`, `.pattern`, `.default`.
- **Filters:** `select(col).options(..)`, `belongs_to::<R>(col)`,
  `boolean(name, label, f)`, `date_range(col)` (`a..b` or presets),
  `custom(name, label, f)`, `.label(..)`. Each is a button in the filter bar over
  the list's table (checkboxes, any/yes/no, from and to, or a text box), with a
  chip for each filter in force and Clear filters. URL form:
  `?search=..&filter[stage][in]=won,lost&filter[open]=true&filter[close][gte]=2026-10-01&filter[min][contains]=5&sort=-amount_cents&page=2`.
- **The list's table:** search, the filter bar, a button over the ticked rows for
  each action (only actions the user may run and the workspace role takes; one with
  a question or fields asks on a page of its own), a menu on each row (Open, Edit,
  Delete as the user may),
  **Export** (`GET .../export.csv`, the list's rows as CSV with a UTF-8 byte order
  mark, a chunk of 500 at a time, the user's access read again on each, ended by
  `helm.export_max_secs`, formulas quoted, audited as `helm.export` and
  `helm.export.finished` with the rows sent; `.hide_on_export()` keeps a field
  out) and counts in the menu
  (`GET /admin/counts`, filled by the script, cached per user and workspace,
  capped at `1,000+`).
- **Actions:** `Action::new(name, label)` + `.ability(M)`, `.confirm(text)`,
  `.destructive()`, `.fields(vec![..])` (read with `ctx.input::<T>(name)` /
  `ctx.inputs()`), `.only_on_detail()`, `.only_on_index()`, `.queued()`
  (`helm.action` job, access rechecked when it runs), `.handle(f)`.
  Results: `ActionResult::message`, `::redirect(url)`, `::download(url)`.
- **Metrics:** `Metric::value | trend | partition(label)` then `.count()`,
  `.sum/.average/.min/.max(col)`, `.filter(cond)`, `.money(cur)`,
  `.over(date_col)` (range picker 7/30/90 days, month, year; a delta badge
  only when the period before has rows to compare with),
  `.by_day/.by_week/.by_month(col, n)`, `.group_by(col)`, `.on_dashboard()`.
  A metric's URL name is its label in kebab case (`open-pipeline`).

## Saving through your services

By default Helm writes form values to the columns as ORM statements in the
request's unit of work: each value goes through `Model::set_attribute`
(casts and mutators), a create starts from a model whose fields are at their
types' defaults (a field type without `Default` means: override `create`),
gets a generated key and `created_at` / `updated_at` (when the model has
them), an update writes the changed columns and sets `updated_at`, a delete
soft-deletes a soft-deleting model. Model hooks and observers don't run on
these writes; each records its change event with the record's serialized
form, so `#[model(hidden)]` and `#[serde(skip_serializing)]` fields never
reach observers or the audit log. Override to run your service and DTO:

```rust,ignore
use navree::BoxFuture;
use navree::helm::{FormData, RecordOf, SaveContext};

fn update<'a>(&'a self, ctx: &'a SaveContext, deal: RecordOf<Self>, form: FormData)
    -> BoxFuture<'a, Result<RecordOf<Self>>> {
    Box::pin(async move {
        let input: UpdateDeal = form.parse().await?;   // validated, errors under fields
        ctx.get::<DealService>().await?.update_model(ctx.uow(), ctx.user_id(), deal, input).await
    })
}
```

`create(ctx, form)` and `delete(ctx, record)` work the same way
(`RecordOf<Self>` is `Record<Self::Model>`). A service that saves with
`record.save(uow)` / `record.update(uow, ..)` runs the model's hooks, and its
change events carry every column: write secret-holding tables with
statements and record the event yourself. An `AppError` 400 with field
errors shows under the fields; others as a message.

## Custom pages

```rust,ignore
m.helm_page(
    HelmPage::new("pipeline", "Pipeline board")
        .group("Sales").icon("kanban")
        .template("admin/pipeline")            // an app template: needs ViewModule
        .can("read", "Deal")
        .data(|ctx| async move { Ok(serde_json::json!({ "columns": [] })) }),
);
```

Served at `/admin/pages/pipeline`; the template gets `title`, `user`,
`csrf_token` (the session's token, masked afresh for each page, for a form's
`_csrf` field), `path` and `data`. No inline scripts: the CSP allows scripts
from the app's origin only.

## Testing template

```rust,ignore
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use serde_json::json;

async fn boot_app() -> TestApp {
    TestApp::builder()
        .temp_database()                           // a SQLite file: dev-dependency feature `sqlite`
        .config("app.key", navree::crypto::generate_key())
        .fake_auth()                               // acting_as next to AuthModule
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(access()))
        .module(DealsModule)
        .module(HelmModule::from_config())
        .build()
        .await
}

#[tokio::test]
async fn reps_edit_only_their_own_deals() {
    let app = boot_app().await;
    // create users with a role that has `helm.view`, and deals (your helpers)
    let rep = app.acting_as(principal(priya_id));
    rep.get("/admin/api/resources/deals").query(&[("search", "Gulati")]).send().await
        .assert_status(200).assert_json_path("meta.total", 1.into());
    rep.patch(&format!("/admin/api/resources/deals/{toms_deal}"))
        .json(&json!({ "title": "Mine now" })).send().await
        .assert_error(403, None);
}
```

API routes: `GET /admin/api/resources` (metadata), `GET|POST
/admin/api/resources/{r}`, `GET|PATCH|DELETE /admin/api/resources/{r}/{id}`,
`GET /admin/api/resources/{r}/options?search=`, `POST
/admin/api/resources/{r}/actions/{action}` (`{"ids", "input"}`), `GET
/admin/api/resources/{r}/metrics/{metric}?range=`. Bearer tokens need no CSRF
token.

## Checks before you finish

- The app boots: Helm validates every resource at boot (unknown column in a
  field, a metric or `TITLE`, bad `pattern`, `belongs_to` to an unregistered resource,
  action without `.handle`, trend without a period, duplicate slug).
- `helm.view` is granted to the roles that need the panel, and each role
  has `read` (and `create`, `update`, `delete`, action names) on the subject.
- A test acts as a restricted user and asserts a 403 on what they may not
  change: hiding a button is never the protection, the API checks too.
- Browsers send the session's CSRF token (`_csrf` in a form, or the
  `X-CSRF-Token` header); never add `/admin/*` to `session.csrf_except`, as the
  session layer is the only check.

## Gotchas

- **A resource is missing from the menu:** the user lacks `read` on its
  subject (a type-level check), or the module never called
  `m.resource::<R>()`.
- **"Choose a workspace first" (400):** the model is `#[model(workspace)]`
  and the request is in no workspace. The panel works in one workspace at a
  time: the unit of work's (entered by the app), else the workspace header
  (`workspace.header`, `X-Workspace` by default) or the cookie that the
  sidebar's form sets. `Workspaces::Every` on the resource spans workspaces
  (the database role decides what it sees and what a create may make; under
  row-level security, only a role that bypasses it sees every workspace).
  An action's handler on such a resource runs in no workspace unless one was
  named: scope its writes with `navree::orm::workspace::scope`.
- **Naming a workspace gets a 403:** a named workspace is entered only for a
  member, through `WorkspaceModule`, or for whoever
  `m.helm_staff_workspaces(|entry| async move { Ok(..) })` lets in (explicit,
  and audited as `helm.workspace.staff`). Being a super-admin is not enough.
  Code `helm_workspace_unchecked` means the app has neither, so nothing can
  check. A cookie for a workspace the user may not enter counts for nothing
  (400, `Choose a workspace first`). Under database isolation the panel can't
  enter a workspace at all. Abilities in the panel are app-wide, and a role
  narrows them without granting any.
- **A write gets a 403, `workspace_read_only`:** the member's role can't write:
  `viewer` and roles of the app's own, whatever they hold app-wide, in queued
  actions and across workspaces too. `workspace.roles.<role>.permissions` decides
  for the models it names (`notes.update` lets a viewer update notes,
  `notes.read` holds a member to reading them); the user still needs the
  app-wide ability. `demo_read_only` is a read-only demo, for staff and across
  workspaces too. `helm_workspace_partitions` is a member limited to partitions.
- **`token_workspace_mismatch`:** a credential limited to a workspace
  (`PinnedWorkspace`) stays in it, staff or not; it names it when the request
  doesn't, and `Workspaces::Every` is scoped to it.
- **Settings screens:** the permission says which types, the level whose: a
  workspace's or organization's are for its members, a user's for that user.
  Across tenants only through `helm_staff_workspaces` and
  `helm_staff_organizations`, audited, reads included.
- **A queued action gets a 403 at once:** in a workspace the app entered itself,
  the job can't ask the app again, so it asks `WorkspaceModule` and
  `helm_staff_workspaces` when the action is requested.
- **The staff audit:** `helm.workspace.staff` entries go on the log's own
  connection and stay when the request or job fails.
- **Buttons that are not there:** the panel hides what the user can't do. A
  resource with `READ_ONLY`, a role that only reads in the workspace, a
  read-only demo and an ability the user lacks all remove New, Edit, Delete and
  the bulk actions from the page; the server still refuses a write made by hand.
  Export, counts and lists are reads and stay.
- **A record is a 404, not a 403:** rows the access filter hides don't exist
  for that user, like `Accessible<M, Read>` in the API.
- **A hidden column is hidden everywhere.** `#[model(hidden)]` and a
  `cannot(READ, ..).fields([..])` rule reach no page, list, API answer, picker,
  related column, form, template or export, and take the column out of sort,
  search, filters, `via` and metrics (a 400, or a 403 for a metric). A title falls
  back to the label and short key. A `#[model(hidden)]` field is write-only. A
  filter's closure says what it reads with `.reads([..])`, or runs only for a user
  with nothing hidden. `make:resource` makes no field for a hidden column or one
  named like a secret.
- **A field is greyed out on edit:** a `.fields([..])` or `cannot(..)` rule
  keeps it; sending it anyway is a 403. A field sent that isn't editable is a
  400 (`property x should not exist`).
- **Action handler type errors:** when the closure calls methods on the
  records, annotate them: `|ctx, deals: Vec<Record<Deal>>|`.
- **`Resource::Model` doesn't satisfy `SubjectRecords`:** the model is no
  access subject; add `subject` (and `permission_prefix`) to its
  `#[model(..)]`.
- **"Helm cannot make a new X from a form":** a field's type has no
  `Default` (an enum without `#[default]`, a custom type); give it one or
  override `create`.
- **Sign-in refused:** the app's sign-in page (`AuthPages`) asks the app's
  `SignInPolicy`s as for `POST /api/auth/login` (a domain that enforces single
  sign-on can't use a password), and the login throttle counts the same
  attempts. Signing out, a password change and offboarding end the session on
  the server.
- **Action records skipped:** records the user can't act on are left out
  (`skipped` in the result); none left is a 403.
- **File fields** store `{slug}/{random}.{ext}` on the field's disk after
  sniffing the content; the JSON API accepts only such paths, not uploads.
- **Money** is in minor units in `values`, `FormData` and `min`/`max`.
- **Metrics** are cached per user, access version and range for
  `helm.metrics_cache_secs` (60; `0` turns it off): a changed number can lag.
  The sidebar's counts share that setting (per user, access and workspace).
- **Messages name a field by its label** (`Project should not be empty`); a
  record with no `TITLE` value is `Report #3f9a1c2e`, not a UUID.
- **Config:** `helm.path` (`/admin`), `title`, `logo`, `theme` (a Navree UI theme), `per_page`
  (1 to 100), `metrics_cache_secs`, `dashboard`, `export_max_secs`; env
  `APP_HELM__TITLE`, `APP_HELM__PER_PAGE`.
- **The look** follows Navree's app UI rules (sidebar shell, 4px corners,
  ink selection, green only for primary actions, focus and positive data).
  Don't add inline styles or scripts to templates: the CSP blocks them.
