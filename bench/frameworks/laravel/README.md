# Laravel (Octane on FrankenPHP, and PHP-FPM + nginx)

One Laravel app, deployed two ways. Both entries in `frameworks.json` build from this directory:

| | `laravel-octane` (`Dockerfile.octane`) | `laravel-fpm` (`Dockerfile.fpm`) |
|---|---|---|
| Runtime | PHP 8.5.12 ZTS (`dunglas/frankenphp:1.13.3-php8.5.12-trixie`) | PHP 8.5.12 (`php:8.5.12-fpm-trixie`) + nginx 1.26.3 (Debian trixie) |
| Server | Laravel Octane 2.22 on FrankenPHP 1.13.3 (Caddy 2.11.7), worker mode | PHP-FPM behind nginx over a Unix socket, started by `docker/fpm-start.sh` |
| Workers | `--workers=$(nproc)`: one per CPU, 8 under the benchmark's cpuset; Octane's default `--max-requests=500` | `pm = static`, `pm.max_children = 64` (8 per CPU); nginx: one worker per CPU |
| Database | Eloquent over `pdo_pgsql`; one connection per worker, kept across requests (Octane default): 8 | Eloquent over `pdo_pgsql`; one connection per request (Laravel default): at most 64 |

| | |
|---|---|
| Framework | Laravel 13.38 (skeleton 13.11, trimmed to an API: no frontend, migrations or tests) |
| ORM | Eloquent: `World`, `User` (Keel's `users` table, password in `password_hash`), `Deal` with `belongsTo` owner |
| JWT | `php-open-source-saver/jwt-auth` 2.9.3 (lcobucci/jwt 5.6.1), the `jwt` guard as `auth:api`, user loaded by `sub` through the Eloquent provider |
| Validation | Form requests (`ListDealsRequest`, `StoreDealRequest`), Laravel's 422 |
| Serialization | API Resources (`DealResource`, `OwnerResource`) |
| Production settings | `APP_ENV=production`, `APP_DEBUG=false`, `LOG_CHANNEL=stderr`, `LOG_LEVEL=warning`; `php.ini-production` plus OPcache with `validate_timestamps=0` (`docker/php.ini`); `php artisan optimize` (config, events, routes, views) at container start; `APP_KEY` generated at build time |

## Notes and judgement calls

- **Routes.** Everything is in `routes/api.php` (the stateless `api` middleware group, so no sessions or cookies), with `apiPrefix: ''` because `/json`, `/db` and `/queries` sit next to `/api/deals`. The deals routes are `Route::apiResource('deals', ...)->only(['index', 'store'])` under `prefix('api')->middleware('auth:api')`.
- **Issuer check.** jwt-auth validates `exp`, `iat` and `nbf`, but has no issuer option. `App\Auth\Claims\Issuer` replaces its `iss` claim class through the claim factory's own `extend()` hook (registered in `AppServiceProvider`) and rejects any issuer other than `config('jwt.issuer')` (`JWT_ISSUER`, default `keel`). Because `iss` is in `required_claims`, a token without `iss` is rejected too.
- **Other jwt-auth settings** (`config/jwt.php`): HS256 with `JWT_SECRET`; `required_claims` without `nbf` (Keel's tokens have none); `lock_subject = false` (no `prv` claim). The blacklist is off (`JWT_BLACKLIST_ENABLED=false`): the API has no logout, and with the blacklist on, every request looks up the token's `jti` in the cache store.
- **401 as JSON.** The conformance client sends no `Accept: application/json`. The skeleton's `shouldRenderJsonWhen(api/* or expectsJson)` makes errors render as JSON. Laravel 13's default `redirectGuestsTo(fn () => route('login'))` is still evaluated by the auth middleware for such requests, and with no `login` route that gives a 500. This is an API-only app, so `bootstrap/app.php` sets `redirectGuestsTo(null)`.
- **List.** `Deal::with('owner')->orderByDesc('created_at')->orderByDesc('id')->paginate(...)` then `DealResource::collection(...)`: a count query, the page query, and one `whereIn` query for the owners. The response also carries Laravel's `links` and `meta.from/to/path/links` keys next to the four the spec asks for.
- **Create.** `$user->deals()->create($validated)` with the owner relation set from the authenticated user. `JsonResource::withoutWrapping()` makes the body the bare deal (paginated collections keep `data`). The resource response gives the 201 itself, because the model `wasRecentlyCreated`. New ids come from `HasUuids`, which makes UUIDv7 (Laravel's default since 12). `stage` is a backed enum (`App\Enums\DealStage`, `Rule::enum`). `amount_cents` uses `integer:strict`, so `"5"` and `true` are rejected.
- **Timestamps.** `created_at` serializes as `2026-01-01T16:40:00.000000Z` (Carbon's JSON format). Eloquent writes timestamps with second precision.
- **Config cache at start, not at build.** `config:cache` bakes in `env()` values, and the database and JWT settings only arrive at run time. It takes about 20 ms; the app is ready in under 0.5 s.
- **Postgres.** `DATABASE_URL` is read through the pgsql connection's `url` (`DB_URL` falls back to it). `DB_SSLMODE=disable`: the benchmark's PostgreSQL has no TLS, and Laravel's default `prefer` costs an extra round trip per connection, which matters under FPM.
- **FPM connects per request.** That's Laravel's default, kept on purpose: each request pays a PostgreSQL backend fork and SCRAM authentication, which dominates FPM's database tests. Persistent PDO connections or PgBouncer are the usual mitigations and aren't used.
- **FPM pool size.** A child serves one request at a time and holds at most one connection, so 64 children is both the concurrency and the connection cap. 8 per CPU suits requests that mostly wait on the database.
- **No access logs.** nginx has `access_log off`. The official FPM image's `docker.conf` enables an FPM access log, which the Dockerfile comments out. Under Octane, Caddy logs at WARN outside `local`, and Octane prints request lines only in `local`.
- **nginx `worker_processes`** is set from `nproc` in `fpm-start.sh`, because `auto` counts the host's 24 CPUs, not the cpuset.
- **Shutdown.** The php image's `STOPSIGNAL` is SIGQUIT, so `fpm-start.sh` traps it and stops FPM and nginx gracefully. Octane handles SIGTERM itself.
- **Octane `--max-requests=500`** (the default) is kept: a worker re-boots the app every 500 requests. Octane's Caddyfile enables response compression, but wrk sends no `Accept-Encoding`.
- **Cache, session, queue** are set to `file`/`array`/`sync`, since the skeleton defaults are `database` and the benchmark schema has no tables for them. No request reads or writes any of them.
- **Builds.** `composer.lock` was resolved in `composer:2.10`, which runs the same PHP 8.5.12. Both Dockerfiles install `vendor/` in a `composer:2.10` stage with a BuildKit cache mount, then copy it into the runtime image (`--classmap-authoritative`, no dev packages).
