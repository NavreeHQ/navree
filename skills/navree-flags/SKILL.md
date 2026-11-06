---
name: navree-flags
description: "Feature flags with navree-flags: the opt-in `flags` facade feature, `FlagsModule` and the `flags` config section, flags as types with `#[flag]` (`name`, `default`, `expose`) registered with `FlagsModule::for_feature::<F>()` or `m.flag::<F>()`, checks with `CurrentFlags`, `Arc<Flags>` and `FlagScope` (user, workspace, both, everyone), `#[requires_flag(F)]` on routes (404, or 403 with `status = 403`), overrides, the global state, percentage rollouts with stable buckets, rollouts by plan through a `dyn PlanResolver`, the kill switch, `GET /api/flags` for the front end, `/api/admin/flags` (`flags.manage`), the `flags:*` commands, caching, `flag.changed` events and audit entries, and `FlagsFake` / `flags.store = \"memory\"` in tests. Use when hiding a feature until it is released, rolling one out to some users, workspaces or plans, adding a kill switch, serving flags to a front end, testing code behind a flag, or debugging a flag that is unexpectedly on or off, a route that answers 404, or `FlagsError::Unregistered`."
---

# Feature flags

`navree::flags` (crate `navree-flags`, facade feature `flags`, **opt-in**: not
part of `full`) keeps shipping code apart from releasing it. A flag is a Rust
type; it is resolved for a scope (a user, a workspace, both or nobody); its
state lives in two tables (`feature_flags`, `feature_flag_overrides`) with the
app's cache in front. Everything below is in the code: when in doubt read
`crates/navree-flags/src` and its tests in `crates/navree-flags/tests/flags/`
and `crates/navree/tests/flags.rs` (the docs examples through the facade).

## Steps: add flags to an app

1. Turn the feature on by name (`full` does not include it; listing it keeps
   the default features):

```toml
[dependencies]
navree = { git = "https://github.com/NavreeHQ/navree", features = ["flags"] }
```

   This brings `navree::flags` (the crate plus the `#[flag]` macro) and puts
   `FlagsModule`, `CurrentFlags` and `ModuleBuilderFlagsExt` in the prelude.

2. Add the module to the app root. `flags.store = "database"` (the default)
   needs `DatabaseModule`; the admin routes (on by default) declare the
   `flags.manage` permission, so they need `AccessModule` (or set
   `flags.admin_routes = false`). `CacheModule` (cache), `AuditModule`
   (audit entries), `EventsModule` (`flag.changed`) and `WorkspaceModule`
   (request workspaces, slugs in commands) are picked up when present:

```rust,ignore
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(DatabaseModule::from_config())
    .module(CacheModule::from_config())
    .module(AuthModule::from_config())
    .module(AccessModule::for_root(AccessOptions::default()))
    .module(WorkspaceModule::from_config())
    .module(FlagsModule::from_config()) // section `flags`; or FlagsModule::for_root(FlagsOptions { .. })
```

   The module is global and contributes the migration set `flags`. Run
   `navree migrate` after adding it.

3. Define each flag as a unit struct. The doc comment is its description:

```rust
use navree::flags::flag;

/// The redesigned deal pipeline board.
#[flag]                                   // name "new-pipeline", default off
pub struct NewPipeline;

/// AI summaries on the deal page.
#[flag(name = "ai-summaries", expose)]    // served by GET /api/flags
pub struct AiSummaries;

/// The CSV export of deals; on unless someone turns it off.
#[flag(default = true)]
pub struct DealExport;
```

4. Register each flag in the module that owns it:

```rust,ignore
#[module(
    imports = [
        FlagsModule::for_feature::<NewPipeline>(),
        FlagsModule::for_feature::<AiSummaries>(),
        FlagsModule::for_feature::<DealExport>(),
    ],
    providers = [DealService],
    controllers = [DealsController],
)]
pub struct DealsModule;
```

   (or `m.flag::<NewPipeline>()` in a `register = Self::wire` function).

## Checking a flag

In a handler, take `CurrentFlags` (the signed-in user and, when the request
names a workspace the user belongs to, that workspace; memoized per request):

```rust,ignore
use navree::flags::CurrentFlags;

#[get("/board")]
#[can(Read, Deal)]
async fn board(&self, uow: Uow, flags: CurrentFlags) -> Result<Json<DealBoard>> {
    let board = if flags.active::<NewPipeline>().await? {
        self.deals.board_v2(&uow).await?
    } else {
        self.deals.board(&uow).await?
    };
    Ok(Json(board))
}
```

A route that only exists behind a flag: `#[controller]` consumes
`#[requires_flag]` (nothing to import). While the flag is off the route
answers 404 (or 403 with `status = 403`) before `#[can]`, other guards,
extractors and the handler; authentication still runs first, so a guest on an
authenticated route gets 401. OpenAPI shows `x-navree-flag: new-pipeline`.

```rust,ignore
#[get("/forecast")]
#[can(Read, Deal)]
#[requires_flag(NewPipeline)]            // or #[requires_flag(NewPipeline, status = 403)]
async fn forecast(&self) -> Result<Json<Forecast>> { /* ... */ }
```

