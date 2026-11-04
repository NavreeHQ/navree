#!/usr/bin/env bash
# The servers the Rust suites test against, as containers on 127.0.0.1, for the
# Linux job of .github/workflows/rust-suites.yml and for the final gate on a
# developer machine. The containers are described in
# scripts/services/compose.yml; this script starts them, waits until each
# answers, sets up the data the suites expect and prints the variables the
# suites read. A run on your machine means what it means in CI.
#
#   scripts/ci-services.sh pull          pull every image
#   scripts/ci-services.sh start         start every server, wait until each
#                                        answers, and print the NAVREE_TEST_*
#                                        variables (also appended to
#                                        $GITHUB_ENV)
#   scripts/ci-services.sh env           print the variables only
#   scripts/ci-services.sh status        show which servers run (exit 1 unless
#                                        all do)
#   scripts/ci-services.sh stop          remove the containers and their volumes
#   scripts/ci-services.sh check-skips LOG
#                                        count the "skipped" notices in the
#                                        output of a test run (exit 1 if a
#                                        service test skipped itself)
#
# Needs Docker with Compose v2, curl and openssl, and free ports 1025-1027,
# 1465, 1883, 3306, 3307, 4222, 4443, 5432, 5433, 5672, 6333, 6379, 7700, 8025,
# 8026, 8108, 9000, 9092, 10000, 12111, 14000 and 27017 on 127.0.0.1. The
# containers are called navree-ci-<service>.
#
# Settings (environment variables):
#   NAVREE_CI_EXISTING_DATABASES=1  Do not start PostgreSQL, MySQL and MongoDB:
#                                   use the test databases already running on
#                                   this machine (containers navree-test-postgres
#                                   on 55432, navree-test-mysql on 53306 and
#                                   navree-test-mongodb on 45017). They are
#                                   never restarted, changed or removed by
#                                   this script.
#   NAVREE_CI_CONSUMER=wsl          The suites run in WSL while Docker runs on
#                                   Windows: print file paths (the Pebble CA)
#                                   as /mnt/c/... paths.
#   NAVREE_CI_SKIP="svc svc"        Leave these services out (names as in
#                                   compose.yml).
#   <NAME>_IMAGE                    Another image for a service (see
#                                   compose.yml), for example MARIADB_IMAGE.
#
# The throwaway credentials below only guard servers that live for one CI job
# on the loopback interface.
set -euo pipefail

# Git Bash on Windows rewrites arguments that look like POSIX paths (`/data`,
# `/CN=localhost`): leave them alone. Harmless elsewhere.
export MSYS_NO_PATHCONV=1
export MSYS2_ARG_CONV_EXCL='*'

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ALLOW_FILE="$ROOT/scripts/services/skips.allow"

# A path Docker Desktop and Compose understand (C:/Users/...) on Windows.
native_path() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -m "$1"
  else
    printf '%s' "$1"
  fi
}

COMPOSE_FILE="$(native_path "$ROOT/scripts/services/compose.yml")"

STATE="${NAVREE_CI_STATE:-$(native_path "$ROOT/target/ci-services")}"
export NAVREE_CI_STATE="$STATE"
# Where Pebble's root certificate is written (its HTTPS certificate is signed
# by it).
PEBBLE_CA="${PEBBLE_CA:-$STATE/pebble.minica.pem}"

PASSWORD=navree
SEARCH_KEY=navree-test-key-0123456789
S3_ACCESS_KEY=navreetest
S3_SECRET_KEY=navreetest-secret
S3_BUCKET=navree-test
GCS_BUCKET=navree-test
AZURE_CONTAINER=navree-test
# Azurite's published development account (Microsoft documents both).
AZURE_ACCOUNT=devstoreaccount1
AZURE_KEY='Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw=='
# The login of the Mailpit servers that ask for one.
SMTP_USER=navree
SMTP_PASSWORD=navree-secret
# Pulled by the Dock suite, which starts and removes its own containers (its
# Mailpit is the image of the mailpit service).
DOCK_IMAGES=(redis:7)

ALL_SERVICES=(
  postgres pgvector mysql mariadb mongodb redis s3 gcs azurite qdrant typesense
  meilisearch stripe-mock pebble nats mqtt kafka rabbitmq mailpit mailpit-auth
  mailpit-starttls mailpit-tls
)

