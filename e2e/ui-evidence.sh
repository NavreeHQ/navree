#!/usr/bin/env bash
# The UI evidence run, sourced by run.sh for `e2e/run.sh ui`. It builds the
# stack like a production deployment, fills every tool with data through the
# app's real paths (k6 traffic, queue jobs, mail), photographs every page of
# every UI tool at 1440 and 390 pixels wide before any traffic, under load
# and after it, photographs the website's mock-ups for comparison, and builds
# the gallery:
#
#   <evidence>/index.html      the gallery
#   <evidence>/shots/          the screenshots and their results.json per phase
#   <evidence>/data/           k6 digests, resource samples, container logs, facts
#
# The folder is <E2E_EVIDENCE_ROOT, default target/evidence>/ui-<date>-<commit>
# (or --evidence-dir). Screenshots are not part of the repository.
#
# Settings (environment): E2E_BUILD_JOBS limits the image build's compiler
# jobs; E2E_EVIDENCE_ROOT moves the folder; E2E_SITE_URL is the website the
# mock-ups are photographed from (default https://navree.com).

# Sets OUT, E2E_OUT_DIR and the evidence folder before run.sh's helpers need them.
ui_paths() {
  local commit short root
  commit="$(git -C "$HOST_ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
  short="${commit:0:8}"
  root="${E2E_EVIDENCE_ROOT:-$ROOT/target/evidence}"
  EVIDENCE="${EVIDENCE_DIR:-$root/ui-$(date -u +%Y-%m-%d)-$short}"
  OUT="$EVIDENCE/data"
  RUN_ID="ui-$short"
  mkdir -p "$EVIDENCE/shots" "$OUT/logs" "$OUT/k6" "$OUT/resources" "$OUT/facts"
  EVIDENCE_HOST="$(cd "$EVIDENCE" && (pwd -W 2>/dev/null || pwd))"
  # Absolute, so compose mounts the folder wherever it is.
  export E2E_OUT_DIR="$EVIDENCE_HOST"
}

# k6_run <script> <base url> [k6 -e flags...]: one scenario of e2e/load/scenarios.
k6_run() {
  local script="$1" base="$2"; shift 2
  compose run --rm --no-deps -T -e BASE_URL="$base" -e K6_SUMMARY_DIR=/out/data/k6 \
    k6 run --quiet "$@" "/scripts/scenarios/$script.js"
}

# ui_crawl <phase> <mode> <service>: the screenshot crawler against one service.
ui_crawl() {
  local phase="$1" mode="$2" svc="$3"
  local runner=browser base="http://localhost:8080"
  case "$svc" in
    web-dev) runner=browser-dev ;;
    mailpit) base="http://mailpit:8025" ;;
  esac
  compose run --rm --no-deps -T -e OUT_DIR=/out/shots -e PHASE="$phase" -e MODE="$mode" \
    -e SERVICE="$svc" -e BASE_URL="$base" "$runner"
}

# ui_website: the website's mock-ups, photographed from the live site.
ui_website() {
  compose run --rm --no-deps -T -e OUT_DIR=/out/shots -e SITE_URL="${E2E_SITE_URL:-https://navree.com}" \
    browser /runner/src/website.mjs
}

# ui_cli: Harbor and Navigator as terminals. The `navree` binary comes from
# its own image (e2e/docker/Dockerfile.cli, built when it is missing); the
# captured text is drawn as images for the gallery.
ui_cli() {
  if ! docker image inspect navree-cli:local > /dev/null 2>&1; then
    local args=()
    [ -n "${E2E_BUILD_JOBS:-}" ] && args+=(--build-arg "CARGO_BUILD_JOBS=$E2E_BUILD_JOBS")
    docker build -f "$HOST_ROOT/e2e/docker/Dockerfile.cli" ${args[@]+"${args[@]}"} -t navree-cli:local "$HOST_ROOT" || return 1
  fi
  mkdir -p "$EVIDENCE/shots/cli"
  docker run --rm -v "$EVIDENCE_HOST/shots/cli:/captures" -v "$HOST_ROOT/e2e/tools/cli-captures.sh:/captures.sh:ro"     navree-cli:local sh /captures.sh || return 1
  compose run --rm --no-deps -T -e IN_DIR=/out/shots/cli -e OUT_DIR=/out/shots/cli browser /runner/src/terminal.mjs
}

