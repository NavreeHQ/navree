---
name: navree-access-control
description: "Authorization with navree-access: `#[roles]`, `#[permissions]` and `#[can]` route checks, subjects and custom actions, the ability rules in `src/abilities.rs`, record checks with `Authorized`, per-user SQL list filters with `Accessible`, field checks, `AccessService`, the `access:*` commands and testing rules. Use when deciding who may do what, adding a role, permission or policy, protecting a route beyond \"signed in\", filtering a list to what the user may see, or explaining an unexpected 403."
---

# Access control

You write the rules once, in Rust, and Navree applies them in four places:
on routes before the handler runs, on loaded records, in SQL (lists return
exactly the rows the user may see), and in the browser (`GET
/api/me/abilities` returns the rules in CASL's JSON format).

Vocabulary: a **subject** is a type of thing (`"Post"`), an **action** is
`read`, `create`, `update`, `delete`, `manage` (any) or your own
(`publish`), a **permission** is a stored string `<prefix>.<action>`
(`posts.publish`, `posts.*`), a **role** groups permissions and is assigned
to users, an **ability** is one user's compiled rules.

## Steps for a new resource

1. Make the model a subject and register it (a `--crud` module from
   `navree make:module` already does both). A navree-orm model is a subject
   through its attribute (`policy = ..` adds its rules and implies `subject`):

```rust
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "posts", subject = "Post", permission_prefix = "posts")] // rules and __type name it; posts.read, posts.*, ...
pub struct Post { /* columns */ }       // `#[can(Read, Post)]`, `Authorized<Post, Update>`
```

```rust
#[module(
    imports = [AccessModule::for_feature::<Post>()], // record loading + prefix mapping
    providers = [PostService],
    controllers = [PostController],
)]
pub struct PostsModule;
```

   A model becomes a subject with `#[model(subject)]` (or `policy = ..`) and
   registers as `for_feature::<Model>()`. navree-auth's users:
   `for_feature::<navree_auth::User>()`.

2. Protect each route with one access attribute (template below).
3. Grant access: permissions to a role (`navree access:grant editor
   posts.update`), or rules in `src/abilities.rs` (`navree make:policy posts
   moderation` adds a policy file and calls it from there).
4. Filter lists with `Accessible<Post, Read>` (an ORM model:
   `scope.query()` / `scope.apply(Post::query())` from
   `navree::orm::access::AccessibleQuery`; SQL you write yourself:
   `scope.sql_filter(&TypedColumns::new("posts").with(..)).to_statement(backend)`,
   a portable fragment); check records with
   `#[can(.., from = "id")]` and `Authorized` (for a model it derefs to
   `Record<M>`; `.into_inner()` is the Record, `.into_inner().into_inner()`
   the model).
5. Test guests (401), users without the grant (403) and with it.

## Routes

```rust
#[controller("/posts", version = "1")]
impl PostController {
    #[get("/")]
    #[public]                                       // guests too: the filter decides what they see
    async fn index(&self, uow: Uow, scope: Accessible<Post, Read>, page: PageQuery) -> Result<Page<Record<Post>>> {
        Ok(scope.query().latest().paginate(&uow, page).await?)
    }

    #[post("/{id}/publish")]
    #[can(Publish, Post, from = "id")]              // 404 when missing, 403 when the rules say no
    async fn publish(&self, uow: Uow, post: Authorized<Post, Publish>) -> Result<Json<Record<Post>>> {
        Ok(Json(self.posts.publish(&uow, post.into_inner()).await?))
    }

    #[patch("/{id}")]
    #[can(Update, Post, from = "id")]
    async fn update(&self, uow: Uow, post: Authorized<Post, Update>, Valid(Json(input)): Valid<Json<UpdatePost>>) -> Result<Json<Record<Post>>> {
        post.check_fields(&input.changed_fields())?; // 403 "You may not change these fields: ..."
        Ok(Json(self.posts.update(&uow, post.into_inner(), input).await?))
    }
}
```

| Attribute | Passes when |
|---|---|
| `#[roles("admin", "editor")]` | the user has any of the roles (the super-admin role always) |
| `#[permissions("posts.export")]` | an **unconditional** rule grants every listed permission |
| `#[can(Create, Post)]` | the type-level check passes (a conditional `can` counts) |
| `#[can(Update, Post, from = "id")]` | the record with that id exists and the rules allow it |

Guests never pass `#[can]`: make such a route `#[public]` and filter with
`Accessible`. A refusal is logged and, with `AuditModule`, written as
`access.denied`.

## Custom actions and subjects that aren't models

```rust
use navree::access::{ActionMarker, Subject};

pub const PUBLISH: &str = "publish";

pub struct Publish;

impl ActionMarker for Publish {
    const NAME: &'static str = PUBLISH;
}

pub struct Report; // not a table

impl Subject for Report {
    const SUBJECT: &'static str = "Report";
    const PERMISSION_PREFIX: &'static str = "reports";
}
// module: imports = [AccessModule::for_subject::<Report>()]
// routes: #[can(Read, Report)], #[can("archive", Report)]
```

