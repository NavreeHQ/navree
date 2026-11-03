---
name: navree-search
description: "Full-text and vector search with navree-search: `SearchModule` and the `search` config section, `#[derive(Searchable)]` on navree-orm models (or a hand-written `Searchable` with related data), engines (`postgres` with SQLite FTS5 in tests, `meilisearch`, `typesense`, `tantivy`, `memory`), syncing through the queue (`search.sync` queued/inline/off, the `search.sync` job `SyncDocuments`, `reindex`, `without_syncing`), the `Search` service and query builder (filters, ranges, sorts, `accessible`, `workspace`, `paginate`, `hits` and highlights, `across`), vectors (`#[searchable(embed)]`, `Embeddings`, pgvector/Qdrant/memory, `similar`, `similar_to`, `hybrid`), the `search:import`/`flush`/`settings`/`status` commands and test helpers. Use when adding a search box or command palette, making a model searchable, picking or configuring a search engine, rebuilding an index, or debugging missing, stale or leaking search results."
---

# Search

The module is `navree::search` (crate `navree-search`, facade feature
`search`, which also turns on `orm`, **not** in `full`; `search-tantivy` adds the
embedded engine).
Everything below is in the code; when in doubt read
`crates/navree-search/src` and its tests in `crates/navree-search/tests/search/`.

## Steps: make a model searchable

1. Turn on the feature and add the module after the database and the queue:

```toml
navree = { git = "https://github.com/NavreeHQ/navree", features = ["search"] }
```

```rust,ignore
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(DatabaseModule::from_config())
    .module(QueueModule::from_config())   // runs `search.sync` jobs
    .module(SearchModule::from_config())  // section `search`; global, exports `Search`
    .module(ContactsModule)
```

```toml
[search]
engine = "postgres"        # postgres | meilisearch | typesense | tantivy | memory
# url = "http://127.0.0.1:7700"   # meilisearch / typesense; key: APP_SEARCH__KEY
prefix = ""                # put in front of every index name
queue = "default"          # the queue of search.sync jobs
sync = "queued"            # queued | inline | off
```

2. Derive `Searchable` next to `Model` on the navree-orm model (it
   implements the trait for the model itself; the model must also derive
   `Serialize` and have a single-column primary key):

```rust
use chrono::{DateTime, Utc};
use navree::search::Searchable;
use serde::Serialize;
use uuid::Uuid;

#[derive(Model, Searchable, Clone, Debug, Default, Serialize)]
#[model(table = "contacts", subject = "Contact")]
#[searchable(name = "Contact", soft_delete = "deleted_at")] // name/index default to the table
pub struct Contact {
    pub id: Uuid,
    #[searchable(boost = 3)]        // full text, weight 1..=10
    pub last_name: String,
    #[searchable]                   // full text, weight 1
    pub title: Option<String>,
    #[searchable(filter)]           // filter / filter_in / range / filter_null
    pub owner_id: Uuid,
    #[searchable(filter, sort)]
    pub score: i64,
    #[searchable(embed)]            // text for vector search
    pub notes: Option<String>,
    #[searchable(rename = "email_address", boost = 2)] // index name; rules still name `email`
    pub email: Option<String>,
    pub deleted_at: Option<DateTime<Utc>>,
}
```

   A field's column is its name or `#[model(column = "..")]`. A
   `#[model(skip)]` or `#[model(columns(..))]` field can be a text field but
   not a filter or sort field (compile error).

3. Register the model in the module that owns it:

```rust
#[module(imports = [SearchModule::for_feature::<Contact>()])]
pub struct ContactsModule;
```

4. Index what is already in the table: `navree search:import Contact`
   (`--all`, `--fresh`). From then on every write through the model API
   (`Model::create`, `record.save`, `record.update`, `record.delete`,
   `WorkspaceRepo`) syncs itself.

## Template: a document with related data

A `Record<M>` dereferences to the model (`deal.title`). Read related rows
through `ctx.uow()`, the unit of work the sync reads in.

