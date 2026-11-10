#!/usr/bin/env bash
# Compiles the facade (`navree`) with the feature sets an app really uses, and
# lists the test targets a feature set leaves out.
#
#   scripts/facade-features.sh each            every feature on its own
#   scripts/facade-features.sh sets            the sets the kits, the tools and the docs name
#   scripts/facade-features.sh skipped FLAGS   the facade's test targets that the
#                                              feature flags leave out, for example
#                                              `skipped --no-default-features --features ui`
#   scripts/facade-features.sh one FLAGS       `cargo check` with those flags
#
# `each` and `sets` run every build, remember the ones that fail, and end with
# the list; the exit status is 1 if any failed. Nothing here runs a test.
set -uo pipefail

cd "$(dirname "$0")/.."

CHECK=(cargo check --locked --package navree)
failed=()

# Runs `cargo check` for a feature set; flags are cargo's own.
check() {
  local name="$1"
  shift
  echo "::group::navree: $name"
  if "${CHECK[@]}" "$@"; then
    echo "ok: $name"
  else
    echo "::error title=navree does not compile::$name"
    failed+=("$name")
  fi
  echo "::endgroup::"
}

# The facade's features, one per line, without `default`.
features() {
  python3 - <<'PY'
import json
import subprocess

meta = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--format-version", "1", "--no-deps",
     "--manifest-path", "crates/navree/Cargo.toml"]))
package = next(p for p in meta["packages"] if p["name"] == "navree")
for name in sorted(package["features"]):
    if name != "default":
        print(name)
PY
}

summary() {
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    {
      echo "### $1"
      if [ "${#failed[@]}" -eq 0 ]; then
        echo "Every build compiled."
      else
        echo "Did not compile:"
        for name in "${failed[@]}"; do echo "- $name"; done
      fi
    } >> "$GITHUB_STEP_SUMMARY"
  fi
  if [ "${#failed[@]}" -ne 0 ]; then
    echo "Did not compile:"
    printf '  %s\n' "${failed[@]}"
    exit 1
  fi
}

# Each feature alone, with the library and the test targets that feature set
# keeps (`--tests` builds them, so a test that names too few features fails
# here).
each() {
  check "no features" --no-default-features --tests
  while read -r feature; do
    check "--no-default-features --features $feature" \
      --no-default-features --features "$feature" --tests
  done < <(features)
  summary "Each feature on its own"
}

# The sets an app builds: the defaults with a driver, the toolkit alone, the
# kits' sets, the tools alone, the opt-in modules that carry test kits, the
# parts of documents, and everything.
sets() {
  check "default" --tests
  check "default + postgres" --features postgres --tests
  check "default + mysql" --features mysql --tests
  check "default + sqlite" --features sqlite --tests
  check "ui" --no-default-features --features ui --tests
  check "ui, orm, sqlite" --no-default-features --features ui,orm,sqlite --tests
  check "ui, validation-phone" --no-default-features --features ui,validation-phone --tests
  # navree-cli's kits (Kit::navree_features): saas and api, then micro.
  check "saas and api kits" --features billing,social,postgres --tests
  check "micro kit" --features micro,micro-nats,postgres --tests
  # The tools, each with only what it needs.
  for tool in fleet lighthouse helm logbook; do
    check "$tool alone" --no-default-features --features "$tool" --tests
  done
  # The modules that have test kits, with and without them.
  for module in oauth grpc mcp micro; do
    check "$module alone" --no-default-features --features "$module" --tests
    check "$module with testing" --no-default-features --features "$module,testing" --tests
  done
  for part in documents-native documents-chrome documents-docx documents; do
    check "$part alone" --no-default-features --features "$part" --tests
  done
  check "all features" --all-features --tests
  summary "The feature sets of apps, kits and tools"
}

# The facade's test targets that a feature set leaves out, from the features
# cargo resolves for it (the flags are `cargo metadata`'s).
skipped() {
  python3 - "$@" <<'PY'
import json
import os
import subprocess
import sys

meta = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--format-version", "1",
     "--manifest-path", "crates/navree/Cargo.toml", *sys.argv[1:]]))
package = next(p for p in meta["packages"] if p["name"] == "navree")
node = next(n for n in meta["resolve"]["nodes"] if n["id"] == package["id"])
enabled = set(node["features"])
left_out = []
for target in package["targets"]:
    if "test" not in target["kind"]:
        continue
    missing = [f for f in target.get("required-features") or [] if f not in enabled]
    if missing:
        left_out.append((target["name"], missing))
flags = " ".join(sys.argv[1:]) or "(the default features)"
lines = ["%d of the facade's test targets are left out with %s:" % (len(left_out), flags)]
for name, missing in sorted(left_out):
    lines.append("  %s (needs %s)" % (name, ", ".join(missing)))
print("\n".join(lines))
summary = os.environ.get("GITHUB_STEP_SUMMARY")
if summary:
    with open(summary, "a", encoding="utf-8") as out:
        out.write("#### Test targets left out with %s\n" % flags)
        for name, missing in sorted(left_out):
            out.write("- `%s`: needs %s\n" % (name, ", ".join(missing)))
PY
}

case "${1:-}" in
  each) each ;;
  sets) sets ;;
  skipped) shift; skipped "$@" ;;
  one) shift; check "$*" "$@"; summary "$*" ;;
  *)
    sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
    ;;
esac