On a hand-written `RouteDef`: `navree::flags::requires_flag::<NewPipeline>(route, 404)`
(guard plus extension) or `.guard(RequireFlag::guard::<NewPipeline>())`.

Anywhere else, inject `Arc<Flags>` and pass a scope:

```rust,ignore
use navree::flags::{FlagScope, Flags};

#[injectable]
pub struct DealSummaries {
    flags: Arc<Flags>,
}

impl DealSummaries {
    pub async fn maybe_summarize(&self, owner_id: Uuid, workspace_id: Uuid) -> Result<Option<String>, BoxError> {
        let scope = FlagScope::user(owner_id).workspace(workspace_id);
        if !self.flags.active::<AiSummaries>(&scope).await? {
            return Ok(None);
        }
        Ok(Some(summarize().await?))
    }
}
```

Scopes: `FlagScope::user(id)` (a `UserScope`; `.workspace(id)` narrows it),
`FlagScope::workspace(id)`, `FlagScope::everyone()`, or a `&Workspace`,
`&CurrentWorkspace`, `&Principal`, `&User`, `&CurrentUser`.
`flags.all(scope)` returns every registered flag's value by name.

## Resolution order

For user U in workspace W, the first rule that decides wins:

1. killed (off for everyone, overrides included);
2. U's override;
3. W's override;
4. the global state (`FlagScope::everyone()` writes it);
5. the rollout, when U (or W, for a workspace rollout) is inside it;
6. the `default` from `#[flag]`.

A scope without a user skips rule 2, one without a workspace rule 3, and a
rollout counting what the scope lacks does not include it. A check with
neither (jobs, commands) is decided by 1, 4 and 6 only; with
`flags.missing_scope = "off"` it is off unless the flag is on for everyone.

## Changing flags

```rust,ignore
use navree::flags::{FlagScope, Flags, Rollout};

flags.activate::<NewPipeline>(FlagScope::workspace(agra.id)).await?;   // override
flags.deactivate::<NewPipeline>(FlagScope::user(grace.id)).await?;     // override
flags.forget::<NewPipeline>(FlagScope::user(grace.id)).await?;         // remove it
flags.activate::<NewPipeline>(FlagScope::everyone()).await?;           // global state

flags.rollout::<NewPipeline>(Rollout::percent(10)).await?;                  // 10% of users
flags.rollout::<NewPipeline>(Rollout::percent(50).of_workspaces()).await?;  // 50% of workspaces
flags.rollout::<AiSummaries>(Rollout::plans(["pro", "business"])).await?;   // every Pro/Business workspace
flags.rollout::<AiSummaries>(Rollout::percent(50).of_workspaces().on_plans(["pro"])).await?;
flags.stop_rollout::<NewPipeline>().await?;

flags.kill::<NewPipeline>().await?;    // incident: off for everyone
flags.revive::<NewPipeline>().await?;  // back to what it had
let state = flags.state::<NewPipeline>().await?; // FlagState { everyone, rollout, killed, updated_at, updated_by, .. }
```

- Writes join the task's unit of work (a request's while its handler runs) or
  open their own; they clear the cached state they touch, emit `FlagChanged`
  (`flag.changed`) after commit with `EventsModule`, and write a
  `flag.changed` audit entry in the same transaction with `AuditModule`.
- The bucket of a user or workspace is `SHA-256("<flag name>:<id>")`, first 8
  bytes big-endian, mod 100 (`navree::flags::bucket`); inside when below the
  percentage. Raising the percentage never removes anyone.
- Plan rollouts read the app's `dyn PlanResolver`, which the app registers
  (one per app; nothing registers it for you). A workspace it gives `None` is
  outside. With billing, read the workspace's local subscription copy (the
  same code runs in `crates/navree/tests/flags.rs`, `with_billing`):

```rust,ignore
use navree::BoxFuture;
use navree::billing::{BillableRef, Billing, Subscription, WORKSPACE_BILLABLE};
use navree::flags::PlanResolver;

#[injectable]
pub struct SubscriptionPlans {
    billing: Arc<Billing>,
}

impl PlanResolver for SubscriptionPlans {
    fn plan(&self, workspace: Uuid) -> BoxFuture<'_, Result<Option<String>, BoxError>> {
        Box::pin(async move {
            let billable = BillableRef::new(WORKSPACE_BILLABLE, workspace);
            let subscription = self.billing.subscription(&billable).await?;
            Ok(subscription.filter(Subscription::valid).map(|s| s.plan))
        })
    }
}

// in the register function of the module that owns billing:
m.plan_resolver::<SubscriptionPlans>();
```

## Front end, admin routes and commands

- `GET /api/flags` (authenticated, `flags.routes`): `{"ai-summaries": true}`,
  only flags with `expose`, resolved for the user and `X-Workspace`;
  `Cache-Control: private, max-age=<cache_ttl_secs>`.