# ui_crm: the example CRM (crm-api on SQLite, crm-web), seeded with its demo
# data and walked with the demo administrator's account. Its two images are
# built when missing.
ui_crm() {
  local args=()
  [ -n "${E2E_BUILD_JOBS:-}" ] && args+=(--build-arg "CARGO_BUILD_JOBS=$E2E_BUILD_JOBS")
  if ! docker image inspect navree-crm-api:local > /dev/null 2>&1; then
    docker build -f "$HOST_ROOT/e2e/docker/Dockerfile.crm-api" ${args[@]+"${args[@]}"} -t navree-crm-api:local "$HOST_ROOT" || return 1
  fi
  if ! docker image inspect navree-crm-web:local > /dev/null 2>&1; then
    docker build -f "$HOST_ROOT/e2e/docker/Dockerfile.crm-web" -t navree-crm-web:local "$HOST_ROOT/apps/crm-web" || return 1
  fi
  compose up -d crm-web || return 1
  compose run --rm --no-deps crm-api migrate:fresh --seed --force || return 1
  compose up -d crm-api || return 1
  local i
  for i in $(seq 1 60); do
    curl -fsS -o /dev/null "http://localhost:${E2E_CRM_PORT:-47030}/login" && break
    sleep 2
  done
  compose run --rm --no-deps -T -e OUT_DIR=/out/shots -e PHASE=after -e MODE=crawl -e SERVICE=crm \
    -e BASE_URL=http://localhost:3000 -e EMAIL=admin@navree.test -e PASSWORD=password123 browser-crm
}

# background <name> <command...>: runs a step in the background and
# remembers it, so wait_background can collect it.
BG_PIDS=()
background() {
  local name="$1"; shift
  ( step "$name" "$@" ) &
  BG_PIDS+=($!)
}
wait_background() {
  local pid
  for pid in ${BG_PIDS[@]+"${BG_PIDS[@]}"}; do wait "$pid" 2>/dev/null; done
  BG_PIDS=()
}

# ui_sql: a SQL file piped into the app's database.
ui_sql() {
  compose exec -T postgres psql -v ON_ERROR_STOP=1 -U navree -d evidence < "$1"
}

# ui_snapshot: what the tools' own APIs and the catchers report at this point
# (written to data/facts/<label>.json), as proof of what the data is.
ui_snapshot() {
  local label="$1"
  compose exec -T web sh -c '
    token=$(curl -fsS -X POST http://127.0.0.1:8080/api/auth/login -H "Content-Type: application/json" \
      -d "{\"email\":\"operator@evidence.test\",\"password\":\"evidence-password\"}" | sed -n "s/.*\"access_token\":\"\([^\"]*\)\".*/\1/p")
    h="Authorization: Bearer $token"
    printf "{\"lighthouse_overview_24h\":"; curl -fsS -H "$h" "http://127.0.0.1:8080/_lighthouse/api/overview?period=24h" || printf null
    printf ",\"lighthouse_open_issues\":"; curl -fsS -H "$h" "http://127.0.0.1:8080/_lighthouse/api/issues?status=open&per_page=100" || printf null
    printf ",\"lighthouse_resolved_issues\":"; curl -fsS -H "$h" "http://127.0.0.1:8080/_lighthouse/api/issues?status=resolved&per_page=100" || printf null
    printf ",\"lighthouse_ignored_issues\":"; curl -fsS -H "$h" "http://127.0.0.1:8080/_lighthouse/api/issues?status=ignored&per_page=100" || printf null
    printf ",\"fleet_masters\":"; curl -fsS -H "$h" "http://127.0.0.1:8080/_fleet/api/masters" || printf null
    printf ",\"fleet_metrics\":"; curl -fsS -H "$h" "http://127.0.0.1:8080/_fleet/api/metrics?period=hour" || printf null
    printf ",\"mailpit\":"; curl -fsS "http://mailpit:8025/api/v1/info" || printf null
    printf "}"
  ' > "$OUT/facts/$label.json" 2> "$OUT/logs/snapshot-$label.log"
}

