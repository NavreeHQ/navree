---
name: navree-settings
description: "Typed settings with inheritance in navree-settings: the `settings` facade feature (in `full`), `SettingsModule` and the `settings` config section, settings types with `#[derive(Settings)]` (`key`, `scopes(global, organization, workspace, user)`, `permission`, `read_permission`) registered with `SettingsModule::for_feature::<T>()` or `m.settings::<T>()`, `Scope`, resolution field by field down global → organization → workspace → user with a `Source` per field, `Resolved<T>`, the `Setting<T>` extractor, `Settings::update` (validated, errors by field), `inherit` and `reset` in a unit of work, `settings.updated` audit entries and `settings.changed` events, cache tags per key and level, `GET`/`PATCH`/`DELETE /api/settings/{key}?scope=` with `{data, sources, overridden}`, the `settings:*` commands, the Helm Settings screens, and `SettingsFake` / `settings.store = \"memory\"` in tests. Use when adding settings an app, organization, workspace or user can change, showing where a value comes from, building a settings screen, resetting a field to inherit, testing code that reads settings, or debugging a value that does not apply, a 400 on a settings write, or `SettingsError::Unregistered`."
---

# Settings with inheritance

`navree::settings` (crate `navree-settings`, facade feature `settings`, in
`full`) stores typed settings per level and resolves them field by field down
the chain global → organization → workspace → user, with the source of every
value. Values live in one table, `settings`, with the app's cache in front.
Everything below is in the code: when in doubt read
`crates/navree-settings/src`, its tests in
`crates/navree-settings/tests/settings/`, and
`crates/navree/tests/settings_docs.rs` (the docs examples through the facade).

## Steps: add settings to an app

1. The feature is in `full`. With a hand-picked feature list, add
   `"settings"`. It brings `navree::settings` (the crate plus
   `#[derive(Settings)]`) and puts `SettingsModule`, `Setting` and
   `ModuleBuilderSettingsExt` in the prelude.

2. Add the module to the app root. `settings.store = "database"` (the
   default) needs `DatabaseModule`. `CacheModule` (cache), `AuditModule`
   (`settings.updated` entries), `EventsModule` (`settings.changed`, cleanup
   of deleted workspaces and organizations), `WorkspaceModule` (organization
   lookups, request scopes, slugs in commands) and `AccessModule` (the
   `settings.manage` check on global writes) are picked up when present:

```rust
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(DatabaseModule::from_config())
    .module(CacheModule::from_config())
    .module(EventsModule::for_root())
    .module(AuditModule::from_config())
    .module(AccessModule::for_root(AccessOptions::default()))
    .module(AuthModule::from_config())
    .module(WorkspaceModule::from_config())
    .module(SettingsModule::from_config()) // section `settings`; or SettingsModule::for_root(SettingsOptions { .. })
```

   The module is global and contributes the migration set `settings`. Run
   `navree migrate` after adding it.

3. Define the type: a struct whose fields are the settings, with
   `Serialize`, `Deserialize`, `Validate` (every field needs a rule
   or `#[validate(skip)]`) and a `Default` that passes its own rules. The doc
   comment is its description:

```rust
use navree::settings::Settings;
use navree::validation::Validate;
use serde::{Deserialize, Serialize};

/// What the member portal shows.
#[derive(Settings, Clone, Debug, Serialize, Deserialize, Validate)]
#[settings(key = "portal", scopes(global, organization, workspace, user))]
pub struct PortalSettings {
    #[validate(skip)]
    pub show_leaderboard: bool,
    #[validate(length(max = 200))]
    pub welcome_text: String,
    #[validate(range(min = 1, max = 100))]
    pub page_size: u32,
}

impl Default for PortalSettings {
    fn default() -> Self {
        Self { show_leaderboard: false, welcome_text: "Welcome".into(), page_size: 25 }
    }
}
```

   `key` defaults to the type name without `Settings`, in kebab case;
   `scopes` to `global, organization, workspace`. `permission = ".."`
   replaces the per-level write permissions; `read_permission = ".."` guards
   reads through the routes.

4. Register it in the module that owns it:

```rust
#[module(
    imports = [SettingsModule::for_feature::<PortalSettings>()],
    providers = [PortalService],
    controllers = [PortalController],
)]
pub struct PortalModule;
```

   or `m.settings::<PortalSettings>()` in a `register` function.

## Reading

```rust
// In a handler: resolved for the request's user, workspace and its organization.
#[get("/home")]
#[authenticated]
async fn home(&self, portal: Setting<PortalSettings>) -> Json<PortalHome> {
    Json(PortalHome { welcome: portal.welcome_text.clone(), leaderboard: portal.show_leaderboard })
}

// Anywhere: inject Arc<Settings> and pass a scope.
let portal = settings.resolve::<PortalSettings>(Scope::workspace(acme.id)).await?;
portal.page_size;                       // derefs to the value
portal.source("welcome_text");          // Some(Source::Organization)
portal.overridden;                      // fields stored at the workspace's own level
let size = settings.get::<PortalSettings>(Scope::user(ada.id)).await?.page_size;
```

Scopes: `Scope::global()`, `Scope::organization(id)`, `Scope::workspace(id)`
(its organization looked up with `WorkspaceModule`, or named with
`.in_organization(org)`), `Scope::user(id).in_workspace(ws)`. A `&Workspace`,
`&CurrentWorkspace`, `&Organization`, `&CurrentOrganization` or `&Principal`
converts. Levels a type does not list are skipped.

