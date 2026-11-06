---
name: navree-oauth
description: "The OAuth 2.1 / OpenID Connect server of navree-oauth: `OAuthServerModule` and the `oauth` config section (scopes mapped to permission strings, `consent_url`, `device_url`, TTLs, Ed25519 or RSA signing keys for EdDSA or RS256 ID tokens), clients (`OAuthClients`, `NewClient`, `navree oauth:client`), the authorization code flow with PKCE and the consent API your front end calls, client credentials, device and refresh grants, `OAuthToken` in handlers, `insufficient_scope`, personal access tokens (`PersonalAccessTokens`), introspection, revocation, discovery, JWKS, userinfo, dynamic registration and `navree_oauth::testing`. Use when letting third-party apps, CLIs, TVs, integrations or AI clients act for your users, issuing personal access tokens for scripts, building a consent or device page, scoping what a token may do, or debugging a 401/403/invalid_grant from OAuth tokens."
---

# OAuth server

`navree::oauth` (crate `navree-oauth`, facade feature `oauth`, **opt-in**:
not in `full`) turns the app into an OAuth 2.1 authorization server and
OpenID Connect provider, and issues personal access tokens. Everything below
is in the code; when in doubt read `crates/navree-oauth/src` and the tests in
`crates/navree-oauth/tests/oauth/`.

## Steps: add the server

1. Turn the feature on: `navree = { ..., features = ["oauth"] }` (keep the
   defaults, or list `auth`, `access`, `workspace`, ... yourself).
2. Add the module after `AuthModule` (needs `ConfigModule`, `CryptoModule`,
   `DatabaseModule`, `AuthModule`; `AccessModule` turns scopes into
   abilities):

```rust,ignore
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(CryptoModule::from_config())
    .module(HttpModule::from_config())
    .module(DatabaseModule::from_config())
    .module(AuthModule::from_config())
    .module(AccessModule::for_root(access_options()))
    .module(OAuthServerModule::from_config()) // section `oauth`
```

3. Define scopes and the front-end pages:

```toml
[oauth]
issuer = "https://api.acme.test"                     # default app.url, else http://localhost (warning)
consent_url = "https://app.acme.test/oauth/consent"  # required for the code flow
device_url = "https://app.acme.test/device"          # required for the device grant
# path = "/oauth"  access_token_ttl_secs = 3600  refresh_token_ttl_secs = 2592000
# request_ttl_secs = 600  code_ttl_secs = 60  token_cache_secs = 10 (0 = off)
# signing_key = "ed25519:..."  (navree oauth:keys)  or "rsa:<base64 DER>" / PEM for RS256
# id_token_signing_alg = "RS256"  (EdDSA | RS256; default: the key's)   previous_signing_keys = [...]
# dynamic_registration = false

[oauth.personal_access_tokens]
enabled = true
max_days = 365     # 0 = tokens may never expire

[oauth.scopes."deals:read"]
description = "Read your deals"      # required
permissions = ["deals.read"]         # navree-access permission strings, `deals.*`, `*`
```

   In code: `OAuthServerModule::for_root(OAuthOptions { consent_url: Some(..), ..OAuthOptions::default() }.scope("deals:read", ScopeOptions::new("Read your deals", ["deals.read"])))`.
   Built-in scopes: `openid`, `profile`, `email`, `offline_access` (no
   permissions; can't be redefined).

4. Create clients: `navree oauth:client "Zapier" --redirect-uri https://... --scopes deals:read,deals:write`
   (options: `--public`, `--grant client_credentials --owner <email>`,
   `--device`, `--trusted`, `--workspace <slug>`, `--introspect`,
   `--logo-url`, `--website`). Or from code:

```rust,ignore
use navree::oauth::{NewClient, OAuthClients};

let created = clients
    .create(&uow, NewClient::confidential("Reports").owner(user.id)
        .redirect_uri("https://reports.example/callback").scopes(["deals:read"]))
    .await?;           // created.client.id, created.secret (Some for confidential, shown once)
```

5. Build the consent page in the front end: it receives `?request=<id>`,
   signs the user in, calls `GET /api/oauth/requests/{id}` (client, scopes
   with descriptions, `already_approved`, `expires_at`), then
   `POST /api/oauth/requests/{id}/approve` (or `/deny`) with the user's
   first-party JWT and sends the browser to the returned `redirect_to`.
   Approve at once when `already_approved` is true (trusted clients, or
   scopes approved before).
6. For the device grant, build the device page: `GET /api/oauth/device/{user_code}`
   then `POST /api/oauth/device/{user_code}/approve` (or `/deny`, both 204).

## Flow facts

- `GET /oauth/authorize`: `response_type=code`, `client_id`,
  `redirect_uri` (optional with exactly one registered), `scope` (default:
  the client's), `state`, `code_challenge` + `code_challenge_method=S256`
  (required; `plain` refused), `nonce`. Unknown client / unregistered
  redirect URI → 400 HTML page; other errors redirect with `error`, `state`,
  `iss`.
- `POST /oauth/token` (form body): `authorization_code` (+ `code_verifier`),
  `client_credentials` (acts as the owner; no refresh, no ID token),
  `refresh_token` (rotated; reuse revokes the family; may narrow `scope`),
  `urn:ietf:params:oauth:grant-type:device_code`
  (`authorization_pending`, `slow_down`, `access_denied`, `expired_token`).
  Confidential clients: HTTP Basic or `client_id` + `client_secret` (not
  both); public: `client_id` only.
- Tokens: `nva_` access, `nvr_` refresh, `nvc_` code, `nvd_` device, `nvp_`
  personal, `nvcs_` client secret; all stored as SHA-256 digests.
- A code works once; presenting it again revokes what it issued.
- Other endpoints: `POST /oauth/device_authorization`, `/oauth/revoke`
  (RFC 7009, 200 always), `/oauth/introspect` (RFC 7662, client with
  `--introspect`), `GET|POST /oauth/userinfo` (needs `openid`),
  `GET /oauth/jwks`, `/.well-known/openid-configuration`,
  `/.well-known/oauth-authorization-server`, `POST /oauth/register`
  (only with `dynamic_registration`).
- ID tokens: EdDSA (Ed25519, default) or RS256 (an RSA key you make with
  OpenSSL; what every OIDC client accepts; `navree oauth:keys --rsa`) JWT,
  JWKS publishes current + previous keys of either kind; `iss`, `sub`, `aud` (client id), `exp`,
  `iat`, `nonce`, `name` (profile), `email` + `email_verified` (email).
- Rate limits (`oauth.rate_limits.<endpoint> = { per_client, per_ip }`,
  per minute, per instance, `0` = off): `token` 300/120, `introspect`
  1200/600, `revoke` 300/60, `device_authorization` 60/30. Per IP is counted
  first (failed client auth included; an IPv6 client is its network, a /64 by
  default: `http.rate_limit_ipv6_prefix`); per client only after the client
  authenticates (a wrong secret can't burn a client's budget); public
  clients are counted per IP only (anyone can name one). Over a
  limit: 429, `Retry-After`, `{"error": "too_many_requests",
  "error_description": "Too many requests; retry in N seconds"}`, audited
  `oauth.rate_limited` once per window. Env:
  `APP_OAUTH__RATE_LIMITS__TOKEN__PER_IP=300`.

## Using tokens in handlers

The `oauth` strategy (after JWT) takes `Bearer nva_...` and `Bearer nvp_...`,
sets `Principal::token_abilities` to the scopes' permissions, so existing
`#[can]`, `#[permissions]`, `Accessible` checks narrow automatically.

```rust,ignore
use navree::oauth::OAuthToken;

#[post("/export")]
#[can(Export, Deal)]
async fn export(&self, token: Option<OAuthToken>) -> Result<NoContent> {
    if let Some(token) = &token {
        token.require_scope("deals:export")?; // 403 + WWW-Authenticate insufficient_scope
    }
    Ok(NoContent)
}
```

`OAuthToken { token_id, client_id: Option<Uuid>, client_name, scopes, grant: Grant, user_id, workspace_id, expires_at }`,
`has_scope`, `require_scope`, `is_personal`. A plain `OAuthToken` argument
is a 401 for non-OAuth requests.

## Personal access tokens

`POST /api/oauth/personal-access-tokens {name, scopes, expires_in_days?}` → 201
with `token` once; `GET` lists (`{data}`), `DELETE /{id}` revokes. From
code: `pats.create(&uow, user_id, "CI", ["deals:read"], Some(Duration::from_secs(30 * 86_400))).await?` →
`created.token`. `None` lifetime = `max_days` (never expires when 0).

## Testing template

```rust,ignore
use navree::oauth::testing::{authorize, pkce_pair, AuthorizeFlow};

// Public client with one redirect URI; user_token is a first-party JWT.
let tokens = authorize(&app.client(), client_id, &user_token, &["deals:read"]).await;
app.client().get("/api/v1/deals").bearer(&tokens.access_token).send().await.assert_status(200);

// Confidential client, nonce, custom paths:
let tokens = AuthorizeFlow::new(client_id).secret(secret).nonce("n")
    .run(&app.client(), &user_token, &["openid", "deals:read"]).await;
```

Fastest scoped token: `app.get::<PersonalAccessTokens>().await.create(&uow, user, "t", ["deals:read"], None)`
then commit the `uow`. Move time with
`override_value::<dyn Clock>(Arc::new(FixedClock::new(now)))`; record audit
entries with `override_value::<dyn AuditSink>(..)`.

## Data

- The tables are navree-orm models (`navree::oauth::Client`,
  `PersonalAccessToken`, `Authorization`, and `entity::{access_token,
  refresh_token, code, device_code, authorization_request}::Model`): query
  with `Client::query().where_(Client::OWNER_ID.eq(id))`, `Client::find(&db, id)`.
- The server writes them with bulk inserts and set-based updates (no
  change events), so digests never reach an audit trail; keep it that
  way (`row.save(&uow)` records every column).
- Errors: `OAuthError::Db(DbError)` and `OAuthError::Orm(OrmError)`.

## Checks

- `cargo test -p navree-oauth` (unit + `tests/oauth/*`).
- `/.well-known/openid-configuration` lists your scopes and endpoints under
  the right issuer.
- A token with only `deals:read` gets 403 + `WWW-Authenticate: Bearer
  error="insufficient_scope", scope="..."` on a write route.

## Gotchas

- The feature is opt-in: `navree::oauth` doesn't exist without
  `features = ["oauth"]`.
- `/api/oauth/*` refuses OAuth tokens and PATs (403): consent, PATs and
  authorizations need the user's first-party sign-in, so a token can't grant
  itself more.
- A scope never adds abilities: a token is the user's abilities narrowed.
  `everything = ["*"]` still only does what the user may do.
- Built-in scopes (`openid`, ...) can't be PAT scopes; `offline_access` is
  needed for a refresh token; client credentials drop `openid` and
  `offline_access`.
- `--workspace` clients are pinned (`PinnedWorkspace`): no `X-Workspace`
  means that workspace, another one is a 403; only members can approve them.
- Revocation is immediate on the revoking instance; other instances may
  serve a cached token for up to `oauth.token_cache_secs`.
- Keep `previous_signing_keys` for a day after rotating `signing_key`, or
  ID tokens issued before stop verifying.
- `dynamic_registration` lets any app register and ask users for consent;
  leave it off unless you need it (MCP OAuth mode).
- Offboarding: the module registers a `CredentialRevoker`, so
  `AuthService::revoke_credentials_of_domain("agra.com")` /
  `revoke_all_credentials(user_id)` also revokes personal access tokens, OAuth
  access and refresh tokens, unexchanged codes and device codes, and the
  remembered consent (`oauth_authorizations`) of those users. Revoked tokens
  stop at once here, within `oauth.token_cache_secs` on other instances.
- 429 `too_many_requests` from `/oauth/*`: a rate limit. Behind a proxy set
  `http.trusted_proxies`, or every client shares the proxy's per-IP budget;
  in tests that hammer an endpoint, set the limit to `0`.
- Audit actions: `oauth.client_created`, `oauth.client_revoked`,
  `oauth.authorized`, `oauth.denied`, `oauth.revoked`,
  `oauth.refresh_reuse_detected`, `oauth.pat_created`, `oauth.pat_revoked`.
- Prune with `navree oauth:prune [--days N]` (schedule it yourself).

Docs: `/docs/packages/oauth-server/`.
