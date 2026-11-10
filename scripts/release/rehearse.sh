#!/usr/bin/env bash
# A release rehearsal on local registries: publishes every crate and the
# @navree/* npm packages to registries that run in Docker on this machine,
# then builds what users will build from them.
#
#   scripts/release/rehearse.sh [command] [arguments]
#
# Commands (the default is `all`):
#   up        Starts the registries: Kellnr (cargo) as navree-rehearsal-cargo
#             and Verdaccio (npm) as navree-rehearsal-npm.
#   env       Prints the `export` lines that point Cargo at the cargo
#             registry, for `eval "$(scripts/release/rehearse.sh env)"`.
#   crates [flags]
#             `cargo xtask release publish --registry navree-rehearsal`, with
#             any of its flags (`--package`, `--limit`, `--skip-verify`...).
#             Run it again after a stop: it resumes.
#   npm       Builds the npm packages and publishes the public ones.
#   apps      Installs the CLI from the cargo registry, creates an app with
#             `navree new` for the minimal app and each kit (saas, api,
#             micro), installs the skills into one, and builds them all, the
#             front ends included, against the two registries. Also builds
#             the reference app in CRM_DIR and the skeleton in SKELETON_DIR,
#             when set. Without Node.js it builds the Rust side and says so.
#   web       Builds the front ends of those apps (the SaaS kit's and the
#             reference app's) against the npm registry: the part of `apps`
#             that needs Node.js, for a machine whose Rust and Node.js are
#             in different places (WSL and Windows).
#   index <crate>...
#             Prints each crate's file in the cargo registry's index.
#   down      Stops and removes the two containers (nothing else).
#   all [flags]
#             up, crates (with the flags), npm, apps; then down, also after
#             a failure.
#
# Settings (environment):
#   REHEARSAL_DIR         state, apps and the CLI (default target/rehearsal)
#   REHEARSAL_CARGO_PORT  the cargo registry's port (default 18300)
#   REHEARSAL_NPM_PORT    the npm registry's port (default 18301)
#   REHEARSAL_TARGET_DIR  the build folder the apps share
#                         (default $REHEARSAL_DIR/target)
#   CRM_DIR, SKELETON_DIR the reference app and the skeleton (optional)
#   DOCKER                the docker command (default docker, or docker.exe
#                         from WSL)
#
# Only the two containers named above are started, stopped or removed, and
# only when they carry this script's label. Nothing is written outside
# REHEARSAL_DIR apart from the build output and Cargo's own caches.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REHEARSAL_DIR="${REHEARSAL_DIR:-$ROOT/target/rehearsal}"
CARGO_PORT="${REHEARSAL_CARGO_PORT:-18300}"
NPM_PORT="${REHEARSAL_NPM_PORT:-18301}"
HOST=127.0.0.1
CARGO_CONTAINER=navree-rehearsal-cargo
NPM_CONTAINER=navree-rehearsal-npm
LABEL=com.navree.rehearsal=1
KELLNR_IMAGE="ghcr.io/kellnr/kellnr:7.0.0"
VERDACCIO_IMAGE="verdaccio/verdaccio:6.2.1"
REGISTRY=navree-rehearsal
CARGO_URL="http://$HOST:$CARGO_PORT"
NPM_URL="http://$HOST:$NPM_PORT/"

# The first docker client that reaches a daemon: in WSL without Docker of
# its own, Docker Desktop's docker.exe.
if [ -z "${DOCKER:-}" ]; then
  DOCKER=docker
  for client in docker docker.exe; do
    if command -v "$client" > /dev/null && "$client" version --format '{{.Server.Version}}' > /dev/null 2>&1; then
      DOCKER="$client"
      break
    fi
  done
fi

