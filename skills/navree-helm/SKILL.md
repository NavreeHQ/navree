---
name: navree-helm
description: "Helm, the admin panel of navree-helm: the `helm` facade feature (in `full`), `HelmModule` and the `helm` config section, `navree make:resource <module> <Model>`, the `Resource` trait over a navree-orm model (LABEL, SINGULAR, GROUP, ICON, TITLE, SEARCH, SLUG, PER_PAGE, DEFAULT_SORT, ORDER, WORKSPACES, fields, filters, actions, metrics, query, create, update, delete), `Field`, `Filter`, `Action` / `ActionContext` / `ActionResult`, `Metric`, `HelmPage` and `m.helm_page(..)`, `SaveContext` and `FormData::parse`, file fields on storage disks, the access rules every screen checks (`helm.view`, then read, create, update, delete and action abilities on each record and field), the audit entries, sign-in, CSRF and the strict CSP, and testing through `/admin/api`. Use when adding an admin panel or back office, exposing a model at `/admin`, adding a filter, bulk action, metric card or custom admin page, saving admin edits through a service, or debugging a resource missing from the menu, a 403 or 404 in the panel, a boot error naming a resource, or a field shown read-only."
---

# Helm: the admin panel

`navree::helm` (crate `navree-helm`, facade feature `helm`, part of `full`)
serves an admin panel at `/admin` generated from your navree-orm models.
Each model (an access subject: `#[model(subject = "Deal")]`) gets a
**resource**: a provider implementing `Resource` that says
which fields to show and edit, how to filter and search, which actions and
metrics it has. Helm renders list, detail, create and edit screens on the
server (navree-view templates, an embedded stylesheet and script, no front-end
build) and a JSON API under `/admin/api` with the same checks.

When in doubt, read the code: `crates/navree-helm/src` (start at
`resource.rs`, `field.rs`, `action.rs`, `metric.rs`), its tests in
`crates/navree-helm/tests/helm/` and `crates/navree/tests/helm.rs` (the docs
page's examples through the facade). The page is
`apps/website/content/docs/tools/helm.mdx`.

## Steps: add the panel to an app

1. The app root needs `DatabaseModule`, `CryptoModule` (the session cookie),
   `AuthModule` (sign-in) and `AccessModule` (every check). Add Helm last:

```rust
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(CryptoModule::from_config())
    .module(HttpModule::from_config())
    .module(DatabaseModule::from_config())
    .module(AuthModule::from_config())
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

```rust
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
    // WORKSPACES (Workspaces::Current by default; Every spans workspaces).

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
  `select` (+ `.options(pairs)`, `.badge()`), `date`, `datetime` (shown in
  the viewer's zone, stored UTC), `email`, `url`, `file(col, disk)`
  (+ `.accept([..])`, `.max_size(bytes)`, 10 MiB default), `belongs_to::<R>`,
  `has_many::<R>(foreign_key)` (a panel on the detail page; `R` needs a
  `belongs_to` on that key), `computed(name, f)`, `view(name, template)`.
  Modifiers: `.label`, `.help`, `.sortable`, `.readonly`, `.hide_on_index`,
  `.hide_on_detail`, `.hide_on_forms`, `.only_on_forms`, `.required`, `.min`,
  `.max`, `.min_length`, `.max_length`, `.pattern`, `.default`.
- **Filters:** `select(col).options(..)`, `belongs_to::<R>(col)`,
  `boolean(name, label, f)`, `date_range(col)` (`a..b` or presets),
  `custom(name, label, f)`, `.label(..)`. URL form:
  `?search=..&filters[stage]=won,lost&sort=-amount_cents&page=2`.
- **Actions:** `Action::new(name, label)` + `.ability(M)`, `.confirm(text)`,
  `.destructive()`, `.fields(vec![..])` (read with `ctx.input::<T>(name)` /
  `ctx.inputs()`), `.only_on_detail()`, `.only_on_index()`, `.queued()`
  (`helm.action` job, access rechecked when it runs), `.handle(f)`.
  Results: `ActionResult::message`, `::redirect(url)`, `::download(url)`.
- **Metrics:** `Metric::value | trend | partition(label)` then `.count()`,
  `.sum/.average/.min/.max(col)`, `.filter(cond)`, `.money(cur)`,
  `.over(date_col)` (range picker 7/30/90 days, month, year, with change),
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

```rust
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

```rust
m.helm_page(
    HelmPage::new("pipeline", "Pipeline board")
        .group("Sales").icon("kanban")
        .template("admin/pipeline")            // an app template: needs ViewModule
        .can("read", "Deal")
        .data(|ctx| async move { Ok(serde_json::json!({ "columns": [] })) }),
);
```

Served at `/admin/pages/pipeline`; the template gets `title`, `user`,
`csrf_token`, `path` and `data`. No inline scripts: the CSP allows scripts
from the app's origin only.

## Testing template

```rust
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
    rep.get("/admin/api/resources/deals").query(&[("search", "Globex")]).send().await
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
- If the app uses `SessionModule`, add `session.csrf_except = ["/admin/*"]`.

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
  enter a workspace at all. Member roles don't narrow abilities: inside the
  workspace the panel uses app-wide abilities.
- **A record is a 404, not a 403:** rows the access filter hides don't exist
  for that user, like `Accessible<M, Read>` in the API.
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
- **Sign-in refused at the panel:** the app's `SignInPolicy`s are asked as for
  `POST /api/auth/login` (a domain that enforces single sign-on can't use a
  password), and the login throttle counts the same attempts.
- **Action records skipped:** records the user can't act on are left out
  (`skipped` in the result); none left is a 403.
- **File fields** store `{slug}/{random}.{ext}` on the field's disk after
  sniffing the content; the JSON API accepts only such paths, not uploads.
- **Money** is in minor units in `values`, `FormData` and `min`/`max`.
- **Metrics** are cached per user, access version and range for
  `helm.metrics_cache_secs` (60; `0` turns it off): a changed number can lag.
- **Config:** `helm.path` (`/admin`), `title`, `logo`, `accent`, `per_page`
  (1 to 100), `metrics_cache_secs`, `dashboard`, `session_minutes`; env
  `APP_HELM__TITLE`, `APP_HELM__PER_PAGE`.
- **The look** follows Navree's app UI rules (sidebar shell, 4px corners,
  ink selection, green only for primary actions, focus and positive data).
  Don't add inline styles or scripts to templates: the CSP blocks them.
