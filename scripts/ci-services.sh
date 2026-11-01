#!/usr/bin/env bash
# The servers the Rust suites test against, as containers on 127.0.0.1, for the
# Linux job of .github/workflows/rust-suites.yml. A local run uses the same
# script, so a run on your machine means what it means in CI.
#
#   scripts/ci-services.sh pull    pull every image (several at a time)
#   scripts/ci-services.sh start   start every server, wait until each answers,
#                                  and print the NAVREE_TEST_* variables the
#                                  suites read (also appended to $GITHUB_ENV)
#   scripts/ci-services.sh env     print the variables only
#   scripts/ci-services.sh stop    remove the containers and their volumes
#
# Needs Docker, and free ports 1883, 3306, 4222, 5432, 5433, 5672, 6333, 6379,
# 7700, 8108, 9000, 9092, 12111, 14000 and 27017 on 127.0.0.1. The containers
# are called navree-ci-<service>.
#
# The throwaway credentials below only guard servers that live for one CI job
# on the loopback interface.
set -euo pipefail

PASSWORD=navree
SEARCH_KEY=navree-test-key-0123456789
S3_ACCESS_KEY=navreetest
S3_SECRET_KEY=navreetest-secret
S3_BUCKET=navree-test
# Where Pebble's root certificate is written (its HTTPS certificate is signed
# by it).
PEBBLE_CA="${PEBBLE_CA:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/pebble.minica.pem}"

# PostgreSQL 17 with TimescaleDB, and a PostgreSQL 17 with pgvector (the
# vector-store suite of navree-ai connects to a server of its own).
POSTGRES_IMAGE=timescale/timescaledb:2.31.0-pg17
PGVECTOR_IMAGE=pgvector/pgvector:pg17
MYSQL_IMAGE=mysql:8.4
# MongoDB refuses to start on Linux kernels 6.19 through 7.0.13 (a TCMalloc
# incompatibility): the job runs on an image with an older kernel.
MONGODB_IMAGE=mongo:8.0
REDIS_IMAGE=redis:8-alpine
S3_IMAGE=rustfs/rustfs:1.0.4
QDRANT_IMAGE=qdrant/qdrant:v1.20.0
TYPESENSE_IMAGE=typesense/typesense:30.1
MEILISEARCH_IMAGE=getmeili/meilisearch:v1.55
STRIPE_MOCK_IMAGE=stripe/stripe-mock:v0.206.0
PEBBLE_IMAGE=ghcr.io/letsencrypt/pebble:latest
NATS_IMAGE=nats:2.14-alpine
MQTT_IMAGE=eclipse-mosquitto:2
KAFKA_IMAGE=apache/kafka:4.0.0
RABBITMQ_IMAGE=rabbitmq:4-alpine
# Pulled by the Dock suite, which starts and removes its own containers.
DOCK_IMAGES=(redis:7 axllent/mailpit:v1.31)

IMAGES=(
  "$POSTGRES_IMAGE" "$PGVECTOR_IMAGE" "$MYSQL_IMAGE" "$MONGODB_IMAGE" "$REDIS_IMAGE"
  "$S3_IMAGE" "$QDRANT_IMAGE" "$TYPESENSE_IMAGE" "$MEILISEARCH_IMAGE"
  "$STRIPE_MOCK_IMAGE" "$PEBBLE_IMAGE" "$NATS_IMAGE" "$MQTT_IMAGE" "$KAFKA_IMAGE"
  "$RABBITMQ_IMAGE" "${DOCK_IMAGES[@]}"
)

SERVICES=(
  postgres pgvector mysql mongodb redis s3 qdrant typesense meilisearch
  stripe-mock pebble nats mqtt kafka rabbitmq
)

say() { printf '%s\n' "$*" >&2; }

