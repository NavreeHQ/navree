#!/usr/bin/env bash
# Static analysis for Navree: Semgrep (registry rulesets plus the repository's
# own rules in .semgrep/) and Gitleaks (the git history and the working tree).
# CI (.github/workflows/sast.yml) and a local run use this same script, so a
# run on your machine means what it means in CI.
#
#   scripts/sast.sh semgrep-test      check the custom rules against their fixtures
#   scripts/sast.sh semgrep           scan the repository with Semgrep
#   scripts/sast.sh secrets           scan the git history, then the working tree
#   scripts/sast.sh secrets-history   the git history only (every branch)
#   scripts/sast.sh secrets-tree      the working tree only
#   scripts/sast.sh all               semgrep-test, semgrep, secrets
#   scripts/sast.sh suppressed-warnings
#                                     each finding the last Semgrep scan saw suppressed in
#                                     the source, as a GitHub warning annotation (needs
#                                     Python; never fails)
#
# Needs Docker. Every command exits non-zero when it finds something, but
# suppressed-warnings, which only reports.
#
# Reports (SARIF and text) land in sast-reports/ (git-ignored); set
# SAST_REPORTS to put them elsewhere. Other knobs:
#   SEMGREP_BASELINE=<commit>   report only Semgrep findings introduced since it
#                               (CI does this on pull requests)
#   SAST_CUSTOM_ONLY=1          run only the rules in .semgrep/, skipping the
#                               registry rulesets (fast, and works offline)
#   GITLEAKS_LOG_OPTS=<range>   which commits to scan, as `git log` options;
#                               default is --all (CI narrows it on pull requests)
#
# The images are pinned by digest so a new scanner release cannot change the
# result without a change here. To update a pin, pull the new tag, copy its
# digest from `docker image inspect --format '{{index .RepoDigests 0}}'`, and
# fix whatever the new version finds in the same change.
set -euo pipefail

SEMGREP_IMAGE="semgrep/semgrep@sha256:3db5c7af55eb1f512eac0abf0ab69a97b52c74b5e9a27314c221aee1a78d7ab2" # 1.181.1
GITLEAKS_IMAGE="zricethezav/gitleaks@sha256:c00b6bd0aeb3071cbcb79009cb16a60dd9e0a7c60e2be9ab65d25e6bc8abbb7f" # 8.30

# The registry rulesets (https://semgrep.dev/explore). Navree's own rules are
# in .semgrep/.
REGISTRY_RULESETS=(p/rust p/typescript p/javascript p/react p/github-actions p/dockerfile)

# The repository is owned by the CI user, not the container's root, and git
# refuses to read such a repository unless it is told it is safe.
GIT_SAFE=(-e GIT_CONFIG_COUNT=1 -e GIT_CONFIG_KEY_0=safe.directory -e "GIT_CONFIG_VALUE_0=*")

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
  # Warnings and errors fail the run; INFO-level registry findings (for
  # example "this reads the temp directory") are advice, not defects.
  local args=(--metrics=off --disable-version-check
    --config .semgrep --exclude .semgrep
    --severity ERROR --severity WARNING
    --error
    --sarif-output=/reports/semgrep.sarif
    --text-output=/reports/semgrep.txt)
  if [ -z "${SAST_CUSTOM_ONLY:-}" ]; then
    for ruleset in "${REGISTRY_RULESETS[@]}"; do args+=(--config "$ruleset"); done
  fi
  if [ -n "${SEMGREP_BASELINE:-}" ]; then
    echo "   diff-aware: only findings introduced since ${SEMGREP_BASELINE}"
    args+=(--baseline-commit "$SEMGREP_BASELINE")
  fi
  local status=0
  docker run --rm "${GIT_SAFE[@]}" -v "$src_mount:/src" -v "$reports_mount:/reports" -w /src \
    "$SEMGREP_IMAGE" semgrep scan "${args[@]}" . || status=$?
  if ! drop_suppressed "$reports/semgrep.sarif" && [ "$status" -eq 0 ]; then
    status=1
  fi
  return "$status"
}

# Semgrep writes a finding silenced by a `nosemgrep` comment into its SARIF
# report too, marked with an in-source suppression. GitHub code scanning does
# not read that mark and would show each one as an open alert, so the report
# for code scanning leaves them out. The whole report stays beside it, as
# semgrep-with-suppressed.sarif, for reviewing the suppressions.
#
# A result is left out only when one of its suppressions is in the source and
# accepted (SARIF's `status` absent or "accepted"); a run without results is
# left as it is. The report is replaced only once the filtered copy is whole.
# This runs where `set -e` may be off (`semgrep_scan || status=1`), so every
# step is checked by hand.
drop_suppressed() {
  local sarif="$1"
  [ -f "$sarif" ] || return 0
  local full="${sarif%.sarif}-with-suppressed.sarif"
  local filtered="$sarif.tmp"
  # The container may own the report; a copy is ours to replace it with.
  if ! cp "$sarif" "$full"; then
    echo "   could not copy $sarif; it keeps its suppressed findings" >&2
    return 1
  fi
  # The first Python that runs (the Windows Store's python3 stand-in does not).
  local python=""
  for candidate in python3 python; do
    if "$candidate" -c "import json" >/dev/null 2>&1; then
      python="$candidate"
      break
    fi
  done
  if [ -z "$python" ]; then
    echo "   no Python found: $sarif keeps its suppressed findings" >&2
    return 0
  fi
  if ! "$python" -I -c '
import json, sys

def accepted_in_source(result):
    return any(
        s.get("kind") == "inSource" and s.get("status", "accepted") == "accepted"
        for s in result.get("suppressions") or []
    )

with open(sys.argv[1], encoding="utf-8") as f:
    report = json.load(f)
for run in report.get("runs", []):
    if "results" in run:
        run["results"] = [r for r in run["results"] if not accepted_in_source(r)]
with open(sys.argv[2], "w", encoding="utf-8") as f:
    json.dump(report, f)
' "$(host_path "$full")" "$(host_path "$filtered")"; then
    rm -f "$filtered"
    echo "   could not filter $sarif; it keeps its suppressed findings" >&2
    return 1
  fi
  if ! mv -f "$filtered" "$sarif"; then
    rm -f "$filtered"
    echo "   could not replace $sarif; it keeps its suppressed findings" >&2
    return 1
  fi
  echo "   $sarif: findings suppressed in the source left out; all of them in $full"
}

