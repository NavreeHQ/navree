#!/usr/bin/env bash
# Publishes the Android client to Maven Central: signs every file of the Maven
# repository scripts/maven-package.sh made, packs it as the Central Portal
# takes it, uploads it through the Portal's publishing API and waits until the
# Portal says it is published.
#
#   scripts/maven-central.sh <folder> <version>
#   scripts/maven-central.sh --bundle-only <folder> <version> [<bundle.zip>]
#
# <folder> is the repository (navree-android/, with com/navree/<module>/<version>/
# inside). With --bundle-only nothing is uploaded: the files are signed and
# packed, and the bundle is kept at <bundle.zip> when one is named. The "Native
# clients" workflow does that with a key it makes for the run.
#
# From the environment:
#
#   MAVEN_GPG_PRIVATE_KEY     the signing key, ASCII-armoured (gpg --armor
#                             --export-secret-keys <fingerprint>)
#   MAVEN_GPG_PASSPHRASE      its passphrase (empty for a key without one)
#   MAVEN_CENTRAL_USERNAME    the two halves of a Central Portal user token
#   MAVEN_CENTRAL_PASSWORD    (not needed with --bundle-only)
#   MAVEN_CENTRAL_WAIT        how many seconds to wait for the Portal to
#                             publish (3600 when unset)
#
# The files are signed with the key's primary key, which is the one Maven
# Central checks, and the key is kept in a folder that is deleted when the
# script ends. The bundle holds, for every file of a module, the file, its
# signature (.asc) and its MD5 and SHA-1 (and SHA-256 and SHA-512) sums; the
# repository's maven-metadata.xml files are left out, since the Portal writes
# its own.
#
# A version Maven Central has already is left as it is, so the script can be
# run again after a stop. The upload is published without a further step
# (publishingType=AUTOMATIC) once the Portal has checked it: what is on Maven
# Central stays there for good.
#
# Needs gpg, curl, zip, python3 and GNU coreutils (md5sum, sha1sum, sha256sum,
# sha512sum): it runs on Linux.
set -euo pipefail

group=com/navree
api=https://central.sonatype.com/api/v1/publisher

usage() {
    echo "usage: $0 [--bundle-only] <folder> <version> [<bundle.zip>]" >&2
    exit 2
}
fail() {
    echo "maven-central: $*" >&2
    exit 1
}

bundle_only=false
if [ "${1:-}" = "--bundle-only" ]; then
    bundle_only=true
    shift