## The abilities function (`src/abilities.rs`)

```rust
use navree::access::{AbilityCtx, CREATE, DELETE, READ, Rules, UPDATE, field};

use crate::modules::posts::PUBLISH;

pub fn define(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules); // each permission of the user's roles: `posts.update` -> can update Post
    crate::modules::posts::policies::moderation::define(ctx, rules);
    // navree:policies
    rules.can(READ, "Post").when(field("published").eq(true)); // guests included
    let Some(user) = &ctx.user else {
        return;
    };
    let own = || field("owner_id").eq(user.id);
    rules.can(READ, "Post").when(own());
    rules.can(CREATE, "Post");
    rules.can(UPDATE, "Post").fields(["name"]).when(own()); // only these fields
    rules.can(PUBLISH, "Post").when(own());
    rules.can(DELETE, "Post").when(own());
    rules
        .cannot(DELETE, "Post")
        .when(field("published").eq(true))
        .because("Published posts are kept."); // last, so it wins
}
```

- **The last matching rule wins**; no match means no. Put `cannot` after the
  `can` rules it restricts.
- Conditions compare the record's own columns: `eq`, `ne`, `lt`, `lte`,
  `gt`, `gte`, `is_in([..])`, `not_in([..])`, combined with `.and(..)`. No
  `$or` (write two rules), no nested fields or relations.
- `ctx.user` is `Some(AccessUser { id, roles })` (`has_role`,
  `has_any_role(&[..])`) or `None` for guests; `ctx.permissions` lists the
  permission strings. The function can't query the database.
- The app root wires it: `AccessModule::for_root(AccessOptions { abilities:
  abilities::define, super_admin_role: Some("admin".to_owned()) })`. The
  super-admin gets `can manage all` after your rules and passes every check.

## Checks in services

`Abilities(ability)` (extractor) or `Authorized::ability()` give the
compiled rules: `ability.can(PUBLISH, "Post", Some(&record_json))` with the
record's JSON (`navree::orm::access::record_json(&record)` for an ORM model,
or `ability.authorize(Publish, &record)?` from `AbilityExt`;
`navree::access::row_json(&row)` for a navree-db record),
`ability.can_field(..)`, `ability.permitted_fields(..)`. `CurrentAccess`
adds roles, permissions and `super_admin`.

## Roles and permissions

```sh
navree access:role editor Publishes posts       # create a role
navree access:grant editor posts.publish        # give it a permission
navree access:explain <user-id> publish Post [post-id]   # which rule decides
```

In code (seeders, commands, admin screens), with a unit of work:

```rust
let uow = db.uow(None, None).await;
access.create_role(&uow, "editor", Some("Publishes anyone's posts")).await?;
access.grant(&uow, "editor", "posts.publish").await?;
access.assign_role(&uow, user_id, "editor").await?;
uow.commit().await?;
```

Also `revoke`, `sync_role_permissions`, `remove_role`, `sync_user_roles`,
`grant_to_user`, `roles_of`, `permissions_of`. The writes return `false`
when nothing changed (seeders can rerun), bump the access version (every
instance rebuilds abilities on the next request) and are audited. HTTP
routes under `/api/access` (permission `roles.manage`) manage roles; the
audit log route needs `audit.read`. Rows changed directly in the database
aren't seen until `AbilityFactory::clear_cache()` or a restart.

## Test the rules as a function

```rust
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use navree::access::{AbilityCtx, DELETE, Rules};
    use serde_json::json;
    use uuid::Uuid;

    use super::define;

    #[test]
    fn owners_cannot_delete_published_posts() {
        let me = Uuid::now_v7();
        let subjects = Arc::new(BTreeMap::from([("posts".to_owned(), "Post".to_owned())]));
        let ctx = AbilityCtx::for_user(me, vec!["writer".into()], vec![]).with_subjects(subjects);
        let mut rules = Rules::new();
        define(&ctx, &mut rules);
        let ability = rules.build();
        assert!(ability.can(DELETE, "Post", Some(&json!({ "owner_id": me, "published": false }))));
        assert!(!ability.can(DELETE, "Post", Some(&json!({ "owner_id": me, "published": true }))));
    }
}
```

`AbilityCtx::guest()` builds a guest. Over HTTP, `principal(id)` has no
roles: it is the right client for the refused side; give real roles through
`AccessService` for the allowed side of `#[roles]`/`#[permissions]`.

## Common mistakes

- Expecting `#[permissions("posts.update")]` to pass with an owner-only
  rule: permissions need unconditional rules. Use `#[can(.., from = "id")]`.
- Listing with `Post::query()` without `Accessible`: users see rows they
  then can't open.
- A `from = "id"` route on a subject not registered with `for_feature` (500).
- Soft-deleted rows still load in record checks: check `deleted_at`.
- Integers above 2^53 - 1 in conditions: pass them as strings.
- Making a route `#[public]` to fix a 403: grant the permission or add a rule.

## Docs

- https://navree.com/docs/security/authorization/
- https://navree.com/docs/basics/access-control/
