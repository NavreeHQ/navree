---
name: navree-workspaces
description: "Multi-tenancy with navree-workspace: `WorkspaceModule` and the `workspace` config section, `CurrentWorkspace` and the `InWorkspace` guard (header, subdomain, path prefix or JWT claim), workspace roles (`owner`, `admin`, `member`, `viewer`, app-defined) feeding navree-access, `WorkspaceRepo` and `WorkspaceScoped`, row isolation with PostgreSQL row-level security (`RlsPolicy`), schema and database isolation (`workspace_migrations`, `WorkspaceUow`, `WorkspaceDatabases`), invitations, events and the `/api/workspaces` routes. Use when building a SaaS or team feature, adding data that belongs to a workspace (team, organization, tenant), checking permissions per workspace, inviting users, or debugging a 400/403/404 from a workspace route or data leaking between workspaces."
---

# Workspaces

A workspace is a tenant: it has members with one role each, invites people
by email, and keeps its data apart from other workspaces'. The module is
`navree::workspace` (crate `navree-workspace`, facade feature `workspace`,
on by default). Everything below is in the code; when in doubt read
`crates/navree-workspace/src` and its tests in
`crates/navree-workspace/tests/workspace/`.

## Steps: make a resource workspace-owned (row isolation, the default)

1. Add the module to the app root (once). It needs `ConfigModule` and
   `DatabaseModule`; its routes need `AccessModule` and an authenticator
   (`AuthModule`); `EventsModule` and `MailModule` (+ `ViewModule`) are
   optional:

```rust
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(CryptoModule::from_config())
    .module(HttpModule::from_config())
    .module(DatabaseModule::from_config())
    .module(EventsModule::for_root())
    .module(ViewModule::from_config())
    .module(MailModule::from_config())
    .module(AuthModule::from_config())
    .module(AccessModule::for_root(AccessOptions::default()))
    .module(WorkspaceModule::from_config()) // section `workspace`
    .module(DealsModule)
```

2. Give the table a `workspace_id uuid not null` column (index it), run the
   migration after the `workspace` set, and add the RLS policy on
   PostgreSQL:

```rust
m.migrations(MigrationSet::new("deals").after(["workspace"]).migration(CreateDeals));

// inside CreateDeals::up, after create_table:
if manager.get_database_backend() == DbBackend::Postgres {
    manager
        .get_connection()
        .execute_unprepared(&RlsPolicy::new("deals").create_sql()) // needs `ConnectionTrait` in scope
        .await?;
}
```

3. Mark the entity and register it as workspace-scoped:

```rust
impl Subject for deal::Entity {
    const SUBJECT: &'static str = "Deal";
    const PERMISSION_PREFIX: &'static str = "deals";
}
impl WorkspaceScoped for deal::Entity {} // column `workspace_id` (override WORKSPACE_COLUMN)
pub type Deal = deal::Entity;

#[module(
    imports = [AccessModule::for_feature::<Deal>()],
    controllers = [DealsController],
    register = Self::wire,
)]
pub struct DealsModule;

impl DealsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("deals").after(["workspace"]).migration(CreateDeals))
            .workspace_scoped::<Deal>(); // `can(.., from = "id")` 404s other workspaces' records
    }
}
```

4. Guard the controller and go through `WorkspaceRepo`:

```rust
use navree::db::paginate;
use navree::prelude::*;

#[injectable]
pub struct DealsController;

#[controller("/deals", version = "1")]
#[guard(InWorkspace::guard())] // resolve + membership + workspace abilities BEFORE route checks
impl DealsController {
    #[get("/")]
    #[permissions("deals.read")]
    async fn index(&self, ws: CurrentWorkspace, uow: Uow, page: PageQuery) -> Result<Page<deal::Model>> {
        Ok(paginate(WorkspaceRepo::<Deal>::select(ws.id()), &*uow.tx().await?, page).await?)
    }

    #[post("/")]
    #[permissions("deals.create")]
    async fn store(&self, ws: CurrentWorkspace, uow: Uow, Json(input): Json<NewDeal>) -> Result<Created<deal::Model>> {
        let deal = WorkspaceRepo::<Deal>::insert(&uow, ws.id(), deal::ActiveModel {
            id: Set(Uuid::now_v7()),
            title: Set(input.title),
            ..Default::default() // workspace_id is set by the repo
        }).await?;
        Ok(Created(format!("/api/v1/deals/{}", deal.id), deal))
    }

    #[patch("/{id}")]
    #[can(Update, Deal, from = "id")]
    async fn update(&self, ws: CurrentWorkspace, uow: Uow, deal: Authorized<Deal, Update>, Json(input): Json<DealChanges>) -> Result<Json<deal::Model>> {
        let mut row = deal.into_inner().into_active_model();
        row.title = Set(input.title);
        Ok(Json(WorkspaceRepo::<Deal>::update(&uow, ws.id(), row).await?))
    }
}
```

