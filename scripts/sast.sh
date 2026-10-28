#!/usr/bin/env bash
# Static analysis for Navree: Semgrep (registry rulesets plus the repository's
# own rules in .semgrep/) and Gitleaks (the git history and the working tree).
# CI (.github/workflows/sast.yml) and a local run use this same script, so a
# run on your machine means what it means in CI.
#
#   scripts/sast.sh semgrep           scan the repository with Semgrep
#   scripts/sast.sh semgrep-test      check the custom rules against their fixtures
#   scripts/sast.sh gitleaks          scan the history, then the working tree
#   scripts/sast.sh all               semgrep-test, semgrep, gitleaks
#
# Needs Docker. Reports (SARIF and text) land in sast-reports/ (git-ignored);
# set SAST_REPORTS to put them elsewhere. Set SEMGREP_BASELINE to a commit to
# report only findings introduced since it (CI does this on pull requests).
#
# The images are pinned by digest so a new scanner release cannot change the
# result without a change here. To update a pin, pull the new tag, copy its
# digest from `docker image inspect --format '{{index .RepoDigests 0}}'`, and
# fix whatever the new version finds in the same change.
set -euo pipefail

SEMGREP_IMAGE="semgrep/semgrep@sha256:3db5c7af55eb1f512eac0abf0ab69a97b52c74b5e9a27314c221aee1a78d7ab2" # 1.181.1
GITLEAKS_IMAGE="zricethezav/gitleaks@sha256:c00b6bd0aeb3071cbcb79009cb16a60dd9e0a7c60e2be9ab65d25e6bc8abbb7f" # 8.30

# The registry rulesets (https://semgrep.dev/explore). Navree's own rules are
# .semgrep/*.yml.
REGISTRY_RULESETS=(p/rust p/typescript p/javascript p/react p/github-actions p/dockerfile)

# Bind mounts need host paths the Docker daemon understands (Git Bash on
# Windows needs Windows-style paths and must not rewrite the container side).
export MSYS_NO_PATHCONV=1
host_path() {
  if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi
}

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
reports="${SAST_REPORTS:-$root/sast-reports}"
mkdir -p "$reports"
src_mount="$(host_path "$root")"
reports_mount="$(host_path "$reports")"

semgrep_test() {
  echo "== semgrep: custom rules against their fixtures"
  # `semgrep --test` does not descend into a dot-directory, so it runs on a
  # copy outside it.
  docker run --rm -v "$src_mount:/src:ro" -w /src "$SEMGREP_IMAGE" sh -c \
    'cp -r .semgrep /tmp/semgrep-rules && semgrep --metrics=off --disable-version-check --test /tmp/semgrep-rules'
}

semgrep_scan() {
  echo "== semgrep: scanning the repository"
  local args=(--metrics=off --disable-version-check
    --config .semgrep --exclude .semgrep
    --severity ERROR --severity WARNING
    --error
    --sarif-output=/reports/semgrep.sarif
    --text-output=/reports/semgrep.txt)
  for ruleset in "${REGISTRY_RULESETS[@]}"; do args+=(--config "$ruleset"); done
  if [ -n "${SEMGREP_BASELINE:-}" ]; then
    echo "   diff-aware: only findings introduced since ${SEMGREP_BASELINE}"
    args+=(--baseline-commit "$SEMGREP_BASELINE")
  fi
  docker run --rm -v "$src_mount:/src" -v "$reports_mount:/reports" -w /src "$SEMGREP_IMAGE" \
    semgrep scan "${args[@]}" .
}

# Gitleaks reads history through git, which cannot follow a linked worktree's
# .git file from inside a container; scan a bare clone of it instead.
gitleaks_scan() {
  local repo="$root" tmp=""
  if [ -f "$root/.git" ]; then
    tmp="$(mktemp -d)"
    git clone --quiet --bare "$root" "$tmp/repo.git"
    repo="$tmp/repo.git"
  fi
  local repo_mount
  repo_mount="$(host_path "$repo")"

  local history=0 tree=0
  echo "== gitleaks: every commit on every branch"
  docker run --rm -v "$repo_mount:/repo" -v "$src_mount/.gitleaks.toml:/config/.gitleaks.toml:ro" \
    -v "$reports_mount:/reports" "$GITLEAKS_IMAGE" \
    git --config /config/.gitleaks.toml --redact --no-banner --exit-code 1 \
    --log-opts="--all" \
    --report-format sarif --report-path /reports/gitleaks-history.sarif /repo || history=$?

  echo "== gitleaks: the working tree"
  docker run --rm -v "$src_mount:/repo:ro" -v "$reports_mount:/reports" "$GITLEAKS_IMAGE" \
    dir --config /repo/.gitleaks.toml --redact --no-banner --exit-code 1 \
    --report-format sarif --report-path /reports/gitleaks-tree.sarif /repo || tree=$?

  [ -z "$tmp" ] || rm -rf "$tmp"
  return $((history | tree))
}

case "${1:-all}" in
  semgrep) semgrep_scan ;;
  semgrep-test) semgrep_test ;;
  gitleaks) gitleaks_scan ;;
  all)
    status=0
    semgrep_test || status=1
    semgrep_scan || status=1
    gitleaks_scan || status=1
    exit "$status"
    ;;
  *)
    echo "usage: scripts/sast.sh [semgrep|semgrep-test|gitleaks|all]" >&2
    exit 2
    ;;
esac