# The variables the suites read.
variables() {
  cat <<EOF
NAVREE_TEST_POSTGRES_URL=postgres://postgres:$PASSWORD@127.0.0.1:5432/navree_test
NAVREE_TEST_PGVECTOR_URL=postgres://postgres:$PASSWORD@127.0.0.1:5433/navree_test
NAVREE_TEST_MYSQL_URL=mysql://root:$PASSWORD@127.0.0.1:3306/navree_test
NAVREE_TEST_MONGODB_URL=mongodb://127.0.0.1:27017/navree_test?replicaSet=rs0
NAVREE_TEST_REDIS_URL=redis://127.0.0.1:6379
NAVREE_TEST_S3_URL=http://$S3_ACCESS_KEY:$S3_SECRET_KEY@127.0.0.1:9000/$S3_BUCKET
NAVREE_TEST_QDRANT_URL=http://127.0.0.1:6333
NAVREE_TEST_TYPESENSE_URL=http://127.0.0.1:8108
NAVREE_TEST_TYPESENSE_KEY=$SEARCH_KEY
NAVREE_TEST_MEILISEARCH_URL=http://127.0.0.1:7700
NAVREE_TEST_MEILISEARCH_KEY=$SEARCH_KEY
NAVREE_TEST_STRIPE_MOCK_URL=http://127.0.0.1:12111
NAVREE_TEST_PEBBLE_URL=https://localhost:14000/dir
NAVREE_TEST_PEBBLE_CA=$PEBBLE_CA
NAVREE_TEST_NATS_URL=nats://127.0.0.1:4222
NAVREE_TEST_MQTT_URL=mqtt://127.0.0.1:1883
NAVREE_TEST_KAFKA_URL=127.0.0.1:9092
NAVREE_TEST_RABBITMQ_URL=amqp://navree:$PASSWORD@127.0.0.1:5672/%2f
EOF
}

# `container <service> <docker run arguments...>`: a detached container named
# navree-ci-<service>, replacing an earlier one.
container() {
  local name="navree-ci-$1"
  shift
  docker rm --force --volumes "$name" >/dev/null 2>&1 || true
  docker run --detach --name "$name" "$@" >/dev/null
}

# `ready <service> <command...>`: runs the command every two seconds (up to
# two minutes) until it succeeds; on a timeout shows the container's log.
ready() {
  local service=$1
  shift
  local _attempt
  for _attempt in $(seq 1 60); do
    if [ "$(docker inspect --format '{{.State.Running}}' "navree-ci-$service")" != "true" ]; then
      break
    fi
    if "$@" >/dev/null 2>&1; then
      say "$service is ready"
      return 0
    fi
    sleep 2
  done
  say "$service did not start:"
  docker logs "navree-ci-$service" 2>&1 | tail -n 40 >&2
  return 1
}

tcp() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }

pull_one() {
  # A pull that fails once (a registry hiccup) is tried twice more.
  local _try
  for _try in 1 2 3; do
    if docker pull --quiet "$1" >/dev/null; then
      return 0
    fi
    sleep 5
  done
  say "could not pull $1"
  return 1
}

pull() {
  export -f pull_one say
  # shellcheck disable=SC2016 # `$1` belongs to the child shell
  printf '%s\n' "${IMAGES[@]}" | xargs -P 6 -I{} bash -c 'pull_one "$1"' _ {}
}

start_mongodb() {
  # mongod sometimes exits right after starting on a fresh runner: show why,
  # and start it again (up to three times) instead of failing.
  local start
  for start in 1 2 3; do
    container mongodb --publish 127.0.0.1:27017:27017 "$MONGODB_IMAGE" \
      --replSet rs0 --bind_ip_all --wiredTigerCacheSizeGB 0.25
    for _ in $(seq 1 30); do
      if [ "$(docker inspect --format '{{.State.Running}}' navree-ci-mongodb)" != "true" ]; then
        say "mongod exited on start $start (exit code $(docker inspect --format '{{.State.ExitCode}}' navree-ci-mongodb)):"
        docker logs navree-ci-mongodb 2>&1 | tail -n 40 >&2
        break
      fi
      if docker exec navree-ci-mongodb mongosh --quiet --eval 'db.runCommand({ ping: 1 }).ok' >/dev/null 2>&1; then
        # The member's address is the one the tests reach it by.
        docker exec navree-ci-mongodb mongosh --quiet --eval \
          'rs.initiate({ _id: "rs0", members: [{ _id: 0, host: "127.0.0.1:27017" }] })' >/dev/null
        return 0
      fi
      sleep 2
    done
  done
  say "MongoDB did not start"
  return 1
}

