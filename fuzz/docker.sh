#!/usr/bin/env sh
# Runs `cargo fuzz` in the pinned toolchain container (see fuzz/Dockerfile).
#
#   fuzz/docker.sh build                              # every target
#   fuzz/docker.sh run jose_token -- -max_total_time=300
#   fuzz/docker.sh list
#
# The repository is mounted at /work. Build output, the cargo registry and the
# working corpora live in named volumes, never in the checkout; crashes are
# written to fuzz/artifacts/<target>/. The corpus of a target is
# /corpus/<target> (seeded from fuzz/seeds/<target>/ on every run).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
repo=$(dirname "$here")
# Git Bash on Windows: hand Docker a Windows path.
if (cd "$repo" && pwd -W) >/dev/null 2>&1; then
  repo=$(cd "$repo" && pwd -W)
fi

docker build -q -t navree-fuzz "$here" >/dev/null

MSYS_NO_PATHCONV=1 exec docker run --rm \
  -v "$repo:/work" \
  -v navree-fuzz-target:/fuzz-target \
  -v navree-fuzz-registry:/usr/local/cargo/registry \
  -v navree-fuzz-corpus:/corpus \
  -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-6}" \
  navree-fuzz sh -c 'for arg; do case "$arg" in /corpus/*) mkdir -p "$arg";; esac; done; exec cargo fuzz "$@"' sh "$@"