```rust,ignore
use navree::orm::{Model, Record};

impl Searchable for Deal {
    fn settings() -> IndexSettings {
        IndexSettings::new("Deal", "deals")
            .text("title", 3)
            .text("company_name", 2)
            .filter("stage")
            .sort("amount_cents")
        // .soft_delete("deleted_at") / .workspace("workspace_id") by hand here
    }

    fn document<'a>(
        deal: &'a Record<Deal>,
        ctx: &'a SearchContext,
    ) -> BoxFuture<'a, Result<Option<Document>, BoxError>> {
        Box::pin(async move {
            if deal.archived {
                return Ok(None); // kept out (and removed if indexed)
            }
            let company = match deal.company_id {
                Some(id) => Company::find(ctx.uow(), id).await?, // Option<Record<Company>>
                None => None,
            };
            Ok(Some(
                Document::new(deal.id)
                    .field("title", &deal.title)
                    .field("company_name", company.map(|c| c.name.clone()))
                    .field("stage", &deal.stage)
                    .field("amount_cents", deal.amount_cents),
            ))
        })
    }
}
```

Records are read without the model's global scopes: soft-deleted rows are
read (the index's `soft_delete` column keeps them out) and a workspace-scoped
model is read in every workspace, each document carrying its workspace.

When the related row changes (a company rename), re-sync the dependents:
`search.reindex::<Deal>(deal_ids).await?`.

## Template: a search endpoint

```rust,ignore
#[injectable]
pub struct ContactSearchController {
    search: Arc<Search>,
}

#[controller("/contacts/search", version = "1")]
impl ContactSearchController {
    #[get("/")]
    #[can(Read, Contact)]
    async fn index(
        &self,
        scope: Accessible<Contact, Read>,
        Query(input): Query<SearchContacts>,
        page: PageQuery,
    ) -> Result<Page<Record<Contact>>> {
        let mut query = self.search.query::<Contact>(&input.q).accessible(scope);
        if let Some(status) = &input.status {
            query = query.filter("status", status);
        }
        Ok(query.paginate(page).await?) // SearchError -> 400 / 503 via `?`; a Record serializes through the ORM
    }
}
```

Other terminal calls: `.hits(page)` (a `Page<Hit<Record<M>>>`: records with
`score` and HTML-escaped `highlights`, change tags with `.highlight_tags("<em>", "</em>")`),
`.ids(limit)`, `.count()`. Sorts: `.sort_asc(field)`, `.sort_desc(field)`;
filters: `.filter`, `.filter_not`, `.filter_in`, `.range(field, 10..)`,
`.filter_null`, `.filter_by(Filter::Any(..))`. Several indexes at once:

```rust,ignore
let hits = search
    .across("ada")
    .include::<Contact>(contacts_scope)
    .include::<Deal>(deals_scope)
    .limit(10)
    .run()
    .await?; // AcrossHit { kind, id, title, score, highlights }; hit.load::<Contact>(&uow) -> Option<Record<Contact>>
```

## Workspaces

A model that implements `navree::workspace::WorkspaceScoped`, or has a
`#[model(workspace)]` column, is scoped automatically (the derive finds the
column; `#[searchable(workspace = "..")]` overrides it; by hand use
`.workspace("workspace_id")`). Every query on it must name the workspace or
it fails with
`SearchError::WorkspaceRequired`:

```rust,ignore
search.query::<Company>("acme").workspace(ws.id()).paginate(page).await?;
```

Only row isolation is supported. Sync jobs carry the workspace and, on
PostgreSQL, read rows inside it (RLS keeps working).

## Vectors

```toml
[search.vectors]
store = "pgvector"   # pgvector | qdrant (url, key) | memory
dimensions = 1536
metric = "cosine"    # cosine | dot | euclidean
```

Register an embeddings provider or the boot fails:

```rust,ignore
impl Embeddings for MyEmbeddings {
    fn embed<'a>(&'a self, texts: &'a [String]) -> BoxFuture<'a, Result<Vec<Vec<f32>>, BoxError>> {
        Box::pin(async move { /* one vector of `dimensions` per text */ todo!() })
    }
}
m.value_arc(Arc::new(MyEmbeddings::new()) as Arc<dyn Embeddings>);
```

Then `search.similar::<M>(text).filter(..).limit(10).run()`,
`search.similar_to::<M>(id).run()` (both return `Vec<Hit<Record<M>>>`) and
`search.query::<M>(text).hybrid(0.4)` (0 = keywords, 1 = vectors).

## Testing

```rust,ignore
let app = TestApp::builder()
    .temp_database()
    .config("search.engine", "memory")
    .config("search.sync", "inline")
    .module(DatabaseModule::from_config())
    .module(SearchModule::from_config())
    .module(ContactsModule)
    .build()
    .await;
// write through the model API, then:
let search = app.get::<Search>().await;
search.flush_pending().await?;                 // waits for inline syncs
search.assert_indexed::<Contact>(ada.id).await;
```

- `sync = "queued"` in tests: `app.get::<Queue>().await.run_pending().await`.
- Assert that a write queues a sync: install `QueueFake`
  (`override_value(Arc::new(fake.queue()))`) and
  `fake.assert_dispatched::<SyncDocuments>(|job| job.index == "contacts")`.
- To test a hand-written `document()`: `SearchContext::new(uow, app.ctx().clone(), None)`
  around a unit of work (it stays yours to commit or roll back), then
  `Deal::document(&record, &ctx)`.
- Vectors in tests: `search.vectors.store = "memory"` plus
  `override_value(Arc::new(fake) as Arc<dyn Embeddings>)`.
- The memory engine has typo tolerance and the same boosts; assert on order
  and membership, never on exact `score`.

## Checks

- `navree search:status`: documents vs rows per index, pending jobs.
- After changing `#[searchable]` fields: `navree search:import --all --fresh`.
- Every column an access rule names should be `#[searchable(filter)]`, or
  totals count records the user can't read (they are dropped from the page).
  Rules on date and time columns always work that way (the index keeps
  milliseconds); rules on boolean, number, string, UUID and enum columns
  filter in the engine exactly, except shapes an engine can't run
  (`SearchEngine::filters_exactly`): ranges on text on Meilisearch and
  Typesense, `!=` / `not_in` / null tests on Typesense, float equality in
  Qdrant. Those also go to the rows.
- The access parity property test
  (`crates/navree-search/tests/search/access_parity.rs`) must stay green
  on every engine after a change to filters or the derive.
- Mass writes (`Builder::update`, raw SQL) are not observed: `reindex` them
  or run `search:import`.

## Gotchas

- `search` is opt-in; `use navree::search::..` fails to resolve without the
  feature. `tantivy` also needs `search-tantivy`.
- The `postgres` (database) engine runs on PostgreSQL and SQLite only and is
  refused at boot on MySQL; the `search` migrations create nothing there.
  `meilisearch`, `typesense`, `tantivy` and `memory` run on a MySQL app.
- The `postgres` engine on SQLite (tests) matches prefixes but not typos;
  PostgreSQL needs `search.postgres.trigram = true` for typos;
  `search.postgres.typo_threshold` (default 0.4) sets how close a word must
  be, per query.
- Filtering or sorting on a field not marked `filter`/`sort` is a 400
  (`NotFilterable` / `NotSortable`), before the engine is called.
- The derive stores filter and sort fields as the stored value the access
  rules compare, text fields as serde serializes them: a `string_enum` cast
  stores the enum's serde name (`filter("stage", "closed_won")`), an
  `int_enum` cast its number (serde writes the name), UUIDs are hyphenated,
  timestamps are UTC with microseconds. A hand-written `document()` should
  do the same (`&deal.stage` for a string column). `#[model(column = "..")]`
  is the field's column.
- A fresh import re-syncs, after its swap, every record synced while it
  ran, so writes during `search:import --fresh` are kept.
- Typesense can't filter on missing values (`filter_null` is a 400).
- `without_syncing(fut)` only skips commits that happen inside `fut`.
- Tantivy: one writing process per directory; use `sync = "inline"` or a
  single worker.
- With `sync = "queued"` the worker must listen on `search.queue`.
