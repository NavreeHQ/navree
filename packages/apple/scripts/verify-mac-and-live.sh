#!/usr/bin/env bash
# Builds and runs the Mac apps (Charkha and Charkha Bar), draws the protocol's
# recorded and health screens through NavreeShots, starts the live store, inbox
# and board servers on SQLite, walks the Mac apps against all three, and runs
# the live simulator walk and appearance checks on iPhone and iPad.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$here/../.." && pwd)"
build="$here/build"
target_dir="${CARGO_TARGET_DIR:-$root/target}"

rm -rf \
    "$build/shots" \
    "$build/mac-evidence" \
    "$build/live-sim-evidence" \
    "$build/live-walk"
mkdir -p \
    "$build/logs" \
    "$build/shots" \
    "$build/mac-evidence" \
    "$build/live-sim-evidence" \
    "$build/live-walk" \
    "$build/live-servers" \
    "$build/results"

run_with_timeout() {
    BUILD_LOGS="$build/logs" python3 -I - "$@" <<'PY'
import glob
import json
import os
import shutil
import signal
import subprocess
import sys
import time

seconds = int(sys.argv[1])
cmd = sys.argv[2:]
started = time.time()
proc = subprocess.Popen(cmd, start_new_session=True)
try:
    rc = proc.wait(timeout=seconds)
except subprocess.TimeoutExpired:
    sys.stderr.write(f"Command timed out after {seconds}s: {' '.join(cmd)}\n")
    sys.stderr.flush()
    try:
        os.killpg(proc.pid, signal.SIGTERM)
        proc.wait(timeout=5)
    except Exception:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except Exception:
            pass
    sys.exit(124)

if rc != 0:
    if rc < 0:
        sig = -rc
        try:
            sig_name = signal.Signals(sig).name
        except Exception:
            sig_name = str(sig)
        sys.stderr.write(f"Command terminated by signal {sig} ({sig_name}): {' '.join(cmd)}\n")
        time.sleep(2)
    else:
        sys.stderr.write(f"Command exited with status {rc}: {' '.join(cmd)}\n")
    diag_dir = os.path.expanduser("~/Library/Logs/DiagnosticReports")
    out_dir = os.path.join(os.environ.get("BUILD_LOGS", "."), "diagnostic-reports")
    if os.path.isdir(diag_dir):
        recent = [
            p for p in glob.glob(os.path.join(diag_dir, "*"))
            if os.path.isfile(p) and os.path.getmtime(p) >= started - 2
        ]
        if recent:
            os.makedirs(out_dir, exist_ok=True)
            for report in sorted(recent, key=os.path.getmtime):
                shutil.copy2(report, os.path.join(out_dir, os.path.basename(report)))
                sys.stderr.write(f"--- Diagnostic report: {os.path.basename(report)} ---\n")
                try:
                    with open(report, "r", encoding="utf-8", errors="replace") as fh:
                        first = fh.readline()
                        body = fh.read()
                    if report.endswith(".ips") and body.strip().startswith("{"):
                        data = json.loads(body)
                        exc = data.get("exception", {})
                        sys.stderr.write(f"Exception: {exc}\n")
                        faulting = data.get("faultingThread", 0)
                        threads = data.get("threads", [])
                        images = data.get("usedImages", [])
                        if 0 <= faulting < len(threads):
                            sys.stderr.write(f"Faulting thread {faulting}:\n")
                            for frame in threads[faulting].get("frames", [])[:25]:
                                idx = frame.get("imageIndex", -1)
                                img = images[idx].get("name", "?") if 0 <= idx < len(images) else "?"
                                sym = frame.get("symbol", hex(frame.get("imageOffset", 0)))
                                sys.stderr.write(f"  {img}: {sym}\n")
                    else:
                        sys.stderr.write((first + body)[:4000] + "\n")
                except Exception as exc:
                    sys.stderr.write(f"Could not parse report: {exc}\n")
    sys.stderr.flush()
    sys.exit(128 + (-rc) if rc < 0 else rc)
sys.exit(0)
PY
}

