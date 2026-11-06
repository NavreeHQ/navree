---
name: navree-directory
description: "Directory sync with navree-directory: `DirectoryModule` and the `directory` config section, connections per workspace or organization (`scim`, `microsoft` with Graph delta queries, `google`, `okta` through navree-integrations' scope set `directory`, `ldap` for LDAP and Active Directory, or a `DirectorySource` of your own), the SCIM 2.0 server at `/scim/v2` (bearer token per connection, filters, PATCH), `impl DirectorySink` to map `DirectoryUser`/`DirectoryGroup` into your model, sync runs (`Directory::sync`, `sync_now`, dry runs, the deactivation limit and `approve`, partial runs, operations), the events, `/api/directory/*` routes, `directory:sync` and `directory:runs`, and tests with `Directory::fake()`/`DirectoryFake`, wiremock or an LDAP server. Use when provisioning a customer's employees from Entra ID, Okta, Google Workspace or Active Directory into your app, when an identity provider's SCIM setup fails, or when a run is held, partial or failed."
---

# Directory sync

`DirectoryModule` (crate navree-directory, facade feature `directory`, in
`full`) keeps each owner's people (workspace, organization or the app) in
step with a directory, pushed over SCIM or pulled on a schedule, and hands
every change to your `DirectorySink`. Everything below is in the code; when
in doubt read `crates/navree-directory/src` and the tests in
`crates/navree-directory/tests/directory/` (`sync.rs` runs on the fake,
`scim.rs` SCIM compliance over HTTP, `sources.rs` every source on wiremock
or an LDAP fixture server, `routes.rs`, `servers.rs` on PostgreSQL and
MySQL) and `crates/navree/tests/directory_docs.rs` (the docs examples
through the facade). The page is `docs/packages/directory-sync`.

Not to be confused with navree-sso: single sign-on signs a person in;
directory sync creates, updates and deactivates people before they sign in.

## Steps: sync a customer's directory into your model

1. Add the modules. Required: Config, Database. Crypto with `APP_KEY` to
   store the LDAP bind password; Integrations for `microsoft`, `google`,
   `okta`; Queue to run syncs as operations; Schedule for
   `directory:sync --due`; Events for the events; Workspace for workspace
   or organization owners; Access + an authenticator for the routes
   (`directory.routes = false` without them).

```rust,ignore
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(CryptoModule::from_config())
    .module(HttpModule::from_config())
    .module(HttpClientModule::from_config())
    .module(DatabaseModule::from_config())
    .module(EventsModule::for_root())
    .module(QueueModule::from_config())
    .module(ScheduleModule::for_root())
    .module(WorkspaceModule::from_config())
    .module(IntegrationsModule::from_config())
    .module(DirectoryModule::from_config())    // section `directory`
    .module(RecipientsModule)                  // imports DirectoryModule::for_feature::<RecipientSink>()
```

2. Write the sink (a provider: `#[injectable]`). Write through
   `ctx.uow()`: the directory row, your rows and the event commit together.

```rust,ignore
impl DirectorySink for RecipientSink {
    fn upsert_user<'a>(&'a self, ctx: &'a SinkContext, change: &'a UserChange)
        -> navree::BoxFuture<'a, Result<Option<String>, BoxError>> {
        Box::pin(async move {
            let uow = ctx.uow();
            // find by change.directory_user_id, else Record::new(..) with ctx.owner().id()
            // set fields from change.user (primary_email(), full_name(), department, active)
            // record.save(uow).await?;
            Ok(Some(local_id))                 // kept as the user's local_id
        })
    }
    fn deactivate_user<'a>(&'a self, ctx: &'a SinkContext, change: &'a UserChange)
        -> navree::BoxFuture<'a, Result<(), BoxError>> { /* archive; change.deleted for SCIM DELETE */ }
    // optional: upsert_group / delete_group (change.added / change.removed by external_id;
    // ctx.local_ids(&change.added) maps them to your ids)
}
```

   Key your records on `change.directory_user_id` (stable per connection),
   not on the email. `change.action`: `Create`, `Update`, `Reactivate`
   (upsert) or `Deactivate`. A user can arrive inactive. A failed chunk is
   retried change by change, so the sink may see a change twice: only
   write through `ctx.uow()`; mail and remote calls go in listeners of the
   events (after commit).
