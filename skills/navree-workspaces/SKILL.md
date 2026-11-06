---
name: navree-workspaces
description: "Multi-tenancy with navree-workspace: `WorkspaceModule` and the `workspace` config section, `CurrentWorkspace` and the `InWorkspace` guard (header, subdomain, path prefix, JWT claim or the workspace's own domain), workspace roles (`owner`, `admin`, `member`, `viewer`, app-defined) feeding navree-access, `WorkspaceRepo` and `WorkspaceScoped`, row isolation with PostgreSQL row-level security (`RlsPolicy`), schema and database isolation (`workspace_migrations`, `WorkspaceUow`, `WorkspaceDatabases`), invitations, events and the `/api/workspaces` routes; organizations above workspaces (`workspace.organizations`, `Organizations`, `OrgMembers`, `OrgRole`, `WorkspaceSelector`, `CurrentOrganization`, `InOrganization`, `OrgRepo`, access requests, `/api/organizations`) and partitions inside them (`workspace.partitions`, `Partitions`, `PartitionScoped`, `PartitionScope`, `CurrentPartition`, `PartitionRepo`, `X-Partition`); shared libraries (`Library`, `LibraryColumns`, `m.library::<M>()`, `LibraryRepo`, `LibraryScope`, clone on use, `library_read_only`); demo workspaces (`DemoSeeder`, fixtures, `workspace.demo` read-only or nightly reset, `X-Navree-Demo`). Use when building a SaaS or team feature, adding data that belongs to a workspace (team, organization, tenant), grouping workspaces under a company or partner, splitting a workspace's data by domain or region, checking permissions per workspace or organization, inviting users, seeding a demo or sales workspace, shipping templates or courses that every tenant sees, extends and clones, or debugging a 400/403/404 from a workspace or organization route or data leaking between workspaces or partitions."
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

```rust,ignore
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
   PostgreSQL. Migrations are navree-schema migrations (`navree::schema`):

```rust,ignore
m.migrations(MigrationSet::new("deals").after(["workspace"]).migration(
    SchemaMigration::new("m20261026_000001_create_deals", CreateDeals),
));

// CreateDeals: `impl navree::schema::Migration`
async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
    schema
        .create("deals", |t| {
            t.uuid("id").primary();
            t.uuid("workspace_id").index();
            t.string("title");
        })
        .await?;
    if schema.target().await?.dialect() == Dialect::Postgres {
        schema.statement(&RlsPolicy::new("deals").create_sql()).await?;
    }
    Ok(())
}
```

3. Make the table a navree-orm model (the access subject comes from the
   attribute), mark it `WorkspaceScoped` and register it as workspace-scoped:

```rust,ignore
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "deals", subject = "Deal", permission_prefix = "deals")]
pub struct Deal {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
}
impl WorkspaceScoped for Deal {} // column `workspace_id` (override WORKSPACE_COLUMN)

#[module(
    imports = [AccessModule::for_feature::<Deal>()],
    controllers = [DealsController],
    register = Self::wire,
)]
pub struct DealsModule;

impl DealsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(/* the set above */)
            .workspace_scoped::<Deal>(); // `can(.., from = "id")` 404s other workspaces' records
    }
}
```

   A model declared `#[model(workspace)]` already scopes its own queries to
   the current workspace; `WorkspaceScoped` + `WorkspaceRepo` is for naming a
   workspace explicitly. Give both the same column.

4. Guard the controller and go through `WorkspaceRepo`:

