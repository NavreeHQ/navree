# Sessions, step-up, impersonation and other sign-ins

## Sessions

- A session is a refresh-token family. Access tokens carry `sid` (the
  session) and `auth_time` (last sign-in or confirmation, Unix seconds).
- `GET /api/auth/sessions` -> `{data: [{id, ip, user_agent, device,
  last_used_at, authenticated_at, expires_at, current, impersonator_id,
  platform}]}`; `DELETE /api/auth/sessions/{id}` -> 204 (404 for unknown or
  someone else's). Access tokens of an ended session live until they expire.

## Step-up

- `#[recent_auth(minutes = N)]` (1..=10080): 403 `reauthentication_required`
  unless `auth_time` is at most N minutes old (impersonation sessions never
  pass).
- `POST /api/auth/confirm` `{password}` or `{code}` (TOTP) -> `{access_token,
  token_type, expires_in, auth_time}`; the session remembers it, refreshed
  tokens keep the new `auth_time`. Wrong password/code: 400 field error,
  counted by the login / two-factor throttles.

## Impersonation

```toml
[auth.impersonation]
enabled = true            # needs AccessModule
ttl_secs = 3600           # fixed, refreshing doesn't extend it
permission = "users.impersonate"
```

- `POST /api/auth/impersonate/{user}` `{reason}` -> the user + tokens +
  `impersonator_id`. Token: `act: {sub: <admin>}`, no `auth_time`.
- Refused: yourself, platform admins, users holding the permission, nested.
- `Actor { user_id, impersonator_id }`, `actor.real_user_id()`.
- Audit: every entry of the session has `impersonator_id`
  (`GET /api/audit?impersonator_id=`); `auth.impersonation_started` (reason)
  and `auth.impersonation_ended`.
- Built-in routes refusing it: `PATCH /me`, `PATCH /me/password`, `2fa/*`
  (not verify), `confirm`, `DELETE /sessions/{id}`, `impersonate`, passkey
  registration and removal.
- `DELETE /api/auth/impersonate` ends it (400 when not impersonating).

## Passwordless

- Magic links: `POST /api/auth/magic-link {email}` -> 202 always; mail links
  `{auth.magic_link.url}?token=..`; `POST /api/auth/magic-link/verify
  {token}` -> login response (or 2FA challenge). Single use, 15 minutes.
- Codes: `POST /api/auth/otp {email}`, `POST /api/auth/otp/verify {email,
  code}`; 6 digits, 10 minutes, burned after `auth.otp.max_attempts` (5),
  counted by the login throttle.
- Passkeys (`auth.passkeys.{enabled, rp_id, origins}`): register options ->
  `navigator.credentials.create` -> `/passkeys/register {name, credential}`;
  login options `{email?}` -> `navigator.credentials.get` ->
  `/passkeys/login {credential}`. ES256 and Ed25519, `attestation: "none"`.
  A user-verifying passkey skips the TOTP challenge.
- Tests: `navree::auth::testing::VirtualAuthenticator::new(rp_id, origin)`;
  `.create(&options["publicKey"])`, `.get(&options["publicKey"])`.

## Browser sign-in (sessions on the cookie)

Features `session` and `ui` of the facade, with `SessionModule` in the app. A
browser signs in through the same steps as a token client (the sign-in
policies, the login and two-factor throttles, two-factor authentication, the
audit trail), but it gets a server-side session bound to its navree-session
session, never a token. Signing in gives the session a new id and CSRF token;
signing out, ending the session from another device (`DELETE
/auth/sessions/{id}`), changing or resetting the password and offboarding end
it on the server at once, for a copied cookie too. `SessionStrategy` reads the
user back on every request (`AuthVia::Session`, the same `AccessClaims` as a
token, so `.authenticated()`, `#[recent_auth]` and `#[not_while_impersonating]`
hold for both).

- `AuthService::sign_in_session(&session, email, password, remember, &meta)`
  answers `SessionSignIn::SignedIn(user)` or `TwoFactorRequired` (the session
  keeps the challenge); `verify_two_factor_session` finishes it.
  `login_with_magic_link_session`, `login_with_code_session` and
  `login_with_passkey_session` are the same for a mailed link, a mailed code
  and a passkey; `sign_in_to_session` is the hook for flows of your own.
  `sign_out_session`, `change_password_session`, `confirm_session`,
  `session_user` and `two_factor_setup_required` complete them.
- `AuthPages::new().app_name("..")` (feature `ui`) is the pages: `/sign-in`,
  `/sign-in/two-factor`, `/forgot-password`, `/reset-password`, `POST
  /sign-out`, the setup page `/account/two-factor` and, when on,
  `/sign-in/link`, `/sign-in/code` and `/sign-in/passkey`. A user whom a
  `TwoFactorPolicy` requires to set two-factor up reaches the setup routes and
  signing out only (403 `two_factor_required` elsewhere); the pages send them
  to the setup page after signing in.
- `POST /api/auth/session/login`, `/session/2fa/verify`,
  `/session/magic-link/verify` and `/session/otp/verify` are the JSON routes of
  a single-page app on the cookie (no token; the `X-XSRF-TOKEN` header).
  `POST /api/auth/logout` signs a browser out, `GET /api/auth/me` reads the user.
- Impersonation in the browser: `POST /api/auth/impersonate/{user}` from a
  signed-in browser calls `impersonate_session` (the session becomes the
  user's, with `act` and no `auth_time`, ending after
  `auth.impersonation.ttl_secs`); `DELETE /api/auth/impersonate` or `POST
  /impersonate/stop` calls `stop_impersonating_session` and the browser is the
  administrator again. Signing out while impersonating ends both sign-ins.
- Settings: `session.secure`, `session.host_prefix` (cookie `__Host-navree_session`),
  `auth.session.lifetime_hours`, `auth.session.remember_days`,
  `auth.password_reset.url`.

## Platform administrators

```toml
[auth.platform]
enabled = true
roles = ["platform-admin"]   # navree-access roles
```

`POST /api/auth/platform/login` -> always a challenge (403
`two_factor_required` without 2FA); `POST /api/auth/platform/2fa/verify` ->
tokens of type `platform` (8-hour sessions, own throttle, role re-checked at
refresh). Guard routes with `PlatformAdmin(user)` or `platform_session()`.

## Trusted token exchange

```toml
[auth.trusted_issuers.legacy]
issuer = "https://legacy.example.com"
jwks_url = "https://legacy.example.com/.well-known/jwks.json"  # or public_key (PEM or JWK)
audience = "navree-app"
enabled = true
```

`POST /api/auth/exchange {token}`: RS256/ES256/EdDSA, `aud`, `exp`/`iat`
(lifetime <= 300 s), single-use `jti`, `email` of a registered user. Every
refusal is 401 "Invalid token"; the reason is in `auth.token_exchange_failed`.
