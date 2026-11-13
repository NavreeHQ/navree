#!/usr/bin/env bash
# Makes NavreeCore.xcframework, the package's binary target: the runtime core
# (crates/navree-native-core) as static libraries for an iPhone, the iOS
# simulator on Apple silicon and a Mac on Apple silicon, with its C header and
# a module map. Never committed.
#
#   scripts/build-core.sh                   # builds the three libraries here
#   scripts/build-core.sh --from <folder>   # takes them from the workflow's
#                                           # navree-native-core artifact
#
# Building needs Rust with the three targets:
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-darwin
# Cargo writes to CARGO_TARGET_DIR (the repository's target/ when unset), with
# JOBS jobs (4 when unset), in the release profile, as the workflow's iOS
# libraries step does.
#
# Writes NavreeCore.xcframework in this package and, under build/core, the
# libraries, the headers and links.txt: what an app links beside each library,
# as the compiler says it. An artifact without a Mac library makes an
# XCFramework without the Mac slice; the package's bridge to the core then
# links only for iOS.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$here/../.." && pwd)"
out="$here/build/core"
from=""
case "${1:-}" in
    --from) from="$(cd "${2:?--from needs the artifact folder}" && pwd)" ;;
    "") ;;
    *) echo "usage: $0 [--from <navree-native-core folder>]" >&2; exit 2 ;;
esac

rm -rf "$out"
mkdir -p "$out/Headers"

# slice name, Rust target, folder in the artifact
slices=(
    "ios aarch64-apple-ios ios/device"
    "ios-simulator aarch64-apple-ios-sim ios/simulator"
    "macos aarch64-apple-darwin macos"
)

if [ -z "$from" ]; then
    export PATH="$HOME/.cargo/bin:/opt/homebrew/opt/rustup/bin:$PATH"
    target_dir="${CARGO_TARGET_DIR:-$root/target}"
    cp "$root/crates/navree-native-core/include/navree_core.h" "$out/Headers/"
    git -C "$root" rev-parse HEAD > "$out/COMMIT"
    for slice in "${slices[@]}"; do
        read -r name target _ <<< "$slice"
        mkdir -p "$out/$name"
        (cd "$root" && CARGO_TARGET_DIR="$target_dir" cargo rustc -p navree-native-core --release \
            --target "$target" --crate-type staticlib -j "${JOBS:-4}" -- --print native-static-libs) \
            2>&1 | tee "$out/$name/build.log"
        # The target folder may be shared with other checkouts: take the
        # library before another build of the same crate replaces it.
        cp "$target_dir/$target/release/libnavree_native_core.a" "$out/$name/"
        sed 's/\x1b\[[0-9;]*m//g' "$out/$name/build.log" | grep -o 'native-static-libs: .*' \
            | sed 's/native-static-libs: //' | tail -1 > "$out/$name/links" || true
    done
else
    (cd "$from" && shasum -a 256 -c SHA256SUMS >/dev/null)
    cp "$from/include/navree_core.h" "$out/Headers/"
    cp "$from/COMMIT" "$out/COMMIT"
    for slice in "${slices[@]}"; do
        read -r name _ folder <<< "$slice"
        [ -f "$from/$folder/libnavree_native_core.a" ] || continue
        mkdir -p "$out/$name"
        cp "$from/$folder/libnavree_native_core.a" "$out/$name/"
        # links.txt in the artifact: one "<name>: <libraries>" line a library.
        key="$(basename "$folder")"
        { grep -h "^$key: " "$from/ios/links.txt" "$from/$folder/links.txt" 2>/dev/null || true; } \
            | head -1 | sed "s/^$key: //" > "$out/$name/links"
    done
fi

printf 'module NavreeCore {\n    header "navree_core.h"\n    export *\n}\n' > "$out/Headers/module.modulemap"

libraries=()
: > "$out/links.txt"
for slice in "${slices[@]}"; do
    read -r name _ _ <<< "$slice"
    library="$out/$name/libnavree_native_core.a"
    [ -f "$library" ] || { echo "No library for $name"; continue; }
    # The five functions of the C interface are in each library. (One part of
    # the archive is the compiler's own notes, which the tool can't read.)
    { nm -g --defined-only "$library" 2>/dev/null || true; } | awk '{print $3}' | { grep -E '^_navree_' || true; } \
        | sort -u > "$out/$name/symbols"
    if [ "$(wc -l < "$out/$name/symbols" | tr -d ' ')" -ne 5 ]; then
        echo "$name: the library does not hold the five functions of the header" >&2
        cat "$out/$name/symbols" >&2
        exit 1
    fi
    # Which of Apple's systems its parts say they were built for.
    built="$({ otool -l "$library" 2>/dev/null || true; } | awk '
        BEGIN { split("macos ios tvos watchos bridgeos maccatalyst ios-simulator", names, " ") }
        $1 == "cmd" { cmd = $2; next }
        cmd ~ /^LC_VERSION_MIN_/ && $1 == "version" { print tolower(substr(cmd, 16)), $2; cmd = "" }
        cmd == "LC_BUILD_VERSION" && $1 == "platform" { platform = ($2 in names) ? names[$2] : $2 }
        cmd == "LC_BUILD_VERSION" && $1 == "minos" { print platform, $2; cmd = "" }' | sort | uniq -c)"
    echo "$name, $(du -h "$library" | cut -f1 | tr -d ' '), parts built for:" $built
    printf '%s: %s\n' "$name" "$(cat "$out/$name/links")" >> "$out/links.txt"
    libraries+=(-library "$library" -headers "$out/Headers")
done

rm -rf "$here/NavreeCore.xcframework"
xcodebuild -create-xcframework "${libraries[@]}" -output "$here/NavreeCore.xcframework" >/dev/null
echo "Links beside each library:"
cat "$out/links.txt"
echo "Built from $(cat "$out/COMMIT")"
du -sh "$here/NavreeCore.xcframework"/*/ | sed "s|$here/||"
