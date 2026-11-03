#!/usr/bin/env bash
# Deploys navree.com once a day, when main has moved since the live release,
# and tells IndexNow which pages changed. Run from forge's crontab at 21:30
# UTC (03:00 IST), well outside the NSE market hours:
#
#   30 21 * * * /home/forge/navree.com/repo/deploy/navree.com/daily.sh >> /home/forge/navree.com/daily.log 2>&1
#
# The whole script is one function, so bash has read all of it before the
# deploy's checkout can change this file on disk.
set -euo pipefail

main() {
  local site=/home/forge/navree.com
  cd "$site/repo"
  git fetch --quiet --prune origin
  local want have
  want=$(git rev-parse --short origin/main)
  have=$(basename "$(readlink -f "$site/current")")
  have=${have##*-}
  if [ "$want" = "$have" ]; then
    echo "$(date -u +%FT%TZ) navree.com is current at $have"
    return 0
  fi
  echo "$(date -u +%FT%TZ) deploying $want (live: $have)"
  INDEXNOW=1 "$site/repo/deploy/navree.com/deploy.sh" origin/main
}

main "$@"
