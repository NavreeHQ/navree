#!/usr/bin/env bash
# Tests of the helpers of `walk.sh`, with a stand-in for `adb` and for `curl`: a screen is never
# read from an older dump, a dump that failed fails the check that asked for it, and what the
# walk expects of a return to a place follows what the demo's shell says.
#
#   packages/android/scripts/walk_test.sh

HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
problems=0

# The stand-in for the device. Its screen is $WORK/screen.xml; the dump is the device's file, written
# only while $WORK/dump-works exists (the real tool often says it could not get the screen and leaves
# the file as it was), or written broken while $WORK/dump-broken exists.
cat > "$WORK/adb" <<'STAND_IN'
#!/usr/bin/env bash
device="$WORK/device-window.xml"
case "$*" in
  "shell rm -f /sdcard/window.xml") rm -f "$device" ;;
  "shell uiautomator dump /sdcard/window.xml")
    if [ -e "$WORK/dump-works" ]; then cp "$WORK/screen.xml" "$device"; fi
    if [ -e "$WORK/dump-broken" ]; then printf '<hierarchy rotation=' > "$device"; fi
    ;;
  "pull /sdcard/window.xml "*) [ -e "$device" ] && cp "$device" "${3}" ;;
  *) exit 0 ;;
esac
STAND_IN
chmod +x "$WORK/adb"
export WORK

# The stand-in for the demo's server: answers by what is in $WORK/shell-first and $WORK/shell-second
# (the headers of the first and of the second answer; absent for no answer at all).
cat > "$WORK/curl" <<'STAND_IN'
#!/usr/bin/env bash
n=$(cat "$WORK/asked" 2>/dev/null || echo 0)
n=$((n + 1))
echo "$n" > "$WORK/asked"
file="$WORK/shell-$([ "$n" = 1 ] && echo first || echo second)"
[ -e "$file" ] || exit 7
cat "$file"
STAND_IN
chmod +x "$WORK/curl"

# The helpers of the script, as the walk has them.
export ADB="$WORK/adb" OUT="$WORK/out"
# shellcheck source=walk.sh
source "$HERE/walk.sh"

expect() {
  local name=$1
  shift
  if "$@"; then echo "ok   $name"; else echo "FAIL $name"; problems=$((problems + 1)); fi
}
refute() {
  local name=$1
  shift
  if "$@"; then echo "FAIL $name"; problems=$((problems + 1)); else echo "ok   $name"; fi
}

SCREEN='<?xml version="1.0"?><hierarchy rotation="0"><node class="android.widget.TextView" text="Dashboard" content-desc="" bounds="[0,0][10,10]" /></hierarchy>'
OLDER='<?xml version="1.0"?><hierarchy rotation="0"><node class="android.widget.TextView" text="Orders" content-desc="" bounds="[0,0][10,10]" /><node class="android.widget.Button" text="Back" content-desc="" bounds="[0,0][10,10]" /></hierarchy>'

echo "== the screen"
echo "$SCREEN" > "$WORK/screen.xml"
touch "$WORK/dump-works"
expect "a dump that works is the screen" dump
expect "and it says what is on it" ui has Dashboard

echo "== a dump that fails never leaves an older screen to read"
rm -f "$WORK/dump-works"
echo "$OLDER" > "$WORK/device-window.xml"
cp "$WORK/device-window.xml" "$OUT/window.xml"
refute "a failed dump fails" dump
refute "there is no file here to read" test -e "$OUT/window.xml"
refute "and none on the device" test -e "$WORK/device-window.xml"
echo "$OLDER" > "$WORK/device-window.xml"
fails=0
check "something is absent" ui absent Back > /dev/null
expect "a check that something is absent fails on no dump, not passes" test "$fails" -eq 1
refute "a question of the older screen has no answer" ui has Orders
refute "the screen can not be looked at" look "the test"
expect "and that is counted a failure of its own" test "$fails" -eq 2

echo "== a dump that is not a dump fails too"
touch "$WORK/dump-broken"
refute "a broken file is not a dump" dump
refute "a check of absence is not answered from it" ui absent Back
rm -f "$WORK/dump-broken"

echo "== and the next dump that works is read again"
touch "$WORK/dump-works"
expect "dump" dump
expect "the screen is the new one" ui has Dashboard
expect "what is not on it is absent" ui absent Back

echo "== what the demo's shell says"
ok='HTTP/1.1 200 OK
content-type: application/vnd.navree.native+json
'
named="$ok
x-navree-person: 0193a4b2-0000-7000-8000-00000000ra01
"
ask() { rm -f "$WORK/asked" "$WORK/shell-first" "$WORK/shell-second"; [ -n "$1" ] && printf '%s' "$1" > "$WORK/shell-first"; [ -n "$2" ] && printf '%s' "$2" > "$WORK/shell-second"; PATH="$WORK:$PATH" person_named; }
expect "a shell that names nobody" test "$(ask "$ok" "$ok")" = no
expect "a shell that names a person from the first answer" test "$(ask "$named" "$named")" = yes
expect "a shell that names its visitor from the second answer on" test "$(ask "$ok" "$named")" = yes
refute "a server that does not answer" ask "" ""
refute "an answer that is not a 200" ask "HTTP/1.1 502 Bad Gateway
" "HTTP/1.1 502 Bad Gateway
"

echo
if [ "$problems" -ne 0 ]; then echo "$problems failed"; exit 1; fi
echo "all passed"
