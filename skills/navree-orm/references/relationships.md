# Relationships and eager loading

A relation is a method in the model's single `#[relations] impl` block
(from `navree::orm::prelude`), returning a typed relation. Its name (the
method's) is what `with`, `has`, `where_has` and `load` take. Foreign keys
are typed columns, so a key of the wrong model doesn't compile. A model
with relations derives `Default`.

```rust,ignore
#[relations]
impl Deal {
    pub fn company(&self) -> BelongsTo<Deal, Company> {
        self.belongs_to(Deal::COMPANY_ID)
    }

    pub fn activities(&self) -> HasMany<Deal, Activity> {
        self.has_many(Activity::DEAL_ID)
    }

    /// Through the `deal_tag` pivot (`deal_id`, `tag_id`).
    pub fn tags(&self) -> BelongsToMany<Deal, Tag> {
        self.belongs_to_many("deal_tag").with_pivot(["added_by"]).with_timestamps()
    }

    /// Notes whose `notable_type` / `notable_id` point at deals.
    pub fn notes(&self) -> MorphMany<Deal, Note> {
        self.morph_many("notable")
    }
}

#[relations]
impl Company {
    /// The most recent deal.
    pub fn latest_deal(&self) -> HasOne<Company, Deal> {
        self.has_many(Deal::COMPANY_ID).latest_of_many()
    }

    /// The activities of the company's deals.
    pub fn activities(&self) -> HasManyThrough<Company, Activity> {
        self.has_many_through::<Activity, Deal, _, _>(Deal::COMPANY_ID, Activity::DEAL_ID)
    }
}
```

| Relation | Defined with | Loaded value |
|---|---|---|
| `BelongsTo<P, R>` | `self.belongs_to(P::FK)` (`.owner_key(R::COL)`) | `Option<Record<R>>` |
| `HasOne<P, R>`, `HasMany<P, R>` | `self.has_one(R::FK)`, `self.has_many(R::FK)` (`.local_key(..)`) | `Option<Record<R>>`, `Collection<R>` |
| one of many | `.latest_of_many()`, `.oldest_of_many()`, `.latest_of_many_by(col)`, `.of_many(..)` on a `has_many` | `Option<Record<R>>` |
| `BelongsToMany<P, R>` | `self.belongs_to_many("pivot")` (`.with_pivot([..])`, `.with_timestamps()`, `.foreign_pivot_key(..)`, `.related_pivot_key(..)`) | `Collection<R>` with pivot rows |
| `HasOneThrough`, `HasManyThrough` | `self.has_one_through(..)`, `self.has_many_through(..)` | one, many |
| `MorphOne`, `MorphMany` | `self.morph_one("name")`, `self.morph_many("name")` | one, many |
| `MorphTo<P>` | `self.morph_to("name").targets::<(Deal, Company)>()` | `Option<AnyRecord>` |
| polymorphic many to many | `self.morph_to_many("name", "table")`, `self.morphed_by_many(..)` | many |

`.with_default()` gives a to-one relation a new record instead of `None`.
`touches = ["company"]` on the model touches the parent's `updated_at` on
save.

## Eager loading (one query per relation)

```rust,ignore
let deals = Deal::query()
    .with(Deal::company)             // a method: a typo doesn't compile
    .with("contact.company")         // nested: one query per level
    .with("company:id,name")         // only these columns (include the key)
    .with_constrained(Deal::activities, |q| q.latest().limit(3)) // 3 per deal
    .with_count("activities")        // extra column `activities_count`
    .with_sum("activities", Activity::MINUTES)
    .get(&db)
    .await?;

let company: Option<&Option<Record<Company>>> = deals[0].loaded(Deal::company);
let count: Option<i64> = deals[0].extra("activities_count")?;
```

- On collections and records after the fact: `load(&db, "company")`,
  `load_missing`, `load_count`, `load_constrained`, `load_sum`, ...
- `deal.related(&db, Deal::company)` reads a relation, loading it if needed
  (a lazy load).
- Model defaults: `#[model(with = ["company"], with_count = ["notes"])]`;
  `without("company")` and `without_eager_loads()` per query.
- Strict mode (`prevent_lazy_loading`, on outside production): a lazy load
  on a record that came in a result fails; a record fetched alone may still
  load lazily. `auto_eager_load = true` loads it for the whole result.

## Filtering by relations

```rust,ignore
Deal::query().has("activities");
Deal::query().has_count("activities", ">=", 3);
Deal::query().doesnt_have("activities");
Deal::query().where_has(Deal::activities, |q| q.where_(Activity::COMPLETED_AT.is_null()));
Deal::query().where_relation("company", Company::NAME.eq("Agra"));
Deal::query().where_belongs_to(&agra);
```

Each is an `EXISTS` (or a counted subquery) in the same query. Paths nest
(`"deals.activities"`); with a string path, annotate the closure
(`|q: Builder<Activity>| ..`). `or_` forms exist; `where_has_morph` filters
polymorphic targets.

## Writing through relations

```rust,ignore
let call = deal.activities().create(&uow, Activity { kind: "call".into(), ..Default::default() }).await?;
deal.activities().save(&uow, &mut task).await?;      // sets deal_id, saves

deal.associate(Deal::company, &agra)?;               // company_id = agra.id (not saved)
deal.save(&uow).await?;

deal.tags().attach(&uow, [1_i64, 2]).await?;
let changes = deal.tags().sync(&uow, [1_i64, 3]).await?; // changes.attached, .detached
deal.tags().toggle(&uow, [9_i64]).await?;
deal.tags().update_existing_pivot(&uow, 3_i64, [col("added_by").set(me)]).await?;
```

To-many relations also have `make`, `create_many`, `save_many`,
`first_or_create`, `update_or_create` and `increment_or_create`;
many-to-many `attach_with`, `detach`, `detach_all`, `sync_with`,
`sync_without_detaching`. Pivot writes are atomic.

Docs: https://navree.com/docs/orm/relationships/,
https://navree.com/docs/orm/eager-loading/
