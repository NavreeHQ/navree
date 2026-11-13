#!/usr/bin/env bash
# Pictures of screens from recorded answers, drawn by the client's own views
# on this Mac, with no simulator: the picture tool in Shots/ is built for the
# Mac as an iPad app (Mac Catalyst), wrapped as an app and run once.
#
#   scripts/shots.sh <out folder> <answer.json>...
#
# Writes, for each answer, light and dark pictures of the screen as it first
# shows and of the whole screen (Shots/Sources/NavreeShots/main.swift says
# which widths). A window opens for the few seconds the drawing takes.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
if [ "$#" -lt 2 ]; then
    echo "usage: $0 <out folder> <answer.json>..." >&2
    exit 2
fi
out="$1"
shift
files=()
for file in "$@"; do files+=("$(cd "$(dirname "$file")" && pwd)/$(basename "$file")"); done
mkdir -p "$out"
out="$(cd "$out" && pwd)"

sdk="$(xcrun --sdk macosx --show-sdk-path)"
(cd "$here/Shots" && swift build --triple arm64-apple-ios26.0-macabi --sdk "$sdk" --product NavreeShots)
binary="$(find "$here/Shots/.build" -path '*maccatalyst*' -name NavreeShots -type f -perm -u+x | head -1)"

app="$here/Shots/.build/NavreeShots.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "$binary" "$app/Contents/MacOS/NavreeShots"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>dev.navree.shots</string>
    <key>CFBundleExecutable</key><string>NavreeShots</string>
    <key>CFBundleName</key><string>NavreeShots</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>26.0</string>
    <key>UIDeviceFamily</key><array><integer>2</integer></array>
    <key>UIApplicationSceneManifest</key>
    <dict><key>UIApplicationSupportsMultipleScenes</key><false/></dict>
    <key>UILaunchScreen</key><dict/>
</dict>
</plist>
PLIST
codesign --sign - --force "$app" >/dev/null
# After a run that crashed, the system would ask whether to reopen its windows
# and wait for an answer: the tool keeps no windows worth reopening.
defaults write dev.navree.shots ApplePersistenceIgnoreState -bool YES
"$app/Contents/MacOS/NavreeShots" "$out" "${files[@]}"
