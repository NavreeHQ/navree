---
name: navree-social
description: "Social sign-in with navree-social: `SocialModule` and the `social` config section, providers (google, github, apple, microsoft, gitlab, slack, linkedin, discord, x, any `oidc` issuer, `custom` via `SocialProvider`), the `/api/auth/social/*` routes, the `Social` service (`redirect`, `user`, `sign_in`, `link`, `unlink`, `accounts`, `token`), linked accounts and `link_by_email`, two-factor on social sign-ins, stored provider tokens, and tests with `Social::fake()`/`SocialFake` or a wiremock provider. Use when adding \"Continue with Google/GitHub/Apple/...\" buttons, single sign-on with Okta, Auth0, Keycloak or Entra ID, connecting providers from a settings page, calling a provider's API on a user's behalf, or debugging a 400 `Invalid or expired state`, 403, 409 or 502 from a social route."
---

# Social sign-in

`SocialModule` (crate navree-social, facade feature `social`, **opt-in**:
not in `full`) signs people in with a provider and ends with the same
Navree user and tokens as a password login (navree-auth). Everything below
is in the code; when in doubt read `crates/navree-social/src` and the tests
in `crates/navree-social/tests/social/` (fake and wiremock providers) and
`crates/navree/tests/social_docs.rs` (the docs examples through the facade).

## Steps: add a provider

1. Turn on the feature: `navree = { ..., features = ["social"] }` (it also
   turns on `auth`, `http-client`, `crypto`, `cache`, `events`, `db`, `orm`
   and `http`).
2. Add the module after `AuthModule` (it needs Crypto, Database and Auth;
   HttpClient and Cache are used when present):

```rust
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(CryptoModule::from_config())     // APP_KEY seals states and encrypts tokens
    .module(HttpModule::from_config())
    .module(HttpClientModule::from_config()) // provider calls
    .module(DatabaseModule::from_config())
    .module(CacheModule::from_config())      // used states (shared store for several instances)
    .module(AuthModule::from_config())
    .module(SocialModule::from_config())     // section `social`
```

