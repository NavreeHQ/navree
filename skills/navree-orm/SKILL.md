---
name: navree-orm
description: "Works with data through Navree's ORM (`navree::orm`): `#[derive(Model)]` models with typed column constants, the query builder, records with dirty tracking (`create`, `save`, `delete`), `#[relations]` and eager loading with `with(..)`, `#[scopes]`, soft deletes, casts and accessors, serialization, model events and observers, pagination and streams, mass assignment, factories, `#[navree::test]` databases and assertions, route model binding (`Bound<M>`), workspace-scoped models and access-control subjects, and MongoDB models. Use when adding or changing a model, writing a query, relation, eager load, scope, cast, observer, factory or model test, binding a model to a route, or debugging lazy-loading, missing-attribute or mass-assignment errors, N+1 queries or a 404 from `find_or_fail`."
---

# The ORM

A model is a plain struct with `#[derive(Model)]`. The derive maps it to a
table and gives it one typed column constant per field (`Deal::TITLE` is a
`Column<Deal, String>`), so a condition comparing a column with the wrong
type doesn't compile. Queries chain on `Deal::query()` and run on an
**executor**; they return `Record<Deal>` (the model plus what the ORM knows:
stored or not, original values, extras, loaded relations) and
`Collection<Deal>`. The ORM runs on navree-db's pool and unit of work, so a
record saved in a handler commits or rolls back with the request and is
recorded for the audit log. Same models on PostgreSQL/TimescaleDB, MySQL,
MariaDB, SQLite and (feature `mongo`) MongoDB.

Feature `orm` (in `full`). Imports: `use navree::orm::prelude::*;` (models,
`#[relations]`, `#[scopes]`, relation types, `Builder`, `Record`,
`Collection`, `col`, `raw`, ...). `navree::prelude` has `Model`, `Record`,
`Collection`, `Builder`, `OrmModule`, `Attributes`, `attrs!` and
`#[accessors]`, but not `#[relations]` or `#[scopes]`.

## Steps: a new model

1. `navree make:model deals Deal --migration --factory` writes
   `src/modules/deals/models/deal.rs`, a `create_deals_table` migration and
   a factory, and adds `m.model::<Deal>()` to the module's `wire`
   (`--seeder`, `--controller`, `--policy`, or `--all` for every companion).
2. Fill in the struct and the migration together: a `Uuid` key is
   `t.uuid_primary()`, `DateTime<Utc>` is `timestamp_tz`, `Option<T>` is
   `.nullable()` (load `navree-migrations` for the column table). On MySQL a
   model's `Uuid` columns must be `BINARY(16)`, not the builder's `CHAR(36)`.
3. Add relations in a `#[relations] impl` block and scopes in `#[scopes]`.
4. Use the model from services that take `&Uow`, and from controllers with
   `Bound<Deal>` for `{deal}` path parameters.
5. Test with `#[navree::test]` and the factory (see
   `references/testing-and-factories.md`), then `cargo test`.

The app's root needs `OrmModule::from_config()` after `DatabaseModule`
(`navree new` adds it).

## Model

```rust
use chrono::{DateTime, Utc};
use navree::orm::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::activity::Activity;
use super::company::Company;
use crate::modules::deals::factories::deal::DealFactory;

/// Stored as its serde name: "lead", "won", "lost".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    #[default]
    Lead,
    Won,
    Lost,
}

/// A deal: the `deals` table (the struct's name in snake case, plural).
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(timestamps, soft_deletes, factory = DealFactory, fillable(title, amount_cents))]
pub struct Deal {
    pub id: Uuid, // the key: a UUID v7 generated on create
    pub company_id: Uuid,
    pub title: String,
    pub amount_cents: i64,
    #[model(cast = "string_enum")]
    pub stage: Stage,
    pub closed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[relations]
impl Deal {
    pub fn company(&self) -> BelongsTo<Deal, Company> {
        self.belongs_to(Deal::COMPANY_ID)
    }

    pub fn activities(&self) -> HasMany<Deal, Activity> {
        self.has_many(Activity::DEAL_ID)
    }
}

#[scopes]
impl Deal {
    /// Deals not closed yet: `Deal::query().open()` (import `DealScopes`).
    #[scope]
    fn open(query: Builder<Self>) -> Builder<Self> {
        query.where_(Deal::CLOSED_AT.is_null())
    }
}
```

A model derives `Model`, `Default` (relations and factories need it) and
`Serialize`. Model attributes (`#[model(..)]` on the struct):

