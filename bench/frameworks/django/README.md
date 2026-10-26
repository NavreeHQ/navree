# Django + Django REST Framework (Python)

| | |
|---|---|
| Runtime | CPython 3.14.9 (`python:3.14.9-slim-bookworm`) |
| Framework | Django 6.1.2, Django REST Framework 3.18.3 |
| Server | Gunicorn 26.2.0, `gthread` worker class, (2 × CPUs) + 1 = 17 workers × 3 threads (from `os.process_cpu_count()`, 8 under the benchmark's cpuset), no access log; see `gunicorn.conf.py` |
| Database | Django ORM on psycopg 3.3.6 (`psycopg[binary]`), persistent connections (`CONN_MAX_AGE=None`), one per Gunicorn thread: at most 51 |
| JWT | djangorestframework-simplejwt 5.5.1 (`JWTAuthentication`, configured through settings only) on PyJWT 2.15.1 |
| Validation | DRF serializers: a `ModelSerializer` for the create body, a plain serializer for `page`/`per_page` (400) |
| Lockfile | `uv.lock`, installed with `uv sync --locked` (uv 0.13.5) |

Layout: `benchsite/` (settings, URLs, WSGI), `crm/` (models, serializers, pagination, views), `gunicorn.conf.py`.

Notes:
- **Python 3.14, not 3.15.1.** Django 6.1 supports Python 3.12 to 3.14. simplejwt 5.5.1 (July 2025) and Gunicorn 26.2.0 still list 3.13 as their newest classifier, but both are pure Python and run unchanged on 3.14.
- **Worker class and count.** Gunicorn's docs say the default `sync` worker has no keep-alive and "requires a buffering proxy (nginx, HAProxy) for production". Their decision guide picks `gthread` when keep-alive is needed. The benchmark's clients connect directly with keep-alive and there is no proxy, so `gthread` is used. The worker count is Gunicorn's `(2 × cores) + 1`. Django holds one connection per thread, so threads are `64 // workers` = 3, which keeps the total at 51 and within the 64-connection limit. `worker_tmp_dir=/dev/shm` follows the Gunicorn FAQ: the heartbeat file goes on a tmpfs, not the container's overlay filesystem. Gunicorn's optional C parser (`gunicorn[fast]`) is not used; the install docs don't mention it.
- **Auth: custom `AUTH_USER_MODEL`, no custom authentication class.** `crm.User` is an `AbstractBaseUser` on the existing `users` table (`managed = False`):
  - `password` is mapped to the `password_hash` column (`db_column`);
  - `last_login = None` removes that field, because the table has no such column;
  - `is_active` stays `AbstractBaseUser`'s constant `True`.

  simplejwt's stock `JWTAuthentication` then validates the token from settings alone: `ALGORITHM="HS256"`, `SIGNING_KEY=JWT_SECRET`, `ISSUER="keel"`, `TOKEN_TYPE_CLAIM="typ"` (must be `"access"`), `JTI_CLAIM="jti"`, `USER_ID_CLAIM="sub"`, `USER_ID_FIELD="id"`. It also loads the user with `User.objects.get(id=sub)`. Keel's argon2 hashes are not in Django's hasher format, so `check_password` would not work for a login view; the benchmark has none.
- **Pagination.** `crm.pagination.PagePagination` (a `BasePagination`) replaces `PageNumberPagination`, which returns 404 past the last page and silently clamps `page_size`. It validates `page`/`per_page` with a serializer (400 with DRF's error body), runs `count()`, slices the queryset and renders `{data, meta}`. A page past the end is an empty 200.
- **Views.** The deals endpoints are one `ListCreateAPIView` with `select_related("owner")`: one JOIN query for the page, plus `COUNT(*)` and the user lookup. `perform_create` saves with `owner=request.user`. `/json`, `/db` and `/queries` are DRF `APIView`s with no authentication or permission classes, so every response goes through DRF's request and renderer pipeline. Only `JSONRenderer` and `JSONParser` are enabled (no browsable API). The default permission is `IsAuthenticated`.
- **Middleware.** It is `startproject`'s list minus sessions, auth and messages, since those apps aren't installed in a stateless JWT API: Security, Common, CSRF (DRF views are CSRF-exempt) and X-Frame-Options. `manage.py check --deploy` only warns about HTTPS settings (HSTS, SSL redirect).
- **Models.** All models are `managed = False` with `db_table`, and no migrations exist or run. `amount_cents` carries `MinValueValidator(0)`/`MaxValueValidator(10**12)`, which the `ModelSerializer` picks up. Like any DRF integer field it rejects `10.5` but accepts numeric strings such as `"100"`. `created_at`/`updated_at` use `auto_now_add`/`auto_now`.
- **psycopg.** The image uses the `psycopg[binary]` wheel. psycopg's docs prefer a local `psycopg[c]` build for production, but the binary wheel has the same C speedups and is what most container deployments install. Connections use Django's defaults: client-side parameter binding and no health checks.
