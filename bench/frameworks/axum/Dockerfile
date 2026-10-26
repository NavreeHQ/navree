# syntax=docker/dockerfile:1
FROM rust:1-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target,id=bench-axum-target \
    cargo build --release --locked --jobs 6 \
    && cp target/release/bench-axum /usr/local/bin/bench-axum

FROM debian:bookworm-slim
COPY --from=build /usr/local/bin/bench-axum /usr/local/bin/bench-axum
EXPOSE 8080
CMD ["bench-axum"]
