#!/usr/bin/env bash
# Takes one picture of the demo app in a simulator, in one of the modes a
# review against Apple's guidelines looks at, and puts every setting back.
#
#   scripts/evidence.sh <device> <mode> <picture.png> [launch arguments...]
#
#   device   a simulator's UDID; booted here if it is not
#   mode     light, dark, largest, ax1 to ax5, rtl, contrast or
#            transparency, or several joined by "+" (dark+largest)
#
#   scripts/evidence.sh $UDID dark out/orders-dark.png -NavreeSelect /orders
#   ORIGIN=http://127.0.0.1:8096 scripts/evidence.sh $UDID rtl out/inbox-rtl.png
#   scripts/evidence.sh $UDID largest out/today.png -NavreeAnswers ~/answers
#
# The modes:
#   light, dark    the system's appearance (simctl ui appearance)
#   largest        the largest accessibility text size (simctl ui content_size)
#   ax1 ... ax5    the five accessibility text sizes, ax5 the largest
#   rtl            right to left: Urdu as written in India, the app's
#                  layout forced right to left (it has no Urdu words of its own)
#   contrast       Increase Contrast (simctl ui increase_contrast)
#   transparency   Reduce Transparency, written to the simulator's
#                  accessibility settings (simctl ui has no such option)
#
# Environment:
#   ORIGIN    the server the app talks to (NAVREE_ORIGIN in the app's environment)
#   APP       the demo's .app to install first (the default build's when it
#             exists and the app is not installed yet)
#   WAIT      seconds after launch before looking for a still screen (10)
#   SETTLE    seconds to wait at most for the screen to stop changing (60)
#   STILL     how many screenshots in a row, two seconds apart, must match
#             the one before (2)
#
# The picture is taken once STILL screenshots in a row are the same as the
# one before, or after SETTLE seconds. A screen with nothing under its status
# bar is never taken for a still one: it is the launch screen, which stays
# up while the app starts (20 s and more for a debug build on a busy machine,
# the more so on the first launch after an install). The status bar shows
# 9:41, full battery and Wi-Fi.
set -euo pipefail

if [ $# -lt 3 ]; then
    sed -n '2,39p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
fi
udid="$1"
mode="$2"
out="$3"
shift 3

here="$(cd "$(dirname "$0")/.." && pwd)"
bundle="com.navree.demo"
app="${APP:-$here/build/DerivedData/Build/Products/Debug-iphonesimulator/NavreeDemo.app}"
wait_first="${WAIT:-10}"
settle="${SETTLE:-60}"
needed="${STILL:-2}"

state="$(xcrun simctl list devices -j | jq -r --arg u "$udid" '.devices[][] | select(.udid == $u) | .state')"
if [ -z "$state" ]; then
    echo "No simulator $udid" >&2
    exit 1
fi
if [ "$state" != "Booted" ]; then
    xcrun simctl boot "$udid"
fi
xcrun simctl bootstatus "$udid" -b >/dev/null
xcrun simctl status_bar "$udid" override --time "9:41" --dataNetwork wifi --wifiMode active --wifiBars 3 \
    --cellularMode active --cellularBars 4 --batteryState charged --batteryLevel 100 >/dev/null 2>&1 || true

if [ -n "${APP:-}" ] || ! xcrun simctl get_app_container "$udid" "$bundle" >/dev/null 2>&1; then
    xcrun simctl install "$udid" "$app"
fi

# Reduce Transparency, as the Settings app keeps it, and the notice running
# apps listen for.
transparency() {
    xcrun simctl spawn "$udid" defaults write com.apple.Accessibility EnhancedBackgroundContrastEnabled -bool "$1"
    xcrun simctl spawn "$udid" notifyutil -p com.apple.accessibility.cache.enhance.background.contrast >/dev/null 2>&1 || true
}

reset() {
    xcrun simctl terminate "$udid" "$bundle" >/dev/null 2>&1 || true
    xcrun simctl ui "$udid" appearance light >/dev/null 2>&1 || true
    xcrun simctl ui "$udid" content_size large >/dev/null 2>&1 || true
    xcrun simctl ui "$udid" increase_contrast disabled >/dev/null 2>&1 || true
    transparency NO >/dev/null 2>&1 || true
}
trap reset EXIT

xcrun simctl terminate "$udid" "$bundle" >/dev/null 2>&1 || true
appearance=light
size=large
contrast=disabled
reduce=NO
arguments=()
IFS='+' read -r -a modes <<< "$mode"
for one in "${modes[@]}"; do
    case "$one" in
        light) appearance=light ;;
        dark) appearance=dark ;;
        largest|ax5) size=accessibility-extra-extra-extra-large ;;
        ax1) size=accessibility-medium ;;
        ax2) size=accessibility-large ;;
        ax3) size=accessibility-extra-large ;;
        ax4) size=accessibility-extra-extra-large ;;
        contrast) contrast=enabled ;;
        transparency) reduce=YES ;;
        rtl) arguments+=(-AppleLanguages "(ur)" -AppleLocale ur_IN -NSForceRightToLeftWritingDirection YES) ;;
        *) echo "Unknown mode $one" >&2; exit 2 ;;
    esac
