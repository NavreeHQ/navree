---
name: navree-auth
description: "Authentication with navree-auth: `AuthModule`, the `/api/auth/*` routes, JWT access tokens and rotating refresh tokens, `CurrentUser`, `AuthService`, account flows (registration, password reset, email verification, profile and password changes), TOTP two-factor, custom strategies such as API keys, the login throttle and tests with real tokens. Use when adding sign-in or sign-up, reading the signed-in user, configuring tokens or the signing key, building password reset or two-factor, adding API keys, or testing authenticated requests."
---

# Authentication

`AuthModule` (crate navree-auth, in the default features) gives an app
email-and-password accounts with bearer tokens. A client posts credentials
to `/api/auth/login` and gets a short-lived **access token** (HS256 JWT, 15
minutes) and an opaque **refresh token** (30 days, single use, rotated on
every refresh). Every other request sends `Authorization: Bearer <access>`.
Routes stay denied until they declare access (see `navree-access-control`).

## Setup

A new app already has it. Otherwise add, once, in the root:

```rust
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
The migration set `auth` creates `users`, `refresh_tokens` and
`password_reset_tokens`; your sets that reference `users` use
`.after(["auth"])`.

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

Details of the account flows: `references/account-flows.md`.

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
is the model and `navree::auth::user::Entity` the entity; its columns are
fixed, so keep app data about users in your own table keyed by `users.id`
(the generated `users` module's `user_profiles` does this). Add fields to
the user JSON of `/api/auth/*` with a `UserResourceExtension` registered by
`m.user_resource_extension::<T>()`.

## AuthService (seeders, commands, admin screens)

Inject `Arc<AuthService>` (global):

```rust
let user = auth.create_user("Ada Lovelace", "ada@example.com", &password).await?; // no password policy
let user = auth.register(&uow, NewUser { name, email, password }).await?;          // with the policy, in your uow
let token = auth.jwt().sign_access(user.id)?;
let found = auth.find_by_email("ada@example.com").await?;
auth.revoke_all_refresh_tokens(user.id).await?;
```

Also: `attempt_login`, `issue_tokens`, `refresh`, `logout`, `find_user`,
`update_password`, `present(&user)` and the account-flow methods.

## API keys and other credentials: a strategy

Navree has no built-in API keys; add a `Strategy` (after the JWT one):

```rust
use navree::BoxFuture;
use navree::auth::Strategy;
use navree::axum::http::request::Parts;
use navree::contracts::{AuthVia, Principal};
use navree::crypto::sha256_hex;
use navree::db::Db;
use navree::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

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
            let found = api_key::Entity::find()
                .filter(api_key::Column::KeyHash.eq(digest))
                .one(self.db.conn())
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
login_throttle_per_minute = 5  # failed logins per email + IP; 0 = off
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

```rust
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
    let user = auth.create_user("Ada Lovelace", "ada@example.com", "correct horse battery").await.unwrap();
    let token = auth.jwt().sign_access(user.id).unwrap();
    app.client()
        .get("/api/v1/profile")
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("email", json!("ada@example.com"));
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

## Docs

- https://navree.com/docs/security/authentication/
- https://navree.com/docs/security/account-flows/
- https://navree.com/docs/security/encryption-and-hashing/