5. Grant the permissions per role in config, then test (template below):

```toml
[workspace.roles.admin]
permissions = ["deals.*"]
[workspace.roles.member]
permissions = ["deals.read", "deals.create", "deals.update"]
[workspace.roles.viewer]
permissions = ["deals.read"]
```

## Resolution

| Resolver | Reads | Config |
|---|---|---|
| `header` | `X-Workspace: acme` (slug or id) | `workspace.header` |
| `path` | segment after the prefix: `/api/v1/w/acme/deals` | `workspace.path_prefix` (`/w`) |
| `subdomain` | `acme.app.example.com` (Host header, one label) | `workspace.base_domain` (unset = off) |
| `claim` | claim of the bearer JWT, authenticated requests only | `workspace.claim` (`workspace`) |

Order: `workspace.resolvers` (default `["header", "path", "subdomain",
"claim"]`, env `APP_WORKSPACE__RESOLVERS=subdomain,header`); first hit wins.
A UUID is looked up by id, anything else by slug. Navree's own access tokens
have no workspace claim (fixed claims `sub iat exp iss aud jti typ`): the
claim resolver is for tokens from another issuer or a custom strategy.

`InWorkspace::guard()` uses the resolvers; `InWorkspace::from_path("workspace")`
reads a route path parameter instead. Answers: 401 guest, 400 `The request
names no workspace.`, 404 `Workspace not found.`, 403 `You are not a member
of this workspace.`

`CurrentWorkspace` (extractor, derefs to the `Workspace` row): `id()`,
`workspace()`, `member()`, `role()`, `permissions()`, `ability()`,
`access()`, `can(action, subject)`, `has_permission("deals.export")`,
`authorize("deals.export")?` (403), `scope(select)`, `isolation()`,
`resolver()`.

## Roles and permissions

| Role | Built-in permissions |
|---|---|
| `owner` | `all.*` |
| `admin` | `workspace.read`, `workspace.update`, `workspace.members.*`, `workspace.invitations.*` |
| `member`, `viewer` | `workspace.read`, `workspace.members.read` |

- `workspace.roles.<name>.permissions` adds to a built-in role or defines a
  new role (`[workspace.roles.billing] permissions = ["invoices.*"]`). Names:
  lower-case letters, digits, `_ - .`.
- Inside a workspace the abilities function gets `ctx.user.roles ==
  [workspace role]`, `ctx.permissions == role permissions`, `ctx.team_id ==
  Some(workspace id)`. Global roles/permissions are NOT included; a global
  super-admin keeps `manage all`; token abilities still narrow.
- Module subjects: `Workspace` (`workspace.*`), `WorkspaceMember`
  (`workspace.members.*`), `WorkspaceInvitation` (`workspace.invitations.*`).

## Services (writes take `&Uow` first, emit events after commit)

```rust
let ws = workspaces.create(&uow, owner_id, NewWorkspace::named("Acme Inc.")).await?; // slug "acme-inc"
workspaces.rename(&uow, ws.id, "Acme Corp").await?;          // slug unchanged
workspaces.for_user(user_id).await?;                          // Vec<Membership { workspace, role }>
workspaces.resolve("acme").await?;                            // slug or id, 404 otherwise
workspaces.transfer_ownership(&uow, ws.id, new_owner).await?; // new owner: owner, old: admin
workspaces.delete(&uow, ws.id).await?;                        // members + invitations cascade
workspaces.enter(&uow, ws.id).await?;                         // jobs: navree.workspace / search_path

members.add(&uow, ws.id, user_id, WorkspaceRole::Member).await?;  // 409 if member
members.change_role(&uow, ws.id, user_id, WorkspaceRole::Admin).await?;
members.remove(&uow, ws.id, user_id).await?;                  // 409 for the owner
members.list(ws.id).await?;                                    // Vec<MemberInfo { member, user }>

let sent = invitations.invite(&uow, ws.id, "grace@example.com", WorkspaceRole::Member, Some(me)).await?;
let joined = invitations.accept(&uow, &token, user_id).await?; // AcceptedInvitation { workspace, member }
invitations.revoke(&uow, ws.id, invitation_id).await?;
invitations.prune_expired(&uow).await?;                        // or `navree workspace:prune-invitations`
```

