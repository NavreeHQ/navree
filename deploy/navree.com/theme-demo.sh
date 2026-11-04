#!/usr/bin/env bash
# Builds the live theme demo (apps/theme-demo) on the production server and
# starts it, or reloads it, under pm2 as navree-theme-demo, on 127.0.0.1:8095.
# Needs no sudo. Run as forge:
#
#   /home/forge/navree.com/repo/deploy/navree.com/theme-demo.sh [git-ref]   (default: origin/main)
#
#   /home/forge/navree.com/theme-demo/target/        the build's own target directory
#   /home/forge/navree.com/theme-demo/releases/      one binary per deploy
#   /home/forge/navree.com/theme-demo/current     -> the live release; pm2 runs current/theme-demo
#
# It needs a Rust toolchain for forge (rustup, in forge's home; no sudo). The build
# takes a few minutes of two cores at the lowest priority: run it after the NSE close
# (15:30 IST), and not while deploy.sh runs, which checks out the same repository.
# Roll back by pointing `current` at an older release and `pm2 reload navree-theme-demo`.
#
# The whole script is one function, so bash has read all of it before the
# checkout below can change this file on disk.
set -euo pipefail

main() {
  local site=/home/forge/navree.com
  local ref="${1:-origin/main}"
  local home="$site/theme-demo"
  local keep=3

  cd "$site/repo"
  git fetch --quiet --prune origin
  git checkout --quiet --detach "$ref"
  local sha
  sha=$(git rev-parse --short HEAD)
  echo "Building the theme demo at $sha"

  mkdir -p "$home/releases"
  CARGO_TARGET_DIR="$home/target" CARGO_BUILD_JOBS=2 \
    nice -n 19 cargo build --release --locked -p theme-demo

  local release
  release="$home/releases/$(date -u +%Y%m%d%H%M%S)-$sha"
  mkdir -p "$release"
  cp "$home/target/release/theme-demo" "$release/theme-demo"

  # Atomic switch: a new symlink renamed over the old one.
  ln -sfn "$release" "$home/current.next"
  mv -Tf "$home/current.next" "$home/current"

  if pm2 describe navree-theme-demo >/dev/null 2>&1; then
    pm2 reload navree-theme-demo
  else
    pm2 start "$site/repo/deploy/navree.com/theme-demo.config.cjs"
  fi
  pm2 save

  # It answers before the release counts as done.
  for _ in $(seq 1 20); do
    if curl -fsS -o /dev/null http://127.0.0.1:8095/demo/ui/health; then
      echo "The theme demo serves $sha from $release"
      break
    fi
    sleep 0.5
  done
  curl -fsS -o /dev/null http://127.0.0.1:8095/demo/ui/health

  # Keep the newest releases only.
  ls -1dt "$home/releases"/*/ | tail -n +$((keep + 1)) | xargs -r rm -rf
}

main "$@"
