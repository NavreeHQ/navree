#!/usr/bin/env bash
# Deploys navree.com (the website and docs) on the production server:
#
#   /home/forge/navree.com/repo        a clone of NavreeHQ/navree (build workspace)
#   /home/forge/navree.com/releases/   one directory per deploy (the static export)
#   /home/forge/navree.com/current  -> the live release; nginx serves this
#   /home/forge/navree.com/shared/media/  the launch film and its poster (large,
#                                      so not in git); every release links its
#                                      media/ to this folder, which is how /media/
#                                      is served. The files must be in it before a
#                                      deploy: apps/website/content/media.json lists
#                                      each with its size and SHA-256, and the deploy
#                                      stops, before it builds, if one is missing or
#                                      differs.
#
# Run as forge on the server:
#   /home/forge/navree.com/repo/deploy/navree.com/deploy.sh [git-ref]   (default: origin/main)
# Roll back by pointing `current` at an older release:
#   ln -sfn /home/forge/navree.com/releases/<older> /home/forge/navree.com/current
#
# The whole script is one function, so bash has read all of it before the
# checkout below can change this file on disk.
set -euo pipefail

main() {
  local site=/home/forge/navree.com
  local ref="${1:-origin/main}"
  local keep=5
  local media="$site/shared/media"

  cd "$site/repo"
  git fetch --quiet --prune origin
  git checkout --quiet --detach "$ref"
  local sha
  sha=$(git rev-parse --short HEAD)

  # The launch film's files are not in git. Check them, as listed in this
  # commit's manifest, before the slow build, so a missing folder or file stops
  # the deploy here and says what to do.
  if [ ! -d "$media" ]; then
    echo "Stopping: $media does not exist. The launch film and its poster must be copied there first;" >&2
    echo "apps/website/content/media.json lists the files, with their sizes and SHA-256 values." >&2
    exit 1
  fi
  node apps/website/scripts/media.ts verify "$media" || {
    echo "Stopping: the files in $media are not the ones apps/website/content/media.json lists (see above)." >&2
    exit 1
  }

  echo "Building navree.com at $sha"

  cd apps/website
  pnpm install --frozen-lockfile --reporter=silent
  pnpm build

  local release
  release="$site/releases/$(date -u +%Y%m%d%H%M%S)-$sha"
  mkdir -p "$release"
  cp -a out/. "$release/"

  # /media/ is the shared folder: the export has none of its own (a build with
  # NAVREE_MEDIA_DIR set would leave a copy, which the link replaces).
  rm -rf "$release/media"
  ln -s "$media" "$release/media"
  [ -d "$release/media/" ] || { echo "Stopping: $release/media does not reach $media." >&2; exit 1; }

  # Every export holds the docs bundle of its own version only
  # (v/<version>/docs.json), and Navigator downloads the one for the Navree
  # version an app uses. Carry the bundles of earlier versions over from the
  # live release; this export's own bundle stays when the version is the same.
  if [ -d "$site/current/v" ]; then
    mkdir -p "$release/v"
    for bundle in "$site/current/v"/*; do
      [ -e "$bundle" ] || continue
      [ -e "$release/v/${bundle##*/}" ] || cp -a "$bundle" "$release/v/"
    done
  fi

  # Atomic switch: a new symlink renamed over the old one.
  ln -sfn "$release" "$site/current.next"
  mv -Tf "$site/current.next" "$site/current"
  echo "navree.com now serves $sha from $release"

  # Opt-in: tell IndexNow which pages changed (see indexnow.sh). A failed ping
  # never fails the deploy.
  if [ "${INDEXNOW:-}" = 1 ]; then
    "$site/repo/deploy/navree.com/indexnow.sh" --send "$release" || true
  fi

  # Keep the newest $keep releases.
  ls -1dt "$site"/releases/*/ | tail -n +$((keep + 1)) | xargs -r rm -rf
}

main "$@"