3. Configure. The redirect URI registered with the provider is
   `{redirect_base}/{key}` (the front end's callback page, not the API):

```toml
[social]
redirect_base = "https://app.acme.test/auth/callback"

[social.providers.github]
client_id = "Iv1.8a61f9b3a7aba766"

[social.providers.okta]          # any OpenID Connect issuer
type = "oidc"
name = "Okta"
issuer = "https://acme.okta.com"
client_id = "0oa8f2kq1xYz3AbC4d5e"
```

   Secrets go in `.env`: `APP_SOCIAL__PROVIDERS__GITHUB__CLIENT_SECRET=...`.
   Apple needs `team_id`, `key_id` and `private_key` (the `.p8` PEM; `\n`
   escapes work in one env var) instead of a secret.
4. Migrations: the `social` set (table `social_accounts`, after `auth`) runs
   with the others (`migrate`, or `database.migrate_on_boot`).
5. Front end (BFF): `GET /api/auth/social/{key}/redirect?return_to=/deals`
   gives `{url, state}`; keep `state` in an httpOnly cookie and redirect to
   `url`. On the callback page compare the URL's `state` with the cookie,
   then `POST /api/auth/social/{key}/callback {code, state}` (add `user`
   for Apple, which form-posts `code`, `state` and `user`). The answer is
   the login body plus `return_to`, or `{two_factor_required,
   challenge_token, expires_in}` (finish with `POST /api/auth/2fa/verify`).

## Routes (under `{api_prefix}{auth.routes_prefix}/social`, default `/api/auth/social`)

| Route | Access | Answer |
|---|---|---|
| `GET {provider}/redirect?return_to&intent` | public; `intent=link` needs the bearer token (401) | `{url, state}` |
| `POST {provider}/callback {code, state, user?}` | public; per-IP `auth.login_throttle_per_minute` (429) | login body + `return_to` |
| `GET accounts` | signed in | `[{provider, provider_name, email, name, avatar_url, linked_at}]` |
| `POST {provider}/link {code, state}` | signed in; state from `?intent=link` by the same user | 201 account |
| `DELETE accounts/{provider}` | signed in | 204; 409 if it is the last way to sign in |

`social.routes = false` removes them; the `Social` service still works.

## Linking rules (in this order)

1. A `social_accounts` row for (provider, provider user id) signs in its user.
2. `link_by_email = "verified"` (default): a **verified** provider email
   matching a user links and signs in. Unverified emails never link.
   Microsoft emails are unverified unless the token has `xms_edov`; GitHub
   uses the primary verified address; Discord's `verified`; X's
   `confirmed_email` (scope `users.email`).
3. Registration on (`social.registration`, default `auth.registration`):
   creates the user (provider name, email, verified when the provider
   verified it, **no password** until a password reset) and links.
4. Else 409 `email_taken` (`An account with this email already exists.
   Sign in, then connect GitHub in your settings.`); registration off: 403
   `registration_closed`; no email for a new user: 400 `missing_email`.

`allowed_email_domains` (section or provider) admits only verified
addresses of those domains (403 `sign_in_not_allowed`). Every refusal has
its own `code` in the error body (`invalid_state`, `provider_refused`,
`account_already_linked`, `last_sign_in_method`,
`reauthorization_required`, `provider_unavailable`, ...); front ends
translate the code, never the message. `two_factor = "require"` (default) gives
2FA users the login challenge; `"skip"` trusts the provider.

Sign-in policies: `Social::sign_in` asks navree-auth's `SignInPolicy`s
(`SignInMethod::Social { provider }`) on the provider's email before
resolving (nothing is created or linked when refused) and on the account
before the session or challenge (`AuthService::sign_in`,
`two_factor_challenge`). A domain enforcing single sign-on answers 403
`sso_required`.

## Your own routes: the Social service

```rust
use navree::auth::{ClientMeta, LoginOutcome};
use navree::prelude::*;
use navree::social::{Social, SocialCallback, SocialRedirect};

#[injectable]
pub struct SsoController {
    social: Arc<Social>,
}

#[controller("/sso", version = "1")]
impl SsoController {
    #[get("/okta")]
    #[public]
    async fn redirect(&self) -> Result<Json<SocialRedirect>> {
        Ok(Json(self.social.redirect("okta").await?))
    }

    #[post("/okta")]
    #[public]
    async fn callback(&self, meta: ClientMeta, Json(cb): Json<SocialCallback>) -> Result<Json<serde_json::Value>> {
        let profile = self.social.user("okta", &cb).await?; // state check + code exchange
        // profile.raw holds the ID token claims / API user (e.g. "groups")
        match self.social.sign_in(&profile, &meta).await? {
            LoginOutcome::Authenticated { user, tokens } => Ok(Json(serde_json::json!({
                "user_id": user.id, "access_token": tokens.access_token,
            }))),
            LoginOutcome::TwoFactorRequired { challenge_token, .. } => Ok(Json(serde_json::json!({
                "two_factor_required": true, "challenge_token": challenge_token,
            }))),
        }
    }
}
```

Other methods: `redirect_with(key, RedirectOptions { scopes, params,
return_to, intent, user_id, .. })` (`Intent::Link` needs `user_id`),
`link(&uow, user_id, &social_user)`, `unlink(&uow, user_id, key)`,
`accounts(user_id)`, `find_user(key, provider_user_id)`, `providers()`,
`provider_name(key)`, `token(user_id, key)`. `SocialError` returned with
`?` keeps its status (the module registers the mapping).

## Provider API calls later

```toml
[social]
store_tokens = true
[social.providers.google]
client_id = "..."
scopes = ["https://www.googleapis.com/auth/calendar.readonly"]
params = { access_type = "offline", prompt = "consent" }
```

```rust
let google = social.token(user.id, "google").await?; // Secret<String>, refreshed when expiring within 60 s
http.get(url).bearer(google.expose()).send().await?;
```

`SocialError::Reauthorize` (refresh refused, or no refresh token): send the
user through `?intent=link` again. `NoToken`: `store_tokens` was off.

## A custom provider

```rust
#[injectable]
pub struct Zoho { http: Arc<navree::http_client::HttpClient> }

impl SocialProvider for Zoho {
    fn key(&self) -> &'static str { "zoho" }   // = [social.providers.zoho]
    fn authorize_url(&self, _c: &ProviderConfig, url: AuthorizeUrl) -> AuthorizeUrl {
        url.endpoint("https://accounts.zoho.com/oauth/v2/auth").scopes(["AaaServer.profile.Read"])
    }
    fn user<'a>(&'a self, _c: &'a ProviderConfig, token: ProviderToken)
        -> BoxFuture<'a, Result<SocialUser, SocialError>> {
        Box::pin(async move {
            let p: serde_json::Value = self.http.get("https://accounts.zoho.com/oauth/user/info")
                .bearer(token.access_token.expose()).send().await?.json()?;
            Ok(SocialUser::new("zoho", p["ZUID"].to_string())
                .email(p["Email"].as_str(), true).token(token).raw(p))
        })
    }
}
// .module(SocialModule::for_feature::<Zoho>())
// [social.providers.zoho] type = "custom", client_id, token_url (required)
```

## Test

```rust
let (social, fake) = Social::fake();
fake.on_code("github", "code-ada",
    SocialUser::new("github", "583231").name("Ada").email(Some("ada@example.com"), true));
let app = TestApp::builder()
    .temp_database()
    .config("app.key", navree::crypto::generate_key())
    .config("social.redirect_base", "https://app.acme.test/auth/callback")
    .config("social.providers.github.client_id", "test")
    .module(CryptoModule::from_config())
    .module(DatabaseModule::from_config())
    .module(AuthModule::from_config())
    .module(SocialModule::from_config())
    .override_value(Arc::new(social))
    .build()
    .await;
// GET /api/auth/social/github/redirect -> state; POST .../callback {code, state} -> 200
```

`fake.on_code_error(key, code, "access_denied")` gives a 400;
`fake.on_refresh(key, ProviderToken::new(..))` answers `Social::token`
refreshes; `fake.exchanged()` lists `(provider, code)`. Unknown codes are
`invalid_grant`. For a real provider integration test, keep the real
`Social`, point `url` / `authorize_url` / `token_url` / `userinfo_url` /
`jwks_url` at a wiremock server and allow it in the HTTP client
(`HttpClientModule::for_root(HttpClientOptions { ssrf: SsrfPolicy {
block_private: true, allow_hosts: vec!["127.0.0.1".into()] }, ..Default::default() })`).
Use `FixedClock` (`override_value::<dyn Clock>`) to expire states (600 s).

## Checks

- `cargo test -p navree-social` and, through the facade,
  `cargo test -p navree --features social --test social_docs`.
- Every provider's registered redirect URI equals `{redirect_base}/{key}`.
- `APP_KEY` is set and stable (states and stored tokens are sealed with it).
- Several instances: `CacheModule` with a shared store, or a used state
  works once per instance.

## Gotchas

- `social` is not in `full`: without the feature `navree::social` does not
  exist.
- `Invalid or expired state` (400) covers: tampered, other provider, other
  app key, older than `state_ttl_secs`, already used, and a link state sent
  to `/callback` (or a sign-in state to `/link`).
- `return_to` must start with `/` (not `//`); anything else is 400 at the
  redirect.
- The callback throttle shares `auth.login_throttle_per_minute` (counted per
  IPv4 address or IPv6 /64, `http.rate_limit_ipv6_prefix`); set it to `0` in
  request-heavy tests.
- An `oidc` provider whose discovery document names another issuer answers
  500 (misconfiguration); the fake skips discovery.
- Users created by social sign-in have no password: `PATCH
  /api/auth/me/password` can't work for them; use password reset.
- Provider keys are lower-case `[a-z0-9_-]`, never `accounts`.
- Apple sends the name only on the first sign-in: pass the `user` field to
  the callback or it is lost.

## Docs

`apps/website/content/docs/packages/social-sign-in.mdx`
(`/docs/packages/social-sign-in/`).