# ui_facts: versions and sizes of what ran, for meta.json.
ui_facts() {
  local f="$OUT/facts"
  docker version --format '{{json .}}' > "$f/docker-version.json" 2>/dev/null
  docker info --format '{{json .}}' > "$f/docker-info.json" 2>/dev/null
  compose ps -a --format json > "$f/compose-ps.jsonl" 2>/dev/null
  git -C "$HOST_ROOT" rev-parse HEAD > "$f/commit.txt" 2>/dev/null
  git -C "$HOST_ROOT" log -1 --format='%an|%ad|%s' > "$f/commit-info.txt" 2>/dev/null
  git -C "$HOST_ROOT" rev-parse --abbrev-ref HEAD > "$f/branch.txt" 2>/dev/null
  : > "$f/images.jsonl"
  for img in $(compose config --images 2>/dev/null | sort -u); do
    docker image inspect "$img" --format '{"image":"{{index .RepoTags 0}}","id":"{{.Id}}","size":{{.Size}},"created":"{{.Created}}"}' >> "$f/images.jsonl" 2>/dev/null
  done
  docker run --rm --entrypoint sh "$IMAGE" -c 'ls -l /usr/local/bin/evidence-app; /usr/local/bin/evidence-app --version 2>&1 | head -2' > "$f/app-binary.txt" 2>&1
  compose exec -T postgres psql -U navree -d evidence -Atc 'select version(); select extversion from pg_extension where extname = $$timescaledb$$;' > "$f/postgres-version.txt" 2>&1
  compose exec -T redis redis-server --version > "$f/redis-version.txt" 2>&1
  compose exec -T postgres psql -U navree -d evidence -At -F ' ' -c "select table_name, (xpath('/row/c/text()', query_to_xml('select count(*) as c from '||table_name, false, true, '')))[1]::text from information_schema.tables where table_schema='public' and table_type='BASE TABLE' order by 1" > "$f/table-counts-$1.txt" 2>&1
}

