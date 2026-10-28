# syntax=docker/dockerfile:1
# Navree benchmark API. Build context: the repository root (frameworks.json),
# so the app can use Navree's crates by path. Dockerfile.dockerignore keeps
# the context to the workspace manifest, crates/ and this directory.

FROM rust:1-bookworm AS build
WORKDIR /src
# The root manifest: Navree's crates inherit their package settings and
# dependency versions from it.
COPY Cargo.toml ./
COPY crates ./crates
COPY bench/frameworks/navree ./bench/frameworks/navree
WORKDIR /src/bench/frameworks/navree
RUN --mount=type=cache,id=navree-bench-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=navree-bench-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=navree-bench-target,target=/src/bench/frameworks/navree/target \
    cargo build --release --locked --jobs 6 \
    && cp target/release/navree-bench /usr/local/bin/navree-bench

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=build /usr/local/bin/navree-bench /usr/local/bin/navree-bench
# config/app.toml is read from the working directory.
COPY bench/frameworks/navree/config ./config
EXPOSE 8080
# A benchmark harness image: built and run only on a benchmarking machine, never deployed.
# nosemgrep: dockerfile.security.missing-user.missing-user
CMD ["navree-bench", "serve"]
