---
name: navree-integrations
description: "Workspace and organization connections to third-party APIs with navree-integrations: `IntegrationsModule` and the `integrations` config section, providers (`microsoft` admin consent and app-only Graph tokens, `google` service accounts with domain-wide delegation, `slack` installs, any `oauth2` API, `api_key`), the `/api/integrations/*` routes, scope sets (`ScopeSet`, `for_feature`, per-scope-set app registrations), the `Integrations` service, typed clients (`GraphClient`, `GoogleClient`, `SlackClient`, `IntegrationClient`) and the `Connection<P, S>` extractor, token refresh under a lock, `integrations:check` health checks and `ConnectionUnhealthy`, and tests with `Integrations::fake()`/`IntegrationsFake` or wiremock. Use when a customer connects Microsoft 365, Google Workspace, Slack, an OAuth API or an API key to their workspace, when a feature calls Graph, the Google Admin SDK or Slack on a tenant's behalf, or when debugging `integration_not_connected`, `integration_reauthorization_required`, `invalid_state` or an unhealthy connection."
---

# Integrations

`IntegrationsModule` (crate navree-integrations, facade feature
`integrations`, in `full`) stores one connection per owner (workspace,
organization or the app), provider and scope set, with the credentials
encrypted by the application key. Everything below is in the code; when in
doubt read `crates/navree-integrations/src` and the tests in
`crates/navree-integrations/tests/integrations/` (fake and wiremock
providers) and `crates/navree/tests/integrations_docs.rs` (the docs
examples through the facade).

Not to be confused with navree-social: social sign-in links a provider
account to one **user**; integrations connect a **tenant's** account to a
workspace or organization.

## Steps: offer a provider

1. Add the module (needs Config, Crypto with `APP_KEY`, Database;
   HttpClient, Cache, Workspace, Events, Schedule are used when present):

```rust
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(CryptoModule::from_config())
    .module(HttpModule::from_config())
    .module(HttpClientModule::from_config())
    .module(DatabaseModule::from_config())
    .module(CacheModule::from_config())        // token locks (shared store for several instances)
    .module(EventsModule::for_root())
    .module(WorkspaceModule::from_config())    // workspace / organization owners
    .module(ScheduleModule::for_root())        // integrations:check --due every 5 minutes
    .module(IntegrationsModule::from_config()) // section `integrations`
```

2. Configure (`redirect_base` is the front end's callback page; register
   `{redirect_base}/{key}` with the provider):

```toml
[integrations]
redirect_base = "https://app.acme.test/settings/integrations/callback"

[integrations.providers.microsoft]          # admin consent, app-only tokens
client_id = "0f6c1d2e-8a3b-4c5d-9e7f-1a2b3c4d5e6f"

[integrations.providers.google]             # service-account keys
scopes = ["https://www.googleapis.com/auth/admin.directory.user.readonly"]

[integrations.providers.slack]
client_id = "4129031.6713"
scopes = ["chat:write"]

[integrations.providers.hubspot]
type = "oauth2"
client_id = "..."
authorize_url = "https://app.hubspot.com/oauth/authorize"
token_url = "https://api.hubapi.com/oauth/v1/token"
api_url = "https://api.hubapi.com"

[integrations.providers.edr]
type = "api_key"
header = "X-Api-Key"
scheme = ""
health_url = "/v1/health"
```

   Secrets: `APP_INTEGRATIONS__PROVIDERS__<KEY>__CLIENT_SECRET`. Keys
   `microsoft`/`google`/`slack` imply their type; others need `type`.
   Bad config fails the boot naming the key.
3. The routes need `AccessModule` + an authenticator; every route requires
   permission `integrations.manage` (in the request's workspace at
   `level = "workspace"`, behind `InWorkspace::guard()`). Turn them off
   with `integrations.routes = false`.
4. Front end (BFF): `GET /api/integrations/{key}/connect?scope_set&return_to`
   gives `{url, state}`; keep `state` in an httpOnly cookie, redirect to
   `url`. The callback page compares `state` with the cookie and forwards
   the provider's query string **unchanged** to
   `GET /api/integrations/{key}/callback` as the same admin (Microsoft
   sends `tenant` + `admin_consent=True`, OAuth providers `code`). 201 with
   the connection and `return_to`.
5. Google: `POST /api/integrations/google/service-account {key, subject, scope_set?}`
   (`key` = the JSON object or its text). API keys:
   `POST /api/integrations/{key}/api-key {key, api_url?, fields?, scope_set?}`.
   Both are tried before they are stored.

## Scope sets (least privilege)

