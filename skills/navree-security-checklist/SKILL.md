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

## 3. Input

- [ ] Every body and query DTO goes through `Valid<..>` / `ValidAsync<..>`
  with garde rules on every field; lengths use `length(chars, ..)`.
- [ ] Uniqueness also has a unique index (a race becomes a 409).
- [ ] No SQL built from strings; raw `Statement`s bind values. Async rule
  table/column names are constants, never input.
- [ ] Mass assignment: DTOs list only fields clients may set (no `owner_id`,
  `role`, `published` unless intended).

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

## 5. Rate limits and client addresses

- [ ] `http.per_ip_rate_limit` is on (600/min default; never `null` in
  production). Expensive or abusable routes have `#[throttle(limit, per)]`.
- [ ] Behind a proxy, load balancer or BFF: `http.trusted_proxies` lists
  it (else everyone shares one IP and one login throttle). Avoid
  `http.trust_proxy = true` unless nothing reaches the app directly.
- [ ] `auth.login_throttle_per_minute` (5) and
  `auth.two_factor.max_attempts_per_minute` (5) are not 0.
- [ ] Know the limits: counters are per process (multiply by instances).

## 6. Browser-facing headers

- [ ] `http.cors.allowed_origins` lists exact origins only (empty when a
  BFF calls the API server-side).
- [ ] Security headers stay on (`http.security_headers` not `false`, unless
  the proxy sets its own); HTML routes set their own CSP.
- [ ] Served over HTTPS (HSTS is sent).
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

- [ ] `AuditModule` is in the app; writes go through `Repo` so data changes
  are audited (plain `update_many` isn't; record those with
  `uow.record_change`).
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
navree check                # module graph
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