# Gitleaks reads history through git, which cannot follow a linked checkout's
# .git file from inside a container; scan a bare clone of it instead.
# $1 is "history", "tree" or "both".
gitleaks_scan() {
  local what="${1:-both}" history=0 tree=0

  if [ "$what" != tree ]; then
    local repo="$root" tmp=""
    if [ -f "$root/.git" ]; then
      tmp="$(mktemp -d)"
      git clone --quiet --bare "$src_mount" "$(host_path "$tmp")/repo.git"
      repo="$tmp/repo.git"
    fi
    echo "== gitleaks: the git history (${GITLEAKS_LOG_OPTS:---all})"
    docker run --rm "${GIT_SAFE[@]}" -v "$(host_path "$repo"):/repo" \
      -v "$src_mount/.gitleaks.toml:/config/.gitleaks.toml:ro" -v "$reports_mount:/reports" \
      "$GITLEAKS_IMAGE" \
      git --config /config/.gitleaks.toml --redact --no-banner --verbose --exit-code 1 \
      --log-opts="${GITLEAKS_LOG_OPTS:---all}" \
      --report-format sarif --report-path /reports/gitleaks-history.sarif /repo \
      2>&1 | tee "$reports/gitleaks-history.txt" || history=$?
    [ -z "$tmp" ] || rm -rf "$tmp"
  fi

  if [ "$what" != history ]; then
    echo "== gitleaks: the working tree"
    docker run --rm -v "$src_mount:/repo:ro" -v "$reports_mount:/reports" "$GITLEAKS_IMAGE" \
      dir --config /repo/.gitleaks.toml --redact --no-banner --verbose --exit-code 1 \
      --report-format sarif --report-path /reports/gitleaks-tree.sarif /repo \
      2>&1 | tee "$reports/gitleaks-tree.txt" || tree=$?
  fi

  return $((history | tree))
}

# A finding suppressed in the source never reaches code scanning (see
# drop_suppressed), so a pull request that adds a suppression would hide its
# finding silently. This reads the findings Semgrep marked as suppressed in
# the source from the last scan's whole report (on a pull request that scan
# is diff-aware, so these are the ones the change introduces), whatever
# spelling silenced them (`nosemgrep`, `nosem`, on the line or the line
# before), and prints each as a warning annotation with its rule, file and
# line, for the reviewer. It never fails.
suppressed_warnings() {
  local full="$reports/semgrep-with-suppressed.sarif"
  if [ ! -f "$full" ]; then
    echo "   no $full: run scripts/sast.sh semgrep first"
    return 0
  fi
  local python=""
  for candidate in python3 python; do
    if "$candidate" -c "import json" >/dev/null 2>&1; then
      python="$candidate"
      break
    fi
  done
  if [ -z "$python" ]; then
    echo "   no Python found: the suppressed findings are not listed" >&2
    return 0
  fi
  "$python" -I -c '
import json, sys

def prop(text):
    for old, new in (("%", "%25"), ("\r", "%0D"), ("\n", "%0A"), (":", "%3A"), (",", "%2C")):
        text = text.replace(old, new)
    return text

with open(sys.argv[1], encoding="utf-8") as f:
    report = json.load(f)
count = 0
for run in report.get("runs", []):
    for result in run.get("results", []):
        if not any(s.get("kind") == "inSource" for s in result.get("suppressions") or []):
            continue
        rule = str(result.get("ruleId", "a rule"))
        for location in result.get("locations", [])[:1]:
            physical = location.get("physicalLocation", {})
            path = str(physical.get("artifactLocation", {}).get("uri", ""))
            line = int(physical.get("region", {}).get("startLine", 1))
            text = rule.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
            message = text + " is suppressed in the source here; check that the reason beside it holds."
            print("::warning file={},line={},title=Semgrep finding suppressed::{}".format(prop(path), line, message))
            count += 1
print("{} finding(s) suppressed in the source".format(count))
' "$(host_path "$full")" || true
}

case "${1:-all}" in
  semgrep) semgrep_scan ;;
  semgrep-test) semgrep_test ;;
  suppressed-warnings) suppressed_warnings ;;
  secrets | gitleaks) gitleaks_scan both ;;
  secrets-history) gitleaks_scan history ;;
  secrets-tree) gitleaks_scan tree ;;
  all)
    status=0
    semgrep_test || status=1
    semgrep_scan || status=1
    gitleaks_scan both || status=1
    exit "$status"
    ;;
  *)
    echo "usage: scripts/sast.sh [semgrep-test|semgrep|secrets|secrets-history|secrets-tree|all|suppressed-warnings]" >&2
    exit 2
    ;;
esac
