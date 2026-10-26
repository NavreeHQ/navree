# Issues found while writing the documentation (2026-10-26)

Seven writers documented every module from its code and tests, and ran most examples (about 150 tests in `target/docs-check/*`). Along the way they found the bugs and rough edges below. None are fixed yet. The docs describe the current behaviour honestly, so fixing an item means updating its page too.

Severity: **H** = wrong or surprising behaviour users will hit; **M** = misleading behaviour or a gap with a workaround; **L** = cosmetic.

## Configuration and boot
| # | Sev | Issue | Where |
|---|---|---|---|
| 1 | H | **`.env` can't override `config/app.toml`.** `ConfigSources::defaults()` reads `.env` first, so a TOML value wins for any key both set (`APP_HTTP__ADDR` in `.env` is ignored). Laravel, and the template comments in `env.example.j2`/`app.toml.j2`, promise the opposite. Several lines in `apps/crm-api/.env.example` therefore do nothing. | navree-config |
| 2 | H | **Every app command boots the whole app**, connecting to the database and, with `migrate_on_boot`, migrating. Consequences:<br>- `key:generate` needs a working database;<br>- a database whose migrations table doesn't match can't be repaired, because even `migrate:fresh` fails at boot;<br>- `migrate_on_boot` re-applies migrations right after `migrate:rollback`. | navree-kernel, navree-db |
| 3 | M | Boot and command errors print as `Debug` (`Error: BootErrors([...])`, `UnknownCommand { name }`), because `main` returns `BoxError`. The readable `Display` messages exist. | navree-kernel `AppBuilder::run`, CLI templates |
| 4 | M | No default `dyn Clock` is registered, so a service that injects `Arc<dyn Clock>` fails to boot unless the app registers one (the CRM does). | navree-kernel / navree-contracts |
| 5 | M | The "private provider" error advises adding an export. Following it leads to a "does not import" error. | navree-kernel `graph.rs` `missing()` |
| 6 | M | With two `dyn Authenticator` providers (for example AuthModule plus a session authenticator), boot fails with a misleading "no `dyn Authenticator` registered". Session sign-in can't sit next to AuthModule. | navree-http `router.rs` |