```rust
pub struct DirectoryRead;
impl ScopeSet for DirectoryRead {
    const NAME: &'static str = "directory";          // [a-z0-9_]
    const DESCRIPTION: &'static str = "Read users and groups";
    fn scopes(provider: &str) -> &'static [&'static str] {
        match provider {
            "microsoft" => &["https://graph.microsoft.com/User.Read.All"],
            "google" => &["https://www.googleapis.com/auth/admin.directory.user.readonly"],
            _ => &[],                                 // not offered for this provider
        }
    }
}
// in the feature module: imports IntegrationsModule::for_feature::<DirectoryRead>()
// or m.integration_scope_set::<DirectoryRead>()
```

- Each scope set is its own connection (unique owner + provider + set).
  `default` = the provider's configured `scopes`.
- Microsoft app-only tokens carry every permission of the registration:
  give a sensitive set its own app,
  `[integrations.providers.microsoft.apps.mail_send] client_id = ...`.

## Calling APIs

```rust
async fn users(&self, graph: Connection<MicrosoftGraph, DirectoryRead>) -> Result<Json<Vec<User>>> {
    Ok(Json(graph.paginate("/users?$select=id,mail").await?))
}
// jobs/commands:
let google = integrations.client_for::<GoogleWorkspace>(Owner::Workspace(id)).await?;
let mail = google.as_user("ada@acme.test");   // domain-wide delegation
let slack = integrations.client_for::<Slack>(owner).await?;
slack.call("chat.postMessage", &json!({ "channel": "C1", "text": "hi" })).await?;
```

- Owner by `integrations.level`: `workspace` (default; `app` without
  WorkspaceModule), `organization`, `app`. A workspace without its own
  connection uses its organization's (own first).
- Tokens: cached encrypted, refreshed 60 s before expiry under the lock
  `integrations:token:{id}` (one refresh across requests and instances),
  rotated refresh tokens kept; a 401 retries once with a new token.
  Absolute URLs only on the provider's hosts (`ForeignUrl` otherwise).
- `send()` returns the raw response; `send_json::<T>()` errors on 4xx/5xx.
- A refused refresh -> `IntegrationsError::Reauthorize` (409
  `integration_reauthorization_required`) and the connection turns
  unhealthy; an outage -> `Unavailable` (502), status unchanged.
- Your own provider type: `impl Provider for HubSpot { const KEY = "hubspot"; type Client = IntegrationClient; }`.

## Health

`integrations:check` (all), `--due` (scheduled every 5 min; due =
`check_interval_mins`, default 60, 0 = off), `--id <uuid>`;
`integrations:list`. A check mints a new token and probes
(`health_url`, Slack `auth.test`): refusal -> unhealthy at once;
transient -> unhealthy after `unhealthy_after` (3) in a row. Events
`integrations.connected`, `.disconnected`, `.unhealthy`
(`ConnectionUnhealthy`, once on the change), `.restored`.

## The table

`integration_connections` is the navree-orm model `IntegrationConnection`
(`use navree::orm::Model as _;`): query it for reports and admin pages,
`IntegrationConnection::query().where_(IntegrationConnection::STATUS.eq("unhealthy")).count(&db)`.
`credentials` / `access_token` are ciphertexts, hidden from serialization
and `Debug`. Never write the table yourself: `Integrations` encrypts,
emits the events and writes the audit entries (`integration.connected`,
`integration.disconnected`; rows are written without change events, so no
ciphertext reaches the audit log). Return `ConnectionInfo` from your own
routes. The migration is a schema-builder one; its UUID columns are
`BINARY(16)` on MySQL/MariaDB (models write UUIDs as 16 bytes there).

## Tests

```rust
let (integrations, fake) = Integrations::fake();
// ... .override_value(Arc::new(integrations)) ...
fake.on_code("slack", "code", FakeGrant::new().access_token("xoxb-1").never_expires().external_id("T1"));
fake.refuse_tokens("microsoft", "unauthorized_client");   // or fail_tokens / restore_tokens
fake.api().on(Method::GET, "https://graph.microsoft.com/v1.0/users*", FakeResponse::new(200).json(&body));
assert_eq!(fake.minted()[0].grant_type, "client_credentials");
```

States, encryption and service-account signatures stay real. For real
provider responses: point `authority_url`/`token_url`/`api_url` at a
wiremock server and allow `127.0.0.1` in `http_client.ssrf.allow_hosts`.
Set `integrations.routes = false` in tests without AccessModule. The
crate's PostgreSQL and MySQL suite: `cargo test -p navree-integrations
--features postgres,mysql --test integrations servers` with
`NAVREE_TEST_POSTGRES_URL` / `NAVREE_TEST_MYSQL_URL` (each test creates and
drops its own database).

## Errors (`code`)

`invalid_state` 400, `consent_denied` 400, `integration_refused` 400,
`invalid_credentials` 400, `unknown_scope_set` 400,
`unsupported_connection` 400, `invalid_return_to` 400,
`unknown_integration` 404, `integration_not_connected` 404,
`connection_not_found` 404, `integration_reauthorization_required` 409,
`integration_unavailable` 502, `integration_api_error` 502.