- `/api/admin/flags` (permission `flags.manage`, `flags.admin_routes`):
  `GET /`, `GET /{name}`, `PUT /{name}` (`{"everyone": true|false|null,
  "rollout": {..}|null, "killed": bool}`, fields present change),
  `PUT /{name}/overrides` (`{"user_id"|"workspace_id", "active"}`),
  `DELETE /{name}/overrides?user_id=..` (or `workspace_id`). Unknown flag 404;
  bad rollout or override target 400.
- Commands (through `navree <command>`): `flags:list`,
  `flags:on|off|forget <flag> [--user <id>] [--workspace <id-or-slug>]`,
  `flags:rollout <flag> <percent> [--of users|workspaces] [--plans pro,business]`,
  `flags:rollout <flag> off`, `flags:kill <flag>`, `flags:revive <flag>`,
  `flags:purge [--dry-run]`.

## Testing templates

The memory store, with the flags each test needs:

```rust,ignore
use navree::access::AccessService;
use navree::db::Db;
use navree::flags::{FlagScope, Flags};
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn the_forecast_is_hidden_until_the_new_pipeline_is_on() {
    let app = TestApp::builder()
        .temp_database()
        .config("flags.store", "memory")
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(DealsModule)
        .module(FlagsModule::from_config())
        .build()
        .await;
    // The route's #[can(Read, Deal)]: the rep needs `deals.read`.
    let rep_id = Uuid::now_v7();
    let uow = app.get::<Db>().await.uow(None, None).await;
    app.get::<AccessService>().await
        .grant_to_user(&uow, rep_id, "deals.read").await.unwrap();
    uow.commit().await.unwrap();
    let rep = app.acting_as(principal(rep_id));

    rep.get("/api/v1/deals/forecast").send().await.assert_error(404, None);
    app.get::<Flags>().await
        .activate::<NewPipeline>(FlagScope::user(rep_id)).await.unwrap();
    rep.get("/api/v1/deals/forecast").send().await
        .assert_status(200)
        .assert_json_path("currency", json!("USD"));
}
```

A `FlagsFake` answers what it is told, whatever the scope, and records checks:

```rust,ignore
use navree::flags::FlagsFake;

let fake = FlagsFake::new().on::<NewPipeline>().off::<AiSummaries>();
let app = TestApp::builder()
    .override_value(Arc::new(fake.flags()))
    // ... modules ...
    .build()
    .await;
// ... exercise the app ...
fake.assert_checked::<NewPipeline>();
fake.assert_not_checked::<DealExport>();
fake.set::<NewPipeline>(false);              // change an answer mid-test
assert_eq!(fake.checked::<NewPipeline>(), 1); // fake.checks_of::<F>() lists the scopes
```

Who is inside a rollout: `Rollout::percent(25).of_workspaces().includes::<NewPipeline>(workspace_id)`.

## Checks before you finish

- [ ] `features = ["flags"]` on the `navree` dependency.
- [ ] Every flag type is registered once (`for_feature` or `m.flag`), in the module that owns it.
- [ ] `AccessModule` in the app, or `flags.admin_routes = false`.
- [ ] The `flags` migrations ran (`feature_flags`, `feature_flag_overrides`).
- [ ] Routes behind a flag still declare access (`#[can]`, `#[authenticated]`, `#[public]`).
- [ ] Only flags meant for the browser have `expose`.
- [ ] Tests cover both the on and off path (memory store or `FlagsFake`).

## Gotchas

- `FlagsError::Unregistered` on the first check of a flag no module registered;
  two types with one name fail the boot, naming both.
- The stored name is the key: renaming the type renames a `#[flag]` without
  `name = ".."` and loses its state. Pin `name` before renaming; clean up with
  `flags:purge`.
- An override is for one user or one workspace: `activate(FlagScope::user(u).workspace(w))`
  is `FlagsError::InvalidScope`.
- A percentage above 100 is `FlagsError::InvalidRollout`; a plan rollout in an
  app without a `dyn PlanResolver` is `FlagsError::NoPlanResolver`.
- `CurrentFlags` drops the workspace when the request names none, an unknown
  one, or one the user is not a member of; a guest resolves for nobody.
- `#[requires_flag]` goes on route methods only, once per route; `status` is
  403 or 404. It needs `navree` with the `flags` feature (or `navree-flags`).
- Several instances: with the memory cache store (the default) the cache is
  per instance, so a change reaches the others within `cache_ttl_secs` (60).
  Lower it, `0` to read the database on every check, or use
  `cache.store = "redis"`, where a change clears the shared entries at once.
- The flags store (`flags.store = "memory"`) is per process too: only for
  tests and single-process tools.
- The memory store has no transactions: changes apply at once, even in a unit
  of work that later rolls back.
- `flags:on --user <id>` checks only that the id is a UUID; it does not look
  the user up. A typo silently creates an override for nobody, so copy ids.
- `flags:list`, the admin routes and `GET /api/flags` list registered flags
  only; state left behind by a deleted flag shows up in `flags:purge --dry-run`.
