---
name: navree-auth
description: "Authentication with navree-auth: `AuthModule`, the `/api/auth/*` routes, JWT access tokens and rotating refresh tokens, `CurrentUser`, `AuthService`, account flows (registration, password reset, email verification, profile and password changes), TOTP two-factor and a policy requiring it, sessions and step-up (`#[recent_auth]`), impersonation (`#[not_while_impersonating]`, `Actor`), magic links, emailed codes, passkeys, platform administrators, trusted token exchange, custom strategies such as API keys, the login throttle and tests with real tokens. Use when adding sign-in or sign-up, reading the signed-in user, configuring tokens or the signing key, building password reset, two-factor or passwordless sign-in, listing or ending sessions, guarding sensitive routes, letting support impersonate users, adding API keys, or testing authenticated requests."
---

# Authentication

`AuthModule` (crate navree-auth, in the default features) gives an app
email-and-password accounts with bearer tokens. A client posts credentials
to `/api/auth/login` and gets a short-lived **access token** (HS256 JWT, 15
minutes, written and read by `navree::crypto::jose`) and an opaque **refresh
token** (30 days, single use, rotated on every refresh). Every other request
sends `Authorization: Bearer <access>`.

`JwtService::verify` takes HS256 only and checks, in order: size (at most
`MAX_TOKEN_LEN`, 8 KiB) and shape, header (`alg` HS256, no `kid`, no `crit`),
the HMAC in constant time, the claims (`sub`, `iat`, `exp`, `iss`, `jti`,
`typ` present; `nbf` if any; `iss`; `aud` when configured), the token type,
`iat` no more than `LEEWAY_SECS` (30) ahead of the clock, and `exp` last
(`TokenExpired`, with the same leeway). There is one key and no `kid`: rotating
`auth.jwt.secret` or `APP_KEY` ends the access tokens in flight, never the
sessions (refresh tokens are opaque). To test hostile tokens, forge one with
`navree::crypto::jose::SigningKey::hmac(Algorithm::HS256, secret)` and
`sign_jws` with the header and payload you want.
Routes stay denied until they declare access (see `navree-access-control`).

## Setup

A new app already has it. Otherwise add, once, in the root:

```rust,ignore
Navree::app()
    .module(ConfigModule::for_root(ConfigSources::defaults()))
    .module(CryptoModule::from_config())   // required: Argon2id, APP_KEY
    .module(HttpModule::from_config())
    .module(DatabaseModule::from_config()) // required: users, refresh tokens
    .module(CacheModule::from_config())    // optional: login throttle counters
    .module(MailModule::from_config())     // optional: reset and verification mail (needs ViewModule)
    .module(AuthModule::from_config())     // section `auth`
```

Set `APP_KEY` (`navree key:generate`): the JWT key is derived from it unless
`auth.jwt.secret` (at least 32 bytes) is set. Without either, tokens are
signed with a random key per process and every restart signs everyone out.
The migration set `auth` creates `users`, `refresh_tokens`,
`password_reset_tokens`, `login_tokens` and `passkeys`; your sets that
reference `users` use `.after(["auth"])`.

## Routes (under `/api/auth`)

| Route | Access | Answer |
|---|---|---|
| `POST /register` `{name, email, password}` | public, when `auth.registration` | 201 user + tokens |
| `POST /login` `{email, password}` | public, throttled | 200 `{access_token, expires_in, refresh_token, token_type, user}` or a 2FA challenge |
| `POST /refresh` `{refresh_token}` | public | 200 a new pair; the old token is spent |
| `POST /logout` `{refresh_token}` or `{all: true}` | signed in | 204 |
| `GET /me`, `PATCH /me`, `PATCH /me/password` | signed in | the user; profile and password changes |
| `POST /forgot-password`, `POST /reset-password` | public | password reset |
| `POST /email/verification`, `GET /email/verify/{id}/{hash}` | signed in / signed link | email verification |
| `POST /2fa/enable`, `/2fa/confirm`, `/2fa/verify`, `/2fa/disable`, `/2fa/recovery-codes` | mixed | TOTP two-factor |
| `GET /sessions`, `DELETE /sessions/{id}`, `POST /confirm` `{password}` or `{code}` | signed in | sessions; step-up confirmation |
| `POST /confirm/options` `{ticket?}`, `POST /confirm/passkey` `{credential}` | signed in | passkey step-up and confirming one action (`auth.passkeys.enabled`) |
| `GET /auth/app/sign-in`, `GET /auth/app/finish` (outside `/api`), `POST /app/exchange` `{platform, app, code, code_verifier}` | public | a native app takes over a sign-in from the system's sign-in session (PKCE S256; apps of `auth.apps` with a `callback`) |
| `POST /impersonate/{user}` `{reason}`, `DELETE /impersonate` | `users.impersonate` / signed in | impersonation (`auth.impersonation.enabled`) |
| `POST /magic-link`, `/magic-link/verify`, `/otp`, `/otp/verify` | public | passwordless (`auth.magic_link.enabled`, `auth.otp.enabled`) |
| `/passkeys/register/options`, `/passkeys/register`, `/passkeys/login/options`, `/passkeys/login`, `GET /passkeys`, `DELETE /passkeys/{id}` | mixed | passkeys (`auth.passkeys.enabled`) |
| `POST /platform/login`, `/platform/2fa/verify` | public | platform administrators (`auth.platform.enabled`) |
| `POST /exchange` `{token}` | public | trusted issuers' JWTs (`auth.trusted_issuers.<name>`) |

