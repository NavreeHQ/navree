---
name: navree-sso
description: "Enterprise single sign-on with navree-sso: `SsoModule` and the `sso` config section, SAML 2.0 and OpenID Connect connections owned by the app, an organization or a workspace (`SsoOwner`), verified email domains (DNS TXT) with `POST /api/sso/discover` and password enforcement (`sso_required`), just-in-time provisioning with group to role mapping (`Provisioning`, `RoleRule`, `SsoUserProvisioned`), the `/api/sso/*` routes and the `sso.manage` management routes, the `Sso` service, `complete_url` and the one-time code exchange, and tests with `SsoFake` and `StaticTxtResolver`. Use when a customer asks for Okta, Entra ID (Azure AD), Google Workspace, AD FS, Ping or Keycloak sign-in per tenant, when setting up an identity provider's ACS URL, entity id or redirect URI, when forcing a company's people to use SSO, or when debugging `invalid_saml_response`, `invalid_state`, `unverified_domain`, `email_taken`, `sso_not_assigned` or `sso_required`."
---

# Single sign-on

`SsoModule` (crate navree-sso, facade feature `sso`, in `full`) gives each
organization or workspace its own SAML 2.0 or OpenID Connect connection and
ends a sign-in with the same navree-auth user and tokens as a password
login. Everything below is in the code; when in doubt read
`crates/navree-sso/src` and its tests (`tests/sso/`: flows, wrapping
attacks, Okta/Entra/Google-shaped samples) and
`crates/navree/tests/sso_docs.rs` (the docs examples through the facade).
The docs page is `apps/website/content/docs/security/single-sign-on.mdx`.

## Steps: add single sign-on to an app

1. Feature: `sso` is in `full`; with your own list add `"sso"` (it turns on
   `http`, `crypto`, `cache`, `events`, `http-client`, `db`, `auth`,
   `workspace`).
2. Modules, after `AuthModule` (and `WorkspaceModule` for tenant-owned
   connections):

```rust
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(CryptoModule::from_config())      // encrypts OIDC client secrets
    .module(HttpModule::from_config())
    .module(HttpClientModule::from_config())  // metadata, discovery, key sets, DoH
    .module(DatabaseModule::from_config())
    .module(CacheModule::from_config())       // states, used assertions, codes (shared store for several instances)
    .module(AccessModule::for_root(access))   // the `sso.manage` management routes
    .module(AuthModule::from_config())
    .module(WorkspaceModule::from_config())
    .module(SsoModule::from_config())         // section `sso`
```

   Without `AccessModule`, set `sso.management_routes = false` or the boot
   fails (`declares permissions:sso.manage but no dyn RouteAuthorizer`).
3. Config:

```toml
[sso]
base_url = "https://api.acme.test"                      # default app.url; builds ACS/metadata/callback URLs
complete_url = "https://app.acme.test/auth/sso/complete" # ACS/callback redirect here with ?code= or ?error=
enforce_for_verified_domains = true                     # refuse passwords for verified domains
```

4. Migrations: set `sso` (after `auth`): `sso_connections`, `sso_domains`,
   `sso_identities`.
5. Create a connection (`POST /api/sso/connections` with `sso.manage`, or
   `Sso::create_connection(&uow, NewConnection::saml(owner, name, xml))` /
   `NewConnection::oidc(owner, name, issuer, client_id, secret)`), add a
   domain (`POST /api/sso/connections/{id}/domains` gives `txt_name` and
   `txt_value`), publish the TXT record, `POST /api/sso/domains/{id}/verify`.
6. Give the identity provider `connection.service_provider`: SAML entity id
   `{base_url}/api/sso/saml/{slug}/metadata`, ACS (HTTP-POST)
   `{base_url}/api/sso/saml/{slug}/acs`; OIDC redirect URI
   `{base_url}/api/sso/oidc/{slug}/callback`.
7. Front end: `POST /api/sso/discover {email}` →
   `{sso, connection, login_url, password_allowed}`; call `login_url`
   (`?return_to=/path`) with `Accept: application/json` → `{url, state}`,
   keep `state` (session storage or the BFF cookie) and send the browser to
   `url`; on `complete_url?code=...` call
   `POST /api/sso/exchange {code, state}` → login JSON (`user`, tokens,
   `return_to`). With `state`, a code from another sign-in is refused
   (login CSRF). Without `complete_url` the ACS/callback answer the login
   JSON directly.

## Rules that matter

- **Linking.** Identity by (connection, subject) first; then a user with
  the same email only when the email's domain is a *verified domain of
  this connection*, the user's own email is verified (`email_verified_at`)
  and OIDC `email_verified` isn't `false`; else create
  (JIT, `provisioning.jit`) only for verified domains unless
  `allow_unverified_emails`. Otherwise 409 `email_taken` / 403
  `unverified_domain` / 403 `sso_provisioning_disabled`.
- **Roles.** `Provisioning { roles: vec![RoleRule { group, role }], default_role, sync_roles }`:
  first matching group wins (ASCII case-insensitive; Entra sends group
  object ids), else `default_role` (`member`; `None` refuses with 403
  `sso_not_assigned`). Workspace owner → `Members::add`/`change_role`;
  organization owner → `OrgMembers::add`/`update` reaching all workspaces.
  The owner's role never changes; rules can't grant `owner`.