# The three databases that already run on this machine.
POSTGRES_CONTAINER=navree-ci-postgres
MYSQL_CONTAINER=navree-ci-mysql
MONGODB_CONTAINER=navree-ci-mongodb
# The port mongod listens on inside its container.
MONGODB_PORT=27017
POSTGRES_URL="postgres://postgres:$PASSWORD@127.0.0.1:5432/navree_test"
MYSQL_URL="mysql://root:$PASSWORD@127.0.0.1:3306/navree_test"
MONGODB_URL='mongodb://127.0.0.1:27017/navree_test?replicaSet=rs0'
# Services the caller leaves out, and services that already run (checked, never
# started, changed or removed).
SKIP=" ${NAVREE_CI_SKIP:-} "
EXISTING=" "
if [ "${NAVREE_CI_EXISTING_DATABASES:-}" = 1 ]; then
  POSTGRES_CONTAINER=navree-test-postgres
  MYSQL_CONTAINER=navree-test-mysql
  MONGODB_CONTAINER=navree-test-mongodb
  MONGODB_PORT=45017
  POSTGRES_URL="postgres://postgres:$PASSWORD@127.0.0.1:55432/navree_test"
  MYSQL_URL="mysql://root:$PASSWORD@127.0.0.1:53306/navree_test"
  MONGODB_URL='mongodb://127.0.0.1:45017/navree_test?replicaSet=rs0'
  EXISTING=" postgres mysql mongodb "
fi

# True when this script starts the service.
managed() {
  case "$SKIP$EXISTING" in
    *" $1 "*) return 1 ;;
    *) return 0 ;;
  esac
}

# True when the service is left out by the caller.
left_out() {
  case "$SKIP" in
    *" $1 "*) return 0 ;;
    *) return 1 ;;
  esac
}

SELECTED=()
for service in "${ALL_SERVICES[@]}"; do
  if managed "$service"; then
    SELECTED+=("$service")
  fi
done

say() { printf '%s\n' "$*" >&2; }

compose() { docker compose --project-name navree-ci --file "$COMPOSE_FILE" "$@"; }

# A file path as the suites will see it.
consumer_path() {
  local path=$1
  if [ "${NAVREE_CI_CONSUMER:-}" = wsl ]; then
    printf '%s' "$path" | sed -E 's#^([A-Za-z]):/#/mnt/\L\1/#'
  else
    printf '%s' "$path"
  fi
}

# The variables the suites read.
variables() {
  cat <<EOF
NAVREE_TEST_REQUIRE_SERVICES=1
NAVREE_TEST_POSTGRES_URL=$POSTGRES_URL
NAVREE_TEST_PGVECTOR_URL=postgres://postgres:$PASSWORD@127.0.0.1:5433/navree_test
NAVREE_TEST_MYSQL_URL=$MYSQL_URL
NAVREE_TEST_MARIADB_URL=mariadb://root:$PASSWORD@127.0.0.1:3307/navree_test
NAVREE_TEST_MONGODB_URL=$MONGODB_URL
NAVREE_TEST_REDIS_URL=redis://127.0.0.1:6379
NAVREE_TEST_S3_URL=http://$S3_ACCESS_KEY:$S3_SECRET_KEY@127.0.0.1:9000/$S3_BUCKET
NAVREE_TEST_GCS_URL=http://127.0.0.1:4443/$GCS_BUCKET
NAVREE_TEST_AZURE_URL=http://127.0.0.1:10000/$AZURE_ACCOUNT/$AZURE_CONTAINER
NAVREE_TEST_QDRANT_URL=http://127.0.0.1:6333
NAVREE_TEST_TYPESENSE_URL=http://127.0.0.1:8108
NAVREE_TEST_TYPESENSE_KEY=$SEARCH_KEY
NAVREE_TEST_MEILISEARCH_URL=http://127.0.0.1:7700
NAVREE_TEST_MEILISEARCH_KEY=$SEARCH_KEY
NAVREE_TEST_STRIPE_MOCK_URL=http://127.0.0.1:12111
NAVREE_TEST_PEBBLE_URL=https://localhost:14000/dir
NAVREE_TEST_PEBBLE_CA=$(consumer_path "$PEBBLE_CA")
NAVREE_TEST_NATS_URL=nats://127.0.0.1:4222
NAVREE_TEST_MQTT_URL=mqtt://127.0.0.1:1883
NAVREE_TEST_KAFKA_URL=127.0.0.1:9092
NAVREE_TEST_RABBITMQ_URL=amqp://navree:$PASSWORD@127.0.0.1:5672/%2f
NAVREE_TEST_MAILPIT_URL=http://127.0.0.1:8025
NAVREE_TEST_SMTP_URL=smtp://127.0.0.1:1025
NAVREE_TEST_MAILPIT_AUTH_URL=http://127.0.0.1:8026
NAVREE_TEST_SMTP_AUTH_URL=smtp://$SMTP_USER:$SMTP_PASSWORD@127.0.0.1:1026
NAVREE_TEST_SMTP_STARTTLS_URL=smtp://$SMTP_USER:$SMTP_PASSWORD@localhost:1027?tls=required
NAVREE_TEST_SMTP_TLS_URL=smtps://$SMTP_USER:$SMTP_PASSWORD@localhost:1465
EOF
}