| Attribute | Does |
|---|---|
| `table = "deals"` | the table (default: snake case plural of the name) |
| `primary_key = "..."`, `key_type = "int" \| "uuid_v7" \| "ulid" \| "manual" \| ..` | the key column and how it gets a value (default `id`, by its type: `Uuid` gets a v7, `i64` the database's) |
| `timestamps` | fills `created_at` / `updated_at`; `timestamps(updated_at = false)` |
| `soft_deletes` | `delete` sets `deleted_at`; queries leave trashed rows out |
| `fillable(..)`, `guarded(..)`, `unguarded` | mass assignment (a model with neither refuses `fill`) |
| `hidden(..)`, `visible(..)`, `appends(..)` | what serializes |
| `with = [".."]`, `with_count = [".."]` | relations every query loads or counts |
| `touches = ["company"]` | saving touches the parent's `updated_at` |
| `per_page = 25` | the default page size |
| `factory = DealFactory` | `Deal::factory()` |
| `route_key = "slug"` | the column `Bound<Deal>` looks up |
| `workspace` | scoped to the current workspace (`workspace_id`) |
| `subject`, `policy = DealPolicy` | records are navree-access subjects |
| `connection = "analytics"`, `driver = "mongodb"` | another connection; a MongoDB model |
| `global_scopes(Scope, ..)` | conditions every query gets |

Field attributes: `column = ".."`, `cast = "json" | "string_enum" |
"int_enum" | "decimal(2)" | "encrypted" | "encrypted_json" | "hashed" |
"binary_uuid" | "vector" | ..`, `default` (a partial select may leave it
out), `skip`, `hidden`, `fillable`, `guarded`.

## Executors

Every method that touches the database is `async` and takes the executor
first.

| Code | Pass |
|---|---|
| a handler, a service taking `&Uow` | `&uow`: the request's transaction; writes record change events for the audit log |
| a service with an injected `Arc<Db>` | `&self.db`: joins the current unit of work when one is open, else the pool |
| a command, job, seeder or test | `&db` (the pool), or a unit of work: `let uow = db.uow(actor, None).await;` then `uow.scope(async { .. })` and `uow.commit()` |

## Service

```rust
use navree::orm::attributes::Attributes;
use navree::prelude::*;
use uuid::Uuid;

use super::models::deal::{Deal, DealScopes, Stage};

#[injectable]
pub struct DealService;

impl DealService {
    /// One page of open deals, newest first, with their companies.
    pub async fn list(&self, uow: &Uow, page: PageQuery) -> Result<Page<Record<Deal>>> {
        Ok(Deal::query().open().with("company").latest().paginate(uow, page).await?)
    }

    /// A new deal from a JSON body: only `fillable` keys are set.
    pub async fn create(&self, uow: &Uow, company_id: Uuid, body: Attributes) -> Result<Record<Deal>> {
        let mut deal = Record::new(Deal { company_id, ..Default::default() });
        deal.fill(body)?;
        deal.save(uow).await?;
        Ok(deal)
    }

    /// Marks a deal won: writes `stage`, `closed_at` and `updated_at` only.
    pub async fn win(&self, uow: &Uow, id: Uuid) -> Result<Record<Deal>> {
        let mut deal = Deal::find_or_fail(uow, id).await?; // 404 when missing
        deal.stage = Stage::Won;
        deal.closed_at = Some(chrono::Utc::now());
        deal.save(uow).await?;
        Ok(deal)
    }

    /// Soft-deletes the deal (`soft_deletes`): `deleted_at` is set.
    pub async fn remove(&self, uow: &Uow, id: Uuid) -> Result<()> {
        Deal::find_or_fail(uow, id).await?.delete(uow).await?;
        Ok(())
    }
}
```

`OrmError` converts into `AppError` with `?`: `ModelNotFound` is a 404, a
unique or foreign-key violation a 409, the rest a 500.

## Controller

```rust
use navree::orm::attributes::Attributes;
use navree::orm::binding::Bound;
use navree::prelude::*;
use uuid::Uuid;

#[injectable]
pub struct DealsController {
    deals: Arc<DealService>,
}

#[controller("/deals", version = "1")]
#[authenticated]
impl DealsController {
    #[get("/")]
    async fn index(&self, uow: Uow, page: PageQuery) -> Result<Page<Record<Deal>>> {
        self.deals.list(&uow, page).await
    }

    /// `{deal}` is looked up by key (soft-deleted deals answer 404).
    #[get("/{deal}")]
    async fn show(&self, deal: Bound<Deal>) -> Json<Record<Deal>> {
        Json(deal.into_inner())
    }

    #[post("/companies/{company}/deals")]
    async fn store(
        &self,
        uow: Uow,
        Path(company): Path<Uuid>,
        Json(body): Json<Attributes>,
    ) -> Result<Created<Record<Deal>>> {
        let deal = self.deals.create(&uow, company, body).await?;
        Ok(Created(format!("/api/v1/deals/{}", deal.id), deal))
    }
}
```

Records serialize as their model's JSON plus extras and loaded relations.
Prefer a validated DTO (`Valid<Json<CreateDeal>>`) for input that has
rules; `Attributes` suits plain partial updates.

## Cheat sheet

| Task | Code |
|---|---|
| find by key | `Deal::find(&db, id)` (`Option`), `Deal::find_or_fail(&db, id)` (404) |
| query | `Deal::query().where_(Deal::STAGE.eq(Stage::Won)).latest().get(&db)` |
| one record | `.first(&db)`, `.first_or_fail(&db)`, `.sole(&db)` |
| a column, existence | `.value(&db, Deal::TITLE)`, `.pluck(&db, Deal::TITLE)`, `.exists(&db)` |
| aggregates | `.count(&db)`, `.sum(&db, Deal::AMOUNT_CENTS)`, `.avg`, `.min`, `.max` |
| OR, groups | `.or_where(..)`, `.where_group(\|g\| ..)`, `.where_not(..)` |
| create | `Deal::create(&uow, Deal { .. })`, `Record::new(model).save(&uow)` |
| change | `deal.title = ..; deal.save(&uow)`, `deal.update(&uow, \|d\| ..)` |
| find or create | `Tag::first_or_create(&uow, [Tag::NAME.set("vip")], \|t\| ..)`, `update_or_create` |
| delete | `deal.delete(&uow)`, `force_delete`, `restore`; `Deal::destroy(&uow, ids)` |
| many rows at once (no events) | `.update(&uow, [Deal::STAGE.set(..)])`, `.delete(&uow)`, `Tag::upsert(..)` |
| eager load | `.with("company")`, `.with("company.owner")`, `.with_count("activities")` |
| filter by relation | `.where_has("activities", \|q\| ..)`, `.doesnt_have("activities")` |
| relation query | `deal.activities().latest().get(&db)`, `deal.activities().create(&uow, ..)` |
| pages | `.paginate(&uow, page)` (`{data, meta}`), `.cursor_paginate(..)`, `.chunk(..)`, `.lazy(..)` |
| lock rows | `.lock_for_update()`, `.shared_lock()`, `.skip_locked()` (in a unit of work) |
| trashed rows | `.with_trashed()`, `.only_trashed()` |
| the SQL | `.to_sql(Dialect::Postgres)` |

`references/queries.md` has the query builder in full;
`references/relationships.md` every relation, pivots and eager loading;
`references/models-in-depth.md` casts, accessors, serialization, events,
scopes, soft deletes, binding, workspaces and access;
`references/testing-and-factories.md` factories and model tests.

## Strict mode

`[orm]` in `config/app.toml`: `strict` (default: on unless the app's `env`
is `production`) turns on three checks, each settable on its own:

- `prevent_lazy_loading`: reading a relation one record at a time out of a
  result fails (`OrmError::LazyLoading`). Fix: `with("relation")` on the
  query, or `collection.load(&db, "relation")`.
- `prevent_missing_attributes`: a record read by a partial `select` that
  left out a column fails (`OrmError::MissingAttribute`). Fix: select the
  column, or mark the field `#[model(default)]`.
- `prevent_discarding_attributes`: `fill` with a key the model doesn't
  allow fails (`OrmError::MassAssignment`). Fix: `fillable(..)`, or don't
  send it.

`auto_eager_load = true` loads a lazily read relation for the whole
result instead.

## Commands

```sh
navree model:show            # the app's models
navree model:show Deal       # columns, key, timestamps, scopes, hooks (--json)
navree model:prune           # deletes what prunable models select
```

## Common mistakes

- A typo in a relation name (`with("compnay")`) fails only when the query
  runs: pass the method (`with(Deal::company)`) and the compiler checks it.
  A model has one `#[relations]` block and derives `Default`.
- `use navree::prelude::*;` alone for `#[relations]` or `#[scopes]`: they
  come from `navree::orm::prelude`.
- Calling a scope without importing its trait (`DealScopes`).
- Passing `db.conn()` (a SeaORM connection) in a request: its writes skip
  the unit of work, so they neither roll back with the request nor reach
  the audit log. Pass `&uow`, or `&db`, which joins the request's unit of
  work.
- Lazy loads in a loop (N+1): eager load with `with`.
- `latest()` on a model without `timestamps`: use `latest_by(column)`.
- Mass updates and deletes (`query.update`, `query.delete`, bulk inserts)
  run no model events and record no audit changes: save records one by one
  when observers or the audit log matter.
- `navree::prelude::ModelFactory` is navree-db's entity factory; ORM
  factories implement `navree::orm::factory::ModelFactory`.
- A MySQL table made with `uuid_primary()` (`CHAR(36)`): inserting a model
  fails with error 1366; declare `t.binary("id").length(16).fixed()`.

## Docs

- https://navree.com/docs/orm/getting-started/
- https://navree.com/docs/orm/models/
- https://navree.com/docs/orm/query-builder/
- https://navree.com/docs/orm/relationships/
- https://navree.com/docs/orm/eager-loading/
- https://navree.com/docs/orm/testing/
- https://navree.com/docs/orm/factories/
