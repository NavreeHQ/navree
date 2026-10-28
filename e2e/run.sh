#!/usr/bin/env bash
# The evidence harness: brings up the compose stack, builds and starts the
# evidence app (web, worker, scheduler), runs the checks (HTTP smoke rows,
# browser screenshots, k6 scenarios) while sampling `docker stats`, collects
# the logs and writes everything to target/evidence/<run-id>/, then tears
# the stack down. See e2e/README.md.
#
#   e2e/run.sh                                  # smoke: every check, short k6 runs
#   e2e/run.sh --load-profile load --duration 2m --vus 50 --scenarios baseline
#   e2e/run.sh --no-build --keep                # reuse the image, leave the stack up
#   e2e/run.sh up | down                        # just the stack (for local runs)
#
# Exit status: 0 when every check passed, 1 otherwise.
set -uo pipefail
# Git Bash on Windows rewrites arguments that look like paths (/scripts/..)
# into Windows paths; container paths must stay as they are.
export MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL="*"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT/e2e/docker/compose.yml"
PROJECT="${E2E_PROJECT:-navree-e2e}"
IMAGE="${E2E_APP_IMAGE:-navree-evidence-app:local}"

LOAD_PROFILE="smoke"
DURATION=""
VUS=""
SCENARIOS="checks,baseline"
BUILD=1
KEEP=0
BROWSER=1
SERVICES_PROFILES="search storage nats"
RUN_ID=""
COMMAND="run"