# `ready <service> <container> <command...>`: runs the command every two
# seconds (up to three minutes) until it succeeds; on a timeout shows the
# container's log.
ready() {
  local service=$1 container=$2
  shift 2
  if left_out "$service"; then
    return 0
  fi
  local _attempt
  for _attempt in $(seq 1 90); do
    if [ "$(docker inspect --format '{{.State.Running}}' "$container" 2>/dev/null)" != "true" ]; then
      break
    fi
    if "$@" >/dev/null 2>&1; then
      say "$service is ready"
      return 0
    fi
    sleep 2
  done
  say "$service did not start:"
  docker logs "$container" 2>&1 | tail -n 40 >&2
  return 1
}

tcp() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }

# Retries a pull that fails (a registry hiccup) twice more.
pull() {
  local _try image
  for _try in 1 2 3; do
    if compose pull --quiet "${SELECTED[@]}"; then
      break
    fi
    if [ "$_try" = 3 ]; then
      say "could not pull the images"
      return 1
    fi
    sleep 5
  done
  for image in "${DOCK_IMAGES[@]}"; do
    docker pull --quiet "$image" >/dev/null
  done
}

# A self-signed certificate for localhost and the password file of the Mailpit
# servers that ask for a login (in $STATE/mailpit, which the compose file
# mounts).
prepare_state() {
  local dir="$STATE/mailpit"
  mkdir -p "$dir"
  if [ ! -s "$dir/cert.pem" ] || [ ! -s "$dir/key.pem" ]; then
    openssl req -x509 -newkey rsa:2048 -nodes -days 30 \
      -keyout "$dir/key.pem" -out "$dir/cert.pem" -subj "/CN=localhost" \
      -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" >/dev/null 2>&1
  fi
  printf '%s:%s\n' "$SMTP_USER" "$SMTP_PASSWORD" >"$dir/auth.txt"
  chmod a+r "$dir/cert.pem" "$dir/key.pem" "$dir/auth.txt"
}

mongodb_is_primary() {
  [ "$(docker exec "$MONGODB_CONTAINER" mongosh --port "$MONGODB_PORT" --quiet --eval 'db.hello().isWritablePrimary')" = "true" ]
}

# mongod sometimes exits right after starting on a fresh runner: show why, and
# start it again (up to three times) instead of failing. Turns the server into
# the replica set rs0 once it answers.
start_mongodb() {
  local start _
  for start in 1 2 3; do
    for _ in $(seq 1 30); do
      if [ "$(docker inspect --format '{{.State.Running}}' "$MONGODB_CONTAINER" 2>/dev/null)" != "true" ]; then
        say "mongod exited on start $start (exit code $(docker inspect --format '{{.State.ExitCode}}' "$MONGODB_CONTAINER")):"
        docker logs "$MONGODB_CONTAINER" 2>&1 | tail -n 40 >&2
        break
      fi
      if docker exec "$MONGODB_CONTAINER" mongosh --quiet --eval 'db.runCommand({ ping: 1 }).ok' >/dev/null 2>&1; then
        # The member's address is the one the tests reach it by.
        docker exec "$MONGODB_CONTAINER" mongosh --quiet --eval \
          'rs.initiate({ _id: "rs0", members: [{ _id: 0, host: "127.0.0.1:27017" }] })' >/dev/null
        return 0
      fi
      sleep 2
    done
    compose up --detach --force-recreate mongodb >/dev/null
  done
  say "MongoDB did not start"
  return 1
}

