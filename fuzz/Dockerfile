# The fuzzing toolchain: a pinned nightly (libFuzzer needs `-Zsanitizer`) and
# a pinned cargo-fuzz on a pinned stable base image. Build it once:
#
#   docker build -t navree-fuzz fuzz
#
# See CONTRIBUTING.md ("Fuzzing") for how to run the targets.
FROM rust:1.99-bookworm

ARG NIGHTLY=nightly-2026-10-20
ARG CARGO_FUZZ=0.13.2

RUN rustup toolchain install ${NIGHTLY} --profile minimal --component llvm-tools \
 && rustup default ${NIGHTLY} \
 && cargo install cargo-fuzz --version ${CARGO_FUZZ} --locked

# The repository is mounted at /work; build artifacts live in a named volume
# (or the container), never in the checkout.
WORKDIR /work/fuzz
ENV CARGO_TARGET_DIR=/fuzz-target \
    CARGO_INCREMENTAL=0
