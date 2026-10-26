# syntax=docker/dockerfile:1
# Keel benchmark API. Build context: the repository root (frameworks.json),
# so the app can use Keel's crates by path. Dockerfile.dockerignore keeps
# the context to the workspace manifest, crates/ and this directory.

FROM rust:1-bookworm AS build
WORKDIR /src
# The root manifest: Keel's crates inherit their package settings and
# dependency versions from it.
COPY Cargo.toml ./
COPY crates ./crates
COPY bench/frameworks/keel ./bench/frameworks/keel
WORKDIR /src/bench/frameworks/keel
RUN --mount=type=cache,id=keel-bench-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=keel-bench-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=keel-bench-target,target=/src/bench/frameworks/keel/target \
    cargo build --release --locked --jobs 6 \
    && cp target/release/keel-bench /usr/local/bin/keel-bench

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=build /usr/local/bin/keel-bench /usr/local/bin/keel-bench
# config/app.toml is read from the working directory.
COPY bench/frameworks/keel/config ./config
EXPOSE 8080
CMD ["keel-bench", "serve"]