mongodb_is_primary() {
  [ "$(docker exec navree-ci-mongodb mongosh --quiet --eval 'db.hello().isWritablePrimary')" = "true" ]
}

start() {
  pull

  # Throwaway data: crash safety traded for speed, as on a developer machine.
  container postgres --publish 127.0.0.1:5432:5432 \
    --env POSTGRES_PASSWORD="$PASSWORD" --env POSTGRES_DB=navree_test \
    "$POSTGRES_IMAGE" \
    -c max_connections=200 -c shared_buffers=256MB -c max_worker_processes=16 \
    -c timescaledb.max_background_workers=8 -c timescaledb.telemetry_level=off \
    -c fsync=off -c synchronous_commit=off -c full_page_writes=off
  container pgvector --publish 127.0.0.1:5433:5432 \
    --env POSTGRES_PASSWORD="$PASSWORD" --env POSTGRES_DB=navree_test \
    "$PGVECTOR_IMAGE" -c fsync=off -c synchronous_commit=off -c full_page_writes=off
  container mysql --publish 127.0.0.1:3306:3306 \
    --env MYSQL_ROOT_PASSWORD="$PASSWORD" --env MYSQL_DATABASE=navree_test \
    "$MYSQL_IMAGE" \
    --character-set-server=utf8mb4 --collation-server=utf8mb4_0900_ai_ci \
    --max_connections=300 --local_infile=OFF --skip-log-bin \
    --innodb_buffer_pool_size=256M --innodb_flush_log_at_trx_commit=2
  start_mongodb
  container redis --publish 127.0.0.1:6379:6379 "$REDIS_IMAGE"
  container s3 --publish 127.0.0.1:9000:9000 \
    --env RUSTFS_ACCESS_KEY="$S3_ACCESS_KEY" --env RUSTFS_SECRET_KEY="$S3_SECRET_KEY" \
    --volume navree-ci-s3:/data "$S3_IMAGE"
  container qdrant --publish 127.0.0.1:6333:6333 "$QDRANT_IMAGE"
  container typesense --publish 127.0.0.1:8108:8108 --tmpfs /data "$TYPESENSE_IMAGE" \
    --data-dir /data --api-key="$SEARCH_KEY" --listen-address 0.0.0.0
  container meilisearch --publish 127.0.0.1:7700:7700 \
    --env MEILI_MASTER_KEY="$SEARCH_KEY" --env MEILI_ENV=development \
    --env MEILI_NO_ANALYTICS=true "$MEILISEARCH_IMAGE"
  container stripe-mock --publish 127.0.0.1:12111:12111 "$STRIPE_MOCK_IMAGE"
  # PEBBLE_VA_ALWAYS_VALID=1: Pebble does not fetch the challenge answers (the
  # suite's domains don't resolve to this machine); the answers' route is
  # covered by the crate's other tests.
  container pebble --publish 127.0.0.1:14000:14000 \
    --env PEBBLE_VA_ALWAYS_VALID=1 --env PEBBLE_VA_NOSLEEP=1 --env PEBBLE_WFE_NONCEREJECT=0 \
    "$PEBBLE_IMAGE"
  container nats --publish 127.0.0.1:4222:4222 "$NATS_IMAGE" --jetstream
  # Mosquitto 2 answers only the loopback interface of its own container
  # unless a listener is configured.
  local mqtt_conf
  mqtt_conf="$(mktemp -d)"
  printf 'listener 1883\nallow_anonymous true\n' >"$mqtt_conf/mosquitto.conf"
  chmod -R a+rX "$mqtt_conf"
  container mqtt --publish 127.0.0.1:1883:1883 \
    --volume "$mqtt_conf/mosquitto.conf:/mosquitto/config/mosquitto.conf:ro" "$MQTT_IMAGE"
  container kafka --publish 127.0.0.1:9092:9092 \
    --env KAFKA_HEAP_OPTS="-Xms256m -Xmx512m" "$KAFKA_IMAGE"
  # The default `guest` user may only connect from the container itself.
  container rabbitmq --publish 127.0.0.1:5672:5672 \
    --env RABBITMQ_DEFAULT_USER=navree --env RABBITMQ_DEFAULT_PASS="$PASSWORD" "$RABBITMQ_IMAGE"

  ready postgres docker exec navree-ci-postgres pg_isready --host 127.0.0.1 --username postgres --dbname navree_test
  # The image installs TimescaleDB in template1 and in the database it
  # creates, so every new database has it. The suites expect a fresh database
  # to have it available but not installed (they install it themselves, and
  # check that they do), as on a server set up from the packages.
  for database in template1 navree_test; do
    ready postgres docker exec navree-ci-postgres psql -X --username postgres --dbname "$database" \
      --command 'DROP EXTENSION IF EXISTS timescaledb'
  done
  ready pgvector docker exec navree-ci-pgvector pg_isready --host 127.0.0.1 --username postgres --dbname navree_test
  ready mysql docker exec navree-ci-mysql mysqladmin --host 127.0.0.1 --user root --password="$PASSWORD" ping
  ready mongodb mongodb_is_primary
  ready redis docker exec navree-ci-redis redis-cli ping
  # The bucket the S3 suite names in its URL. (Signed with curl's own SigV4:
  # the empty payload's hash goes in a header because older curl versions
  # don't add it.)
  ready s3 curl --fail --silent --show-error --request PUT \
    --user "$S3_ACCESS_KEY:$S3_SECRET_KEY" --aws-sigv4 'aws:amz:us-east-1:s3' \
    --header 'x-amz-content-sha256: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855' \
    "http://127.0.0.1:9000/$S3_BUCKET"
  ready qdrant curl --fail --silent http://127.0.0.1:6333/readyz
  ready typesense curl --fail --silent http://127.0.0.1:8108/health
  ready meilisearch curl --fail --silent http://127.0.0.1:7700/health
  ready stripe-mock curl --fail --silent --header 'Authorization: Bearer sk_test_123' http://127.0.0.1:12111/v1/customers
  # The root of Pebble's own HTTPS certificate (for localhost).
  curl --fail --silent --show-error --location --output "$PEBBLE_CA" \
    https://raw.githubusercontent.com/letsencrypt/pebble/main/test/certs/pebble.minica.pem
  ready pebble curl --fail --silent --cacert "$PEBBLE_CA" https://localhost:14000/dir
  ready nats tcp 4222
  ready mqtt tcp 1883
  ready kafka docker exec navree-ci-kafka /opt/kafka/bin/kafka-broker-api-versions.sh --bootstrap-server 127.0.0.1:9092
  ready rabbitmq docker exec navree-ci-rabbitmq rabbitmq-diagnostics -q check_port_connectivity

  variables
  if [ -n "${GITHUB_ENV:-}" ]; then
    variables >>"$GITHUB_ENV"
  fi
}

stop() {
  local service
  for service in "${SERVICES[@]}"; do
    docker rm --force --volumes "navree-ci-$service" >/dev/null 2>&1 || true
  done
  docker volume rm --force navree-ci-s3 >/dev/null 2>&1 || true
}

case "${1:-}" in
  pull) pull ;;
  start) start ;;
  env) variables ;;
  stop) stop ;;
  *)
    say "usage: $0 pull|start|env|stop"
    exit 2
    ;;
esac
