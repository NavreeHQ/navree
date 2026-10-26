# syntax=docker/dockerfile:1
FROM python:3.14.9-slim-bookworm AS build
COPY --from=ghcr.io/astral-sh/uv:0.13.5 /uv /bin/uv
ENV UV_COMPILE_BYTECODE=1 \
    UV_LINK_MODE=copy \
    UV_PYTHON_DOWNLOADS=never \
    UV_PROJECT_ENVIRONMENT=/app/.venv
WORKDIR /app
RUN --mount=type=cache,target=/root/.cache/uv \
    --mount=type=bind,source=uv.lock,target=uv.lock \
    --mount=type=bind,source=pyproject.toml,target=pyproject.toml \
    uv sync --locked --no-dev --no-install-project

FROM python:3.14.9-slim-bookworm
WORKDIR /app
ENV PATH="/app/.venv/bin:$PATH" \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1
COPY --from=build /app/.venv /app/.venv
COPY app ./app
RUN python -m compileall -q app
EXPOSE 8080
# One Uvicorn worker per CPU (nproc honours the container's cpuset: 8 in the
# benchmark). Uvicorn reads WEB_CONCURRENCY as its --workers default, and the
# app sizes its connection pool from the same variable. No access log.
CMD ["sh", "-c", "export WEB_CONCURRENCY=\"${WEB_CONCURRENCY:-$(nproc)}\" && exec uvicorn app.main:app --host 0.0.0.0 --port 8080 --no-access-log"]