# Creates the Azurite container `$AZURE_CONTAINER` with a request signed with
# the account's shared key. 201 is created, 409 is "exists already".
azurite_create_container() {
  local date_header version=2021-08-06 string_to_sign hexkey signature status
  date_header="$(LC_ALL=C date -u '+%a, %d %b %Y %H:%M:%S GMT')"
  string_to_sign="PUT\n\n\n\n\n\n\n\n\n\n\n\nx-ms-date:${date_header}\nx-ms-version:${version}\n/${AZURE_ACCOUNT}/${AZURE_ACCOUNT}/${AZURE_CONTAINER}\nrestype:container"
  hexkey="$(printf '%s' "$AZURE_KEY" | base64 -d | od -An -v -tx1 | tr -d ' \n')"
  signature="$(printf '%b' "$string_to_sign" | openssl dgst -sha256 -mac HMAC -macopt "hexkey:$hexkey" -binary | base64 | tr -d '\n')"
  status="$(curl --silent --output /dev/null --write-out '%{http_code}' --request PUT \
    "http://127.0.0.1:10000/${AZURE_ACCOUNT}/${AZURE_CONTAINER}?restype=container" \
    --header "x-ms-date: ${date_header}" --header "x-ms-version: ${version}" \
    --header "Content-Length: 0" \
    --header "Authorization: SharedKey ${AZURE_ACCOUNT}:${signature}")"
  [ "$status" = 201 ] || [ "$status" = 409 ]
}

# Creates the fake-gcs-server bucket (200 created, 409 exists already).
gcs_create_bucket() {
  local status
  status="$(curl --silent --output /dev/null --write-out '%{http_code}' --request POST \
    'http://127.0.0.1:4443/storage/v1/b?project=navree-test' \
    --header 'Content-Type: application/json' --data "{\"name\":\"$GCS_BUCKET\"}")"
  [ "$status" = 200 ] || [ "$status" = 409 ]
}

# The bucket the S3 suite names in its URL. (Signed with curl's own SigV4: the
# empty payload's hash goes in a header because older curl versions don't add
# it.)
s3_create_bucket() {
  curl --fail --silent --show-error --request PUT \
    --user "$S3_ACCESS_KEY:$S3_SECRET_KEY" --aws-sigv4 'aws:amz:us-east-1:s3' \
    --header 'x-amz-content-sha256: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855' \
    "http://127.0.0.1:9000/$S3_BUCKET"
}

start() {
  prepare_state
  # Start from nothing: leftovers from an earlier run keep old data and ports.
  stop
  pull
  compose up --detach "${SELECTED[@]}"

  if managed postgres; then
    ready postgres navree-ci-postgres docker exec navree-ci-postgres pg_isready --host 127.0.0.1 --username postgres --dbname navree_test
    # The image installs TimescaleDB in template1 and in the database it
    # creates, so every new database has it. The suites expect a fresh
    # database to have it available but not installed (they install it
    # themselves, and check that they do), as on a server set up from the
    # packages.
    local database
    for database in template1 navree_test; do
      ready postgres navree-ci-postgres docker exec navree-ci-postgres psql -X --username postgres --dbname "$database" \
        --command 'DROP EXTENSION IF EXISTS timescaledb'
    done
  else
    ready postgres "$POSTGRES_CONTAINER" docker exec "$POSTGRES_CONTAINER" pg_isready --host 127.0.0.1 --username postgres --dbname navree_test
  fi
  ready pgvector navree-ci-pgvector docker exec navree-ci-pgvector pg_isready --host 127.0.0.1 --username postgres --dbname navree_test
  ready mysql "$MYSQL_CONTAINER" docker exec "$MYSQL_CONTAINER" mysqladmin --host 127.0.0.1 --user root --password="$PASSWORD" ping
  ready mariadb navree-ci-mariadb docker exec navree-ci-mariadb mariadb-admin --host 127.0.0.1 --user root --password="$PASSWORD" ping
  if managed mongodb; then
    start_mongodb
  fi
  ready mongodb "$MONGODB_CONTAINER" mongodb_is_primary
  ready redis navree-ci-redis docker exec navree-ci-redis redis-cli ping
  ready s3 navree-ci-s3 s3_create_bucket
  ready gcs navree-ci-gcs gcs_create_bucket
  ready azurite navree-ci-azurite azurite_create_container
  ready qdrant navree-ci-qdrant curl --fail --silent http://127.0.0.1:6333/readyz
  ready typesense navree-ci-typesense curl --fail --silent http://127.0.0.1:8108/health
  ready meilisearch navree-ci-meilisearch curl --fail --silent http://127.0.0.1:7700/health
  ready stripe-mock navree-ci-stripe-mock curl --fail --silent --header 'Authorization: Bearer sk_test_123' http://127.0.0.1:12111/v1/customers
  # The root of Pebble's own HTTPS certificate (for localhost).
  if ! left_out pebble; then
    curl --fail --silent --show-error --location --output "$PEBBLE_CA" \
      https://raw.githubusercontent.com/letsencrypt/pebble/main/test/certs/pebble.minica.pem
  fi
  ready pebble navree-ci-pebble curl --fail --silent --cacert "$PEBBLE_CA" https://localhost:14000/dir
  ready nats navree-ci-nats tcp 4222
  ready mqtt navree-ci-mqtt tcp 1883
  ready kafka navree-ci-kafka docker exec navree-ci-kafka /opt/kafka/bin/kafka-broker-api-versions.sh --bootstrap-server 127.0.0.1:9092
  ready rabbitmq navree-ci-rabbitmq docker exec navree-ci-rabbitmq rabbitmq-diagnostics -q check_port_connectivity
  local mailpit
  for mailpit in mailpit mailpit-auth mailpit-starttls mailpit-tls; do
    ready "$mailpit" "navree-ci-$mailpit" docker exec "navree-ci-$mailpit" /mailpit readyz
  done

  variables
  if [ -n "${GITHUB_ENV:-}" ]; then
    variables >>"$GITHUB_ENV"
  fi
}