## Routes (`workspace.routes = true` by default)

`GET/POST /api/workspaces`, `GET/PATCH/DELETE /api/workspaces/{workspace}`,
`POST .../transfer`, `POST .../leave`, `GET/POST .../members`,
`PATCH/DELETE .../members/{user}`, `GET/POST .../invitations`,
`DELETE .../invitations/{invitation}`, `POST /api/invitations/accept {token}`.
Permissions: `workspace.update|delete|transfer`,
`workspace.members.read|create|update|delete`,
`workspace.invitations.read|create|delete`. Granting or touching the `owner`
role needs the caller to be an owner (403 otherwise).

## Schema or database isolation

```toml
[workspace]
isolation = "schema"   # PostgreSQL only: schema workspace_<id>, search_path per request
# isolation = "database"
# database_url = "sqlite://storage/workspaces/{id}.db?mode=rwc"
```

- Tenant tables come from `m.workspace_migrations(MigrationSet::new("notes").migration(CreateNotes))`,
  not `m.migrations`. Schema: run when the workspace is created; database:
  on first open per process; both: `navree workspace:migrate [slug...]`.
- Handlers use `WorkspaceUow` (derefs to `Uow`): the request's unit of work
  for row/schema, a unit of work on the workspace's database for database
  isolation, committed when the status < 400.
- Jobs: `databases.db(id).await?.uow(None, None).await` + `commit()`
  (`WorkspaceDatabases`), or `workspaces.enter(&uow, id)` for schema.

## Testing template

```rust
let app = TestApp::from_builder(TestAuthenticator::install(app(sources))).await;
let auth = app.get::<AuthService>().await;
let ada = auth.create_user("Ada", "ada@example.com", "secret-password").await.unwrap();
let owner = app.acting_as(principal(ada.id));
owner.post("/api/workspaces").json(&json!({ "name": "Acme" })).send().await.assert_status(201);
owner
    .post("/api/v1/deals")
    .header("X-Workspace", "acme")
    .json(&json!({ "title": "Big one" }))
    .send()
    .await
    .assert_status(201);
app.acting_as(principal(Uuid::now_v7()))
    .get("/api/v1/deals")
    .header("X-Workspace", "acme")
    .send()
    .await
    .assert_error(403, "You are not a member of this workspace.");
```

Read invitation tokens from `app.get::<Mailer>().await.array_transport().unwrap().sent()`
(config `mail.transport = "array"`); move time with
`override_value::<dyn Clock>(Arc::new(FixedClock::new(now)))`.

## Gotchas

- Forgetting `#[guard(InWorkspace::guard())]`: route `#[roles]` /
  `#[permissions]` / `#[can]` then check GLOBAL access (usually 403), because
  extractors (including `CurrentWorkspace`) run after route checks.
- Querying a workspace-owned table with `Entity::find()` / `Repo` instead of
  `WorkspaceRepo` leaks rows across workspaces on SQLite (PostgreSQL RLS
  hides them). Always scope: `WorkspaceRepo::<E>::select(ws.id())`.
- `#[can(.., from = "id")]` on a workspace-owned entity without
  `m.workspace_scoped::<E>()` lets a member with an unconditional permission
  load another workspace's record on SQLite.
- RLS: the app must connect as a role that is neither superuser nor
  `BYPASSRLS`; without `navree.workspace` set (jobs that forgot
  `workspaces.enter`) every row is invisible.
- The owner (`workspaces.owner_id`) can't be removed, demoted or leave
  (409): transfer ownership first. Invitations can't offer `owner`.
- `require_matching_email` (default true) needs a user directory that knows
  the accepting user (`workspace.users_table`, default `users`); with no
  users table every accept is a 403 until you register a `UserDirectory`
  or turn the check off.
- Database isolation on PostgreSQL does not create databases: create the
  one `workspace.database_url` names for the workspace yourself (e.g. in a
  `WorkspaceCreated` listener). SQLite files are created on first use.
- Events (`workspace.created`, `.member_added`, `.member_role_changed`,
  `.member_removed`, `.ownership_transferred`, `.invitation_sent`,
  `.invitation_accepted`, `.deleted`) fire only after commit.

Docs: `/docs/workspaces/introduction/`, `/docs/workspaces/resolution/`,
`/docs/workspaces/members-and-roles/`, `/docs/workspaces/invitations/`,
`/docs/workspaces/isolation/`.
