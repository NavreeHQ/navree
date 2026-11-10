# syntax=docker/dockerfile:1
# The system app's image: one binary, three process types (the compose
# file runs `serve`, `queue:work` and `schedule:work` from it).
#
# Build context: the repository root (the app is a workspace member and
# uses Navree's crates by path). Dockerfile.dockerignore keeps the context
# to the workspace manifests, crates/ and apps/system-app.
#
#   docker build -f e2e/docker/Dockerfile -t navree-system-app:local .
#
# Layers: cargo-chef cooks the external dependencies from a recipe of the
# workspace's manifests, so a change to Navree's or the app's code rebuilds
# only the workspace crates (cook runs without --locked: cargo-chef masks
# the workspace crates' versions in its skeleton, which the lock file does
# not list; the real build below is --locked). A BuildKit cache mount
# keeps the cargo registry between builds.

ARG RUST_IMAGE=lukemathwalker/cargo-chef:0.1.78-rust-1.99.1-bookworm

FROM ${RUST_IMAGE} AS chef
WORKDIR /src
ENV CARGO_TERM_COLOR=never \
    CARGO_INCREMENTAL=0

# The recipe: every manifest and the lock file, nothing else.
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json --bin system-app

# Dependencies first (cached until a manifest or the lock file changes),
# then the workspace.
FROM chef AS builder
ARG CARGO_BUILD_JOBS=16
ENV CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS}
COPY --from=planner /src/recipe.json recipe.json
RUN --mount=type=cache,id=navree-e2e-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=navree-e2e-cargo-git,target=/usr/local/cargo/git \
    cargo chef cook --release --recipe-path recipe.json --bin system-app
COPY . .
RUN --mount=type=cache,id=navree-e2e-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=navree-e2e-cargo-git,target=/usr/local/cargo/git \
    cargo build --release --locked --bin system-app \
 && cp target/release/system-app /usr/local/bin/system-app \
 && strip /usr/local/bin/system-app

# The runtime: Debian slim with CA certificates (rustls trusts the
# system's roots for outgoing HTTPS), curl for the health check and tini
# as PID 1 (signals reach the app, zombies are reaped). Non-root.
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl tini \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd --system --gid 10001 navree \
 && useradd --system --uid 10001 --gid navree --home-dir /app --shell /usr/sbin/nologin navree
WORKDIR /app
COPY --from=builder /usr/local/bin/system-app /usr/local/bin/system-app
# config/app.toml is read from the working directory.
COPY apps/system-app/config ./config
RUN mkdir -p storage/app && chown -R navree:navree /app
USER navree:navree
ENV APP_ENV=production
EXPOSE 8080
# The web process answers /health/live; worker and scheduler disable the
# check in compose.
HEALTHCHECK --interval=5s --timeout=3s --start-period=20s --retries=5 \
    CMD curl -fsS http://127.0.0.1:8080/health/live || exit 1
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/system-app"]
CMD ["serve"]
