---
name: navree-security-checklist
description: "A pre-release security review for Navree apps covering deny-by-default routes, abilities and list filters, validation, tokens, rate limits and trusted proxies, CORS and security headers, CSRF for cookie sessions, secrets and APP_KEY, the audit log, uploads, SSRF and operations, with the commands that check each item. Use before shipping or deploying, during a security review or audit, or when asked whether a Navree app is safe to release."
---

# Security checklist

Navree is secure by default: routes deny until they declare access, input
is validated at the edge, errors never leak internals, Argon2id hashes
passwords, refresh tokens rotate with reuse detection, security headers and
rate limits are on. This checklist finds what an app can still get wrong.
Go through it in order; each item says how to check.

## 1. Every route says who may call it

- [ ] `navree routes` (or `navree routes --json`): read the ACCESS column.
  Every `public` route must be meant to be public (sign-in, health, webhooks
  with their own signature check, public content).
- [ ] No route was made `#[public]` to work around a 401/403.
- [ ] Hand-written `RouteDef` routes declare access (the boot refuses them
  otherwise).
- [ ] Admin-only routes use `#[roles("admin")]` or a permission, not just
  `#[authenticated]`.
- [ ] Webhooks on `#[public]` routes verify a signature (a `#[guard]`, or
  `navree::http::ValidSignature::guard()` for links you signed).

## 2. Authorization is right, not just present

- [ ] Record routes use `#[can(Action, Subject, from = "id")]` (404/403
  before the handler), not a type-level check plus a manual lookup.
- [ ] Every list filters with `Accessible<E, Read>`: compare what a
  low-privilege user gets from the list with what they can open.
- [ ] Updates call `post.check_fields(&input.changed_fields())` when rules
  limit fields (`.fields([..])`).
- [ ] `src/abilities.rs` has unit tests per role and guest
  (`AbilityCtx::for_user`, `AbilityCtx::guest()`); `cannot` rules come after
  the `can` rules they restrict.
- [ ] `navree access:explain <user> <action> <Subject> [id]` for the
  surprising cases.
- [ ] Soft-deleted records: handlers check `deleted_at` (record checks
  still load them).
- [ ] Tests cover guest (401), user without the grant (403) and owner/role
  (2xx) for every sensitive route.
- [ ] Workspace apps: `navree check` passes `workspace.entities`,
  `workspace.repo` and `workspace.tables`, and every workspace module has
  an isolation suite (`app.isolation_suite::<M>()`, skill
  `navree-module-conformity`).

## 3. Input

- [ ] Every body and query DTO goes through `Valid<..>` / `ValidAsync<..>`
  with `#[validate]` rules (or `skip`) on every field; `length(..)` counts
  characters (`length(bytes, ..)` when the limit is about bytes).
- [ ] Uniqueness also has a unique index (a race becomes a 409).
- [ ] No SQL built from strings; raw `Statement`s bind values. Async rule
  table/column names are constants, never input.
- [ ] Mass assignment: DTOs list only fields clients may set (no `owner_id`,
  `role`, `published` unless intended).
- [ ] Rich text that is stored and rendered as HTML is a `SafeHtml` field
  (feature `validation-html`), never a `String`: grep the DTOs for fields
  named `body`, `content`, `description`, `notes` and `html`. A custom
  `validation.html` policy allows no `img` or `iframe` unless the app means
  to load images from any server.

## 4. Authentication and tokens

- [ ] `APP_KEY` is set (or `auth.jwt.secret` ≥ 32 bytes), identical across
  `serve`, `queue:work` and `schedule:work`, and only in the environment.
- [ ] `auth.jwt.access_ttl_secs` stays short (default 900): access tokens
  can't be revoked.
- [ ] `auth.registration = false` unless public sign-up is intended.
- [ ] Browser front ends keep tokens server-side (BFF with httpOnly cookies);
  no tokens in `localStorage`.
- [ ] Features that need a verified email check `user.email_verified_at`.
- [ ] Custom strategies (API keys) store digests (`sha256_hex`), return
  `Err` for unknown credentials (never a guest) and set `token_abilities`.
- [ ] Every sign-in flow the app adds ends with `AuthService::sign_in`
  (or `check_sign_in` before `issue_tokens`), so sign-in policies such as
  single sign-on enforcement hold on it; `rg "issue_tokens" src` finds
  the ones that don't. Platform break-glass (`auth.platform.break_glass`)
  is a deliberate choice, and `auth.break_glass` entries are reviewed.
- [ ] Tokens other services sign (ID tokens, partner callbacks) are verified
  with `navree::crypto::jose` against a configured key or the provider's
  JWKS, never a key the token itself names, and their `exp`, `aud` and
  `iss` are checked (skill `navree-jose`).

## 5. Rate limits and client addresses

- [ ] `http.per_ip_rate_limit` is on (600/min default; never `null` in
  production). Expensive or abusable routes have `#[throttle(limit, per)]`.
