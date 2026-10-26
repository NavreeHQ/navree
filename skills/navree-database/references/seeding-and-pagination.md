# Seeders, factories and pagination

## Seeder

A seeder is a provider implementing `navree::db::Seeder`, registered with
`m.seeder::<T>()` in the module's `wire`. `navree db:seed` runs every
seeder (modules in dependency order); `navree db:seed demo-posts` runs the
named ones; `navree migrate:fresh --seed` starts over. There is no
`make:seeder`: write the file by hand.

```rust
use chrono::Utc;
use navree::BoxFuture;
use navree::db::{Db, Seeder};
use navree::prelude::*;
use sea_orm::ActiveValue::Set;
use sea_orm::{EntityTrait, PaginatorTrait};
use uuid::Uuid;

/// `db:seed demo-posts`: three posts, only into an empty table.
#[injectable]
pub struct PostSeeder;

impl Seeder for PostSeeder {
    fn name(&self) -> &'static str {
        "demo-posts"
    }

    fn run<'a>(&'a self, _ctx: &'a AppContext, db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            if post::Entity::find().count(db.conn()).await? > 0 {
                return Ok(()); // safe to run twice
            }
            db.transaction(|uow| async move {
                for name in ["Hello", "Second", "Third"] {
                    let now = Utc::now();
                    Repo::<post::Entity>::insert(
                        &uow,
                        post::ActiveModel {
                            id: Set(Uuid::now_v7()),
                            owner_id: Set(None),
                            name: Set(name.to_owned()),
                            published: Set(true),
                            created_at: Set(now),
                            updated_at: Set(now),
                        },
                    )
                    .await?;
                }
                Ok(())
            })
            .await?;
            Ok(())
        })
    }
}
```

- Make seeders idempotent; separate what every environment needs (roles)
  from demo data, as two seeders.
- Inserting through `Repo` in a unit of work records audit entries; plain
  SeaORM through `db.conn()` doesn't.
- Use services through `ctx.get::<T>()`: create users with
  `AuthService::create_user` so passwords are hashed properly.
- In tests: `app.run_command("db:seed", &["demo-posts"]).await?`.

## Factory

```rust
use chrono::Utc;
use navree::db::Factory;
use sea_orm::ActiveValue::Set;
use uuid::Uuid;

/// Posts named `Post 0`, `Post 1`, ...
pub struct PostFactory {
    pub owner_id: Uuid,
    pub published: bool,
}

impl PostFactory {
    pub fn new(owner_id: Uuid) -> Self {
        Self { owner_id, published: false }
    }

    pub fn published(self) -> Self {
        Self { published: true, ..self }
    }
}

impl Factory for PostFactory {
    type Entity = post::Entity;

    fn definition(&self, n: usize) -> post::ActiveModel {
        let now = Utc::now();
        post::ActiveModel {
            id: Set(Uuid::now_v7()),
            owner_id: Set(Some(self.owner_id)),
            name: Set(format!("Post {n}")),
            published: Set(self.published),
            created_at: Set(now),
            updated_at: Set(now),
        }
    }
}
```

```rust
use navree::db::{create, create_many};

let db = app.get::<Db>().await;
let first = create(&PostFactory::new(owner), db.conn()).await?;              // definition 0
let posts = create_many(&PostFactory::new(owner).published(), db.conn(), 10).await?; // 0..10
```

`n` restarts at 0 for every call, so values derived only from `n` repeat
across calls; use `Uuid::now_v7()` for unique columns. Factories insert with
plain SeaORM: no change events, no service rules. In the prelude the trait
is `ModelFactory` (`Factory` there is the kernel's provider factory).

## Offset pagination

```rust
use navree::db::{Page, PageQuery, paginate};

#[get("/")]
#[can(Read, Post)]
async fn index(&self, uow: Uow, scope: Accessible<Post, Read>, page: PageQuery) -> Result<Page<Model>> {
    let select = scope.apply(Entity::find().order_by_desc(Column::CreatedAt).order_by_desc(Column::Id));
    Ok(paginate(select, &*uow.tx().await?, page).await?)
}
```

`GET /api/v1/posts?page=2&per_page=15` answers
`{"data": [...], "meta": {"current_page": 2, "per_page": 15, "total": 31, "last_page": 3}}`.
`page` defaults to 1, `per_page` to 15 and is capped at 100; a non-positive
value is a 400. A page past the end is empty, not an error. Convert items
with `page.map(PostResource::from)`; build a page from other rows with
`Page::new(data, query, total)`. For your own limits, validate a query DTO
and build `PageQuery::new(page, per_page)`.

## Cursor pagination

```rust
use navree::db::{CursorPage, CursorQuery, cursor_paginate_desc};

#[get("/feed")]
#[can(Read, Post)]
async fn feed(&self, uow: Uow, scope: Accessible<Post, Read>, cursor: CursorQuery) -> Result<CursorPage<Model>> {
    let tx = uow.tx().await?;
    Ok(cursor_paginate_desc(scope.apply(Entity::find()), Column::Id, &*tx, cursor).await?)
}
```

`?cursor=..&limit=..` (limit default 15, max 100); the answer is
`{"data": [...], "next_cursor": "..."}` (`null` on the last page). The column
must be a UUID (v7, so its order is creation order); don't order the query
yourself. `cursor_paginate` gives oldest first.

## OpenAPI

`Page<T>` has no `ToSchema`; for the document, declare a named page type
(`PostPage { data: Vec<PostResource>, meta: .. }`) and name it in
`#[api(response = PostPage)]` (see the `navree-openapi` skill).