- **Writes** of a sign-in (user, identity, membership) run in their own
  transaction: a refused sign-in writes nothing, even when answered with a
  redirect.
- **Enforcement** is one navree-auth `SignInPolicy` (`SsoSignInPolicy`):
  for a verified domain of an enabled, enforcing connection, every sign-in
  but that connection's own gets 403 `sso_required` ("Sign in with
  {connection name}"): password login, register and password change,
  magic link and emailed code (request and use), passkey, social sign-in
  (before any account is created or linked), token exchange,
  `POST /auth/2fa/verify` of a challenge started earlier, and other
  connections (checked on the IdP's email before provisioning, and on the
  account). Refusals are audited `auth.login_refused` with
  `metadata.method`. Existing sessions are not ended
  (`revoke_all_refresh_tokens` when turning enforcement on). Break-glass:
  platform administrators still reach the platform console
  (`auth.platform.break_glass`, default true, audited `auth.break_glass`),
  never the app. `enforce: Some(false)` on a connection opts out;
  `follow_default_enforcement: true` resets; a disabled connection
  enforces nothing. A sign-in flow you write ends with
  `AuthService::sign_in(&user, SignInMethod::sso(connection_id), &meta)`,
  never bare `issue_tokens`.
- **SAML checks** (`saml::validate_response`): success status, exactly one
  unencrypted assertion child of the response, response and/or assertion
  signed by a metadata certificate (every present signature must verify;
  RSA-SHA256/384/512, ECDSA; SHA-1 refused; RSA ≥ 2048 bits), unique ids,
  `Destination`, issuers, `InResponseTo` (IdP-initiated only with
  `sso.saml.allow_idp_initiated`), bearer `Recipient` and `NotOnOrAfter`,
  conditions and audience, `SessionNotOnOrAfter`, then the assertion id is
  remembered (400 `assertion_replayed`). Reasons are logged under
  `navree::security`; clients only see `invalid_saml_response`.
- **No encrypted assertions, no signed AuthnRequests, no single logout**
  (HTTP-Redirect for requests, HTTP-POST for responses).
- **Cookie sessions**: with `SessionModule`, exempt the ACS:
  `session.csrf_except = ["/api/sso/saml/*"]`.
- **OIDC**: discovery issuer must match; code flow + PKCE S256 + nonce;
  ID token verified against the JWKS (no `none`/HMAC), `iss`, `aud`/`azp`,
  `exp`, `iat`, `nbf`, `nonce`, `sub`; userinfo fills a missing email.
- Metadata URLs and issuers must be `https` (or `http://127.0.0.1` /
  `localhost` for tests).

## Testing

```rust
let idp = SsoFake::new();                       // SAML IdP whose responses pass every check
let sso = app.get::<Sso>().await;
let uow = app.get::<navree::db::Db>().await.uow(None, None).await;
sso.create_connection(&uow, NewConnection::saml(SsoOwner::App, "Acme", idp.metadata_xml()).slug("acme")).await?;
let domain = sso.add_domain(&uow, "acme", "acme.test").await?;
sso.mark_domain_verified(&uow, domain.id).await?;   // skips DNS
uow.commit().await?;
let url = app.client().get("/api/sso/acme/login").send().await.json_value()["url"].as_str().unwrap().to_owned();
let answer = idp.respond_to(&url)?.name_id("alice@acme.test").attribute("groups", ["Admins"]);
app.client().post("/api/sso/saml/acme/acs").form(&answer.form()).send().await.assert_status(200);
```

- `FakeResponse`: `sign_response(true)`, `sign_assertion(false)`,
  `failure(status, msg)`, `audience(..)`, `assertion_id(..)` (replays),
  `valid_for(..)`, `relay_state(..)`; `SsoFake::unsolicited(&sp, at)` for
  IdP-initiated.
- DNS: `.override_value::<dyn TxtResolver>(Arc::new(StaticTxtResolver::new()))`,
  then `dns.add(domain, &txt_value)`.
- OIDC: point `issuer` at a wiremock server serving
  `/.well-known/openid-configuration`, the JWKS and `/token`; sign ID
  tokens with `navree_crypto::jose::SigningKey` (see
  `crates/navree-sso/tests/sso/oidc_flow.rs`).
- Tests without `AccessModule`: `.config("sso.management_routes", false)`.

## Debugging

| Symptom | Look at |
|---|---|
| 400 `invalid_saml_response` | the `navree::security` log line: wrong audience/recipient (`base_url`, slug), clock skew, issuer vs metadata entity id, unsigned response, SHA-1 |
| 400 `invalid_state` | state used twice, expired (`sso.state_ttl_secs`), another connection, or several instances without a shared cache |
| 403 `unverified_domain` / 409 `email_taken` | the domain isn't verified for this connection |
| 403 `sso_not_assigned` | no `RoleRule` matched and `default_role` is null |
| 403 `sso_required` on `/auth/login` | the email's domain is verified and enforced |
| 502 `sso_provider_unavailable` | metadata URL, discovery, JWKS, token endpoint or DoH resolver unreachable (SSRF policy blocks private hosts) |
