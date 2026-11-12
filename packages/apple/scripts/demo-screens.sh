#!/usr/bin/env bash
# Builds the demo app for an iPhone simulator, runs its UI tests once for each
# appearance and text size, and writes what each test saw as PNG files.
#
#   scripts/demo-screens.sh                 # the newest iOS, an iPhone Pro
#   DEVICE="iPhone Air" scripts/demo-screens.sh
#   PASSES="light dark" scripts/demo-screens.sh
#
# Needs Xcode, XcodeGen and jq. Writes to build/ in this package:
# screens/<pass>/*.png, results/<pass>.xcresult, logs/*.log.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
build="$here/build"
screens="${OUT:-$build/screens}"
passes="${PASSES:-light dark light-largest dark-largest rtl}"
mkdir -p "$build/logs" "$build/results" "$screens"

pretty() {
    if command -v xcbeautify >/dev/null 2>&1; then xcbeautify --quiet; else cat; fi
}

# The newest iOS runtime this Xcode has (not one made for a single device,
# such as iPhone Duo's), and an iPhone on it.
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
echo "Runtime $runtime, device $(xcrun simctl list devices -j | jq -r --arg u "$udid" '.devices[][] | select(.udid == $u) | .name') ($udid)"
destination="platform=iOS Simulator,id=$udid"

cd "$here/Demo"
xcodegen generate --quiet

xcodebuild build-for-testing \
    -project NavreeDemo.xcodeproj -scheme NavreeDemo \
    -destination "$destination" -derivedDataPath "$build/DerivedData" \
    CODE_SIGNING_ALLOWED=NO 2>&1 | tee "$build/logs/build-for-testing.log" | pretty

xcrun simctl boot "$udid" 2>/dev/null || true
xcrun simctl bootstatus "$udid" -b >/dev/null
xcrun simctl status_bar "$udid" override --time "09:41" --dataNetwork wifi --wifiMode active \
    --wifiBars 3 --cellularMode active --cellularBars 4 --batteryState charged --batteryLevel 100 || true

status=0
for pass in $passes; do
    appearance=light
    size=large
    rtl=0
    case "$pass" in
        light) ;;
        dark) appearance=dark ;;
        light-largest) size=accessibility-extra-extra-extra-large ;;
        dark-largest) appearance=dark; size=accessibility-extra-extra-extra-large ;;
        rtl) rtl=1 ;;
        *) echo "Unknown pass $pass" >&2; exit 1 ;;
    esac
    xcrun simctl ui "$udid" appearance "$appearance"
    xcrun simctl ui "$udid" content_size "$size"
    rm -rf "$build/results/$pass.xcresult" "$screens/$pass"
    echo "Pass $pass: $appearance, $size"
    if ! TEST_RUNNER_NAVREE_SHOTS="$screens/$pass" TEST_RUNNER_NAVREE_RTL="$rtl" TEST_RUNNER_NAVREE_PASS="$pass" \
        xcodebuild test-without-building \
            -project NavreeDemo.xcodeproj -scheme NavreeDemo \
            -destination "$destination" -derivedDataPath "$build/DerivedData" \
            -resultBundlePath "$build/results/$pass.xcresult" \
            2>&1 | tee "$build/logs/test-$pass.log" | pretty; then
        status=1
    fi
    # The test writes its PNGs itself; when it could not, take them from the result.
    if ! ls "$screens/$pass"/*.png >/dev/null 2>&1 && [ -d "$build/results/$pass.xcresult" ]; then
        mkdir -p "$screens/$pass"
        xcrun xcresulttool export attachments --path "$build/results/$pass.xcresult" \
            --output-path "$screens/$pass" || true
    fi
done

xcrun simctl ui "$udid" appearance light
xcrun simctl ui "$udid" content_size large
exit $status