# Removes the containers of this script and their volumes. The test databases
# of this machine (navree-test-*) have other names and are never touched.
stop() {
  compose down --volumes --remove-orphans >/dev/null 2>&1 || true
  local leftovers
  leftovers="$(docker ps --all --quiet --filter 'name=^navree-ci-' 2>/dev/null || true)"
  if [ -n "$leftovers" ]; then
    # shellcheck disable=SC2086 # one container id per line
    docker rm --force --volumes $leftovers >/dev/null 2>&1 || true
  fi
  docker volume rm --force navree-ci-s3 >/dev/null 2>&1 || true
}

status() {
  local service container down=0
  for service in "${SELECTED[@]}"; do
    container="navree-ci-$service"
    if [ "$(docker inspect --format '{{.State.Running}}' "$container" 2>/dev/null)" = "true" ]; then
      printf '%-18s running\n' "$service"
    else
      printf '%-18s NOT RUNNING\n' "$service"
      down=1
    fi
  done
  local pair
  for pair in "$POSTGRES_CONTAINER" "$MYSQL_CONTAINER" "$MONGODB_CONTAINER"; do
    case "$pair" in
      navree-test-*)
        if [ "$(docker inspect --format '{{.State.Running}}' "$pair" 2>/dev/null)" = "true" ]; then
          printf '%-18s running (existing)\n' "${pair#navree-test-}"
        else
          printf '%-18s NOT RUNNING (existing)\n' "${pair#navree-test-}"
          down=1
        fi
        ;;
    esac
  done
  return "$down"
}

# The notices a service test prints when it skips itself, in the output of a
# test run made with `-- --show-output` (cargo hides the output of tests that
# pass). A run with NAVREE_TEST_REQUIRE_SERVICES=1 must have none: each
# service test fails instead. The by-design skips (a test written for one
# database on a run for another, a tool the test machine cannot have) are
# listed with their reason in scripts/services/skips.allow.
check_skips() {
  local log=${1:-}
  if [ -z "$log" ] || [ ! -f "$log" ]; then
    say "usage: $0 check-skips LOG (the output of a cargo test run made with -- --show-output)"
    return 2
  fi
  local notices allowed=0 unexpected=0
  notices="$(grep -nE '(SKIPPED|skipped[: ]|skipping )' "$log" | grep -vE '^[0-9]+:test ' || true)"
  local patterns
  patterns="$(grep -vE '^\s*(#|$)' "$ALLOW_FILE" | sed -E 's/\s+#.*$//' || true)"
  local line
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    if [ -n "$patterns" ] && printf '%s\n' "$line" | grep -qE -f <(printf '%s\n' "$patterns"); then
      allowed=$((allowed + 1))
    else
      unexpected=$((unexpected + 1))
      printf '%s\n' "$line"
    fi
  done <<<"$notices"
  say "skip notices: $unexpected unexpected, $allowed allowed (scripts/services/skips.allow)"
  [ "$unexpected" = 0 ]
}

case "${1:-}" in
  pull) pull ;;
  start) start ;;
  env) variables ;;
  status) status ;;
  stop) stop ;;
  check-skips) check_skips "${2:-}" ;;
  *)
    say "usage: $0 pull|start|env|status|stop|check-skips LOG"
    exit 2
    ;;
esac