echo "=== 1. Checking generated native models ==="
(cd "$root" && cargo run --locked -p xtask -- native-models --check)

echo "=== 2. Building macOS applications (NavreeDemoMac & NavreeDemoMenuBar) ==="
(
    cd "$here/Demo"
    xcodegen generate --quiet
    xcodebuild build \
        -project NavreeDemo.xcodeproj \
        -scheme NavreeDemoMac \
        -destination "platform=macOS,arch=arm64" \
        -derivedDataPath "$build/DerivedDataMac" \
        >"$build/logs/build-macos-app.log" 2>&1 || {
        tail -80 "$build/logs/build-macos-app.log" >&2
        exit 1
    }
    tail -20 "$build/logs/build-macos-app.log"

    xcodebuild build \
        -project NavreeDemo.xcodeproj \
        -scheme NavreeDemoMenuBar \
        -destination "platform=macOS,arch=arm64" \
        -derivedDataPath "$build/DerivedDataMac" \
        >"$build/logs/build-macos-menubar.log" 2>&1 || {
        tail -80 "$build/logs/build-macos-menubar.log" >&2
        exit 1
    }
    tail -20 "$build/logs/build-macos-menubar.log"
)

charkha="$build/DerivedDataMac/Build/Products/Debug/Charkha.app/Contents/MacOS/Charkha"
charkha_bar="$build/DerivedDataMac/Build/Products/Debug/CharkhaBar.app/Contents/MacOS/CharkhaBar"
test -x "$charkha"
test -x "$charkha_bar"

echo "=== 3. Drawing recorded store, health, and protocol screens with NavreeShots ==="
SHOT_WIDTHS=402,834,1024,1366 run_with_timeout 900 bash "$here/scripts/shots.sh" "$build/shots/store" \
    "$here/Demo/NavreeDemo/Recorded/index.json" \
    "$here/Demo/NavreeDemo/Recorded/orders.json" \
    "$here/Demo/NavreeDemo/Recorded/orders_1.json" \
    "$here/Demo/NavreeDemo/Recorded/inventory.json"
SHOT_WIDTHS=402,834,1024,1366 run_with_timeout 900 bash "$here/scripts/shots.sh" "$build/shots/corpus" \
    "$root"/crates/navree-native/corpus/screen-*.json

store_shots="$(find "$build/shots/store" -name '*.png' -size +0c | wc -l | tr -d ' ')"
corpus_shots="$(find "$build/shots/corpus" -name '*.png' -size +0c | wc -l | tr -d ' ')"
echo "NavreeShots wrote $store_shots store PNGs and $corpus_shots corpus PNGs"
if [ "$store_shots" -lt 32 ] || [ "$corpus_shots" -lt 112 ]; then
    echo "Expected at least 32 store PNGs and 112 corpus PNGs" >&2
    exit 1
fi

echo "=== 4. Building and starting live store-demo, inbox-demo, and board-demo ==="
(
    cd "$root"
    cargo build --locked -p store-demo -p inbox-demo -p board-demo \
        --features store-demo/native-preview,inbox-demo/native-preview,board-demo/native-preview
)

server_pids=()
cleanup() {
    if [ "${#server_pids[@]}" -gt 0 ]; then
        kill "${server_pids[@]}" 2>/dev/null || true
        wait "${server_pids[@]}" 2>/dev/null || true
    fi
}
trap cleanup EXIT

for port in 8095 8096 8097; do
    if curl -fs -o /dev/null "http://127.0.0.1:$port/"; then
        echo "Port $port is already in use before starting test servers" >&2
        exit 1
    fi
done

