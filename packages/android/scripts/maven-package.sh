#!/usr/bin/env bash
# Makes the Android client as Maven packages, the way Maven Central takes them:
# com.navree:navree-native and com.navree:navree-push-fcm at one version, as a
# Maven repository in a folder.
#
#   scripts/maven-package.sh <version> <out-dir>
#
# Writes into <out-dir>:
#
#   navree-android/                            the repository: for each module,
#                                              com/navree/<module>/<version>/ with
#                                              the library (.aar), its POM and
#                                              Gradle metadata, the sources jar
#                                              and the javadoc jar
#   navree-android-<version>.tar.gz, .sha256   the same folder, packed
#
# Nothing in it is signed: scripts/maven-central.sh signs what it uploads. The
# script stops when a module lacks one of its files, when a POM lacks what
# Maven Central asks of it (its name, description and address, the licences,
# the developers, the source repository, and a version for every dependency),
# or when navree-native does not hold the runtime core for both ABIs. Run
# scripts/build-core.sh first (the release's android-package job does), or put
# the libraries of the "Native clients" workflow's navree-native-core artifact
# in navree-native/src/main/jniLibs.
#
# Needs Gradle 9.6 on the path (GRADLE names another command), JDK 17, the
# Android SDK with platform 36, unzip and python3.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
group=com/navree
modules=(navree-native navree-push-fcm)

usage() {
    echo "usage: $0 <version> <out-dir>" >&2
    exit 2
}
fail() {
    echo "maven-package: $*" >&2
    exit 1
}

[ $# -eq 2 ] || usage
version="$1"
out="$2"

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || fail "not a version: $version"
for abi in arm64-v8a x86_64; do
    [ -f "$here/navree-native/src/main/jniLibs/$abi/libnavree_native_core.so" ] \
        || fail "no runtime core for $abi in navree-native/src/main/jniLibs (run scripts/build-core.sh first)"
done

# Only this version's files are in the folder the build publishes to.
rm -rf "$here/build/maven"
(cd "$here" && "${GRADLE:-gradle}" --console=plain --stacktrace "-PnavreeVersion=$version" \
    publishAllPublicationsToFolderRepository)

for module in "${modules[@]}"; do
    folder="$here/build/maven/$group/$module/$version"
    [ -d "$folder" ] || fail "$module was not published to $folder"
    for file in "$module-$version.aar" "$module-$version.pom" "$module-$version.module" \
        "$module-$version-sources.jar" "$module-$version-javadoc.jar"; do
        [ -s "$folder/$file" ] || fail "$module has no $file"
    done
    # What Maven Central asks of a POM beside the coordinates.
    pom="$folder/$module-$version.pom"
    for element in name description url licenses developers scm; do
        grep "<$element>" "$pom" > /dev/null || fail "the POM of $module has no <$element>"
    done
    grep "<version>$version</version>" "$pom" > /dev/null || fail "the POM of $module is not at $version"
    grep '<packaging>aar</packaging>' "$pom" > /dev/null || fail "the POM of $module does not say it is an aar"
    # Every dependency is named with a version, the ones a BOM gives a version to among them.
    python3 -I - "$pom" <<'PY' || fail "the POM of $module names a dependency without a version"
import sys
import xml.etree.ElementTree as tree

space = "{http://maven.apache.org/POM/4.0.0}"
project = tree.parse(sys.argv[1]).getroot()
missing = [
    "%s:%s" % (dependency.findtext(space + "groupId"), dependency.findtext(space + "artifactId"))
    for dependency in project.findall(space + "dependencies/" + space + "dependency")
    if not (dependency.findtext(space + "version") or "").strip()
]
for name in missing:
    print("maven-package: no version for " + name, file=sys.stderr)
sys.exit(1 if missing else 0)
PY
    # The sources jar holds the module's Kotlin, and the javadoc jar its page.
    unzip -Z1 "$folder/$module-$version-sources.jar" | grep '\.kt$' > /dev/null \
        || fail "the sources jar of $module holds no Kotlin file"
    unzip -Z1 "$folder/$module-$version-javadoc.jar" | grep -x 'README.md' > /dev/null \
        || fail "the javadoc jar of $module does not hold its page"
done

# The library carries the runtime core for a phone and for the emulator.
library="$here/build/maven/$group/navree-native/$version/navree-native-$version.aar"
entries="$(unzip -Z1 "$library")"
for abi in arm64-v8a x86_64; do
    printf '%s\n' "$entries" | grep -x "jni/$abi/libnavree_native_core.so" > /dev/null \
        || fail "navree-native-$version.aar does not hold the runtime core for $abi"
done
# The push module names the library at the same version.
grep -A 2 '<artifactId>navree-native</artifactId>' \
    "$here/build/maven/$group/navree-push-fcm/$version/navree-push-fcm-$version.pom" \
    | grep "<version>$version</version>" > /dev/null \
    || fail "the POM of navree-push-fcm does not depend on navree-native $version"

mkdir -p "$out"
out="$(cd "$out" && pwd)"
archive="navree-android-$version.tar.gz"
rm -rf "$out/navree-android" "$out/$archive" "$out/$archive.sha256"
cp -R "$here/build/maven" "$out/navree-android"
tar -czf "$out/$archive" -C "$out" navree-android
if command -v sha256sum > /dev/null; then
    (cd "$out" && sha256sum "$archive" > "$archive.sha256")
else
    (cd "$out" && shasum -a 256 "$archive" > "$archive.sha256")
fi

(cd "$out/navree-android" && find . -type f ! -name '*.md5' ! -name '*.sha1' ! -name '*.sha256' ! -name '*.sha512' | sort)
echo "made $out/navree-android ($version) and $out/$archive"