```rust,ignore
use navree::prelude::*;

#[injectable]
pub struct DealsController;

#[controller("/deals", version = "1")]
#[guard(InWorkspace::guard())] // resolve + membership + workspace abilities BEFORE route checks
impl DealsController {
    #[get("/")]
    #[permissions("deals.read")]
    async fn index(&self, ws: CurrentWorkspace, uow: Uow, page: PageQuery) -> Result<Page<Record<Deal>>> {
        Ok(WorkspaceRepo::<Deal>::query(ws.id()).paginate(&uow, page).await?)
    }

    #[post("/")]
    #[permissions("deals.create")]
    async fn store(&self, ws: CurrentWorkspace, uow: Uow, Json(input): Json<NewDeal>) -> Result<Created<Deal>> {
        let deal = Deal { id: Uuid::now_v7(), title: input.title, ..Deal::default() }; // workspace_id is set by the repo
        let deal = WorkspaceRepo::<Deal>::insert(&uow, ws.id(), deal).await?;
        Ok(Created(format!("/api/v1/deals/{}", deal.id), deal.into_inner()))
    }

    #[patch("/{id}")]
    #[can(Update, Deal, from = "id")]
    async fn update(&self, ws: CurrentWorkspace, uow: Uow, deal: Authorized<Deal, Update>, Json(input): Json<DealChanges>) -> Result<Json<Deal>> {
        let mut record = deal.into_inner(); // Record<Deal>
        record.title = input.title;
        WorkspaceRepo::<Deal>::update(&uow, ws.id(), &mut record).await?;
        Ok(Json(record.into_inner()))
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
| `header` | `X-Workspace: agra` (slug or id) | `workspace.header` |
| `path` | segment after the prefix: `/api/v1/w/agra/deals` | `workspace.path_prefix` (`/w`) |
| `subdomain` | `agra.app.example.com` (Host header, one label) | `workspace.base_domain` (unset = off) |
| `claim` | claim of the bearer JWT, authenticated requests only | `workspace.claim` (`workspace`) |
| `domain` | the workspace's own verified domain (`crm.agra-corp.com`), marked by `DomainsModule` | list it first: `resolvers = ["domain", "header"]`; workspaces only |
| `session` | the workspace a browser signed in on the session cookie switched to (`POST /workspace/switch`, `workspace.switch_path`; a form with `workspace` and `next`, or JSON); `WorkspaceSwitcher` posts to it | list it first for Navree UI pages: `resolvers = ["session", "header"]`; with `workspace.session_default = true` a browser that has chosen none is in the first workspace its account belongs to |

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
`authorize("deals.export")?` (403), `scope(query)` (a `Builder<M>`), `isolation()`,
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
- Module subjects (navree-orm models, registered by the module itself):
  `Workspace` (`workspace.*`), `WorkspaceMember` (`workspace.members.*`),
  `WorkspaceInvitation` (`workspace.invitations.*`), plus `Organization`,
  `OrganizationMember`, `AccessRequest` and `WorkspacePartition`. Read the
  tables with `Workspace::query()`, `Member::query().where_(Member::WORKSPACE_ID.eq(id))`.

## Services (writes take `&Uow` first, emit events after commit)

```rust,ignore
let ws = workspaces.create(&uow, owner_id, NewWorkspace::named("Agra Inc.")).await?; // slug "agra-inc"
workspaces.rename(&uow, ws.id, "Agra Corp").await?;          // slug unchanged
workspaces.set_time_zone(&uow, ws.id, Some("Asia/Calcutta"), Some("in")).await?; // stored Asia/Kolkata, IN
workspaces.for_user(user_id).await?;                          // Vec<Membership { workspace, role, granted_by }>
workspaces.resolve("agra").await?;                            // slug or id, 404 otherwise
workspaces.transfer_ownership(&uow, ws.id, new_owner).await?; // new owner: owner, old: admin
workspaces.delete(&uow, ws.id).await?;                        // members + invitations cascade
workspaces.enter(&uow, ws.id).await?;                         // jobs: navree.workspace / search_path

members.add(&uow, ws.id, user_id, WorkspaceRole::Member).await?;  // 409 if member
members.change_role(&uow, ws.id, user_id, WorkspaceRole::Admin).await?;
members.remove(&uow, ws.id, user_id).await?;                  // 409 for the owner
members.list(ws.id).await?;                                    // Vec<MemberInfo { member, user }>