# run_ui: the whole run.
run_ui() {
  local load_vus="${VUS:-50}" load_duration="${DURATION:-10m}" ui_vus="${UI_VUS:-6}"
  local started
  started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  export E2E_APP_IMAGE="$IMAGE"
  log "ui evidence: $EVIDENCE (load ${load_vus} VUs for ${load_duration}, ui traffic ${ui_vus} VUs)"
  cp "$ROOT/e2e/browser/tools.json" "$EVIDENCE/tools.json"
  docker info --format '{{.NCPU}} CPUs, {{.MemTotal}} bytes, kernel {{.KernelVersion}}' > "$OUT/host.txt" 2>/dev/null

  # 1. The image: a release build, as it is deployed.
  if [ "$BUILD" = 1 ]; then
    step "image: build" build_image || { teardown; return 1; }
  fi
  docker image inspect "$IMAGE" --format '{{.Size}}' > "$OUT/image-size.txt" 2>/dev/null \
    || { record "image: present" fail 0 "no image $IMAGE (run without --no-build)"; teardown; return 1; }

  # 2. The backing services, a fresh schema and the seeded data.
  step "stack: services healthy" compose up -d --wait "${SERVICES[@]}" || { teardown; return 1; }
  step "app: migrate and seed" compose run --rm --no-deps web migrate:fresh --seed --force || { teardown; return 1; }
  step "app: web, web-dev, worker and scheduler up" compose up -d --wait web web-dev worker scheduler || { teardown; return 1; }
  step "browser: image" compose build browser || { teardown; return 1; }
  # Lighthouse records the migration and the seeder like any other work; a
  # tool before any traffic should start from nothing.
  step "lighthouse: forget what migrate and seed recorded" ui_sql "$ROOT/e2e/sql/lighthouse-reset.sql"
  ui_facts before

  # 3. Before any traffic: the first page of each tool, empty.
  step "browser: empty tools (web)" ui_crawl empty roots web
  step "browser: empty tools (web-dev)" ui_crawl empty roots web-dev
  step "browser: empty mail catcher" ui_crawl empty roots mailpit

  # 4. Fill the tools through the real paths: a mix of reads, writes, failures,
  #    slow calls, jobs and mail; Logbook's process gets its own traffic.
  start_stats fill
  background "k6: ui traffic on web-dev (for Logbook)" k6_run ui http://web-dev:8080 \
    -e PROFILE=load -e VUS=3 -e DURATION=3m -e BURST=0 -e POOL=10 -e SUMMARY_NAME=ui-web-dev
  step "k6: ui traffic (fill)" k6_run ui http://web:8080 -e PROFILE=load -e VUS=8 -e DURATION=4m -e SUMMARY_NAME=ui-fill
  wait_background
  step "lighthouse: triage issues (resolve, ignore)" k6_run lighthouse-lifecycle http://web:8080 -e STAGE=triage -e SUMMARY_NAME=lighthouse-triage
  # The same failures again: what was resolved comes back as a regression.
  step "k6: ui traffic (regressions)" k6_run ui http://web:8080 -e PROFILE=load -e VUS=8 -e DURATION=90s -e BURST=0 -e SUMMARY_NAME=ui-regressions
  stop_stats
  if [ -f "$ROOT/e2e/sql/lighthouse-history.sql" ]; then
    step "lighthouse: back-dated history (copies of the recorded minutes)" ui_sql "$ROOT/e2e/sql/lighthouse-history.sql"
  fi
  ui_snapshot filled

  # 5. Under load: the baseline mix at the profile's users and the failing,
  #    slow and queued traffic next to it, while the browser walks every tool.
  start_stats load
  local t_load=$SECONDS
  background "k6: load (baseline ${load_vus} VUs + ui traffic ${ui_vus} VUs)" k6_run ui-load http://web:8080 \
    -e PROFILE=load -e VUS="$load_vus" -e UI_VUS="$ui_vus" -e DURATION="$load_duration" -e BURST=0 -e SUMMARY_NAME=ui-load
  background "k6: load on web-dev (for Logbook)" k6_run ui http://web-dev:8080 \
    -e PROFILE=load -e VUS=3 -e DURATION="$load_duration" -e BURST=0 -e POOL=10 -e SUMMARY_NAME=ui-load-web-dev
  # The ramp to full load takes 30 s; start walking when it has.
  sleep 45
  local t_walk=$SECONDS
  background "browser: every page, under load (web)" ui_crawl load crawl web
  background "browser: every page, under load (web-dev)" ui_crawl load crawl web-dev
  wait_background
  log "the walk under load took $((SECONDS - t_walk)) s of the $((SECONDS - t_load)) s the load ran so far (load duration $load_duration)"
  echo "{\"load_vus\":$load_vus,\"ui_vus\":$ui_vus,\"duration\":\"$load_duration\",\"walk_seconds\":$((SECONDS - t_walk))}" > "$OUT/facts/load-phase.json"
  ui_snapshot under-load
  stop_stats

  # 6. After the load: let the queues drain, triage more issues now that
  #    nothing reopens them, then walk everything again.
  sleep 30
  step "lighthouse: triage issues after the load" k6_run lighthouse-lifecycle http://web:8080 -e STAGE=settle -e SUMMARY_NAME=lighthouse-settle
  ui_snapshot after
  step "browser: every page, after the load (web)" ui_crawl after crawl web
  step "browser: every page, after the load (web-dev)" ui_crawl after crawl web-dev
  step "browser: mail catcher" ui_crawl after crawl mailpit
  ui_facts after

  # 7. What the website shows.
  if [ "$WEBSITE" = 1 ]; then
    step "browser: website mock-ups" ui_website
  fi

  # 8. The example CRM, when asked for (--crm).
  if [ "$CRM" = 1 ]; then
    step "crm: every screen of the example CRM" ui_crm
  fi

  # 8b. The command line tools, as terminals.
  step "cli: Harbor and Navigator terminal captures" ui_cli

  # 9. The app processes must have survived.
  local svc restarts
  for svc in web web-dev worker scheduler; do
    restarts="$(docker inspect -f '{{.State.Status}} {{.RestartCount}} {{.State.OOMKilled}}' "$(compose ps -q "$svc")" 2>/dev/null)"
    case "$restarts" in
      "running 0 false") record "app: $svc survived" pass 0 "" ;;
      *) record "app: $svc survived" fail 0 "state: $restarts" ;;
    esac
  done

  local finished
  finished="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "{\"started\":\"$started\",\"finished\":\"$finished\",\"load_vus\":$load_vus,\"ui_vus\":$ui_vus,\"load_duration\":\"$load_duration\"}" > "$OUT/facts/run.json"
  teardown

  # 10. The gallery.
  docker run --rm -v "$EVIDENCE_HOST:/e" -v "$HOST_ROOT/e2e/tools:/tools:ro" node:24-bookworm-slim \
    sh -c 'node /tools/ui-meta.mjs /e && node /tools/gallery.mjs /e' > "$OUT/logs/gallery.log" 2>&1 \
    || log "gallery failed: see data/logs/gallery.log"
  log "gallery: $EVIDENCE/index.html"
  [ "$(grep -c '"status":"fail"' "$CHECKS" 2>/dev/null)" = 0 ]
}
