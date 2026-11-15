#!/usr/bin/env bash
# Walks the demo app on a device the way a person does, and says what it saw.
#
# The app is installed, its server (the store demo) answers on 10.0.2.2:8787.
# Each step is a check that prints PASS or FAIL; the pictures of a start, second
# by second, are folders of frames with the time of each. Needs adb, python3.
#
#   ADB=/path/to/adb OUT=walk packages/android/scripts/walk.sh

ADB=${ADB:-adb}
OUT=${OUT:-walk}
PKG=com.navree.demo
ACTIVITY=com.navree.client.NavreeActivity
ORIGIN=http://10.0.2.2:8787
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT"
fails=0

say() { echo "$*" | tee -a "$OUT/walk.txt"; }

check() {
  local name=$1
  shift
  if "$@"; then say "PASS $name"; else say "FAIL $name"; fails=$((fails + 1)); fi
}

# What is on the screen, written to $OUT/window.xml; tried again when the device was not still.
dump() {
  local try
  for try in 1 2 3; do
    "$ADB" shell uiautomator dump /sdcard/window.xml > /dev/null 2>&1
    if "$ADB" pull /sdcard/window.xml "$OUT/window.xml" > /dev/null 2>&1; then return 0; fi
    sleep 1
  done
  return 1
}

ui() { python3 -I "$HERE/ui.py" "$OUT/window.xml" "$@"; }

# Waits up to $2 seconds for a node that says $1.
wait_for() {
  local end=$((SECONDS + ${2:-20}))
  while [ "$SECONDS" -lt "$end" ]; do
    dump && ui has "$1" && return 0
    sleep 1
  done
  return 1
}

wait_for_part() {
  local end=$((SECONDS + ${2:-20}))
  while [ "$SECONDS" -lt "$end" ]; do
    dump && ui contains "$1" && return 0
    sleep 1
  done
  return 1
}

# Taps the node that says $1.
tap() {
  dump || return 1
  local at
  at=$(ui tap "$1") || return 1
  "$ADB" shell input tap $at
}

shot() { "$ADB" exec-out screencap -p > "$OUT/$1.png"; }

# $2 frames of the screen from now, the time of each, taken one after the other.
strip() {
  mkdir -p "$OUT/strip-$1"
  : > "$OUT/strip-$1/times.txt"
  local start now i
  start=$(date +%s%N)
  for i in $(seq 1 "$2"); do
    now=$(date +%s%N)
    "$ADB" exec-out screencap -p > "$OUT/strip-$1/$(printf %02d "$i").png"
    echo "$(printf %02d "$i") $(((now - start) / 1000000)) ms" >> "$OUT/strip-$1/times.txt"
  done
}

launch() { "$ADB" shell am start -n "$PKG/$ACTIVITY" > /dev/null; }
stop_app() { "$ADB" shell am force-stop "$PKG"; }
alive() { [ -n "$("$ADB" shell pidof "$PKG" | tr -d '\r')" ]; }

net() {
  if [ "$1" = off ]; then
    "$ADB" shell svc wifi disable
    "$ADB" shell svc data disable
    sleep 3
    ! "$ADB" shell ping -c 1 -W 2 10.0.2.2 > /dev/null 2>&1
  else
    "$ADB" shell svc wifi enable
    "$ADB" shell svc data enable
    local end=$((SECONDS + 30))
    while [ "$SECONDS" -lt "$end" ]; do
      "$ADB" shell ping -c 1 -W 2 10.0.2.2 > /dev/null 2>&1 && return 0
      sleep 2
    done
    return 1
  fi
}

rotate() {
  "$ADB" shell settings put system accelerometer_rotation 0
  "$ADB" shell settings put system user_rotation "$1"
  sleep 3
}

# The smallest number among the orders (CS-nnnnnn) that are in the dump.
lowest_order() { ui texts | grep -o 'CS-[0-9]*' | sort | head -1 | tr -d 'CS-'; }

# ---------------------------------------------------------------- starts

say "== a start, second by second"
"$ADB" shell pm clear "$PKG" > /dev/null
rotate 0
launch
strip cold-first 14
check "a first start shows the first screen" wait_for Dashboard 20
shot first-screen

"$ADB" shell input keyevent KEYCODE_HOME
sleep 2
stop_app
launch
strip cold-second 14
check "a second start shows the first screen" wait_for Dashboard 20

say "== no network at start, with a page kept"
"$ADB" shell input keyevent KEYCODE_HOME
stop_app
check "the network is off" net off
launch
strip offline-kept 14
dump
shot offline-kept
check "the app is still running with no network" alive
ui texts > "$OUT/offline-kept.txt"
say "offline with a page kept says: $(ui texts | tr '\n' '|' | cut -c1-300)"

say "== the network comes back"
check "the network is on" net on
if ui has "Try again" > /dev/null 2>&1; then tap "Try again"; fi
check "the screen comes once the network is back" wait_for Dashboard 30

