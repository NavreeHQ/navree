#!/usr/bin/env bash
# Builds the core as a static library for this machine, compiles tests/c/smoke.c
# against include/navree_core.h, and runs it: the header and the library agree,
# and a C program can drive the core.
#
# Run from the repository's root: bash crates/navree-native-core/tests/c/run.sh
set -euo pipefail
crate="$(cd "$(dirname "$0")/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$crate/../../target}"
cargo rustc -p navree-native-core --release --crate-type staticlib
out="$(mktemp -d)"
cc -std=c11 -Wall -Wextra -Werror -I "$crate/include" "$crate/tests/c/smoke.c" \
  "$target/release/libnavree_native_core.a" -lpthread -ldl -lm -o "$out/smoke"
"$out/smoke"
rm -rf "$out"