let sent = invitations.invite(&uow, ws.id, "gauri@example.com", WorkspaceRole::Member, Some(me)).await?;
let joined = invitations.accept(&uow, &token, user_id).await?; // AcceptedInvitation { workspace, member }
invitations.revoke(&uow, ws.id, invitation_id).await?;
invitations.prune_expired(&uow).await?;                        // or `navree workspace:prune-invitations`
```

## Routes (`workspace.routes = true` by default)

`GET/POST /api/workspaces` (`{name, slug?, timezone?, country?}`),
`GET/PATCH/DELETE /api/workspaces/{workspace}`,
`PUT .../time-zone {timezone, country}` (`workspace.update`; 400 on an
unknown zone or country), `POST .../transfer`, `POST .../leave`, `GET/POST .../members`,
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

- Tenant tables come from `m.workspace_migrations(MigrationSet::new("notes").migration(SchemaMigration::new("m..._create_notes", CreateNotes)))`,
  not `m.migrations`; they are navree-schema migrations like every other.
  Schema: run when the workspace is created; database:
  on first open per process; both: `navree workspace:migrate [slug...]`.
- Handlers use `WorkspaceUow` (derefs to `Uow`, a navree-orm executor): the request's unit of work
  for row/schema, a unit of work on the workspace's database for database
  isolation, committed when the status < 400.
- Jobs: `databases.db(id).await?.uow(None, None).await` + `commit()`
  (`WorkspaceDatabases`), or `workspaces.enter(&uow, id)` for schema.

## Organizations (above workspaces, off by default)

```toml
[workspace.organizations]
enabled = true                       # APP_WORKSPACE__ORGANIZATIONS__ENABLED=true
# resolvers = ["header", "path", "subdomain", "claim"]  X-Organization, /o/{slug}, base_domain, claim `organization`

[workspace.organizations.roles.member]
permissions = ["reports.read"]       # extra org permissions
workspace_role = "viewer"            # the workspace role it gives (default: same name, must exist)
```

```rust,ignore
let agra = organizations.create(&uow, owner, NewOrganization::named("Agra Group").country("gb")).await?;
organizations.attach(&uow, agra.id, ws.id).await?;   // 409 if attached elsewhere
org_members.add(&uow, agra.id, gauri, OrgRole::Admin, WorkspaceSelector::All).await?;
org_members.grant(&uow, agra.id, linus, OrgRole::Member, WorkspaceSelector::only([ws.id])).await?;
access_requests.approve(&uow, agra.id, request_id, Some(admin), Approval::default()).await?;
```

- Org roles: `owner` (`all.*`), `admin` (`organization.read|update|attach|detach`,
  `organization.members.*`, `organization.access_requests.*`), `member`/`viewer`
  (`organization.read`, `organization.members.read`), app-defined. Each maps to a
  workspace role.
- `CurrentWorkspace` admits a workspace member OR an org member whose selector
  reaches the workspace (mapped role). Membership wins; `ws.granted_by()` says
  which. Org-granted users have no `workspace_members` row.
- Org-level routes: `#[guard(InOrganization::guard())]` + `CurrentOrganization`
  (`workspace_ids()`, `reaches`, `authorize`, `scope`). Read across workspaces
  with `OrgRepo::<Deal>::query(org.workspace_ids())` (a `Builder<M>`; read-only; writes go through
  `WorkspaceRepo` in one workspace). 400 `The request names no organization.`,
  404 `Organization not found.`, 403 `You are not a member of this organization.`
- An organization's `timezone` / `country` (checked like a workspace's,
  stored as the current zone name) is the zone of its workspaces that have
  neither, in requests, jobs and `per_workspace_local` schedules
  (`OrganizationZones`, see navree-time-zones).
- PostgreSQL: `RlsPolicy::new("deals").organizations()` adds a SELECT-only
  policy on `navree.organization` (set by `CurrentOrganization`, or
  `organizations.enter(&uow, id)` in jobs).
- Routes `/api/organizations`, `/{organization}`, `/transfer`, `/leave`,
  `/members[/{user}]`, `/workspaces[/{workspace}]` (attach needs owning the
  workspace), `/access-requests[/{request}/approve|decline]`.
- `AbilityCtx::organization_id` is the org (org requests) or the workspace's org.

## Partitions (inside a workspace, off by default)

```toml
[workspace.partitions]
enabled = true          # resolvers header (X-Partition), path (/p), claim; no subdomain
```

