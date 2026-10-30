# Seeders, factories and pagination

## Seeder

A seeder is a provider implementing `navree::db::Seeder`, registered with
`m.seeder::<T>()` in the module's `wire`. `navree db:seed` runs every
seeder (modules in dependency order); `navree db:seed demo-posts` runs the
named ones; `navree migrate:fresh --seed` starts over. `navree make:seeder
posts demo_posts` writes and registers one (`make:model --seeder` writes one
that uses the model's factory).

```rust
use navree::BoxFuture;
use navree::db::{Db, Seeder};
use navree::orm::OrmError;
use navree::prelude::*;

/// `db:seed demo-posts`: three posts, only into an empty table.
#[injectable]
pub struct PostSeeder;

impl Seeder for PostSeeder {
    fn name(&self) -> &'static str {
        "demo-posts"
    }

    fn run<'a>(&'a self, _ctx: &'a AppContext, db: &'a Db) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            if Post::query().exists(db).await? {
                return Ok(()); // safe to run twice
            }
            // A unit of work, so the posts are audited and commit together.
            let uow = db.uow(None, None).await;
            let created = uow
                .scope(async {
                    for name in ["Hello", "Second", "Third"] {
                        Post::create(
                            &uow,
                            Post { name: name.to_owned(), published: true, ..Post::default() },
                        )
                        .await?;
                    }
                    Ok::<_, OrmError>(())
                })
                .await;
            match created {
                Ok(()) => uow.commit().await?,
                Err(error) => {
                    uow.rollback().await?;
                    return Err(error.into());
                }
            }
            Ok(())
        })
    }
}
```

- Make seeders idempotent; separate what every environment needs (roles)
  from demo data, as two seeders.
- Creating records through a unit of work (or `&db`, which joins one)
  records change events for the audit log; mass writes (`insert` of many
  rows) and raw statements don't.
- Use services through `ctx.get::<T>()`: create users with
  `AuthService::create_user` so passwords are hashed properly.
- In tests: `app.run_command("db:seed", &["demo-posts"]).await?`.

## Factory

A model factory is a `navree::orm::factory::ModelFactory`, linked to its
model with `#[model(factory = PostFactory)]` (the `navree-orm` skill has
the whole API: states, sequences, relations, `make`, `create`):

```rust
use navree::orm::factory::{FactoryContext, ModelFactory};

/// Posts with fake names.
#[derive(Default)]
pub struct PostFactory;

impl ModelFactory for PostFactory {
    type Model = Post;

    fn definition(&self, ctx: &mut FactoryContext<'_, Post>) -> Post {
        Post {
            name: ctx.fake().title(),
            ..Post::default()
        }
    }
}

impl PostFactory {
    /// A state: `Post::factory().state(PostFactory::published)`.
    pub fn published(post: &mut Post) {
        post.published = true;
    }
}
```

```rust
let db = app.get::<Db>().await;
let first = Post::factory().create_one(&*db).await?;
let posts = Post::factory().state(PostFactory::published).count(10).create(&*db).await?;
```

Unique columns take `ctx.fake().unique(|f| f.word())` or `Uuid::now_v7()`.
`navree make:factory posts post` writes the file; `navree make:model posts
Post --factory` links one to a new model. In the prelude the trait is
`ModelFactory` (`Factory` there is the kernel's provider factory).

## Offset pagination

```rust
use navree::db::{Page, PageQuery};
use navree::orm::Record;

#[get("/")]
#[can(Read, Post)]
async fn index(&self, uow: Uow, scope: Accessible<Post, Read>, page: PageQuery) -> Result<Page<Record<Post>>> {
    Ok(scope.query().order_by_desc(Post::CREATED_AT).order_by_desc(Post::ID).paginate(&uow, page).await?)
}
```

(`scope.query()` comes from `navree::orm::access::AccessibleQuery`.)
`GET /api/v1/posts?page=2&per_page=15` answers
`{"data": [...], "meta": {"current_page": 2, "per_page": 15, "total": 31, "last_page": 3}}`.
`page` defaults to 1, `per_page` to 15 (or the model's `per_page`) and is
capped at 100; a non-positive value is a 400. A page past the end is empty,
not an error. Convert items with `page.map(PostResource::from)`; build a
page from other rows with `Page::new(data, query, total)`. For your own
limits, validate a query DTO and build `PageQuery::new(page, per_page)`.
`simple_paginate` skips the count and answers `has_more` instead.

## Cursor pagination

```rust
use navree::orm::{CursorPage, CursorRequest, Record};

#[get("/feed")]
#[can(Read, Post)]
async fn feed(&self, uow: Uow, scope: Accessible<Post, Read>, cursor: CursorRequest) -> Result<CursorPage<Record<Post>>> {
    Ok(scope.query().latest().cursor_paginate(&uow, cursor).await?)
}
```

`?cursor=..&limit=..` (limit default 15, max 100); the answer is
`{"data": [...], "next_cursor": "...", "prev_cursor": null}` (`null` when
there is no such page). The cursor encodes the query's order columns and the
last row's values, with the key appended as a tie-breaker, so any mix of
directions works over non-`NULL` columns; a cursor that doesn't match the
query is a 400.

## OpenAPI

`Page<T>` has no `ToSchema`; for the document, declare a named page type
(`PostPage { data: Vec<PostResource>, meta: .. }`) and name it in
`#[api(response = PostPage)]` (see the `navree-openapi` skill).