usage() {
  sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
  cat <<'EOF'

Options:
  --load-profile smoke|load|spike   k6 profile for the scenarios (default smoke)
  --duration 2m                     sustained phase of the profile
  --vus 50                          virtual users of the profile
  --scenarios a,b                   k6 scenarios (e2e/load/scenarios/*.js; `checks` = HTTP rows)
  --services "search storage nats"  compose profiles to start besides the app
  --no-build                        use the existing image
  --no-browser                      skip the screenshots
  --keep                            leave the stack running afterwards
  --run-id ID                       name of the evidence folder (default: UTC time + profile)
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    up|down) COMMAND="$1" ;;
    --load-profile) LOAD_PROFILE="$2"; shift ;;
    --duration) DURATION="$2"; shift ;;
    --vus) VUS="$2"; shift ;;
    --scenarios) SCENARIOS="$2"; shift ;;
    --services) SERVICES_PROFILES="$2"; shift ;;
    --no-build) BUILD=0 ;;
    --no-browser) BROWSER=0 ;;
    --keep) KEEP=1 ;;
    --run-id) RUN_ID="$2"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 2 ;;
  esac
  shift
done

profile_args=()
for p in $SERVICES_PROFILES; do profile_args+=(--profile "$p"); done

compose() {
  docker compose -p "$PROJECT" -f "$COMPOSE_FILE" "${profile_args[@]}" --profile app --profile load --profile browser "$@"
}

SERVICES=(postgres redis mailpit)
for p in $SERVICES_PROFILES; do
  case "$p" in
    search) SERVICES+=(meilisearch) ;;
    typesense) SERVICES+=(typesense) ;;
    storage) SERVICES+=(minio minio-bucket) ;;
    nats) SERVICES+=(nats) ;;
    mysql) SERVICES+=(mysql) ;;
    mongo) SERVICES+=(mongo) ;;
  esac
done

if [ "$COMMAND" = "up" ]; then
  compose up -d --wait "${SERVICES[@]}"
  exit $?
fi
if [ "$COMMAND" = "down" ]; then
  compose down -v --remove-orphans
  exit $?
fi

RUN_ID="${RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)-$LOAD_PROFILE}"
OUT="$ROOT/target/evidence/$RUN_ID"
mkdir -p "$OUT/logs" "$OUT/k6" "$OUT/screenshots" "$OUT/resources"
# compose resolves this relative to e2e/docker/ (portable across Git Bash,
# WSL and Linux path styles).
export E2E_OUT_DIR="../../target/evidence/$RUN_ID"
export E2E_APP_IMAGE="$IMAGE"

CHECKS="$OUT/checks.jsonl"
: > "$CHECKS"
STARTED="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
FAILED=0

log() { printf '[%s] %s\n' "$(date -u +%H:%M:%S)" "$*" | tee -a "$OUT/run.log"; }

# record <name> <pass|fail|skip> <seconds> <detail>
record() {
  local detail="${4//\\/\\\\}"
  detail="${detail//\"/\\\"}"
  detail="$(printf '%s' "$detail" | tr '\n\r\t' '   ')"
  printf '{"name":"%s","status":"%s","seconds":%s,"detail":"%s"}\n' "$1" "$2" "$3" "$detail" >> "$CHECKS"
  [ "$2" = "fail" ] && FAILED=1
  log "$2  $1  (${3}s) $detail"
}

# step <name> <command...>: runs it, records pass or fail with its time.
step() {
  local name="$1"; shift
  local t0=$SECONDS
  local logfile="$OUT/logs/step-${name//[^a-zA-Z0-9]/-}.log"
  if "$@" > "$logfile" 2>&1; then
    record "$name" pass $((SECONDS - t0)) ""
    return 0
  fi
  record "$name" fail $((SECONDS - t0)) "$(tail -n 5 "$logfile")"
  return 1
}

# Samples `docker stats` for the project's containers every 2 s into
# resources/<label>.jsonl until stop_stats.
STATS_PID=""
start_stats() {
  local file="$OUT/resources/$1.jsonl"
  (
    while true; do
      docker stats --no-stream --format '{{json .}}' $(compose ps -q 2>/dev/null) 2>/dev/null \
        | sed "s/^{/{\"t\":\"$(date -u +%H:%M:%S)\",/" >> "$file"
      sleep 2
    done
  ) &
  STATS_PID=$!
}
stop_stats() {
  [ -n "$STATS_PID" ] && kill "$STATS_PID" 2>/dev/null
  wait "$STATS_PID" 2>/dev/null
  STATS_PID=""
}

collect_logs() {
  for svc in $(compose ps -a --services 2>/dev/null); do
    compose logs --no-color --timestamps "$svc" > "$OUT/logs/$svc.log" 2>&1
  done
}

teardown() {
  stop_stats
  collect_logs
  if [ "$KEEP" = 1 ]; then
    log "keeping the stack (--keep): e2e/run.sh down removes it"
  else
    compose down -v --remove-orphans >> "$OUT/run.log" 2>&1
  fi
}
trap 'teardown; exit 130' INT TERM

log "run $RUN_ID: profile=$LOAD_PROFILE scenarios=$SCENARIOS services=($SERVICES_PROFILES) out=$OUT"
git -C "$ROOT" rev-parse HEAD > "$OUT/commit.txt" 2>/dev/null
docker version --format '{{.Server.Version}}' > "$OUT/docker-version.txt" 2>/dev/null
docker info --format '{{.NCPU}} CPUs, {{.MemTotal}} bytes, kernel {{.KernelVersion}}' > "$OUT/host.txt" 2>/dev/null

# 1. The image.
if [ "$BUILD" = 1 ]; then
  step "image: build" docker build -f "$ROOT/e2e/docker/Dockerfile" -t "$IMAGE" "$ROOT" || { teardown; exit 1; }
fi
docker image inspect "$IMAGE" --format '{{.Size}}' > "$OUT/image-size.txt" 2>/dev/null \
  || { record "image: present" fail 0 "no image $IMAGE (run without --no-build)"; teardown; exit 1; }

# 2. The backing services.
step "stack: services healthy" compose up -d --wait "${SERVICES[@]}" || { teardown; exit 1; }

# 3. Schema and seed data (a one-off container from the app image).
step "app: migrate and seed" compose run --rm --no-deps web migrate:fresh --seed --force || { teardown; exit 1; }

# 4. The app: web, worker, scheduler.
step "app: web, worker and scheduler up" compose up -d --wait web worker scheduler || { teardown; exit 1; }
for svc in worker scheduler; do
  state="$(docker inspect -f '{{.State.Status}}' "$(compose ps -q "$svc")" 2>/dev/null)"
  if [ "$state" = "running" ]; then record "app: $svc running" pass 0 ""; else record "app: $svc running" fail 0 "state: $state"; fi
done

k6_env=(-e "PROFILE=$LOAD_PROFILE")
[ -n "$DURATION" ] && k6_env+=(-e "DURATION=$DURATION")
[ -n "$VUS" ] && k6_env+=(-e "VUS=$VUS")

# 5. k6: the HTTP rows and the scenarios, with resource sampling.
IFS=',' read -r -a scenario_list <<< "$SCENARIOS"
for scenario in "${scenario_list[@]}"; do
  if [ "$scenario" = "checks" ]; then
    script="/scripts/checks/http.js"; label="checks"; env=(-e PROFILE=smoke)
  else
    script="/scripts/scenarios/$scenario.js"; label="k6: $scenario ($LOAD_PROFILE)"; env=("${k6_env[@]}")
  fi
  start_stats "${scenario}-${LOAD_PROFILE}"
  step "$label" compose run --rm --no-deps k6 run --quiet "${env[@]}" "$script"
  stop_stats
done

# 6. Screenshots of the UI tools at 1440 and 390.
if [ "$BROWSER" = 1 ]; then
  step "browser: screenshots" compose run --rm --no-deps --build browser
fi

# 7. The app processes must have survived the run.
for svc in web worker scheduler; do
  restarts="$(docker inspect -f '{{.State.Status}} {{.RestartCount}} {{.State.OOMKilled}}' "$(compose ps -q "$svc")" 2>/dev/null)"
  case "$restarts" in
    "running 0 false") record "app: $svc survived" pass 0 "" ;;
    *) record "app: $svc survived" fail 0 "state: $restarts" ;;
  esac
done

FINISHED="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
teardown

# 8. summary.json (assembled in a node container: no host tools needed).
docker run --rm -v "$OUT:/out" -v "$ROOT/e2e/tools:/tools:ro" \
  -e RUN_ID="$RUN_ID" -e STARTED="$STARTED" -e FINISHED="$FINISHED" -e LOAD_PROFILE="$LOAD_PROFILE" \
  node:24-bookworm-slim node /tools/summarize.mjs /out > "$OUT/logs/summarize.log" 2>&1 \
  || log "summarize failed: see logs/summarize.log"

log "evidence: $OUT"
[ -f "$OUT/summary.md" ] && cat "$OUT/summary.md"
exit $FAILED