```rust,ignore
impl WorkspaceScoped for Contact {}
impl PartitionScoped for Contact {}   // Contact: a #[derive(Model)]; column `partition_id` (PARTITION_COLUMN)

let uk = partitions.create(&uow, ws.id, NewPartition::named("agra.co.uk")).await?; // slug agra-co-uk
partitions.grant(&uow, ws.id, gauri, PartitionSelector::only([uk.id])).await?;

// lists: the named partition, else every partition the member sees
async fn index(&self, scope: PartitionScope, uow: Uow, page: PageQuery) -> Result<Page<Record<Contact>>> {
    Ok(PartitionRepo::<Contact>::query(scope.workspace_id(), scope.selector()).paginate(&uow, page).await?)
}
// writes: CurrentPartition requires X-Partition (400 otherwise)
PartitionRepo::<Contact>::insert(&uow, ws.id(), partition.id(), contact).await?;
// also update(&uow, ws, &selector, &mut record), delete(&uow, ws, &selector, record), find, find_or_fail
```

- Members see `All` by default; the workspace owner and org-granted users always
  see all. `Only` members don't see rows without a partition.
- 404 `Partition not found.`, 403 `You don't have access to this partition.`
- `RlsPolicy::new("contacts").partitioned()` narrows to `navree.partition` only
  while a request names a partition; listing for `Only` members relies on
  `PartitionScope`/`PartitionRepo`.
- Built-in roles gain `workspace.partitions.*` (admin) / `.read` (member, viewer).
  Routes `/api/workspaces/{workspace}/partitions[/{partition}]`,
  `GET|PUT .../members/{user}/partitions {partitions: "all" | [ids]}`.
- `partitions.delete` keeps rows' `partition_id`: move or delete them first.

## Steps: a shared library (global, organization and workspace content)

Content the product ships and tenants adapt (templates, courses, badges,
policies): one table, each row global, an organization's or a workspace's;
a workspace reads all three as one list, changes only its own, clones the
rest. `navree::workspace::library`.

1. Migration (schema builder): `LibraryColumns` adds `scope`,
   `organization_id`, `workspace_id`, `cloned_from_id`, their indexes and
   the CHECK `<table>_library_owner_check`:

```rust,ignore
let library = LibraryColumns::new(schema.target().await?.dialect());
schema.create("templates", |t| {
    library.uuid(t, "id").primary();   // BINARY(16) on MySQL, uuid elsewhere
    library.add(t);
    t.string("name");
    t.timestamp_tz("created_at");
    t.timestamp_tz("updated_at");
}).await
```

2. Model + registration:

```rust,ignore
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "templates", timestamps)]   // NOT #[model(workspace)]
pub struct Template {
    pub id: Uuid,
    pub scope: LibraryScope,                // global | organization | workspace (default)
    pub organization_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,
    pub cloned_from_id: Option<Uuid>,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
impl Library for Template {}               // cloning(&mut self, src) / cloned(uow, src, copy) hooks optional

m.migrations(MigrationSet::new("templates").after(["workspace"]).migration(SchemaMigration::new("m.._create_templates", CreateTemplates)))
 .library::<Template>();                   // LibraryVisibility scope + LibraryGuard hook + navree check class
```

3. Use it:

```rust,ignore
Template::query().get(&uow).await?;                          // in a workspace request: global + its org's + its own; no workspace: global only
LibraryRepo::<Template>::visible_to(&ws).get(&uow).await?;    // explicit viewer (&CurrentWorkspace, &Workspace, &CurrentOrganization)
LibraryRepo::<Template>::owned_by(LibraryOwner::Organization(org.id())).get(&uow).await?;
LibraryRepo::<Template>::find_editable_or_fail(&uow, &ws, id).await?;  // 404 unseen, 403 library_read_only shared
LibraryRepo::<Template>::create(&uow, LibraryOwner::Organization(org.id()), item).await?; // outside a workspace request
let copy = LibraryRepo::<Template>::clone_into(&uow, id, ws.id()).await?;  // cloned_from_id set; event library.item_cloned after commit
LibraryRepo::<Template>::clone_to(&uow, ws_item, LibraryOwner::Organization(org)).await?; // publish up (in unguarded(..) inside a ws request)
LibraryRepo::<Template>::seed_global(db, "slug", items).await?;          // seeders: create/update global rows by a unique column
```

