# Models in depth

Casts, accessors and mutators, serialization, events and observers, scopes
and soft deletes, pruning, route model binding, workspaces, access control,
validation rules and MongoDB models.

## Casts

`#[model(cast = "..")]` on a field stores it in another form; reads, saves,
dirty tracking, `fill`, serialization and conditions all go through it
(`Deal::STAGE.eq(Stage::Won)` binds `'won'`). On `Option<T>` it casts the
inner value.

| `cast =` | Field | Stored as |
|---|---|---|
| `"string_enum"` | unit enum (serde names) | text |
| `"int_enum"` | `#[repr]` enum with `Into<i64>` / `TryFrom<i64>` | integer |
| `"json"` | any serde type | JSON |
| `"decimal(N)"` | `Decimal` | decimal rounded to `N` places |
| `"datetime_format(\"%Y-%m-%d\")"`, `"unix_timestamp"`, `"unix_timestamp_ms"` | date-times | text, integer seconds or milliseconds |
| `"encrypted"`, `"encrypted_json"` | `String`, `Vec<u8>`, any serde type | AES-256-GCM payload (the app key) |
| `"hashed"` | `String` | Argon2id hash |
| `"binary_uuid"`, `"binary_ulid"` | `Uuid`, `Ulid` | 16 bytes |
| `"vector"` | `Vec<f32>` | pgvector `vector`, or bytes elsewhere |

Your own cast implements `navree::orm::cast::Cast` (`cast = path::to::Cast`).
Hide secrets from JSON with `hidden(api_token)`.

## Accessors and mutators

```rust
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "people", appends(initials))]
pub struct Person {
    pub id: Uuid,
    #[model(set_with = split_name)]      // runs on fill, set_attribute, set_full_name
    pub full_name: String,
    pub first_name: String,
    pub last_name: String,
}

#[accessors]
impl Person {
    #[accessor]
    pub fn initials(&self) -> String {
        self.first_name.chars().take(1).chain(self.last_name.chars().take(1)).collect()
    }
}

fn split_name(person: &mut Person, full_name: String) { /* .. */ }
```

## Serialization

A record serializes as its model, then its extras and loaded relations
(under the relation's name). `hidden(..)` / `visible(..)` on the model
(or `#[model(hidden)]` on a field), `make_hidden([..])` / `make_visible([..])`
on a collection, `appends(..)` adds accessors. `#[model(resource = DealResource)]` and
`navree::api` resources shape API output. A queued job carries
`ModelRef<Deal>` (`deal.to_ref()?`, then `job.deal.restore(&uow)`), not the
record's values.

## Events and observers

Events: `retrieved`, `saving`, `creating`, `created`, `updating`,
`updated`, `saved`, `deleting`, `deleted`, `trashed`, `force_deleting`,
`force_deleted`, `restoring`, `restored`, `replicating`. The "-ing" ones
may cancel (`Ok(Flow::Cancel)`; `save` and `delete` then return `false`); inside a unit of work the "-ed" ones run
after commit unless the observer sets `const IN_TRANSACTION: bool = true`.

```rust
#[injectable]
pub struct WonDealMailer {
    mailer: Arc<Mailer>,
}

impl Observer<Deal> for WonDealMailer {
    async fn updated(&self, deal: &Record<Deal>, _ctx: &ExecContext) -> Result<()> {
        if deal.was_changed_on([Deal::STAGE]) && deal.stage == Stage::Won {
            // queue the mail
        }
        Ok(())
    }
}

// in the module's wire
m.model::<Deal>().observer::<Deal, WonDealMailer>();
```

Quiet writes skip events: `save_quietly`, `delete_quietly`,
`without_events(future)`. Seeders can opt out with
`Seeder::without_model_events`. Tests: `ModelEventFake`
(`assert_fired::<Deal>(ModelEvent::Created)`).

## Scopes

```rust
#[scopes]
impl Deal {
    #[scope]
    fn owned_by(query: Builder<Self>, owner: Uuid) -> Builder<Self> {
        query.where_(Deal::OWNER_ID.eq(owner))
    }
}
// use crate::modules::deals::models::deal::DealScopes;
Deal::query().owned_by(me).get(&db).await?;
```

Global scopes: `impl GlobalScope<Project> for NotArchived` and
`#[model(global_scopes(NotArchived))]`; remove with
`without_global_scope::<NotArchived>()` or `without_global_scopes()`.

## Soft deletes and pruning

`#[model(soft_deletes)]` plus `pub deleted_at: Option<DateTime<Utc>>`
(`t.soft_deletes_tz()` in the migration): `delete` sets `deleted_at`,
queries leave trashed records out, `restore` clears it, `force_delete`
removes the row, `trashed()` tells. Queries: `with_trashed()`,
`only_trashed()`. Pruning: `#[model(prunable)]` plus
`impl navree::orm::prune::Prunable for Note` (`fn prunable(query) -> query`
selecting what to delete), register the model with `m.model::<Note>()`, and
run `navree model:prune` (chunked by `[orm] prune_chunk`).

## Route model binding

```rust
use navree::orm::binding::{Bound, BoundChild, WithTrashed};

#[get("/{company}")]
async fn show(&self, company: Bound<Company>) -> Json<Record<Company>> {
    Json(company.into_inner())
}
```

The parameter named after the model in snake case (else `{id}`), looked up
by the route key (`#[model(route_key = "slug")]`), in the request's unit of
work, with global scopes: a missing or soft-deleted record is a 404.
`Bound<M, WithTrashed>` includes trashed records; `BoundChild` scopes a
nested parameter to its parent (`/companies/{company}/deals/{deal}`).

## Workspaces and access control

- `#[model(workspace)]` with a `workspace_id` column: in a workspace
  request every read is filtered to the current workspace, new records get
  it, and another workspace's record is a 404.
- `#[model(subject, policy = TicketPolicy)]` (feature `access`): register
  with `m.model::<Ticket>().import(AccessModule::for_feature::<Ticket>())`;
  routes use `#[can(Read, Ticket)]` with `Accessible<Ticket, Read>`
  (`scope.query()`) and `#[can(Update, Ticket, from = "ticket")]` with
  `Authorized<Ticket, Update>`. Outside routes:
  `Ticket::query().accessible(&ability, Read)` and
  `ability.authorize(Update, &ticket)?` (`navree::orm::access`).
- Validation (feature `validation`): `navree::orm::validation::{unique,
  exists}` build `AsyncRule`s that query the model
  (`unique(Deal::SLUG, value).ignore_path_param("deal")`).

## MongoDB

Feature `mongo`. `#[model(collection = "events", driver = "mongodb")]`,
keys as `ObjectId` (stored in `_id`); `MongoModule::from_config()` provides
`MongoDb`, the executor. The same builder compiles to filters and
pipelines (`to_mongo()` shows them); units of work are sessions with
transactions; migrations run with `mongo:migrate`. See
https://navree.com/docs/orm/mongodb/.

Docs: https://navree.com/docs/orm/casts-and-accessors/,
https://navree.com/docs/orm/serialization/,
https://navree.com/docs/orm/events-and-observers/,
https://navree.com/docs/orm/scopes-and-soft-deletes/,
https://navree.com/docs/orm/route-model-binding/,
https://navree.com/docs/orm/workspaces-and-access/