say "== no network at start, nothing kept"
"$ADB" shell input keyevent KEYCODE_HOME
stop_app
"$ADB" shell pm clear "$PKG" > /dev/null
check "the network is off again" net off
launch
strip offline-clean 14
dump
shot offline-clean
check "nothing is shown but the way to try again" ui has "Try again"
check "no person is named with no server to say who" bash -c "! python3 -I '$HERE/ui.py' '$OUT/window.xml' contains 'Account,'"
check "the network is back" net on
tap "Try again"
check "trying again with the network on shows the first screen" wait_for Dashboard 30

# ---------------------------------------------------------------- going about

say "== orders: search, more rows, back"
tap Orders
check "Orders opens" wait_for Search 20
dump
first=$(ui texts | grep -o 'CS-[0-9]*' | head -1 | tr -d 'CS-')
say "first order $first"
shot orders
for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do "$ADB" shell input swipe 540 1900 540 600 250; sleep 0.4; done
sleep 2
dump
lowest=$(lowest_order)
say "after scrolling the lowest order in view is $lowest"
check "a long list loads more as it is scrolled" bash -c "[ -n '$first' ] && [ -n '$lowest' ] && [ '$lowest' -lt \$(( $first - 25 )) ]"
shot orders-scrolled

"$ADB" shell input swipe 540 600 540 2000 250
sleep 1
"$ADB" shell input swipe 540 600 540 2000 250
sleep 2
check "going to the top again shows the search" wait_for Search 10

tap Search
sleep 1
dump
at=$(ui field) || at=""
if [ -n "$at" ]; then "$ADB" shell input tap $at; fi
"$ADB" shell input text Kaur
sleep 3
dump
shot search
check "search shows only matches" bash -c "n=\$(python3 -I '$HERE/ui.py' '$OUT/window.xml' count 'CS-[0-9]+'); k=\$(python3 -I '$HERE/ui.py' '$OUT/window.xml' count 'Kaur'); [ \"\$n\" -gt 0 ] && [ \"\$k\" -ge \"\$n\" ]"

say "== turning the device keeps the place and the words"
rotate 1
dump
shot search-turned
check "the typed words stay when the device is turned" ui contains Kaur
rotate 0
"$ADB" shell input keyevent KEYCODE_BACK
sleep 1
"$ADB" shell input keyevent KEYCODE_BACK
sleep 1

say "== an order opens and closes"
dump
tap "$(ui texts | grep -o 'CS-[0-9]*' | head -1)"
sleep 3
dump
shot order
check "an order opens" ui contains "Order CS-"
"$ADB" shell input keyevent KEYCODE_BACK
sleep 2
check "back closes the order and the list is there" wait_for Search 10

say "== back, one step at a time"
"$ADB" shell input keyevent KEYCODE_BACK
sleep 2
check "back from another destination goes to the first one" wait_for_part "Revenue today" 10
"$ADB" shell input keyevent KEYCODE_BACK
sleep 2
check "back from the first destination leaves the app" bash -c "! '$ADB' shell dumpsys window | grep -q 'mCurrentFocus.*$PKG'"

say "== a link from outside"
launch
wait_for Dashboard 20
"$ADB" shell am start -n "$PKG/$ACTIVITY" -a android.intent.action.VIEW -d "$ORIGIN/inventory" > /dev/null
sleep 3
dump
shot link-inventory
check "a link to the app's address opens its screen" ui contains "Units of"

say "== a form, a send, the message that follows"
dump
at=$(ui field) || at=""
if [ -n "$at" ]; then
  "$ADB" shell input tap $at
  for _ in 1 2 3 4 5 6 7 8; do "$ADB" shell input keyevent KEYCODE_DEL; done
  "$ADB" shell input text 7
  shot restock-typed
  "$ADB" shell input keyevent KEYCODE_ENTER
  sleep 3
  shot restock-sent
  check "the keyboard's action sends the restock and a message follows" wait_for_part "Restocked" 10
else
  say "FAIL no field to type in"
  fails=$((fails + 1))
fi

# ---------------------------------------------------------------- the system ends the app

say "== the app ended by the system, and opened again, with no network"
tap Orders
wait_for Search 20
"$ADB" shell input keyevent KEYCODE_HOME
sleep 3
"$ADB" shell am kill "$PKG"
sleep 1
check "the app's process is gone" bash -c "! '$ADB' shell pidof $PKG | grep -q ."
check "the network is off for the return" net off
"$ADB" shell monkey -p "$PKG" -c android.intent.category.LAUNCHER 1 > /dev/null 2>&1
strip returned-offline 12
dump
shot returned-offline
say "returned with no network says: $(ui texts | tr '\n' '|' | cut -c1-300)"
check "it returns to the destination it was on" bash -c "python3 -I '$HERE/ui.py' '$OUT/window.xml' has Search || python3 -I '$HERE/ui.py' '$OUT/window.xml' has 'Try again'"
check "no person is named before the server has said who" bash -c "! python3 -I '$HERE/ui.py' '$OUT/window.xml' contains 'Account,'"
check "the network is back" net on
tap "Try again" > /dev/null 2>&1
check "the person is named once the server has answered" wait_for_part "Account, " 30

say "walk done: $fails failed"
[ "$fails" -eq 0 ]