3. Create connections (routes, or in code):
   `directory.create_connection(Owner::Workspace(id), NewConnection::new("Acme Entra ID", "scim"))`
   returns `CreatedConnection { connection, scim_token, scim_url }` (token
   shown once, SHA-256 at rest). Pull connections:
   `NewConnection::new(name, "ldap").settings(json!({..})).secret(pw).interval(30)`.
4. Turn a pull connection on with a dry run first:
   `directory.sync_now(id, SyncOptions::dry_run())`, then
   `directory.changes(run.id, 0, 100)` (status `planned`).

## Sources

| `source` | Settings | Credentials |
|---|---|---|
| `scim` | none | `nvscim_...` bearer token |
| `microsoft` | `provider`, `scope_set` (`directory`), `groups` | integrations admin consent, scope set `directory` |
| `google` | `customer` (`my_customer`) or `domain`, `query`, `admin_email`, `groups` | service account (domain-wide delegation, acts as `admin_email`) or OAuth |
| `okta` | `search`, `group_filter`, `groups` | integrations `api_key` with `scheme = "SSWS"` and the org URL as `api_url`, or `oauth2` |
| `ldap` | `url`, `base_dn` (required), `bind_dn`, `starttls`, `ca_certificate`, `insecure_skip_verify`, `flavor` (`active_directory`/`openldap`), `user_filter`, `group_filter`, `id_attribute`, `attributes`, `page_size` (500), `timeout_secs` (30), `user_base_dn`, `group_base_dn`, `groups` | bind password = the connection's secret |

- Scope set `directory` (`DirectoryScopes`) is registered by the module:
  Graph `User.Read.All` + `GroupMember.Read.All`, Google directory
  user/group/member readonly, Okta `okta.users.read` + `okta.groups.read`.
  A provider under another key: set `"provider"` and a scope set of your own
  (or `"scope_set": "default"`).
- Graph: first run = full delta round (snapshot), cursor = both delta
  links; later runs read changes, re-read changed objects in full (15 per
  `$filter=id in (...)`), members from `members@delta`; 410 -> snapshot.
  No managers from Graph.
- Google and Okta: snapshot each run. Okta omits deprovisioned users (they
  deactivate as missing); suspended/staged = inactive.
- LDAP: AD ids are `objectGUID` as UUIDs (little-endian first fields),
  disabled = `userAccountControl & 2`; OpenLDAP `entryUUID`,
  `pwdAccountLockedTime`/`nsAccountLock`. Members from `member`,
  `uniqueMember`, `memberUid` and users' `memberOf`. No nested groups.
  TLS on rustls/ring; `ca_certificate` for an internal CA.
  Host policy: hosts resolving to loopback/private/link-local/shared
  addresses fail the run unless listed in `directory.ldap.allow_hosts`
  (`APP_DIRECTORY__LDAP__ALLOW_HOSTS=a,b`) or `directory.ldap.block_private = false`.
  Tests with a local LDAP server need `allow_hosts = ["127.0.0.1"]`.
- Your own: `impl DirectorySource` (`check_settings`, `pull` ->
  `Pulled::snapshot(users, groups)` or `Pulled::delta(..)` with
  `removed_user`, `removed_group`, `member_added`, `member_removed`,
  `.cursor(..)`), `m.directory_source("hr", HrSource)`.

## Runs

- `directory.sync(id, opts)` queues: an operation `directory.sync` when the
  app has navree-queue operations (progress, cancel), else the
  `directory.sync` job through the dispatcher, else a task after commit.
  `sync_now` runs in the caller (commands, jobs, tests; not inside a
  request on SQLite).
- Stage (pull) -> diff (fingerprints) -> apply in chunks
  (`directory.sync.chunk_size`, 100): users created/updated/reactivated,
  groups, deleted groups, deactivations. Snapshot: missing active users
  deactivate, missing groups delete. Delta: only what it names.
- Statuses: `queued`, `running`, `succeeded`, `partial` (some changes
  failed; recorded `failed`; cursor kept for a retry), `held`, `failed`
  (source unreadable; nothing changed), `cancelled`.
- Held: more than `min_deactivations` (10) AND more than
  `max_deactivation_percent` (20) of active users would deactivate.
  `directory.approve(run_id)` / `POST /api/directory/runs/{id}/approve` /
  `directory:sync <id> --force` apply it. SCIM is never held.
- A second sync while one is queued/running: 409 `directory_sync_running`
  until the first is older than `stale_after_mins` (120), then it is closed
  as failed.