say() { printf '\n== %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

mkdir -p "$REHEARSAL_DIR"

# The workspace version: the CLI and every crate share it.
version() {
  sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1
}

token() {
  local file="$REHEARSAL_DIR/cargo-token"
  if [ ! -s "$file" ]; then
    od -An -N24 -tx1 /dev/urandom | tr -d ' \n' > "$file"
  fi
  cat "$file"
}

cargo_env() {
  export CARGO_REGISTRIES_NAVREE_REHEARSAL_INDEX="sparse+$CARGO_URL/api/v1/crates/"
  export CARGO_REGISTRIES_NAVREE_REHEARSAL_TOKEN
  CARGO_REGISTRIES_NAVREE_REHEARSAL_TOKEN="$(token)"
}

ours() {
  [ "$("$DOCKER" inspect --format '{{index .Config.Labels "com.navree.rehearsal"}}' "$1" 2> /dev/null)" = 1 ]
}

exists() {
  "$DOCKER" inspect "$1" > /dev/null 2>&1
}

wait_for() {
  local url="$1" what="$2"
  for _ in $(seq 1 90); do
    if curl --silent --fail --max-time 5 "$url" > /dev/null; then
      return 0
    fi
    sleep 2
  done
  die "$what did not answer at $url"
}

up() {
  for name in "$CARGO_CONTAINER" "$NPM_CONTAINER"; do
    if exists "$name" && ! ours "$name"; then
      die "a container named $name exists and was not started by this script"
    fi
  done

  if ! exists "$CARGO_CONTAINER"; then
    say "starting the cargo registry ($KELLNR_IMAGE) on port $CARGO_PORT"
    "$DOCKER" run --detach --name "$CARGO_CONTAINER" --label "$LABEL" \
      --publish "$HOST:$CARGO_PORT:8000" \
      --env KELLNR_ORIGIN__HOSTNAME="$HOST" \
      --env KELLNR_ORIGIN__PORT="$CARGO_PORT" \
      --env KELLNR_SETUP__ADMIN_TOKEN="$(token)" \
      --env KELLNR_REGISTRY__MAX_CRATE_SIZE=20 \
      "$KELLNR_IMAGE" > /dev/null
  fi

  if ! exists "$NPM_CONTAINER"; then
    say "starting the npm registry ($VERDACCIO_IMAGE) on port $NPM_PORT"
    local conf="$REHEARSAL_DIR/verdaccio"
    mkdir -p "$conf"
    cat > "$conf/config.yaml" << 'YAML'
storage: /verdaccio/storage/data
auth:
  htpasswd:
    file: /verdaccio/storage/htpasswd
    max_users: 10
uplinks:
  npmjs:
    url: https://registry.npmjs.org/
packages:
  "@navree/*":
    access: $all
    publish: $authenticated
  "**":
    access: $all
    proxy: npmjs
max_body_size: 50mb
log: { type: stdout, format: pretty, level: warn }
YAML
    "$DOCKER" create --name "$NPM_CONTAINER" --label "$LABEL" \
      --publish "$HOST:$NPM_PORT:4873" "$VERDACCIO_IMAGE" > /dev/null
    # Through a tar stream, so the path works for docker.exe from WSL too.
    tar -C "$conf" -cf - config.yaml | "$DOCKER" cp - "$NPM_CONTAINER:/verdaccio/conf/"
    "$DOCKER" start "$NPM_CONTAINER" > /dev/null
  fi

  wait_for "$CARGO_URL/api/v1/crates/config.json" "the cargo registry"
  wait_for "${NPM_URL}-/ping" "the npm registry"

  if [ ! -s "$REHEARSAL_DIR/npmrc" ]; then
    local password npm_token
    password="$(od -An -N12 -tx1 /dev/urandom | tr -d ' \n')"
    npm_token="$(curl --silent --fail -X PUT "${NPM_URL}-/user/org.couchdb.user:rehearsal" \
      -H 'content-type: application/json' \
      -d "{\"name\":\"rehearsal\",\"password\":\"$password\"}" \
      | sed -n 's/.*"token": *"\([^"]*\)".*/\1/p')"
    [ -n "$npm_token" ] || die "could not create the npm registry's user"
    printf 'registry=%s\n//%s:%s/:_authToken=%s\n' "$NPM_URL" "$HOST" "$NPM_PORT" "$npm_token" \
      > "$REHEARSAL_DIR/npmrc"
  fi
  say "registries up: cargo $CARGO_URL, npm $NPM_URL"
}

down() {
  for name in "$CARGO_CONTAINER" "$NPM_CONTAINER"; do
    if exists "$name"; then
      if ours "$name"; then
        "$DOCKER" rm --force "$name" > /dev/null
        say "removed $name"
      else
        say "left $name: it was not started by this script"
      fi
    fi
  done
  rm -f "$REHEARSAL_DIR/npmrc" "$REHEARSAL_DIR/cargo-token"
  # Its metadata describes the registry that was just removed.
  rm -rf "$REHEARSAL_DIR/pnpm-cache"
}

# Cargo publishes a path dependency as a dependency on crates.io unless it
# names a registry. While the crates are published here, every Navree entry
# of [workspace.dependencies] names this one; the manifest is put back when
# the publish ends, however it ends. (The publish runs with --allow-dirty for
# that reason.)
MANIFEST="$ROOT/Cargo.toml"
MANIFEST_BACKUP="$REHEARSAL_DIR/Cargo.toml.orig"

restore_manifest() {
  if [ -f "$MANIFEST_BACKUP" ]; then
    cp "$MANIFEST_BACKUP" "$MANIFEST"
    rm -f "$MANIFEST_BACKUP"
  fi
}

publish_crates() {
  cargo_env
  restore_manifest
  cp "$MANIFEST" "$MANIFEST_BACKUP"
  sed -i -E "/^navree[-a-z0-9]* = \{ path = \"crates\//s/^(navree[-a-z0-9]*) = \{ /\1 = { registry = \"$REGISTRY\", /" "$MANIFEST"
  say "publishing the crates to $REGISTRY"
  local status=0
  (cd "$ROOT" && cargo xtask release publish --registry "$REGISTRY" --allow-dirty "$@") || status=$?
  restore_manifest
  return "$status"
}

# pnpm keeps package metadata per registry host and port, and trusts it for
# an exact version: after an earlier rehearsal on the same port, `pnpm -r
# publish` takes every package for published ("There are no new packages
# that should be published") and installs read the old registry's
# metadata. The rehearsal's pnpm commands get a cache of their own.
pnpm_cache() {
  export pnpm_config_cache_dir="$REHEARSAL_DIR/pnpm-cache"
}

# The public packages of packages/js, one `name version` line each.
npm_packages() {
  local file
  for file in "$ROOT"/packages/js/*/package.json; do
    grep -q '"private": *true' "$file" && continue
    printf '%s %s\n' \
      "$(sed -n 's/^  "name": *"\([^"]*\)".*/\1/p' "$file")" \
      "$(sed -n 's/^  "version": *"\([^"]*\)".*/\1/p' "$file")"
  done
}