- Helm enters a workspace a request names only for a member (through the
  module's `dyn WorkspaceGate`, `navree::access::WorkspaceGate`, which other
  modules can ask too) or for staff the app names with
  `m.helm_staff_workspaces(..)`. Under database isolation the panel can't enter
  a workspace. The gate also reports `writes` (`WorkspaceWrites`): a read-only
  demo, and whether the role writes (not `viewer` or the app's own roles, unless
  `workspace.roles` names the model), which Helm enforces whatever the user holds
  app-wide.
- Helm: the library model derives `navree::orm::Model`; `Resource::query`
  filters `Template::SCOPE.eq(LibraryScope::Global)`, `create` calls
  `LibraryRepo::create(ctx.uow(), LibraryOwner::Global, ..)`.

## Steps: give the app a demo workspace

A demo is a workspace with `kind = "demo"` (`Workspace::kind()`,
`is_demo()`), filled by each module's `DemoSeeder` from JSON fixtures.

1. One seeder per module with demo data (`navree::workspace::{DemoSeed,
   DemoSeeder}`), registered next to its migrations:

```rust,ignore
#[injectable]
pub struct CrmDemo;

impl DemoSeeder for CrmDemo {
    fn name(&self) -> &'static str { "crm" }                       // unique
    fn tables(&self) -> &'static [&'static str] { &["companies"] } // children first; cleared on reset
    fn seed<'a>(&'a self, demo: &'a DemoSeed) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let companies: Vec<CompanyFixture> = demo.fixture("crm/companies")?; // fixtures/demo/crm/companies.json
            for c in companies {
                let company = Company { id: Uuid::now_v7(), name: c.name, ..Company::default() }; // workspace_id is set by the repo
                WorkspaceRepo::<Company>::insert(demo.uow(), demo.id(), company).await?;
            }
            Ok(())
        })
    }
    // Override `clear` for another workspace column: `demo.clear::<Company>().await?`
    // (`clear` and `clear_table` return Result<u64, navree_db::DbError>).
}

// in the module's wire():
m.migrations(..).workspace_scoped::<Company>().demo_seeder::<CrmDemo>(); // Company: a #[derive(Model)] + WorkspaceScoped
```