done
xcrun simctl ui "$udid" appearance "$appearance"
xcrun simctl ui "$udid" content_size "$size"
xcrun simctl ui "$udid" increase_contrast "$contrast"
transparency "$reduce"

if [ -n "${ORIGIN:-}" ]; then
    export SIMCTL_CHILD_NAVREE_ORIGIN="$ORIGIN"
fi
xcrun simctl launch "$udid" "$bundle" ${arguments[@]+"${arguments[@]}"} "$@" >/dev/null

# Whether a screenshot holds nothing but its status bar: one colour from a
# twelfth of the way down to the foot, as the launch screen is.
blank() {
    sips -Z 64 -s format bmp "$1" --out "$scratch/small.bmp" >/dev/null 2>&1 || return 1
    python3 -I - "$scratch/small.bmp" <<'CHECK'
import struct, sys
data = open(sys.argv[1], "rb").read()
start = struct.unpack_from("<I", data, 10)[0]
width, height = struct.unpack_from("<ii", data, 18)
size = struct.unpack_from("<H", data, 28)[0] // 8
rows = abs(height)
stride = (width * size + 3) // 4 * 4
def pixel(x, y):
    at = start + (y if height < 0 else rows - 1 - y) * stride + x * size
    return data[at:at + 3]
ground = pixel(width // 2, rows // 2)
for y in range(rows // 12, rows):
    for x in range(width):
        if max(abs(a - b) for a, b in zip(pixel(x, y), ground)) > 8:
            sys.exit(1)
sys.exit(0)
CHECK
}

mkdir -p "$(dirname "$out")"
scratch="$(mktemp -d)"
sleep "$wait_first"
previous="$scratch/previous.png"
current="$scratch/current.png"
xcrun simctl io "$udid" screenshot --type=png "$previous" >/dev/null 2>&1
end=$((SECONDS + settle))
still=0
while [ $SECONDS -lt $end ]; do
    sleep 2
    xcrun simctl io "$udid" screenshot --type=png "$current" >/dev/null 2>&1
    if blank "$current"; then
        still=0
    elif cmp -s "$previous" "$current"; then
        still=$((still + 1))
        [ $still -ge "$needed" ] && break
    else
        still=0
    fi
    mv "$current" "$previous"
done
if [ $still -ge "$needed" ]; then
    mv "$current" "$out"
else
    mv "$previous" "$out"
    if blank "$out"; then
        echo "The app had drawn nothing after ${settle} s; kept the launch screen" >&2
    else
        echo "The screen was still changing after ${settle} s; kept the last picture" >&2
    fi
fi
rm -rf "$scratch"
echo "$out ($mode)"
