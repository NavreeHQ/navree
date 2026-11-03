#!/usr/bin/env bash
# Tells IndexNow (https://www.indexnow.org/) which pages of navree.com
# changed in a deploy, so the search engines that take part (Bing, Yandex,
# Naver, Seznam, Yep) crawl them now instead of at their next visit. Google
# does not take part; it finds changes through the sitemap.
#
# How it knows what changed: each release keeps its sitemap.xml, and every
# page's <lastmod> in it is the date of its last git commit. A page is
# "changed" when its URL is new or its <lastmod> differs from the previous
# release's. The sitemap's /changelog/ moves with every commit, so every
# deploy at least pings that.
#
# The key: IndexNow checks that the host owns the key by fetching
# https://navree.com/<key>.txt, which holds the key itself. That file is
# apps/website/public/<key>.txt, so it is part of every export. The key is
# public by design; changing it means renaming that file and the line below.
#
# Usage (as forge, on the server, after deploy.sh has switched `current`):
#
#   indexnow.sh                      list what would be sent (nothing is sent)
#   indexnow.sh --send               send the pages that changed since the previous release
#   indexnow.sh --send --all         send every page of the sitemap (first time, or after a big change)
#   indexnow.sh [--send] NEW OLD     compare two release directories instead of
#                                    `current` and the release before it
#
# deploy.sh runs it with --send when INDEXNOW=1 is set in its environment:
#
#   INDEXNOW=1 /home/forge/navree.com/repo/deploy/navree.com/deploy.sh
#
# It is off by default: a deploy should not depend on another service being up,
# and a failed ping never fails the deploy (this script exits 0 after a
# warning when the service does not answer 200 or 202).
set -euo pipefail

INDEXNOW_KEY="5d1d0a4f216664f7f23783654d129363"
HOST="navree.com"
ENDPOINT="${INDEXNOW_ENDPOINT:-https://api.indexnow.org/indexnow}"
SITE="${SITE_DIR:-/home/forge/navree.com}"
BATCH=1000 # IndexNow accepts 10 000 URLs a call; stay far below it.

send=0
all=0
args=()
for arg in "$@"; do
  case "$arg" in
    --send) send=1 ;;
    --all) all=1 ;;
    -h | --help)
      sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) args+=("$arg") ;;
  esac
done

new="${args[0]:-$SITE/current}"
new="$(readlink -f "$new")"
old="${args[1]:-}"
if [ -z "$old" ] && [ "$all" = 0 ]; then
  # The release before the one `current` points at: releases are named <UTC time>-<sha>.
  old="$(ls -1d "$SITE"/releases/*/ 2>/dev/null | sed 's#/$##' | sort | grep -v -x -F "$new" | tail -n 1 || true)"
fi

[ -f "$new/sitemap.xml" ] || { echo "indexnow: $new/sitemap.xml not found" >&2; exit 1; }
[ -f "$new/$INDEXNOW_KEY.txt" ] || { echo "indexnow: the key file $INDEXNOW_KEY.txt is not in $new" >&2; exit 1; }

# "<url> <lastmod>" per line, sorted.
entries() {
  tr -d '\n' <"$1/sitemap.xml" |
    sed 's#</url>#</url>\n#g' |
    sed -n 's#.*<loc>\([^<]*\)</loc>.*<lastmod>\([^<]*\)</lastmod>.*#\1 \2#p' |
    sort
}

if [ "$all" = 1 ] || [ -z "$old" ] || [ ! -f "$old/sitemap.xml" ]; then
  urls="$(entries "$new" | cut -d' ' -f1)"
  echo "indexnow: every page of $new"
else
  urls="$(comm -13 <(entries "$old") <(entries "$new") | cut -d' ' -f1)"
  echo "indexnow: pages that changed from $(basename "$old") to $(basename "$new")"
fi

count="$(printf '%s\n' "$urls" | grep -c . || true)"
if [ "$count" = 0 ]; then
  echo "indexnow: nothing changed"
  exit 0
fi
printf '%s\n' "$urls" | sed 's/^/  /'
echo "indexnow: $count URL(s)"

if [ "$send" = 0 ]; then
  echo "indexnow: dry run; add --send to ping $ENDPOINT"
  exit 0
fi

# One call per batch. The JSON is built by hand: the URLs come from our own
# sitemap, and quotes and backslashes are escaped all the same.
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
printf '%s\n' "$urls" | split -l "$BATCH" - "$tmp/chunk."
json_escape() { sed 's/\\/\\\\/g; s/"/\\"/g'; }
for chunk in "$tmp"/chunk.*; do
  list="$(json_escape <"$chunk" | sed 's/.*/"&"/' | paste -sd, -)"
  payload="{\"host\":\"$HOST\",\"key\":\"$INDEXNOW_KEY\",\"keyLocation\":\"https://$HOST/$INDEXNOW_KEY.txt\",\"urlList\":[$list]}"
  code="$(curl -sS -o /dev/null -w '%{http_code}' -X POST "$ENDPOINT" -H 'Content-Type: application/json; charset=utf-8' -d "$payload" || true)"
  code="${code:-000}"
  case "$code" in
    200 | 202) echo "indexnow: sent a batch, the service answered $code" ;;
    *) echo "indexnow: warning: the service answered $code for a batch" >&2 ;;
  esac
done
# A failed ping is a warning, never a failed deploy.
exit 0
