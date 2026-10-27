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
