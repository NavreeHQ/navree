#!/usr/bin/env bash
# Makes the Apple client as a Swift package of its own, for the package's
# repository (https://github.com/NavreeHQ/navree-swift), from a built core:
#
#   scripts/swift-package.sh <version> <core-url-base> <out-dir> [--core <NavreeCore.xcframework>]
#
# Writes into <out-dir>:
#
#   NavreeCore.xcframework.zip, .sha256      the core, as the package downloads it
#   navree-swift/                             the package: Package.swift, Sources/,
#                                             README.md and the licences
#   navree-swift-<version>.tar.gz, .sha256   the same, packed for the package
#                                             repository's workflow
#
# The manifest is this package's Package.swift with two changes: the core is
# downloaded from <core-url-base>/NavreeCore.xcframework.zip and checked by its
# SHA-256 (SwiftPM's checksum), and the test targets are left out (the tests
# and their recorded answers stay in this repository). The script stops when
# Package.swift is not in the form it changes, checks that the new manifest has
# the same targets and products, and builds it for the Mac against the zip it
# made. Run scripts/build-core.sh first (the release's apple-package job does).
#
# SWIFT_PACKAGE_SOURCE names another package folder to read Package.swift and
# Sources/ from (this one when unset).
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$here/../.." && pwd)"
source_dir="$(cd "${SWIFT_PACKAGE_SOURCE:-$here}" && pwd)"

usage() {
    echo "usage: $0 <version> <core-url-base> <out-dir> [--core <NavreeCore.xcframework>]" >&2
    exit 2
}
fail() {
    echo "swift-package: $*" >&2
    exit 1
}

[ $# -ge 3 ] || usage
version="$1"
base="${2%/}"
out="$3"
shift 3
core="$source_dir/NavreeCore.xcframework"
while [ $# -gt 0 ]; do
    case "$1" in
        --core) core="${2:?--core needs a folder}"; shift 2 ;;
        *) usage ;;
    esac