publish_npm() {
  [ -s "$REHEARSAL_DIR/npmrc" ] || die "run up first"
  pnpm_cache
  say "publishing the npm packages to $NPM_URL"
  (
    cd "$ROOT/packages/js"
    pnpm install --frozen-lockfile
    pnpm build
    npm_config_userconfig="$REHEARSAL_DIR/npmrc" \
      pnpm -r publish --registry "$NPM_URL" --no-git-checks
  )
  # pnpm succeeds when it publishes nothing, so ask the registry.
  local name version missing=0
  while read -r name version; do
    if curl --silent --fail "$NPM_URL${name/\//%2f}/$version" > /dev/null; then
      say "$name $version is on the npm registry"
    else
      printf 'error: %s %s is not on the npm registry\n' "$name" "$version" >&2
      missing=1
    fi
  done < <(npm_packages)
  [ "$missing" = 0 ] || die "the npm publish left packages out"
}

index() {
  for name in "$@"; do
    local lower="${name,,}" path
    case "${#lower}" in
      1) path="1/$lower" ;;
      2) path="2/$lower" ;;
      3) path="3/${lower:0:1}/$lower" ;;
      *) path="${lower:0:2}/${lower:2:2}/$lower" ;;
    esac
    printf '%s: ' "$name"
    curl --silent --fail "$CARGO_URL/api/v1/crates/$path" || printf '(not in the index)'
    echo
  done
}

# A Cargo configuration that takes every published crate from the cargo
# registry instead of crates.io.
patch_config() {
  local version="$1"
  printf '[patch.crates-io]\n'
  (cd "$ROOT" && cargo xtask release crates) | while read -r name _; do
    printf '%s = { version = "=%s", registry = "%s" }\n' "$name" "$version" "$REGISTRY"
  done
}

# Whether a package.json depends on a @navree/* package (a key such as
# "@navree/client": "^1.0.0"); a package that only carries the scope in its
# name, such as a vendored copy, does not.
uses_navree_packages() {
  grep -q '"@navree/[^"]*" *: *"' "$1"
}

point_at_registries() {
  local dir="$1" version="$2"
  mkdir -p "$dir/.cargo"
  patch_config "$version" >> "$dir/.cargo/config.toml"
  # Front ends: the @navree scope from the npm registry.
  find "$dir" -name package.json -not -path '*/node_modules/*' | while read -r file; do
    if uses_navree_packages "$file"; then
      printf '@navree:registry=%s\n' "$NPM_URL" >> "$(dirname "$file")/.npmrc"
    fi
  done
}

