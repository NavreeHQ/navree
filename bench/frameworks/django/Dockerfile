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
    PYTHONUNBUFFERED=1 \
    DJANGO_SETTINGS_MODULE=benchsite.settings
COPY --from=build /app/.venv /app/.venv
COPY manage.py gunicorn.conf.py ./
COPY benchsite ./benchsite
COPY crm ./crm
RUN python -m compileall -q benchsite crm gunicorn.conf.py
EXPOSE 8080
# Settings in gunicorn.conf.py: gthread, (2 x CPUs) + 1 workers, 3 threads each, no access log.
# A benchmark harness image: built and run only on a benchmarking machine, never deployed.
# nosemgrep: dockerfile.security.missing-user.missing-user
CMD ["gunicorn"]
