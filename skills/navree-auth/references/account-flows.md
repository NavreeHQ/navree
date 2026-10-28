# Account flows and two-factor authentication

All routes are under `{http.api_prefix}{auth.routes_prefix}` (`/api/auth`).
Each one calls an `AuthService` method you can call yourself (`meta` is the
`navree::auth::ClientMeta` extractor, or `ClientMeta::default()` outside a
request). Every step is written to the audit log when the app has
`AuditModule`; navree-auth publishes no events on the event bus.

## Before you start

- Mail: reset and verification mails go through `Mailer` (needs
  `MailModule` and `ViewModule`), after the request commits. With
  `QueueModule` they are queued (recommended in production, so
  `forgot-password` answers as fast for unknown addresses).
- Links: set `auth.password_reset.url` (your front end's reset page) and
  `app.url` (or `auth.verification.base_url`).

## Registration

`POST /register` `{name, email, password}` → 201 with the user and a token
pair. Name 1-255 chars (trimmed), email unique regardless of case, password
at least `auth.password_min_length` (8) and at most 1024 characters. A taken
address is a 400 `email has already been taken`. No verification mail is
sent automatically. Turn public sign-up off with `auth.registration = false`
(the route then answers 404) and create accounts with
`auth.register(&uow, NewUser { name, email, password })`.

## Password reset

1. `POST /forgot-password` `{email}` → always 202
   `{"message": "If the email address is registered, a password reset link has been sent."}`.
   A 64-character token (stored as a SHA-256 digest) is mailed; one mail per
   address per `auth.password_reset.throttle_secs` (60).
2. The mail links to `auth.password_reset.url?token=..&email=..` (or contains
   the token when no URL is set).
3. `POST /reset-password` `{email, token, password}` → 204; signs out every
   session (refresh tokens revoked). A bad or expired token (1 hour,
   `auth.password_reset.ttl_secs`) is a 400
   `Invalid or expired password reset token`.

Methods: `send_password_reset_link(email, &meta)`,
`reset_password(email, token, password, &meta)`.

## Email verification

- `POST /email/verification` (signed in) → 202 and a mail with a signed
  link, or 204 when already verified.
- `GET /email/verify/{id}/{hash}?expires=..&signature=..` (public; the
  signature is the credential) → 200; sets `users.email_verified_at`.
  A bad or expired signature is a 401.
- Navree records verification but doesn't enforce it: check
  `user.email_verified_at` where it matters.

Methods: `send_verification_email(&user)` (returns `false` when already
verified), `verification_url(&user)`, `verify_email(user_id, hash, &meta)`.

## Profile and password

- `PATCH /me` `{name?, email?, current_password?}`: changing the email needs
  `current_password` (wrong ones count towards the login throttle), makes
  the new address unverified, notifies the old one and cancels pending
  resets. Method: `update_profile(user_id, ProfileUpdate { .. }, &meta)`.
- `PATCH /me/password` `{current_password, password}` → a new token pair for
  this device; every other session ends. Store the new tokens. Method:
  `change_password(user_id, current, new, meta)`.

## Two-factor (TOTP)

1. `POST /2fa/enable` (signed in) → `{otpauth_url, qr_svg, secret}`. Show the
   QR code (`qr_svg` is a standalone SVG, black on white, at least 200 px wide
   with a `viewBox`: put it in the page as it is); nothing is active yet. 409
   when already on.
2. `POST /2fa/confirm` `{code}` → `{recovery_codes: [8 codes]}`; two-factor is
   now on. The codes are shown only here.
3. Login with a correct password now answers
   `{challenge_token, expires_in: 300, two_factor_required: true}`.
4. `POST /2fa/verify` `{challenge_token, code}` (or `recovery_code` instead of
   `code`, exactly one) → the user and a token pair. Wrong codes: 401
   `Invalid two-factor code`; more than
   `auth.two_factor.max_attempts_per_minute` (5) per user: 429.
5. `POST /2fa/recovery-codes` replaces the codes; `POST /2fa/disable`
   `{password}` turns it off (204).

Codes are RFC 6238 (SHA-1, 30-second steps, one step of drift), each
accepted once. Secrets and recovery codes are encrypted with the app key:
when rotating `APP_KEY`, keep the old one in `app.previous_keys`.
`navree::auth::two_factor_enabled(&user)` tells whether it is on. In code:
`enable_two_factor(user_id, &meta)`, `confirm_two_factor(user_id, code, &meta)`,
`verify_two_factor(challenge, TwoFactorProof::Code(..), &meta)`,
`disable_two_factor(user_id, password, &meta)`,
`regenerate_recovery_codes(user_id, &meta)`, and `attempt_login(email,
password, &meta)` returning `LoginOutcome::Authenticated { user, tokens }` or
`LoginOutcome::TwoFactorRequired { user, challenge_token }`.

## Configuration

```toml
[auth.password_reset]
ttl_secs = 3600
throttle_secs = 60
# url = "https://app.example.com/reset-password"
# mail_template = "mail/reset-password"     # a view template pair (.html/.txt)

[auth.verification]
ttl_secs = 3600
# base_url = "https://app.example.com"      # default: app.url
# mail_template = "mail/verify-email"

[auth.two_factor]
issuer = "Navree"
challenge_ttl_secs = 300
recovery_codes = 8
max_attempts_per_minute = 5
```

Mail templates get `name`, `email`, `token`, `url`, `expires_in_minutes`
(reset) and `name`, `email`, `url`, `expires_in_minutes` (verification).

## Testing

Use `mail.transport = "array"` and read
`app.get::<Mailer>().await.array_transport().unwrap().sent()` (with a queue
in the app, run `Queue::run_pending` first). For two-factor, compute codes
from the `secret` with `navree::crypto::totp`
(`Totp::builder(Secret::from_base32(&secret)?).build()?.generate(now)`) and
move a `FixedClock` to the next 30-second step between codes.

Codes and QR codes for your own screens: `navree::crypto::totp` (`Secret::
generate`, `Totp::builder(secret).issuer(..).account(..).build()`, `generate`,
`check(code, time) -> Option<step>`, `to_url`, `from_url`; HMAC-SHA-1, 6
digits, 30 s, skew 1 by default; check is constant time and replay protection
is yours: refuse a step not newer than the last accepted) and
`navree::crypto::qr` (`QrCode::encode(bytes, EcLevel::M)?.to_svg(200)`;
levels L/M/Q/H, versions 1 to 40 chosen automatically, numeric, alphanumeric
and byte segments, `QrError::TooLong` past version 40).
