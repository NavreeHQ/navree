# FastAPI + SQLAlchemy (Python)

| | |
|---|---|
| Runtime | CPython 3.14.9 (`python:3.14.9-slim-bookworm`) |
| Framework | FastAPI 0.162.0 (`fastapi[standard]`) on Starlette 1.10.0 |
| Server | Uvicorn 0.54.0 with uvloop 0.23.0 and httptools 0.9.0 (`uvicorn[standard]`), one worker process per CPU (`--workers` from `WEB_CONCURRENCY` = `nproc`, 8 under the benchmark's cpuset), `--no-access-log` |
| Database | SQLAlchemy 2.1.6 async ORM (declarative models) on asyncpg 0.32.0, `pool_size=8, max_overflow=0` per worker (64 in total) |
| JWT | PyJWT 2.15.1 (HS256, `issuer="keel"`, `exp`/`iss`/`sub` required) in a `Depends(get_current_user)` dependency behind `HTTPBearer`; it loads the user with `session.get(User, sub)` and raises `HTTPException(401)` |
| Validation | Pydantic 2.14.1: a body model for create, `Query(ge=, le=)` for `page`/`per_page` (FastAPI's 422); responses go through `response_model`s |
| Lockfile | `uv.lock`, installed with `uv sync --locked` (uv 0.13.5) |

Layout: `app/main.py` (routes), `models.py` (ORM), `schemas.py` (Pydantic), `auth.py`, `database.py` (engine and the per-request `AsyncSession` dependency), `config.py` (pydantic-settings, from the environment).

Notes:
- **Python 3.14, not 3.15.1.** FastAPI 0.162, Uvicorn 0.54 and asyncpg 0.32 declare support up to 3.14, and PyYAML 6.0.3 (a `uvicorn[standard]` dependency) has no cp315 wheel.
- **`uvicorn` rather than `fastapi run`.** FastAPI's Docker guide uses `fastapi run --workers N`, and its "Server Workers" page gives `uvicorn --workers N` as the equivalent. `fastapi run` installs its own logging config with the access log on and has no option to turn it off, so the image runs the `uvicorn` CLI with `--no-access-log`. The server and worker model are the same.
- **One `AsyncSession` per request**, from a `yield` dependency. FastAPI caches dependencies per request, so `get_current_user` and the endpoint share the session. SQLAlchemy wraps each session's work in a transaction (`BEGIN` … `ROLLBACK`/`COMMIT`), so `/db` is three round trips. That is SQLAlchemy's default and is kept.
- **`/queries`** runs its N queries one after another in that session; an `AsyncSession` can't run queries concurrently. It uses `select(World).where(World.id == id)` rather than `session.get()`, because `get()` would skip the query for an id already in the session's identity map.
- **Deals list:** `select(func.count()).select_from(Deal)`, then `select(Deal).options(joinedload(Deal.owner, innerjoin=True))` ordered by `created_at DESC, id DESC` with `LIMIT`/`OFFSET`, which is one JOIN query. `Deal.owner` is `lazy="raise"`, so an accidental per-row load would raise instead of running N+1 queries.
- **Create:** `Deal(**body, owner=current_user, created_at=now, updated_at=now)`, `session.add`, `commit`. The id is a client-side `uuid4`, so the INSERT needs no `RETURNING`. With `expire_on_commit=False` the response is built without reloading the row.
- **Built-in OpenTelemetry.** FastAPI 0.162 has built-in OpenTelemetry instrumentation. It is left at its defaults and stays inactive unless OTel providers or `OTEL_*` / `FASTAPI_OTEL_AUTO_CONFIGURE` variables are set; while inactive it costs one check per request. `fastapi[standard]` also installs fastapi-cloud-cli, sentry-sdk and the OTel SDK, none of which run here.
- The pool size is `64 // WEB_CONCURRENCY`, so 8 per worker under the benchmark's 8 CPUs.