rm -f "$build/live-servers"/*.db*
PORT=8095 STORE_SIMULATOR=off STORE_DATABASE_URL="sqlite://$build/live-servers/store.db?mode=rwc" \
    "$target_dir/debug/store-demo" >"$build/logs/store-server.log" 2>&1 &
store_pid=$!
server_pids+=("$store_pid")

PORT=8096 INBOX_SIMULATOR=off INBOX_TICK=off INBOX_SCALE=2000 INBOX_DATABASE_URL="sqlite://$build/live-servers/inbox.db?mode=rwc" \
    "$target_dir/debug/inbox-demo" >"$build/logs/inbox-server.log" 2>&1 &
inbox_pid=$!
server_pids+=("$inbox_pid")

PORT=8097 BOARD_SIMULATOR=off BOARD_DATABASE_URL="sqlite://$build/live-servers/board.db?mode=rwc" \
    "$target_dir/debug/board-demo" >"$build/logs/board-server.log" 2>&1 &
board_pid=$!
server_pids+=("$board_pid")

for pair in "8095:$store_pid:store" "8096:$inbox_pid:inbox" "8097:$board_pid:board"; do
    IFS=':' read -r port pid label <<< "$pair"
    up=0
    for _ in $(seq 1 120); do
        if ! kill -0 "$pid" 2>/dev/null; then
            echo "$label-demo (PID $pid) exited during startup" >&2
            tail -40 "$build/logs/$label-server.log" >&2 || true
            exit 1
        fi
        if curl -fs -o /dev/null "http://127.0.0.1:$port/"; then
            up=1
            break
        fi
        sleep 1
    done
    if [ "$up" -ne 1 ]; then
        echo "Server on port $port did not answer in 120s" >&2
        tail -40 "$build/logs/$label-server.log" >&2 || true
        exit 1
    fi
    echo "Live server http://127.0.0.1:$port/ ($label-demo) is answering"
done

echo "=== 5. Walking native macOS Charkha.app and CharkhaBar.app against live servers ==="
defaults write com.navree.demo.mac ApplePersistenceIgnoreState -bool YES || true
defaults write com.navree.demo.menubar ApplePersistenceIgnoreState -bool YES || true

clean_mac_state() {
    rm -rf \
        "${TMPDIR:-/tmp}/com.navree.demo.mac.savedState" \
        "${TMPDIR:-/tmp}/com.navree.demo.menubar.savedState" \
        "$HOME/Library/Saved Application State/com.navree.demo.mac.savedState" \
        "$HOME/Library/Saved Application State/com.navree.demo.menubar.savedState"
}

clean_mac_state
NAVREE_ORIGIN="http://127.0.0.1:8095" run_with_timeout 180 "$charkha" \
    -NavreeSelect /orders \
    -NavreePictures "$build/mac-evidence/store" \
    -NavreeSteps "wait:4;size:1440x900;wait:1;frame;menus;toolbar:toolbar-orders;elements:ax-orders;picture:orders-light;composed:orders-composed-light;appearance:dark;wait:1;picture:orders-dark;appearance:light;wait:1;drag:760x560;wait:1;picture:narrow;size:1440x900;wait:1;sidebar:0;wait:2;picture:dashboard;sidebar:2;wait:2;picture:inventory;sidebar:1;wait:2;choose:0;wait:2;picture:order-detail;about:about;icon:appicon;quit"

sleep 1
clean_mac_state
NAVREE_ORIGIN="http://127.0.0.1:8096" run_with_timeout 180 "$charkha" \
    -NavreePictures "$build/mac-evidence/inbox" \
    -NavreeSteps "wait:4;size:1440x900;wait:1;frame;toolbar:toolbar-inbox;elements:ax-inbox;picture:inbox-light;appearance:dark;wait:1;picture:inbox-dark;quit"

sleep 1
clean_mac_state
NAVREE_ORIGIN="http://127.0.0.1:8097" run_with_timeout 180 "$charkha" \
    -NavreePictures "$build/mac-evidence/board" \
    -NavreeSteps "wait:4;size:1440x900;wait:1;frame;toolbar:toolbar-board;elements:ax-board;picture:board-light;scroll:240;wait:1;picture:board-scrolled;appearance:dark;wait:1;picture:board-dark;quit"

sleep 1
clean_mac_state
NAVREE_ORIGIN="http://127.0.0.1:8095" run_with_timeout 180 "$charkha_bar" \
    -NavreePictures "$build/mac-evidence/menubar" \
    -NavreeSteps "wait:4;presence;status:bar-icon;panel:popover;badge:3;dockmenu;openinwindow;wait:2;picture:promoted-window;closewindow;badge:none;quit"

bad_steps='frame: no window|no table with that row|no sidebar|may not be chosen|unknown step|: not found or disabled|: not taken|about: not found|, no panel|no window under the item|no such item|found nothing saying|nothing under|no event'
for sub in store inbox board menubar; do
    echo "--- macOS $sub steps ---"
    cat "$build/mac-evidence/$sub/steps.txt"
    test -s "$build/mac-evidence/$sub/steps.txt"
    if grep -Eq "$bad_steps" "$build/mac-evidence/$sub/steps.txt"; then
        echo "macOS $sub walk reported a step failure above" >&2
        exit 1
    fi
    sub_pngs="$(find "$build/mac-evidence/$sub" -name '*.png' -size +0c | wc -l | tr -d ' ')"
    if [ "$sub_pngs" -lt 2 ]; then
        echo "macOS $sub walk wrote too few PNGs ($sub_pngs)" >&2
        exit 1
    fi
done
grep -q "row 0 chosen of" "$build/mac-evidence/store/steps.txt"
grep -q 'picture dashboard-1.png .* "Today"' "$build/mac-evidence/store/steps.txt"
grep -q 'picture inventory-1.png .* "Inventory"' "$build/mac-evidence/store/steps.txt"
echo "macOS walks wrote $(find "$build/mac-evidence" -name '*.png' -size +0c | wc -l | tr -d ' ') PNG files"

echo "=== 6. Running live WalkTests and appearance checks on iPhone and iPad simulators ==="
runtime="$(xcrun simctl list runtimes -j | jq -r '
    [.runtimes[] | select(.isAvailable and (.identifier | test("SimRuntime\\.iOS-[0-9-]+$")))]
    | sort_by(.version | split(".") | map(tonumber)) | last | .identifier')"
iphone="$(xcrun simctl list devices available -j | jq -r --arg rt "$runtime" '
    (.devices[$rt] // []) | map(select(.name | startswith("iPhone"))) as $phones
    | ($phones | map(select(.name | test("^iPhone [0-9]+ Pro$")))) + $phones
    | first | .udid // empty')"
ipad="$(xcrun simctl list devices available -j | jq -r --arg rt "$runtime" '
    (.devices[$rt] // []) | map(select(.name | startswith("iPad"))) as $pads
    | ($pads | map(select(.name | startswith("iPad Pro 11")))) + $pads | first | .udid // empty')"

if [ -z "$iphone" ]; then
    echo "No iPhone simulator found on $runtime" >&2
    exit 1
fi
if [ "${LIVE_IPAD:-true}" = "true" ] && [ -z "$ipad" ]; then
    echo "LIVE_IPAD=true, and no iPad simulator was found on $runtime" >&2
    exit 1
fi

(
    cd "$here/Demo"
    xcodebuild build-for-testing \
        -project NavreeDemo.xcodeproj \
        -scheme NavreeDemo \
        -destination "platform=iOS Simulator,id=$iphone" \
        -derivedDataPath "$build/DerivedData" \
        CODE_SIGNING_ALLOWED=NO \
        >"$build/logs/build-for-live-sim.log" 2>&1 || {
        tail -80 "$build/logs/build-for-live-sim.log" >&2
        exit 1
    }
)

app="$build/DerivedData/Build/Products/Debug-iphonesimulator/NavreeDemo.app"
warm_sim() {
    local dev="$1"
    xcrun simctl boot "$dev" 2>/dev/null || true
    xcrun simctl bootstatus "$dev" -b >/dev/null
    xcrun simctl install "$dev" "$app"
    xcrun simctl launch "$dev" com.navree.demo -NavreeRecorded YES >/dev/null || true
    sleep 20
    xcrun simctl terminate "$dev" com.navree.demo 2>/dev/null || true
}

take_evidence() {
    local origin="$1"
    local dev="$2"
    local mode="$3"
    local out_png="$4"
    shift 4
    local err_file="$build/logs/evidence-last.err"
    ORIGIN="$origin" bash "$here/scripts/evidence.sh" "$dev" "$mode" "$out_png" "$@" 2>"$err_file" || {
        cat "$err_file" >&2
        exit 1
    }
    if [ -s "$err_file" ]; then
        cat "$err_file" >&2
    fi
    if grep -q "had drawn nothing" "$err_file"; then
        echo "Simulator screenshot stayed blank on launch screen: $out_png" >&2
        exit 1
    fi
    test -s "$out_png"
}

run_live_walk() {
    local name="$1"
    local dev="$2"
    rm -rf "$build/results/live-walk-$name.xcresult" "$build/live-walk/$name"
    mkdir -p "$build/live-walk/$name"
    (
        cd "$here/Demo"
        TEST_RUNNER_NAVREE_WALK=1 \
        TEST_RUNNER_NAVREE_WALK_ORIGIN="http://127.0.0.1:8095" \
        TEST_RUNNER_NAVREE_WALK_ARGUMENTS="-NavreeSelect|/" \
        TEST_RUNNER_NAVREE_WALK_NAME="store-home" \
        TEST_RUNNER_NAVREE_INBOX_ORIGIN="http://127.0.0.1:8096" \
        TEST_RUNNER_NAVREE_SHOTS="$build/live-walk/$name" \
        xcodebuild test-without-building \
            -project NavreeDemo.xcodeproj \
            -scheme NavreeDemo \
            -destination "platform=iOS Simulator,id=$dev" \
            -derivedDataPath "$build/DerivedData" \
            -resultBundlePath "$build/results/live-walk-$name.xcresult" \
            -only-testing:NavreeDemoUITests/WalkTests \
            -retry-tests-on-failure -test-iterations 2 \
            >"$build/logs/live-walk-$name.log" 2>&1 || {
            tail -80 "$build/logs/live-walk-$name.log" >&2
            exit 1
        }
    )
    tail -20 "$build/logs/live-walk-$name.log"
}

xcrun simctl shutdown all 2>/dev/null || true
warm_sim "$iphone"
run_live_walk "iphone" "$iphone"

for mode in light dark largest rtl contrast transparency; do
    take_evidence "http://127.0.0.1:8095" "$iphone" "$mode" \
        "$build/live-sim-evidence/iphone/store-orders-$mode.png" -NavreeSelect /orders
done
for mode in light dark largest rtl; do
    take_evidence "http://127.0.0.1:8095" "$iphone" "$mode" \
        "$build/live-sim-evidence/iphone/store-today-$mode.png" -NavreeSelect /
    take_evidence "http://127.0.0.1:8096" "$iphone" "$mode" \
        "$build/live-sim-evidence/iphone/inbox-$mode.png" -NavreeSelect /inbox
    take_evidence "http://127.0.0.1:8097" "$iphone" "$mode" \
        "$build/live-sim-evidence/iphone/board-$mode.png" -NavreeSelect /
done
xcrun simctl shutdown "$iphone" 2>/dev/null || true

if [ "${LIVE_IPAD:-true}" = "true" ]; then
    warm_sim "$ipad"
    run_live_walk "ipad" "$ipad"
    for mode in light dark largest rtl; do
        take_evidence "http://127.0.0.1:8095" "$ipad" "$mode" \
            "$build/live-sim-evidence/ipad/store-orders-$mode.png" -NavreeSelect /orders
        take_evidence "http://127.0.0.1:8096" "$ipad" "$mode" \
            "$build/live-sim-evidence/ipad/inbox-$mode.png" -NavreeSelect /inbox
        take_evidence "http://127.0.0.1:8097" "$ipad" "$mode" \
            "$build/live-sim-evidence/ipad/board-$mode.png" -NavreeSelect /
    done
    xcrun simctl shutdown "$ipad" 2>/dev/null || true
fi

echo "Live simulator runs wrote $(find "$build/live-sim-evidence" "$build/live-walk" -name '*.png' -size +0c | wc -l | tr -d ' ') PNG files"