fi
[ $# -ge 2 ] || usage
[ -d "$1" ] || fail "no folder at $1"
folder="$(cd "$1" && pwd)"
version="$2"
kept="${3:-}"
if [ "$bundle_only" = true ]; then
    [ $# -le 3 ] || usage
else
    [ $# -eq 2 ] || usage
fi

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || fail "not a version: $version"
[ -n "${MAVEN_GPG_PRIVATE_KEY:-}" ] || fail "MAVEN_GPG_PRIVATE_KEY is not set"
if [ "$bundle_only" = false ]; then
    [ -n "${MAVEN_CENTRAL_USERNAME:-}" ] || fail "MAVEN_CENTRAL_USERNAME is not set"
    [ -n "${MAVEN_CENTRAL_PASSWORD:-}" ] || fail "MAVEN_CENTRAL_PASSWORD is not set"
    # Maven Central keeps -SNAPSHOT for versions that are not releases, and refuses one.
    [[ "$version" != *-SNAPSHOT ]] || fail "$version is not a version Maven Central takes"
fi

modules=()
for path in "$folder/$group"/*/; do
    [ -d "$path" ] || continue
    module="$(basename "$path")"
    [ -d "$path$version" ] || fail "$module is not in $folder at $version"
    modules+=("$module")
done
[ "${#modules[@]}" -gt 0 ] || fail "no module under $folder/$group"

# What Maven Central has already. All of the modules go up in one upload, so
# either all of them are there or none is.
if [ "$bundle_only" = false ]; then
    there=0
    for module in "${modules[@]}"; do
        address="https://repo1.maven.org/maven2/$group/$module/$version/$module-$version.pom"
        status="$(curl --silent --location --max-time 60 --retry 3 --retry-delay 5 \
            --output /dev/null --write-out '%{http_code}' "$address" || true)"
        case "$status" in
            200)
                echo "com.navree:$module:$version is on Maven Central"
                there=$((there + 1))
                ;;
            404) ;;
            *) fail "Maven Central answered $status for $address" ;;
        esac
    done
    if [ "$there" -eq "${#modules[@]}" ]; then
        echo "Maven Central has $version already: nothing was uploaded"
        exit 0
    fi
    [ "$there" -eq 0 ] || fail "Maven Central has only some of the modules at $version: publish the others in the Central Portal by hand"
fi

umask 077
work="$(mktemp -d)"
export GNUPGHOME="$work/gnupg"
mkdir "$GNUPGHOME"
cleanup() {
    gpgconf --kill gpg-agent > /dev/null 2>&1 || true
    rm -rf "$work"
}
trap cleanup EXIT

printf '%s\n' "$MAVEN_GPG_PRIVATE_KEY" | gpg --batch --quiet --import \
    || fail "MAVEN_GPG_PRIVATE_KEY is not a key gpg can read"
# The fingerprint of the first secret key, which is its primary key.
key="$(gpg --batch --with-colons --list-secret-keys | awk -F: '$1 == "fpr" && !seen { print $10; seen = 1 }')"
[ -n "$key" ] || fail "MAVEN_GPG_PRIVATE_KEY holds no secret key"
echo "signing with the key $key"

# "<fingerprint>!" has gpg sign with that key and no subkey of it.
sign() {
    gpg --batch --yes --pinentry-mode loopback --passphrase-fd 3 --local-user "$key!" \
        --armor --detach-sign --output "$1.asc" "$1" 3<<< "${MAVEN_GPG_PASSPHRASE:-}" \
        || fail "could not sign $(basename "$1") (is MAVEN_GPG_PASSPHRASE the key's passphrase, and can its primary key sign?)"
    gpg --batch --quiet --verify "$1.asc" "$1" 2> /dev/null \
        || fail "the signature of $(basename "$1") does not verify"
}

stage="$work/bundle"
for module in "${modules[@]}"; do
    from="$folder/$group/$module/$version"
    to="$stage/$group/$module/$version"
    mkdir -p "$to"
    for file in "$from"/*; do
        name="$(basename "$file")"
        case "$name" in
            *.md5 | *.sha1 | *.sha256 | *.sha512 | *.asc | maven-metadata*) continue ;;
        esac
        [ -f "$file" ] || continue
        cp "$file" "$to/$name"
        for sum in md5 sha1 sha256 sha512; do
            "${sum}sum" "$to/$name" | cut -d ' ' -f 1 > "$to/$name.$sum"
        done
        sign "$to/$name"
    done
    for file in "$module-$version.pom" "$module-$version.aar" "$module-$version-sources.jar" "$module-$version-javadoc.jar"; do
        [ -s "$to/$file" ] || fail "$module has no $file"
        [ -s "$to/$file.asc" ] || fail "$file has no signature"
    done
done

bundle="$work/navree-android-$version.zip"
(cd "$stage" && zip --quiet --recurse-paths "$bundle" com)
(cd "$stage" && find com -type f | sort)

if [ "$bundle_only" = true ]; then
    if [ -n "$kept" ]; then
        mkdir -p "$(dirname "$kept")"
        cp "$bundle" "$kept"
        echo "made the bundle $kept ($version); nothing was uploaded"
    else
        echo "made the bundle ($version); nothing was uploaded"
    fi
    exit 0
fi

printf 'Authorization: Bearer %s\n' "$(printf '%s:%s' "$MAVEN_CENTRAL_USERNAME" "$MAVEN_CENTRAL_PASSWORD" | base64 | tr -d '\n')" \
    > "$work/headers"

# The upload is sent once: sent again after a doubt, it would be a second
# deployment of the same version, which the Portal refuses.
status="$(curl --silent --show-error --connect-timeout 30 --max-time 1800 \
    --output "$work/upload" --write-out '%{http_code}' \
    --request POST --header @"$work/headers" \
    --form "bundle=@$bundle;type=application/octet-stream" \
    "$api/upload?name=navree-android-$version&publishingType=AUTOMATIC")" \
    || fail "the upload did not reach the Central Portal"
if [ "$status" != 201 ]; then
    cat "$work/upload" >&2
    echo >&2
    fail "the Central Portal answered $status to the upload"
fi
id="$(tr -d '[:space:]' < "$work/upload")"
[[ "$id" =~ ^[0-9A-Fa-f-]+$ ]] || fail "the Central Portal gave no deployment id: $id"
echo "uploaded as the deployment $id"

# PENDING, VALIDATING, PUBLISHING, then PUBLISHED or FAILED. An answer that
# cannot be had or read is asked for again.
wait_for="${MAVEN_CENTRAL_WAIT:-3600}"
deadline=$((SECONDS + wait_for))
state=""
while :; do
    now=""
    if curl --silent --show-error --fail --max-time 60 --output "$work/status" \
        --request POST --header @"$work/headers" "$api/status?id=$id"; then
        now="$(python3 -I -c 'import json, sys; print(json.load(open(sys.argv[1])).get("deploymentState", ""))' "$work/status" || true)"
    fi
    if [ -n "$now" ] && [ "$now" != "$state" ]; then
        state="$now"
        echo "the deployment is $state"
    fi
    case "$state" in
        PUBLISHED) break ;;
        FAILED)
            python3 -I -c 'import json, sys; print(json.dumps(json.load(open(sys.argv[1])).get("errors"), indent=2))' "$work/status" >&2 || cat "$work/status" >&2
            fail "the Central Portal refused the deployment $id (https://central.sonatype.com/publishing/deployments)"
            ;;
    esac
    if [ "$SECONDS" -ge "$deadline" ]; then
        fail "the deployment $id was still ${state:-unknown} after $wait_for seconds: see https://central.sonatype.com/publishing/deployments, and run this again once Maven Central has $version (it is then left as it is)"
    fi
    sleep 15
done

for module in "${modules[@]}"; do
    echo "published com.navree:$module:$version"
done
