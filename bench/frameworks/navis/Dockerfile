# syntax=docker/dockerfile:1
# Navis benchmark API. Build context: the repository root (frameworks.json),
# so the app can use Navis's crates by path. Dockerfile.dockerignore keeps
# the context to the workspace manifest, crates/ and this directory.

FROM rust:1-bookworm AS build
WORKDIR /src
# The root manifest: Navis's crates inherit their package settings and
# dependency versions from it.
COPY Cargo.toml ./
COPY crates ./crates
COPY bench/frameworks/navis ./bench/frameworks/navis
WORKDIR /src/bench/frameworks/navis
RUN --mount=type=cache,id=navis-bench-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=navis-bench-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=navis-bench-target,target=/src/bench/frameworks/navis/target \
    cargo build --release --locked --jobs 6 \
    && cp target/release/navis-bench /usr/local/bin/navis-bench

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=build /usr/local/bin/navis-bench /usr/local/bin/navis-bench
# config/app.toml is read from the working directory.
COPY bench/frameworks/navis/config ./config
EXPOSE 8080
CMD ["navis-bench", "serve"]