- Schedule: `directory:sync --due` every 5 min (per-connection
  `sync_interval_mins`, 0 = manual), `directory:prune` daily 03:50
  (`keep_runs_days` 90).
- `directory.runs(id, n)`, `directory.run(id)`, `directory.changes(run, offset, limit)`
  (up to `max_recorded_changes`, 5000 per run; counts always complete),
  `directory.users(id)`, `user(id, external_id)`, `groups(id)`,
  `groups_of(directory_user_id)`.

## SCIM server

- Mounted at `directory.scim.path` (`/scim/v2`, outside the API prefix,
  public routes); URL for the IdP = `directory.scim.base_url`, else
  `app.url` + path. Token picks the connection; discovery endpoints need
  none. 401 + `WWW-Authenticate: Bearer realm="SCIM"`, paused -> 403.
- Entra ID: Tenant URL + Secret Token. Okta: base URL, unique id
  `userName`, HTTP Header auth.
- Resources are stored as the client wrote them (no `password`, no
  `id`/`meta`/`groups`) and read back unchanged; the normalized user is
  derived. `userName` and `externalId` unique per connection
  (case-insensitive userName) -> 409 `uniqueness`.
- Filters: full grammar, at most 16 KiB and 32 levels deep (`invalidFilter`);
  `userName eq`, `externalId eq`, `id eq`, `displayName eq` are indexed. PATCH: RFC ops plus `Replace`, `"False"`,
  path keys in path-less values, `remove members` with a value list,
  add/replace through a filter that matches nothing.
- Errors: SCIM shape (`invalidSyntax`, `invalidFilter`, `invalidPath`,
  `invalidValue`, `noTarget`, `uniqueness`), `application/scim+json`.
- Not supported: bulk, sort, `/.search`, `/Me`, ETags, password change,
  nested groups.
- `DELETE /Users/{id}`: sink `deactivate_user` with `deleted`, event with
  `deleted: true`, row and memberships removed. `active: false` keeps the
  row.

## Events, routes, commands

- `directory.user_created`, `directory.user_updated` (`changed`, sorted;
  `reactivated`), `directory.user_deactivated` (`deleted`),
  `directory.sync_finished`; after commit, with `EventsModule`.
- Routes under `/api/directory` (permission `directory.manage`,
  `InWorkspace` at workspace level): `sources`, `connections` CRUD,
  `connections/{id}/token`, `/sync` (202, `operation_id`), `/runs`,
  `/users` (paged), `/groups`, `runs/{id}`, `runs/{id}/changes` (paged),
  `runs/{id}/approve`.
- `directory:sync <id> | --all | --due [--dry-run] [--force] [--full]`,
  `directory:runs [<id>] [--limit N]`, `directory:connections`,
  `directory:token <id>`, `directory:prune`.

## Tests

```rust,ignore
let (directory, fake) = Directory::fake();
// TestApp::builder().temp_database().config("directory.routes", false)
//   .module(DatabaseModule::from_config()).module(DirectoryModule::from_config())
//   .module(RecipientsModule).override_value(Arc::new(directory)).build().await
fake.user(DirectoryUser::new("00u1", "ada@acme.test").email("ada@acme.test"))
    .group(DirectoryGroup::new("g1", "Engineering").members(["00u1"]));
fake.next_pull(Pulled::delta(vec![], vec![]).removed_user("00u1")); // or fail_next_pull("..")
let run = app.get::<Directory>().await.sync_now(id, SyncOptions::default()).await?;
assert_eq!(fake.pulls()[0].cursor, None);
```

Every pull connection of a fake reads the fake; the diff, runs, sink,
events and SCIM stay real. SCIM: create a `scim` connection and use
`app.client().post("/scim/v2/Users").bearer(token.expose())`. Real
responses: point integrations' `api_url`/`authority_url`/`token_url` at
wiremock and allow `127.0.0.1` in the HTTP client's SSRF policy. Servers:
`cargo test -p navree-directory --features test-postgres,test-mysql --test directory servers`
with `NAVREE_TEST_POSTGRES_URL` / `NAVREE_TEST_MYSQL_URL`.

## Errors (`code`)

`unknown_directory_source` 400, `invalid_directory_settings` 400,
`directory_push_only` 400, `directory_not_scim` 400,
`directory_connection_not_found` 404, `directory_run_not_found` 404,
`directory_connection_paused` 409, `directory_sync_running` 409,
`directory_run_not_held` 409, `directory_source_failed` 502. Integrations
answers pass through (`integration_not_connected`).