build_rust() {
  local dir="$1"
  say "building $dir"
  (cd "$dir" && cargo build && cargo test --no-run)
}

has_node() {
  command -v node > /dev/null && node --version > /dev/null 2>&1 && command -v pnpm > /dev/null
}

# The front ends of the rehearsal's apps and of the reference app: the
# packages that use the @navree/* packages and have a build script.
front_ends() {
  local saas="$REHEARSAL_DIR/apps/rehearsal-saas/web"
  [ -d "$saas" ] && echo "$saas"
  if [ -n "${CRM_DIR:-}" ]; then
    find "$CRM_DIR" -name package.json -not -path '*/node_modules/*' | while read -r file; do
      if uses_navree_packages "$file" && grep -q '"build" *:' "$file"; then
        dirname "$file"
      fi
    done
  fi
}

web() {
  has_node || die "Node.js and pnpm are needed to build the front ends"
  pnpm_cache
  local dir found=0
  while read -r dir; do
    [ -n "$dir" ] || continue
    found=1
    say "building the front end in $dir"
    (cd "$dir" && pnpm install --no-frozen-lockfile && pnpm build)
  done < <(front_ends)
  [ "$found" = 1 ] || die "no front end found: run apps first"
}

apps() {
  cargo_env
  local version cli apps
  version="$(version)"
  cli="$REHEARSAL_DIR/cli"
  apps="$REHEARSAL_DIR/apps"
  export CARGO_TARGET_DIR="${REHEARSAL_TARGET_DIR:-$REHEARSAL_DIR/target}"

  say "installing navree-cli $version from $REGISTRY"
  cargo install navree-cli --version "=$version" --registry "$REGISTRY" --locked --root "$cli" --force
  local navree="$cli/bin/navree"

  rm -rf "$apps"
  mkdir -p "$apps"
  local kit
  for kit in minimal saas api micro; do
    local name="rehearsal-$kit"
    say "navree new $name ($kit)"
    if [ "$kit" = minimal ]; then
      (cd "$apps" && "$navree" new "$name")
    else
      (cd "$apps" && "$navree" new "$name" --kit "$kit")
    fi
    grep -q "navree = .*\"$version\"" "$apps/$name/Cargo.toml" \
      || die "$name does not depend on navree $version from the registry"
    point_at_registries "$apps/$name" "$version"
  done

  say "navree skills:install"
  (cd "$apps/rehearsal-minimal" && "$navree" skills:install)
  [ -f "$apps/rehearsal-minimal/.agents/skills/navree/SKILL.md" ] \
    || die "skills:install installed no skills: Navigator was published without them"

  for kit in minimal saas api micro; do
    build_rust "$apps/rehearsal-$kit"
  done

  if [ -n "${CRM_DIR:-}" ]; then
    point_at_registries "$CRM_DIR" "$version"
    build_rust "$CRM_DIR"
  else
    say "CRM_DIR is not set: the reference app is not built"
  fi
  if [ -n "${SKELETON_DIR:-}" ]; then
    point_at_registries "$SKELETON_DIR" "$version"
    build_rust "$SKELETON_DIR"
  else
    say "SKELETON_DIR is not set: the skeleton is not built"
  fi

  if has_node; then
    web
  else
    say "NOT BUILT: the front ends (no Node.js here). Run \`scripts/release/rehearse.sh web\` where Node.js and pnpm are, with the same REHEARSAL_DIR and CRM_DIR."
    return 0
  fi
  say "every app built against the registries"
}

command="${1:-all}"
[ $# -gt 0 ] && shift
# An interrupted publish still puts the manifest back.
trap 'exit 130' INT TERM
trap restore_manifest EXIT
case "$command" in
  up) up ;;
  env)
    cargo_env
    printf 'export CARGO_REGISTRIES_NAVREE_REHEARSAL_INDEX=%q\n' "$CARGO_REGISTRIES_NAVREE_REHEARSAL_INDEX"
    printf 'export CARGO_REGISTRIES_NAVREE_REHEARSAL_TOKEN=%q\n' "$CARGO_REGISTRIES_NAVREE_REHEARSAL_TOKEN"
    ;;
  crates) publish_crates "$@" ;;
  npm) publish_npm ;;
  apps) apps ;;
  web) web ;;
  index) index "$@" ;;
  down) down ;;
  all)
    trap 'restore_manifest; down' EXIT
    up
    publish_crates "$@"
    publish_npm
    apps
    ;;
  *) die "unknown command $command (up, env, crates, npm, apps, web, index, down, all)" ;;
esac