## HTTP
| # | Sev | Issue | Where |
|---|---|---|---|
| 7 | M | A `#[controller]` method can't return plain `impl IntoResponse` (Rust 2024 captures `&self`, E0716); `Response` or `impl IntoResponse + use<>` works. | navree-macros |
| 8 | M | `http.per_ip_rate_limit` can't be turned off from config: `0` fails validation and TOML has no null. | navree-http config |
| 9 | M | **Maintenance mode:**<br>- every refused request is logged as an ERROR (503s go through the error layer's 5xx logging);<br>- the realtime relay endpoint also answers 503, so workers' broadcasts fail while the app is down;<br>- queue workers and the scheduler ignore maintenance mode. | navree-http `router.rs`, navree-realtime |
| 10 | L | Timeouts are logged inconsistently: the app-wide timeout is WARN on `navree::request`, a route's `#[timeout]` is ERROR on `navree::error`. | navree-http |
| 11 | L | Two different 413 bodies, depending on whether `Content-Length` was sent. | navree-http |
| 12 | L | There's no per-route body-limit attribute in `#[controller]`, and no per-route Tower layer without a hand-written controller. | navree-macros |

## Auth, access and the CRM
| # | Sev | Issue | Where |
|---|---|---|---|
| 13 | M | The CRM's password-reset mail links to crm-web's `/reset-password`, but crm-web has no such page (404). | apps/crm-web |
| 14 | M | crm-web's BFF only sends `X-Forwarded-For` when `NAVREE_TRUSTED_PROXY_HOPS` is above 0 (the default is 0), so by default all browsers share the BFF's per-IP limit, contrary to the comment in `apps/crm-api/config/app.toml`. | apps/crm-web BFF |
| 15 | M | `POST /auth/email/verification` answers 202 "A verification link has been sent." even when no `Mailer` is registered. | navree-auth `account.rs` |
| 16 | M | The login throttle's window comes from the app clock but its counters expire on the cache's own timer, so moving a `FixedClock` doesn't end a lockout in tests. | navree-auth `throttle.rs` |
| 17 | M | No command assigns a role to a user; only `access:role`, `access:grant` and `access:explain` exist. | navree-access |
| 18 | L | The OpenAPI document lists 409 for `PATCH /auth/me`, but a taken email returns 400. | navree-auth |

## Queues, mail, scheduling, events, cache, real time
| # | Sev | Issue | Where |
|---|---|---|---|
| 19 | H | **`QueueFake` disagrees with the real queue:**<br>- it records a plain `dispatch` from a unit of work that later rolls back (the real queue drops it);<br>- it accepts jobs nobody registered (the real queue fails with `UnknownJob`, a 500).<br>Tests can pass while production fails. | navree-queue `queue.rs` |
| 20 | H | **`Mailer::queue` without a queue sends immediately**, even inside a unit of work that may roll back; the open-issues note says "after the commit". Queued mail also stores full rendered bodies, reset links included, in the `jobs` table. | navree-mail `mailer.rs` |
| 21 | M | Scheduler: a crash during a run leaves a `running` row, so `without_overlapping` skips that task for up to 24 h. On scheduled jobs, overlap prevention covers only the dispatch, not the job. | navree-schedule |
| 22 | M | **Queued listeners:**<br>- they can't set `JobOptions`;<br>- their job name includes the full Rust type path, so moving or renaming the type orphans queued jobs.<br>With the memory driver, a separate `queue:work` never sees jobs, and nothing warns about it. | navree-events, navree-queue |
| 23 | M | **The memory cache is per process:**<br>- `cache:clear` from the command line can't clear a running server;<br>- locks only work within one process;<br>- the login throttle only counts within one process. | navree-cache |
| 24 | M | **Realtime broadcasts can silently go nowhere:**<br>- the sync `Realtime::broadcast` reaches nobody in worker processes (only the `Broadcaster` trait relays);<br>- an empty `relay_url` drops worker broadcasts silently;<br>- seeding with `relay_url = "auto"` and no `serve` logs a warning per user, and each relay attempt takes about 2 s. | navree-realtime |
| 25 | L | `TaggedCache` has no `pull`; `run_pending` ignores non-default queues; the HTTP client ignores `Retry-After`; macro routes without `#[name]` get no OpenAPI `operationId`. | various |

## Docs, templates and messages
| # | Sev | Issue | Where |
|---|---|---|---|
| 26 | M | **Doc comments name things that don't exist:**<br>- `#[event]`, `#[listener(E)]` (navree-events `event.rs`);<br>- `#[config(section = "...")]` (navree-config `module.rs`).<br>Also, the header of `navree-session/tests/session/spa.rs` claims navree-auth reads the session. | navree-events, navree-config, navree-session |
| 27 | M | The facade doesn't re-export `tracing` and the `navree new` template doesn't add it, so new apps can't log until they add it. | navree, navree-cli templates |
| 28 | L | `navree help` shows the CLI's clap help, but the generated README says it lists the app's commands (use `navree list`). | navree-cli templates |
| 29 | L | Grammar in messages: "1 seconds", "at least 1 elements", "longer than or equal to 1 characters". | navree-auth `error.rs`, navree-validation |
| 30 | L | The generator report prints `updated …/mod.rs` twice when it edits a file twice. | navree-cli |

## Found by the final docs review
| # | Sev | Issue | Where |
|---|---|---|---|
| 31 | H | `navree new` writes a path dependency into Cargo's git checkout after `cargo install --git`, or `navree = "0.1.0"`, which isn't published. It never writes a git dependency, so a new app's dependency is broken until the crates are published. | navree-cli `new_app.rs` |
| 32 | M | `migrate` silently ignores its arguments, yet the CLI's own docs use `navree migrate --seed`. | navree-cli `forward.rs` |
| 33 | M | The realtime relay key ignores `APP_KEY` unless `CryptoModule` is registered, unlike storage and signed URLs. | navree-realtime `module.rs` |
| 34 | M | A queued listener is constructed in the process that emits the event, not only on the worker. | navree-events `registry.rs` |
| 35 | L | tower-http can answer 406 after a handler has already committed its unit of work. | navree-http |

## Already known (`framework-open-issues.md`), re-confirmed
- 2FA `enable` and `recovery-codes` need only an access token, not the password.
- Refresh tokens are never pruned.
- The record loader and `#[can(.., from = "id")]` ignore soft deletes.
- `AsyncRule::exists` can't add `deleted_at IS NULL`.
- `PageQuery` caps `per_page` at 100 silently.
- sqlx's `test_before_acquire` can't be turned off.
- `Page<T>` has no `ToSchema`.
- The OpenAPI document omits query parameters and user-resource extension fields.