2. Fixtures: `workspace.demo.fixtures` (default `fixtures/demo`), JSON; a
   string that is exactly `{{ now }}`, `{{ now - 2d }}` (m/h/d/w),
   `{{ today }}` or `{{ today + 1w }}` (d/w) becomes a time/date relative to
   `demo.now()` (the app's `dyn Clock`). `demo.parse_fixture(include_str!(..))`
   for embedded JSON.
3. Create: `navree workspace:demo-create "Agra demo" --owner <id|email> [--slug demo]`
   or `demos.create(&uow, owner, NewWorkspace::named("Agra demo"))`
   (`DemoWorkspaces`; seeds in `uow`, which stays entered in the demo; under
   database isolation seeds the tenant DB and commits it).
4. Pick the mode:

```toml
[workspace.demo]
mode = "read_only"          # default: POST/PUT/PATCH/DELETE -> 403 "Demo workspaces are read-only." (code demo_read_only)
# mode = "reset"            # writes allowed; workspace:demo-reset clears (reverse order) + seeds nightly
reset_at = "03:00"          # UTC; needs ScheduleModule + `schedule:work`
# guest_role = "demo_guest" # signed-in non-members enter demos (CurrentWorkspace::is_guest()); only GET /api/workspaces/{demo} of the module's routes
editor_roles = []           # roles that still write to read-only demos
allow = []                  # route names or "METHOD /full/path" still writable
```

Responses about a demo carry `X-Navree-Demo: read_only|reset`
(`DEMO_HEADER`, CORS-exposed); `GET /api/workspaces` items have `kind`
(`live`/`demo`) for a Live/Demo switch. `CurrentWorkspace::is_demo()`,
`demo_mode()`. `demos.reset(id)` (409 `NotDemo` for live), `reset_all()`,
`all()`, `seeders()`. Event `workspace.demo_seeded {workspace_id, slug,
seeders, reset}`. Test the nightly reset with a `FixedClock` at `reset_at`
plus `app.get::<Scheduler>().await.tick(at)`.

## Testing template

```rust,ignore
let app = TestApp::from_builder(TestAuthenticator::install(app(sources))).await;
let auth = app.get::<AuthService>().await;
let anu = auth.create_user("Anu", "anu@example.com", "secret-password").await.unwrap();
let owner = app.acting_as(principal(anu.id));
owner.post("/api/workspaces").json(&json!({ "name": "Agra" })).send().await.assert_status(201);
owner
    .post("/api/v1/deals")
    .header("X-Workspace", "agra")
    .json(&json!({ "title": "Big one" }))
    .send()
    .await
    .assert_status(201);
app.acting_as(principal(Uuid::now_v7()))
    .get("/api/v1/deals")
    .header("X-Workspace", "agra")
    .send()
    .await
    .assert_error(403, "You are not a member of this workspace.");
```

Every workspace module gets an isolation suite: cross-workspace tests for
each of its routes, from the route table and a model factory (skill
`navree-module-conformity`):

```rust,ignore
use navree::workspace::testing::IsolationSuiteExt;

app.isolation_suite::<DealsModule>()
    .factory(Deal::factory()) // a navree-orm Factory (#[model(factory = DealFactory)], or Factory::<DealFactory>::new()); binds {id}; workspace_id is set by the suite
    .body("POST", "/api/v1/deals", json!({ "title": "Big one" }))
    .run()
    .await;
```

Classify every model (`m.workspace_scoped::<M>()`,
`m.workspace_entity::<M>()` or `m.global_entity::<M>()`; a
`#[model(workspace)]` model scopes its own queries) and run
`navree check`: `workspace.tables` checks the workspace column, its index
and RLS of every classified table.

Read invitation tokens from `app.get::<Mailer>().await.array_transport().unwrap().sent()`
(config `mail.transport = "array"`); move time with
`override_value::<dyn Clock>(Arc::new(FixedClock::new(now)))`.

## Gotchas

- Forgetting `#[guard(InWorkspace::guard())]`: route `#[roles]` /
  `#[permissions]` / `#[can]` then check GLOBAL access (usually 403), because
  extractors (including `CurrentWorkspace`) run after route checks.
- Querying a workspace-owned table with `M::query()` (on a model that isn't
  `#[model(workspace)]`) leaks rows across workspaces on SQLite (PostgreSQL
  RLS hides them). Always scope: `WorkspaceRepo::<M>::query(ws.id())`.
- `#[can(.., from = "id")]` on a workspace-owned model without
  `m.workspace_scoped::<M>()` lets a member with an unconditional permission
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
  `.invitation_accepted`, `.deleted`, `.demo_seeded`) fire only after commit.
- A read-only demo refuses writes on every route that resolves the
  workspace, the module's own `/api/workspaces/{demo}/...` included; seed or
  curate it with seeders, `editor_roles` or `allow`, never by turning the
  mode off. A demo seeder that writes outside `demo.uow()` (or forgets
  `tables()`/`clear`) leaves rows a reset doesn't remove.
- A library model without `m.library::<M>()` has no visibility scope and no
  guard: `Template::query()` then returns every workspace's rows. Never mark
  it `#[model(workspace)]` (that hides global rows). Outside a workspace
  request the scope shows global rows only: use `LibraryRepo::visible_to`
  / `owned_by` for organization screens and commands. Inside a workspace
  request a write to a global or organization item is a 403
  `library_read_only` (clone it), another workspace's item a 404; writing
  for another owner there needs `unguarded(..)` plus your own permission
  check.

Docs: `/docs/workspaces/introduction/`, `/docs/workspaces/resolution/`,
`/docs/workspaces/members-and-roles/`, `/docs/workspaces/invitations/`,
`/docs/workspaces/isolation/`, `/docs/workspaces/organizations/`,
`/docs/workspaces/partitions/`, `/docs/workspaces/shared-libraries/`,
`/docs/workspaces/demo-workspaces/`.
