#!/usr/bin/env bash
# Runs the package's tests in an iPhone simulator (the newest iOS this Xcode
# has, an iPhone Pro on it), with the core's simulator library linked. This is
# how the tests run where NavreeCore.xcframework has no Mac library.
#
#   scripts/test-in-simulator.sh                              # every test target
#   ONLY=NavreeCoreBridgeTests scripts/test-in-simulator.sh   # one of them
#
# Needs Xcode, jq and NavreeCore.xcframework (scripts/build-core.sh). Writes to
# build/ in this package: logs/test-simulator.log, results/package.xcresult.
# A simulator this script boots is shut down again when it ends, except on a
# cloud runner (CI set), where the demo's screens follow on the same one.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
build="$here/build"
mkdir -p "$build/logs" "$build/results"

pretty() {
    if command -v xcbeautify >/dev/null 2>&1; then xcbeautify --quiet; else cat; fi
}

runtime="$(xcrun simctl list runtimes -j | jq -r '
    [.runtimes[] | select(.isAvailable and (.identifier | test("SimRuntime\\.iOS-[0-9-]+$")))]
    | sort_by(.version | split(".") | map(tonumber)) | last | .identifier')"
udid="$(xcrun simctl list devices available -j | jq -r --arg rt "$runtime" --arg name "${DEVICE:-}" '
    (.devices[$rt] // []) | map(select(.name | startswith("iPhone"))) as $phones
    | ($phones | map(select(.name == $name))) + ($phones | map(select(.name | test("^iPhone [0-9]+ Pro$")))) + $phones
    | first | .udid // empty')"
if [ -z "$udid" ]; then
    echo "No iPhone simulator on $runtime" >&2
    exit 1
fi
state="$(xcrun simctl list devices -j | jq -r --arg u "$udid" '.devices[][] | select(.udid == $u) | .state')"
echo "Runtime $runtime, device $(xcrun simctl list devices -j | jq -r --arg u "$udid" '.devices[][] | select(.udid == $u) | .name') ($udid), $state"
if [ "$state" != "Booted" ] && [ -z "${CI:-}" ]; then
    trap 'xcrun simctl shutdown "$udid" 2>/dev/null || true' EXIT
fi

cd "$here"
# Xcode names the scheme that holds every test after the package, with
# "-Package" once the package has more than one product.
scheme="$(xcodebuild -list -json 2>/dev/null | jq -r '
    .workspace.schemes as $s | if ($s | index("NavreeNative-Package")) then "NavreeNative-Package" else "NavreeNative" end')"
rm -rf "$build/results/package.xcresult"
xcodebuild test \
    -scheme "$scheme" \
    -destination "platform=iOS Simulator,id=$udid" \
    -derivedDataPath "$build/PackageData" \
    -resultBundlePath "$build/results/package.xcresult" \
    ${ONLY:+-only-testing:"$ONLY"} \
    CODE_SIGNING_ALLOWED=NO 2>&1 | tee "$build/logs/test-simulator.log" | pretty