Details of the account flows: `references/account-flows.md`. Sessions,
step-up, impersonation, passwordless, platform and token exchange, and signing
a browser in on the session cookie (the Navree UI pages, `AuthPages`, and the
JSON routes of a single-page app): `references/sessions-and-sign-in.md`.

## Sensitive routes: step-up and impersonation guards

```rust,ignore
#[controller("/account", version = "1")]
#[authenticated]
impl AccountController {
    #[delete("/")]
    #[recent_auth(minutes = 10)]   // 403 reauthentication_required unless signed in or confirmed in 10 min
    #[not_while_impersonating]     // 403 impersonation_forbidden in an impersonation session
    async fn destroy(&self, CurrentUser(user): CurrentUser) -> Json<String> {
        Json(format!("Deleting {}", user.email))
    }
}
```

Both are guards (the route still needs an access declaration) and may sit on
the impl block. On a 403 `reauthentication_required` the client posts the
password (or a TOTP code) to `POST /api/auth/confirm`, swaps in the returned
access token and retries. Hand-written routes: `.guard(recent_auth(dur))`,
`.guard(not_while_impersonating())`, `.guard(platform_session())`.
Extractors: `AccessClaims` (the token's `sid`, `auth_time`, impersonator),
`Actor` (user and impersonator), `PlatformAdmin(user)`.

Approving one thing (a payment, a refund, a role change) needs more than a
recent sign-in: build a `ConfirmAction` from the server's own record and
check the passkey assertion with
`auth.confirm_action(&claims, &action, ConfirmProof::Passkey(credential), &meta)`.
The challenge was derived from the same action, so a confirmation of ₹100
never approves ₹1,00,000. Results a client posts back later carry a sealed
one-time ticket (`dyn navree::contracts::CallTickets`). Native apps: passkey
origins and the association files of `auth.apps`. All in
`references/sessions-and-sign-in.md`.

## Read the signed-in user

```rust
use navree::auth::UserResource;
use navree::http::CurrentPrincipal;
use navree::prelude::*;

#[injectable]
pub struct ProfileController;

#[controller("/profile", version = "1")]
impl ProfileController {
    #[get("/")]
    #[authenticated]
    async fn show(&self, CurrentUser(user): CurrentUser) -> Json<UserResource> {
        Json(UserResource::from(&user)) // the `users` row: id, name, email, email_verified_at, ...
    }

    #[get("/id")]
    #[authenticated]
    async fn id(&self, CurrentPrincipal(me): CurrentPrincipal) -> String {
        me.user_id.to_string() // no database query
    }
}
```

`uow.actor()` also gives the user id inside services. `navree::auth::User`
is a navree-orm model (`User::query()`, `User::EMAIL`, `User::find(&db, id)`)
and the access subject `User` (`#[model(subject = "User", permission_prefix =
"users")]`): register it with `AccessModule::for_feature::<navree_auth::User>()`
and take `Authorized<User, Read>`, which derefs to `Record<User>`
(`.into_inner()` is the Record, `.into_inner().into_inner()` the `User`).
Modules
that create users (social sign-in, single sign-on) write them with
`navree::auth::user::insert(&uow, user)`, which records the change event
without the secrets. Its
columns are fixed and `AuthService` writes them (change events carry no
password hash or two-factor data), so keep app data about users in your own
table keyed by `users.id`
(the generated `users` module's `user_profiles` does this). Add fields to
the user JSON of `/api/auth/*` with a `UserResourceExtension` registered by
`m.user_resource_extension::<T>()`.

## AuthService (seeders, commands, admin screens)

Inject `Arc<AuthService>` (global):

```rust,ignore
let user = auth.create_user("Anu Lakhotia", "anu@example.com", &password).await?; // no password policy
let user = auth.register(&uow, NewUser { name, email, password }).await?;          // with the policy, in your uow
let token = auth.jwt().sign_access(user.id)?;
let found = auth.find_by_email("anu@example.com").await?;
auth.revoke_all_refresh_tokens(user.id).await?;                  // sessions only
let ended = auth.revoke_credentials_of_domain("agra.com").await?; // offboarding: see below
```

Also: `attempt_login`, `sign_in`, `check_sign_in`, `two_factor_challenge`,
`issue_tokens` (no sign-in policy: tests and seeders), `refresh`, `logout`, `find_user`,
`update_password`, `present(&user)`, the account-flow methods, and
`sessions`, `revoke_session`, `confirm`, `impersonate`, `stop_impersonating`,
`two_factor_required`, `send_magic_link`, `login_with_magic_link`,
`send_login_code`, `login_with_code`, the passkey methods,
`is_platform_admin`, `platform_login` and `exchange_token`.

## Sign-in policies (one rule for every path)

A `SignInPolicy` (registered with `m.sign_in_policy::<P>()`) gets a
`SignInAttempt { email, user_id, method, audience }` (`attempt.domain()` is
normalized: lower-case ASCII, `xn--` for international names, no trailing dot,
full-width letters and `。` mapped; `navree::auth::email_domain(email)` gives
the same form) and answers `Ok(())` or
`AuthError::SignInRefused { code, message }` (403 with that code, audited
`auth.login_refused`). Every flow asks it before issuing anything:
password login/register/password change (`Password`), an email change
on `PATCH /me` (`Password`, for the old and the new address), magic link and email
code at request and use (`MagicLink`, `Otp`), passkeys (`Passkey`), token
exchange (`TokenExchange { issuer }`), `POST /auth/2fa/verify` (the first
step's method, carried in the challenge's `sign_in` claim), social
(`Social { provider }`), SSO (`Sso { connection }`), platform sign-in
(audience `Platform`; `auth.platform.break_glass = true` lets
administrators past a refusal, audited `auth.break_glass`). navree-sso's
enforcement is such a policy. Your own sign-in flow ends with
`auth.sign_in(&user, method, &meta)`; a two-factor challenge you hand out
comes from `auth.two_factor_challenge(&user, &method)`. Impersonation and
refresh don't ask (support sessions; existing sessions).

### Offboarding: end every credential

Policies decide at sign-in only. When a rule starts to apply (SSO
enforcement turns on for a domain), call
`auth.revoke_credentials_of_domain("agra.com")` (or
`auth.revoke_all_credentials(user_id)`), not just
`revoke_all_refresh_tokens`: it revokes the sessions and real-time
subscriptions of navree-auth, then asks every `dyn CredentialRevoker`
(navree-oauth registers one: personal access tokens, OAuth access and
refresh tokens, unexchanged codes, remembered consent). It returns
`CredentialsRevoked { users, by_kind }` (`revoked("personal_access_tokens")`,
`total()`) and audits `auth.credentials_revoked`. The domain is read like
`attempt.domain()`. Access JWTs already issued live out `auth.jwt.access_ttl_secs`
(15 minutes) but can't be refreshed. Your module can take part:
`m.credential_revoker::<R>()` (`navree::contracts::ModuleBuilderRevocationExt`)
with `R: CredentialRevoker` and `revoke_for_users(&[Uuid])`. A failing revoker
yields `AuthError::Revocation` after all others ran; repeat the call. Finding a
domain's users scans the users' emails in pages: for an offboarding, not a
request.

## Require two-factor authentication

`auth.two_factor.required_for = ["admin"]` (needs `AccessModule`), or a
`TwoFactorPolicy` registered with `m.two_factor_policy::<P>()` (workspace or
organization rules). Users it applies to without 2FA get
`two_factor_setup_required: true` and a token that reaches only the setup
routes (`2fa/enable`, `2fa/confirm`, `GET /me`, `logout`, `refresh`);
everything else is 403 `two_factor_required`. After confirming, the client
refreshes for a full token.

## API keys and other credentials: a strategy

Navree has no built-in API keys; add a `Strategy` (after the JWT one):

```rust
use http::request::Parts;
use navree::BoxFuture;
use navree::auth::Strategy;
use navree::contracts::{AuthVia, Principal};
use navree::crypto::sha256_hex;
use navree::db::Db;
use navree::prelude::*;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// An API key's row: only the digest is stored.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "api_keys", timestamps)]
pub struct ApiKey {
    pub id: Uuid,
    pub user_id: Uuid,
    pub key_hash: String,
    pub abilities: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[injectable]
pub struct ApiKeyStrategy {
    db: Arc<Db>,
}

impl Strategy for ApiKeyStrategy {
    fn name(&self) -> &'static str {
        "api-key"
    }

    fn authenticate<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        Box::pin(async move {
            let Some(key) = parts.headers.get("x-api-key") else {
                return Ok(None); // not ours: the next strategy, or a guest
            };
            let digest = sha256_hex(key.as_bytes()); // store digests, never keys
            let found = ApiKey::query()
                .where_(ApiKey::KEY_HASH.eq(digest))
                .first(&*self.db)
                .await
                .map_err(AppError::internal)?;
            let Some(row) = found else {
                return Err(AppError::unauthenticated().with_detail("Invalid API key")); // never a guest
            };
            Ok(Some(Principal {
                user_id: row.user_id,
                via: AuthVia::Token,
                token_abilities: Some(row.abilities.split(',').map(str::to_owned).collect()), // narrows abilities
                session_id: None,
            }))
        })
    }
}

// in a module's wire: m.strategy::<ApiKeyStrategy>();
```

## Configuration

```toml
[auth]
registration = true            # false: POST /auth/register answers 404
login_throttle_per_minute = 5  # failed logins per email + client (IPv4 address or IPv6 /64); 0 = off
password_min_length = 8
routes_prefix = "/auth"

[auth.jwt]
issuer = "navree"
access_ttl_secs = 900
refresh_ttl_secs = 2592000
# secret = ">= 32 bytes"       # set it with APP_AUTH__JWT__SECRET, never in the TOML file
```

Or `AuthModule::for_root(AuthOptions { registration: false, jwt: JwtOptions { .. }, ..AuthOptions::default() })`.
A new app's `config/app.toml` sets `auth.registration`; change it there or
with a real `APP_AUTH__REGISTRATION` environment variable.

## Browser front ends

Don't keep tokens in browser JavaScript. Use a backend-for-frontend: the web
server holds the tokens in httpOnly cookies and calls the API with
`Authorization: Bearer` (what `navree new --ui next` generates). Refresh
once per token (concurrent refreshes count as reuse and revoke the session),
and list the BFF in `http.trusted_proxies` so throttles see real client IPs.

## Test

```rust,ignore
use navree::auth::AuthService;
use navree::crypto::generate_key;
use navree::prelude::*;
use navree::testing::TestApp;
use serde_json::json;

#[tokio::test]
async fn shows_the_signed_in_users_profile() {
    let app = TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("hashing.memory_kib", 1024) // fast Argon2id in tests
        .config("hashing.iterations", 1)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(ProfileModule)
        .build()
        .await;
    let auth = app.get::<AuthService>().await;
    let user = auth.create_user("Anu Lakhotia", "anu@example.com", "correct horse battery").await.unwrap();
    let token = auth.jwt().sign_access(user.id).unwrap();
    app.client()
        .get("/api/v1/profile")
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("email", json!("anu@example.com"));
    app.client().get("/api/v1/profile").send().await.assert_error(401, "Unauthorized");
    app.shutdown().await.unwrap();
}
```

With `AuthModule` in the app, `app.acting_as(..)` panics unless you build
with `.fake_auth()` (then bearer tokens stop working; `CurrentUser` still
needs a real `users` row). To test sign-in itself, post to
`/api/auth/login` and read `access_token`.

## Common mistakes

- Sending a stale or invalid bearer token to a public route (login,
  refresh): it is a 401 on every route; drop it first.
- Refreshing the same refresh token twice (parallel tabs): the family is
  revoked as stolen.
- Expecting logout or a password change to kill access tokens: they stay
  valid until they expire (15 minutes); keep `access_ttl_secs` short.
- Different `APP_KEY`s across instances or processes: tokens from one fail on another.
- Adding `SessionModule`'s authenticator next to `AuthModule`: an app has one
  `dyn Authenticator`.
- Unverified emails aren't blocked: check `user.email_verified_at` where it matters.
- Writing a "danger zone" route without `#[recent_auth]` or
  `#[not_while_impersonating]`: impersonation sessions and old tokens pass
  plain `#[authenticated]`.
- Turning on impersonation without `AccessModule`: its route requires the
  permission `users.impersonate`.
- Building a `ConfirmAction` from what the client posted (the amount in the
  form) instead of the server's record: the binding then proves nothing.
- Listing an Android debug certificate in production `auth.apps`: any
  debug-signed copy of the app can then use the site's passkeys.
- A native app exchanging a hand-over code that arrived when it had no
  sign-in session open, or with another `state`: that is how a planted code
  would arrive. Only exchange the callback of the app's own session.

## Docs

- https://navree.com/docs/security/authentication/
- https://navree.com/docs/security/account-flows/
- https://navree.com/docs/security/sessions/
- https://navree.com/docs/security/passwordless/
- https://navree.com/docs/security/impersonation/
- https://navree.com/docs/security/encryption-and-hashing/