`Setting<T>` refuses a workspace the user may not enter (as
`CurrentWorkspace` does), resolves without a workspace when the request names
none, gives guests the global values, and checks no permission.

## Writing

```rust
settings.update::<PortalSettings>(&uow, Scope::workspace(acme.id), json!({"welcome_text": "Hi"})).await?;
settings.inherit::<PortalSettings>(&uow, Scope::workspace(acme.id), "welcome_text").await?;
settings.reset::<PortalSettings>(&uow, Scope::workspace(acme.id)).await?;
```

- The patch is anything that serializes to a JSON object. Writes go to the
  scope's target (its most specific level).
- The values after the patch, inherited ones included, are validated with the
  type's rules: `SettingsError::Invalid` names the fields (a 400 with
  `errors` when returned from a handler). Unknown fields are
  `UnknownFields` (400), a level the type does not allow `ScopeNotAllowed`
  (400).
- A field set to the value it inherits is still stored (pinned at that level).
  A patch that changes nothing writes nothing.
- Writes join the unit of work's transaction, write `settings.updated` to the
  audit log in it, clear the cache tags they touch, and emit
  `SettingsChanged` (`settings.changed`) after the commit.

By key, as JSON (what routes and commands use): `resolve_key`, `update_key`,
`inherit_key`, `reset_key`.

## Routes, commands and Helm

| Route (signed in) | Answers |
|---|---|
| `GET /api/settings` | Every registered type: `{data: [{key, description, scopes, fields}]}` |
| `GET /api/settings/{key}?scope=` | `{data, sources, overridden}` |
| `PATCH /api/settings/{key}?scope=` | Sets fields; the result |
| `DELETE /api/settings/{key}?scope=` | All fields at the level inherit; the result |
| `DELETE /api/settings/{key}/{field}?scope=` | One field inherits; the result |

`scope` = `global`, `organization` (`X-Organization`), `workspace`
(`X-Workspace`) or `user`; default: the most specific level the type allows.
Writes need the type's `permission`, else `settings.manage` (global,
app-wide), `organization.update` or `workspace.update` (in the request's
organization or workspace); users write their own. `settings.routes = false`
turns the routes off.

```sh
navree settings:list
navree settings:show portal --workspace acme
navree settings:set portal page_size=50 "welcome_text=Hello there"   # global
navree settings:set portal show_leaderboard=true --organization holding
navree settings:inherit portal welcome_text --workspace acme
navree settings:inherit portal --all --user 0192e4c1-5b1e-7a40-9f43-2c0d1b8e6a11
```

Helm (features `helm` and `settings`, both in `full`): **System → Settings**
for users holding `settings.manage` (or the type's permission) app-wide;
edits a level named by id or slug, saves only the changed fields, and has an
**Inherit** button per field set at the level.

## Testing templates

App test on the database store:

```rust
let app = TestApp::builder()
    .temp_database()
    .module(DatabaseModule::from_config())
    .module(PortalModule)
    .module(SettingsModule::from_config())
    .build()
    .await;
let settings = app.get::<Settings>().await;
let uow = app.get::<Db>().await.uow(None, None).await;
settings.update::<PortalSettings>(&uow, Scope::global(), json!({"welcome_text": "Hello"})).await.unwrap();
uow.commit().await.unwrap();
```

Unit test with the fake (real resolution and validation, in memory, no
registration):

```rust
let fake = SettingsFake::new().attach(acme, holding);      // workspace → organization
fake.set::<PortalSettings>(Scope::organization(holding), json!({"show_leaderboard": true}))
    .await
    .unwrap();                                              // setup, not recorded
let service = PortalService { settings: Arc::new(fake.settings()) };
```

In an app test, `override_value(Arc::new(fake.settings()))` installs it;
`fake.assert_changed::<T>(scope)`, `assert_not_changed::<T>()`,
`changes_of::<T>(scope)` inspect writes. Call `.register::<T>()` first when
the code reads by key (routes, commands).

## Checks before you finish

- [ ] Every settings type is registered once (`for_feature` or `m.settings`).
- [ ] Its `Default` passes its own validation rules (otherwise every write fails).
- [ ] `scopes(..)` lists only the levels that make sense for it.
- [ ] Writes take the request's or job's `Uow`.
- [ ] The `settings` migrations ran.
- [ ] Screens use `sources` and `overridden` to show inherited values and
      offer a reset.
- [ ] Tests cover the inherited and the overridden case.

## Gotchas

- `SettingsError::Unregistered` on the first read of a type no module
  registered; two types with one key fail the boot, naming both.
- The key is the stored identity: renaming the type changes a derived key
  and orphans its rows. Pin `key = ".."` before renaming.
- Validation runs on the merged values: a global value that breaks a newer
  rule makes every write below it fail on that field until it is fixed.
- A stored value whose type changed is skipped (logged), so the level below
  shows through; fix it by setting the field again.
- `Scope::from(&workspace)` uses the model's `organization_id` as loaded; a
  model read before an attach has none. `Scope::workspace(id)` looks it up.
- Without `EventsModule`, deleted workspaces and organizations keep their
  rows (`Settings::forget_scope` removes them), and attaching a workspace
  does not clear its cached organization until `cache_ttl_secs` passes.
- The memory store is per process and not transactional: a write sticks even
  when its unit of work rolls back. Use it for tests and single-process tools.
- Several instances: with the memory cache store, a change reaches the others
  within `settings.cache_ttl_secs` (300); use `cache.store = "redis"` or a
  lower TTL for faster propagation.
- `--user <id>` in the commands is not looked up; copy ids.
