# syntax=docker/dockerfile:1
FROM rust:1-bookworm AS build
WORKDIR /usr/src/app
COPY Cargo.toml Cargo.lock ./
COPY migration ./migration
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/usr/src/app/target,id=bench-loco-target \
    cargo build --release --locked --jobs 6 \
    && cp target/release/bench-loco /usr/local/bin/bench-loco

# The layout `cargo loco generate deployment docker` produces: the binary and
# `config/` side by side in /usr/app, started from there.
FROM debian:bookworm-slim
WORKDIR /usr/app
COPY config ./config
COPY --from=build /usr/local/bin/bench-loco ./bench-loco
ENV LOCO_ENV=production
EXPOSE 8080
# A benchmark harness image: built and run only on a benchmarking machine, never deployed.
# nosemgrep: dockerfile.security.missing-user.missing-user
CMD ["/usr/app/bench-loco"]
