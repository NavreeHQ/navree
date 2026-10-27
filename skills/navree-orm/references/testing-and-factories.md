# Factories and model tests

## A factory

```rust
// src/modules/deals/factories/deal.rs
use navree::orm::factory::{FactoryContext, ModelFactory};

use crate::modules::deals::models::company::Company;
use crate::modules::deals::models::deal::{Deal, Stage};

#[derive(Default)]
pub struct DealFactory;

impl ModelFactory for DealFactory {
    type Model = Deal;

    fn definition(&self, ctx: &mut FactoryContext<'_, Deal>) -> Deal {
        Deal {
            company_id: ctx.belongs_to(Deal::COMPANY_ID, Company::factory()),
            title: ctx.fake().title(),
            amount_cents: ctx.fake().number(1_000..=500_000),
            ..Deal::default()
        }
    }
}

impl DealFactory {
    /// A state: `Deal::factory().state(DealFactory::won)`.
    pub fn won(deal: &mut Deal) {
        deal.stage = Stage::Won;
    }
}
```

Link it on the model with `#[model(factory = DealFactory)]`, which gives
`Deal::factory()`. `navree make:factory deals deal` writes one;
`navree make:model deals Deal --factory` links one to a new model. The
trait is `navree::orm::factory::ModelFactory` (the prelude's
`ModelFactory` is navree-db's SeaORM entity factory).

| Builder | Does |
|---|---|
| `.count(n)` | `n` records |
| `.state(DealFactory::won)`, `.set(Deal::TITLE, "Acme")` | change the definition |
| `.sequence(Sequence::new([DealFactory::won, DealFactory::lost]))`, `Sequence::column(Deal::TITLE, [..])` | states or values in turn across the batch |
| `.trashed()` | soft-deleted records |
| `.for_(Company::factory(), Deal::COMPANY_ID)`, `.for_record(..)` | one parent for all |
| `.has(Activity::factory().count(2), Activity::DEAL_ID)` | children of each |
| `.recycle(&companies)` | reuse existing parents instead of creating new ones |
| `.seed(42)` | the same fake data every run |
| `.make()`, `.make_one()` | unsaved records |
| `.create(exec)`, `.create_one(exec)` | saved records (events run, keys and timestamps filled) |
| `.create_quietly(exec)` | saved without model events |

`ctx.fake()` is a seeded `Faker`: `name`, `email`, `safe_email`, `company`,
`title`, `sentence`, `paragraph`, `number(range)`, `decimal(low, high,
scale)`, `boolean`, `pick(&items)`, `past(within)`, `future(within)`,
`uuid`, ... `ctx.fake().unique(|f| f.word())` for unique columns.

## A test with its own database

```rust
use navree::BoxError;
use navree::orm::prelude::*;
use navree::orm::testing::database::{Template, TestDb};
use navree::orm::testing::*;
use navree::schema::Schema;

/// The tables, built once per test binary; every test gets a copy.
static DATABASE: Template = Template::new("deals", |template| {
    Box::pin(async move {
        let schema = Schema::new(template.db().conn());
        schema
            .create("companies", |t| {
                t.uuid_primary();
                t.string("name");
                t.timestamps_tz();
                t.soft_deletes_tz();
            })
            .await?;
        schema
            .create("deals", |t| {
                t.uuid_primary();
                t.foreign_uuid("company_id").constrained();
                t.string("title");
                t.big_integer("amount_cents");
                t.string("stage").length(16).default("lead");
                t.timestamp_tz("closed_at").nullable();
                t.timestamps_tz();
                t.soft_deletes_tz();
            })
            .await?;
        Ok(())
    })
});

#[navree::test(template = DATABASE)]
async fn won_deals_are_not_open(db: TestDb) -> Result<(), BoxError> {
    let deals = Deal::factory().count(3).create(db.db()).await?;
    let mut won = Deal::factory().state(DealFactory::won).create_one(db.db()).await?;
    won.closed_at = Some(chrono::Utc::now());
    won.save(db.db()).await?;

    assert_eq!(Deal::query().open().count(db.db()).await?, 3);
    assert_database_count(db.db(), "companies", 4).await;
    assert_database_has(db.db(), Deal::table_name(), [Deal::STAGE.set(Stage::Won)]).await;

    let mut first = deals.into_iter().next().unwrap();
    first.delete(db.db()).await?;
    assert_soft_deleted(db.db(), &first).await;
    Ok(())
}

#[navree::test(template = DATABASE)]
async fn eager_loads_are_batched(db: TestDb) -> Result<(), BoxError> {
    Deal::factory().count(5).create(db.db()).await?;
    expect_query_count(2, async {
        let deals = Deal::query().with("company").get(db.db()).await.unwrap();
        assert!(deals.iter().all(|d| d.loaded(Deal::company).is_some()));
    })
    .await;
    Ok(())
}
```

- `#[navree::test]` needs no `#[tokio::test]`. `database = "sqlite"`
  (default), `"memory"`, `"postgres"` or `"mysql"`; real servers come from
  `NAVREE_TEST_POSTGRES_URL` / `NAVREE_TEST_MYSQL_URL` (or
  `target/test-databases.env`) and the test returns early without one.
- A template can boot the app on `template.url()` with
  `database.migrate_on_boot` to run the real migrations once.
- Assertions: `assert_database_has`, `assert_database_missing`,
  `assert_database_count`, `assert_database_empty`, `assert_model_exists`,
  `assert_model_missing`, `assert_soft_deleted`, `assert_not_soft_deleted`.
- Queries: `expect_query_count(n, future)`, `count_queries(future)`.
- Events: `ModelEventFake` in a `TestApp` (`override_value(fake.clone())`),
  then `fake.assert_fired::<Deal>(ModelEvent::Created)`.
- Through the app: `TestApp::builder().temp_database()` migrates a fresh
  SQLite file per test; use the assertions on `app.get::<Db>().await`.

Docs: https://navree.com/docs/orm/factories/,
https://navree.com/docs/orm/testing/