- [ ] The `http.server` limits are on (never `null` for
  `header_read_timeout_ms`, `idle_timeout_ms`, `body_read_timeout_ms`,
  `write_stall_timeout_ms`, `max_connections` or `max_connections_per_ip` in
  production), and `BodyReadLimit::lift()` appears only on routes whose body
  is a stream, after the caller is authenticated: they stop clients holding
  connections.
- [ ] WebSocket routes authenticate the caller and have a heartbeat. The
  server does not bound a quiet socket unless
  `http.server.upgraded_idle_timeout_ms` is set. A route that trusts a cookie
  checks `upgrade.origin_allowed(&allowed)`: CORS does not apply to an
  upgrade. A raised `max_frame_size` or `max_message_size` has a reason.
- [ ] A raised `http.multipart` limit or `form.with_limits(..)` has a reason,
  and `field.file_name()` is never used as a path.
- [ ] Behind a proxy, load balancer or BFF: `http.trusted_proxies` lists
  it (else everyone shares one IP and one login throttle). Avoid
  `http.trust_proxy = true` unless nothing reaches the app directly.
- [ ] Per-IP limits count an IPv6 client by network (`http.rate_limit_ipv6_prefix`,
  64 by default), IPv4 by address; limits you write key with
  `HttpOptions::rate_limit_key(ip)` (`navree::http::rate_limit_key`), not the
  raw address.
- [ ] Turning on SSO enforcement (or removing someone) also calls
  `AuthService::revoke_credentials_of_domain` / `revoke_all_credentials`:
  `revoke_all_refresh_tokens` leaves personal access tokens and OAuth grants.
- [ ] `auth.login_throttle_per_minute` (5) and
  `auth.two_factor.max_attempts_per_minute` (5) are not 0.
- [ ] With the OAuth server: `oauth.rate_limits` (token, introspect,
  revoke, device_authorization; per client and per IP) are not 0.
- [ ] Know the limits: counters are per process (multiply by instances).

## 6. Browser-facing headers

- [ ] `http.cors.allowed_origins` lists exact origins only (empty when a
  BFF calls the API server-side).
- [ ] Security headers stay on (`http.security_headers` not `false`, unless
  the proxy sets its own); HTML routes set their own CSP.
- [ ] Served over HTTPS (HSTS is sent).
- [ ] A compressed page with a secret and reflected input in one body, on cookie
  auth that a cross-site request carries, can give the secret away through its
  length (BREACH): put `#[no_compression]` (`.no_compression()`) on the route or
  send `Cache-Control: no-transform`, or keep the secret off that page.
- [ ] With `SessionModule` (cookie sessions): CSRF stays on; `session.csrf_except`
  only covers signed webhooks; `session.secure = true`; call
  `session.regenerate()` on sign-in and `invalidate()` on sign-out.

## 7. Secrets and errors

- [ ] No secrets in `config/app.toml` or the repository; `.env` is
  gitignored; production secrets come from the environment.
- [ ] Your config sections wrap secrets in `Secret<T>` (redacted in logs).
- [ ] Errors use `AppError`; no internal details in `message`; 5xx details
  only in logs. `impl From<_> for AppError` isn't used (use `map_error`).
- [ ] Resources don't serialize secrets: models returned as JSON skip them
  (`#[serde(skip)]`) or use a `*Resource` struct.
- [ ] Rotating `APP_KEY`: old key in `APP_PREVIOUS_KEYS`.

## 8. Audit and monitoring

- [ ] `AuditModule` is in the app; writes go through models and a unit of
  work so data changes are audited (mass updates and raw statements aren't;
  record those with `uow.record_change`).
- [ ] Sensitive actions carry `#[audit(action = "..")]` (exports, deletions).
- [ ] `audit:prune` is scheduled and `audit.retention_days` matches policy.
- [ ] `audit.read` and `roles.manage` are granted only to admins.
- [ ] `/metrics` has `observe.metrics_token` or is blocked at the proxy.

## 9. Files and outgoing HTTP

- [ ] Uploads use `store_upload` with `UploadRules` (content-sniffed types,
  size limit); `original_name` is never used as a path.
- [ ] Temporary URLs have short expiries and aren't logged.
- [ ] `HttpClient` keeps `ssrf.block_private = true` when any URL comes from
  users; internal services are listed in `ssrf.allow_hosts`.

## 10. Operations

- [ ] `database.migrate_on_boot = false` in production; `migrate:fresh`
  never runs there.
- [ ] Dependencies scanned (`cargo audit` or OSV) and updated.
- [ ] `cargo clippy --all-targets -- -D warnings` and `cargo test` pass.

## Commands

```sh
navree routes --json        # access, throttle, guards and audit action per route
navree check                # module graph, then the conformity checks
navree gate                 # fmt, clippy, tests, check and the front end
navree access:explain <user> <action> <Subject> [id]
cargo test
```

## Docs

- https://navree.com/docs/basics/access-control/
- https://navree.com/docs/security/authorization/
- https://navree.com/docs/security/authentication/
- https://navree.com/docs/basics/rate-limiting/
- https://navree.com/docs/basics/cors-and-security-headers/
- https://navree.com/docs/basics/sessions-and-csrf/
- https://navree.com/docs/security/audit-log/
- https://navree.com/docs/security/encryption-and-hashing/
