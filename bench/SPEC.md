# Navree benchmark: the contract every implementation follows

Navree is compared with other web frameworks by having each implement the same
small API, then measuring them one at a time under identical conditions. This
file is the contract. `tools/conformance.mjs` checks it, and `run.mjs` runs
the measurements.

## 1. Test environment

Everything runs in Linux containers on one Docker network (`bench`):

| Container | CPUs (cpuset) | Memory | Notes |
|---|---|---|---|
| `bench-pg` (PostgreSQL 17) | 10 | 4 GB | data on tmpfs, `synchronous_commit=off` |
| the app under test | 8 | 4 GB | one framework at a time |
| `bench-wrk` (load generator) | 6 | none | wrk, 6 threads |

Only one app runs at a time. Its database is recreated from the seeded
template before each app starts, so every app sees identical data.

## 2. Database

`db/init.sql` creates and seeds the template database (deterministic data):

- `world (id integer pk, randomnumber integer)`: 10,000 rows, `randomnumber = ((id * 7919) % 10000) + 1`.
- `users`: Navree's users table (`id uuid pk, name, email unique, password_hash, email_verified_at, two_factor_secret, two_factor_recovery_codes, two_factor_confirmed_at, created_at, updated_at`). 100 rows: user `i` has `id = md5('user-' || i)::uuid`, `name = 'User ' || i`, `email = 'user' || i || '@bench.test'`.
- `deals (id uuid pk, title varchar(200), amount_cents bigint, stage varchar(20), owner_id uuid → users, created_at timestamptz, updated_at timestamptz)`: 1,000 rows. Deal `i` has `id = md5('deal-' || i)::uuid`, `title = 'Deal ' || i`, `amount_cents = (i * 104729) % 100000000`, `stage = ['lead','qualified','proposal','negotiation','won','lost'][i % 6]` (0-based), `owner = user ((i % 100) + 1)`, `created_at = 2026-01-01T00:00:00Z + i minutes`. Indexes: `(created_at DESC, id DESC)` and `(owner_id)`.

Apps must not create or migrate tables; the schema already exists.

## 3. Endpoints

All responses are JSON (`Content-Type: application/json`). Port 8080 on 0.0.0.0.

### 3.1 `GET /json`: framework overhead
`200 {"message":"Hello, World!"}`, built per request (no cached bytes).

### 3.2 `GET /db`: one query
Read one random `world` row (id uniformly in 1..10000) with one query:
`200 {"id": 3217, "randomNumber": 4512}`. Note the camelCase key.

### 3.3 `GET /queries?queries=N`: N queries
N separate single-row queries (no `IN (...)` batching); N is clamped to
1..500, and a missing or non-numeric value means 1. Returns an array of `/db` objects.

### 3.4 `GET /api/deals?page=P&per_page=S`: the realistic endpoint
What a typical authenticated CRUD list endpoint does:

1. **Authenticate.** `Authorization: Bearer <JWT>`, HS256, secret `JWT_SECRET`. Check the signature, `exp` and `iss == "navree"`. Then load the user row by `sub` from `users`. A missing, malformed, expired, wrong-issuer or wrong-signature token, or an unknown user, gives `401`.
2. **Validate.** `page` is an integer ≥ 1 (default 1) and `per_page` an integer 1..100 (default 20). Anything else gives `400` (or `422` where that is the framework's idiomatic validation status), with a JSON body.
3. **Query.** Count all deals, then load page P, ordered by `created_at DESC, id DESC`, with each deal's owner. Use a join or one eager-load query for the owners, never one query per deal.
4. **Respond** `200`:

```json
{
  "data": [
    {
      "id": "…uuid…",
      "title": "Deal 1000",
      "amount_cents": 4729000,
      "stage": "won",
      "owner": { "id": "…uuid…", "name": "User 1" },
      "created_at": "2026-01-01T16:40:00Z"
    }
  ],
  "meta": { "current_page": 1, "per_page": 20, "total": 1000, "last_page": 50 }
}
```

`amount_cents` is a JSON number. `created_at` is any ISO-8601 timestamp. `last_page = max(1, ceil(total / per_page))`.

### 3.5 `POST /api/deals`: validated insert
Authentication as in 3.4. Body:
`{"title": string 1..200 chars, "amount_cents": integer 0..1000000000000, "stage": one of lead|qualified|proposal|negotiation|won|lost}`.
Invalid input gives `400`/`422` with a JSON body. Otherwise insert the deal, with a new UUID, `owner_id` = the authenticated user and `created_at`/`updated_at` = now. Return `201` with the deal in the 3.4 item shape, whose owner is the current user.

## 4. Fairness rules

- **Production mode.** Release builds and production settings (`NODE_ENV=production`, `APP_DEBUG=false`, Django `DEBUG=False`, `RAILS_ENV=production`, and so on). No per-request access logging, no debug toolbars.
- **Use all 8 CPUs** the way the framework recommends in production: one multi-threaded process (Rust, Go, Java, .NET) or one worker per CPU (Node cluster, Gunicorn/Uvicorn workers, Puma workers, Octane workers, FPM children).
- **At most 64 database connections in total** across all processes (for example 8 workers × 8, or one pool of 64).
- **Idiomatic stacks.** Full-stack frameworks use their own ORM, validation, auth/JWT integration and serializers, as a real app on that framework would. Micro-frameworks use their most common companions (named in each app's README). No hand-written SQL in full-stack apps unless the ORM's documented query builder is the idiomatic path.
- **No result caching** of any kind: every request reads the database.
- **Latest stable versions** of each framework and runtime, pinned in lockfiles. Debian/glibc-based images (not musl). No custom allocators unless the framework ships them by default.
- **Fast startup.** Ready within 60 s of the container starting. No migrations or seeding at boot.

## 5. Container contract

- `frameworks/<name>/Dockerfile`; the build context is `frameworks/<name>` unless `frameworks.json` says otherwise (Navree builds from the repo root).
- These environment variables are supplied at run time:
  - `DATABASE_URL=postgres://bench:bench@bench-pg:5432/bench`
  - `DB_HOST=bench-pg`
  - `DB_PORT=5432`
  - `DB_NAME=bench`
  - `DB_USER=bench`
  - `DB_PASSWORD=bench`
  - `JWT_SECRET`
  - `JWT_ISSUER=navree`
- `frameworks/<name>/README.md` lists the versions, server and worker setup, DB library and pool size, JWT and validation libraries, and anything notable.

## 6. Measurements (`run.mjs`)

For each app, one at a time:

1. Start time (container start to the first `200` from `/json`) and idle memory.
2. Conformance: `tools/conformance.mjs`. An app that fails is not benchmarked.
3. For each test (json, db, queries (20), deals list, deals create):
   - a 10 s warmup, then 3 × 15 s runs of `wrk -t6 -c256`;
   - report the median run: requests/s, latency p50/p90/p99, errors;
   - record peak memory and CPU while the test runs.

The deals-list run cycles through pages 1..50. The deals-create run sends a new valid deal per request.
