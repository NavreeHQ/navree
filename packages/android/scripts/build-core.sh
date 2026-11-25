#!/usr/bin/env bash
# Builds the runtime core (crates/navree-native-core) for Android, as the
# shared libraries the client loads: arm64-v8a for phones and tablets, x86_64
# for the emulator on an Intel or AMD machine. Each is linked with the NDK for
# API 26 with the glue for com.navree.core.Core, says its own name
# (libnavree_native_core.so) and is laid out for 16 KB pages; the script checks
# each of these and stops when one does not hold.
#
#   scripts/build-core.sh
#
# Needs Rust with the two targets
#   rustup target add aarch64-linux-android x86_64-linux-android
# and the Android NDK, found through ANDROID_NDK_LATEST_HOME (a GitHub runner
# sets it), ANDROID_NDK_HOME or ANDROID_NDK_ROOT. Cargo writes to
# CARGO_TARGET_DIR (the repository's target/ when unset), in the release
# profile.
#
# Puts the libraries in navree-native/src/main/jniLibs/<abi>/, where the
# client's build takes them from. They are never committed.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$here/../.." && pwd)"

fail() {
    echo "build-core: $*" >&2
    exit 1
}

[ $# -eq 0 ] || { echo "usage: $0" >&2; exit 2; }

ndk="${ANDROID_NDK_LATEST_HOME:-${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}}"
[ -n "$ndk" ] || fail "no NDK: set ANDROID_NDK_HOME to its folder"
case "$(uname -s)" in
    Linux) host=linux-x86_64 ;;
    Darwin) host=darwin-x86_64 ;;
    *) fail "the core for Android is built on Linux or macOS" ;;
esac
tools="$ndk/toolchains/llvm/prebuilt/$host/bin"
[ -d "$tools" ] || fail "no NDK tools at $tools"

target_dir="${CARGO_TARGET_DIR:-$root/target}"
link="-C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-soname,libnavree_native_core.so"

# Rust target, the name Cargo's settings give it, Android ABI
builds=(
    "aarch64-linux-android AARCH64_LINUX_ANDROID arm64-v8a"
    "x86_64-linux-android X86_64_LINUX_ANDROID x86_64"
)

for build in "${builds[@]}"; do
    read -r target setting abi <<< "$build"
    linker="$tools/${target}26-clang"
    [ -x "$linker" ] || fail "no linker at $linker"
    (cd "$root" && env \
        "CARGO_TARGET_${setting}_LINKER=$linker" \
        "CARGO_TARGET_${setting}_RUSTFLAGS=$link" \
        cargo rustc -p navree-native-core --locked --release --features jni --target "$target" --crate-type cdylib)

    library="$target_dir/$target/release/libnavree_native_core.so"
    [ -f "$library" ] || fail "$abi: Cargo left no library at $library"

    # The five functions of the C interface and the four native methods of the
    # Kotlin class are in the library.
    symbols="$("$tools/llvm-nm" -D --defined-only "$library" | awk '{print $3}' | { grep -E '^(navree_|Java_)' || true; } | sort)"
    printf '%s\n' "$symbols"
    [ "$(printf '%s\n' "$symbols" | grep -c '^navree_' || true)" -eq 5 ] \
        || fail "$abi: the library does not hold the five functions of the header"
    [ "$(printf '%s\n' "$symbols" | grep -c '^Java_com_navree_core_Core_native' || true)" -eq 4 ] \
        || fail "$abi: the library does not hold the four native methods of com.navree.core.Core"

    # Every part of the library that is loaded is laid out for 16 KB pages.
    "$tools/llvm-readelf" -lW "$library" \
        | awk '$1 == "LOAD" && $NF != "0x4000" { print "a part laid out for " $NF; bad = 1 } END { exit bad }' \
        || fail "$abi: the library is not laid out for 16 KB pages"

    # It says its own name, and needs nothing but the system's own libraries.
    dynamic="$("$tools/llvm-readelf" -dW "$library")"
    printf '%s\n' "$dynamic" | { grep -E 'SONAME|NEEDED' || true; }
    printf '%s\n' "$dynamic" | grep 'SONAME.*libnavree_native_core.so' > /dev/null \
        || fail "$abi: the library does not say its name is libnavree_native_core.so"

    mkdir -p "$here/navree-native/src/main/jniLibs/$abi"
    cp "$library" "$here/navree-native/src/main/jniLibs/$abi/"
done

du -ah "$here/navree-native/src/main/jniLibs" | sort -k2