done

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || fail "not a version: $version"
# SwiftPM downloads a binary target over https only.
[[ "$base" == https://* ]] || fail "the core's address must start with https://: $base"
[ -d "$core" ] || fail "no core at $core (run scripts/build-core.sh first)"
[ -f "$source_dir/Package.swift" ] || fail "no Package.swift in $source_dir"

mkdir -p "$out"
out="$(cd "$out" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The zip holds the folder at its root under the binary target's name, with
# no resource forks, extended attributes or quarantine marks.
zipped="$core"
if [ "$(basename "$core")" != "NavreeCore.xcframework" ]; then
    zipped="$work/NavreeCore.xcframework"
    cp -R "$core" "$zipped"
fi
rm -f "$out/NavreeCore.xcframework.zip" "$out/NavreeCore.xcframework.zip.sha256"
ditto -c -k --keepParent --norsrc --noextattr --noqtn "$zipped" "$out/NavreeCore.xcframework.zip"
(cd "$out" && shasum -a 256 NavreeCore.xcframework.zip > NavreeCore.xcframework.zip.sha256)
checksum="$(cut -d ' ' -f 1 "$out/NavreeCore.xcframework.zip.sha256")"
url="$base/NavreeCore.xcframework.zip"

# The manifest: the one local binary target becomes the download, and each
# one-line test target goes with the comment lines right above it.
tree="$work/navree-swift"
mkdir -p "$tree"
URL="$url" SUM="$checksum" perl -e '
    my $from = q{.binaryTarget(name: "NavreeCore", path: "NavreeCore.xcframework"),};
    my $to = qq{.binaryTarget(name: "NavreeCore", url: "$ENV{URL}", checksum: "$ENV{SUM}"),};
    my (@kept, @comments);
    my $found = 0;
    while (my $line = <STDIN>) {
        if ($line =~ m{^\s*//}) { push @comments, $line; next; }
        if ($line =~ m{^\s*\.testTarget\(.*\),\s*$}) { @comments = (); next; }
        push @kept, @comments;
        @comments = ();
        $found += ($line =~ s/\Q$from\E/$to/g);
        push @kept, $line;
    }
    push @kept, @comments;
    my $text = join q{}, @kept;
    die "the line $from is there $found times, not once\n" unless $found == 1;
    die "a test target is left that is not on one line\n" if $text =~ /testTarget/;
    print $text;
' < "$source_dir/Package.swift" > "$tree/Package.swift" \
    || fail "Package.swift is not in the form this script changes; nothing was made"

cp -R "$source_dir/Sources" "$tree/Sources"
cp "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$tree/"
cat > "$tree/README.md" <<EOF
# Navree Native for Apple platforms

The Apple client of [Navree](https://github.com/NavreeHQ/navree) $version, as a Swift package: an app for iPhone, iPad and Mac (iOS 26, iPadOS 26 and macOS 26 or later) whose screens a Navree server draws, natively. This repository is made by each release of Navree; change the client in [\`packages/apple\`](https://github.com/NavreeHQ/navree/tree/v$version/packages/apple), where its documentation, tests and demo app are.

## Adding it

A new project: \`navree native:init\` in a Navree app writes one that uses this package.

An existing project: in Xcode, File > Add Package Dependencies, \`https://github.com/NavreeHQ/navree-swift\`, Up to Next Minor Version from $version, and the product \`NavreeNative\`. In a \`Package.swift\`:

\`\`\`swift
.package(url: "https://github.com/NavreeHQ/navree-swift", .upToNextMinor(from: "$version"))
\`\`\`

Then \`import NavreeShell\`, make a \`ShellConfiguration\` with the server's origin, and show \`NavreeScenes(configuration:)\`. Keep the package at the minor version of Navree the server runs: what a native client is answered may change in a minor release.

The runtime core is a prebuilt binary (\`NavreeCore.xcframework\`, downloaded from the Navree release and checked by its SHA-256) for Apple silicon only: an iPhone or iPad, the iOS simulator on an Apple silicon Mac, and an Apple silicon Mac. A Mac app sets \`ARCHS = arm64\`, and a simulator build leaves out \`x86_64\` (\`EXCLUDED_ARCHS[sdk=iphonesimulator*] = x86_64\`).

## Licence

MIT or Apache 2.0, at your option, as Navree.
EOF

# The checks: the new manifest has the same targets (but the tests) and
# products as this one, and the core is the download; then it builds for the
# Mac against the zip just made (the bytes the address will serve).
(cd "$source_dir" && swift package dump-package) > "$work/source.json" 2> "$work/dump.log" \
    || { cat "$work/dump.log" >&2; fail "cannot read $source_dir/Package.swift"; }
(cd "$tree" && swift package dump-package) > "$work/tree.json" 2> "$work/dump.log" \
    || { cat "$work/dump.log" >&2; fail "cannot read the new Package.swift"; }
python3 -I - "$work/source.json" "$work/tree.json" "$url" "$checksum" <<'PY' || fail "the new manifest does not match Package.swift"
import json
import sys

source, made = (json.load(open(path)) for path in sys.argv[1:3])
url, checksum = sys.argv[3:5]


def targets(manifest):
    kept = {}
    for target in manifest["targets"]:
        if target["type"] == "test":
            continue
        kept[target["name"]] = {
            key: target.get(key)
            for key in ("type", "dependencies", "settings", "exclude", "resources")
        }
    return kept


def products(manifest):
    return sorted(json.dumps(product, sort_keys=True) for product in manifest["products"])


problems = []
if targets(source) != targets(made):
    problems.append("the targets differ")
if products(source) != products(made):
    problems.append("the products differ")
if any(target["type"] == "test" for target in made["targets"]):
    problems.append("a test target is left")
core = next((t for t in made["targets"] if t["name"] == "NavreeCore"), {})
if core.get("url") != url or core.get("checksum") != checksum or core.get("path"):
    problems.append("NavreeCore is not the download at %s with checksum %s" % (url, checksum))
for problem in problems:
    print("swift-package: " + problem, file=sys.stderr)
sys.exit(1 if problems else 0)
PY

cp -R "$tree" "$work/build"
cp "$out/NavreeCore.xcframework.zip" "$work/build/"
perl -pi -e 's{url: "[^"]*", checksum: "[^"]*"}{path: "NavreeCore.xcframework.zip"}' "$work/build/Package.swift"
(cd "$work/build" && swift build) || fail "the package does not build against the zip"

# The package, and its archive for the package repository: what SwiftPM left
# while reading the manifest goes, and nothing else may be there.
rm -rf "$tree/.build" "$tree/.swiftpm" "$tree/Package.resolved"
entries="$(cd "$tree" && ls -A | tr '\n' ' ')"
[ "$entries" = "LICENSE-APACHE LICENSE-MIT Package.swift README.md Sources " ] \
    || fail "the package holds more than it should: $entries"
rm -rf "$out/navree-swift" "$out/navree-swift-$version.tar.gz" "$out/navree-swift-$version.tar.gz.sha256"
mv "$tree" "$out/navree-swift"
COPYFILE_DISABLE=1 tar -czf "$out/navree-swift-$version.tar.gz" -C "$out" navree-swift
(cd "$out" && shasum -a 256 "navree-swift-$version.tar.gz" > "navree-swift-$version.tar.gz.sha256")
echo "made $out/navree-swift ($version), its archive, and the core's zip (checksum $checksum)"
