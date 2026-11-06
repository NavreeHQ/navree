#!/usr/bin/env bash
# The servers behind the integration suites: connectors that
# scripts/ci-services.sh does not start, as containers on 127.0.0.1, for
# .github/workflows/integrations.yml and for the final gate on a developer
# machine. The containers are described in scripts/services/integrations.yml;
# this script starts them, waits until each answers, sets up the data the
# suites expect and prints the variables the suites read.
#
#   scripts/integration-services.sh pull [GROUP...]
#                                       pull (and build) the images
#   scripts/integration-services.sh start [GROUP...]
#                                       start the servers, wait until each
#                                       answers, and print the NAVREE_TEST_*
#                                       variables (also appended to $GITHUB_ENV)
#   scripts/integration-services.sh env      print the variables only
#   scripts/integration-services.sh status [GROUP...]
#                                       show which servers run (exit 1 unless
#                                       all do)
#   scripts/integration-services.sh stop     remove the containers and their
#                                            volumes
#
# The groups (all of them when none is named), one per job of the workflow:
#
#   mail      Mailpit over TLS, LocalStack (SES), GreenMail (SMTP, IMAP, POP3),
#             the Pub/Sub emulator
#   identity  Keycloak (SAML and OpenID Connect), OpenLDAP
#
# Needs Docker with Compose v2, curl and openssl, and free ports 12222, 13027,
# 13465 and 18027 to 18465 on 127.0.0.1. The containers are called
# navree-int-<service>.
#
# The Mailpit servers of scripts/ci-services.sh (plain, login, STARTTLS, TLS)
# are not repeated here: `scripts/ci-services.sh start` with every other
# service left out starts them (the workflow does that).
#
# Settings (environment variables):
#   NAVREE_INT_SKIP="svc svc"       Leave these services out (names as in
#                                   scripts/services/integrations.yml).
#   NAVREE_INT_CONSUMER=wsl         The suites run in WSL while Docker runs on
#                                   Windows: print file paths as /mnt/c/...
#                                   paths.
#   <NAME>_IMAGE                    Another image for a service (see the
#                                   compose file).
#
# The throwaway credentials below only guard servers that live for one CI job
# on the loopback interface.
set -euo pipefail

# Git Bash on Windows rewrites arguments that look like POSIX paths (`/data`,
# `/CN=localhost`): leave them alone. Harmless elsewhere.
export MSYS_NO_PATHCONV=1
export MSYS2_ARG_CONV_EXCL='*'

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# A path Docker Desktop and Compose understand (C:/Users/...) on Windows.
native_path() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -m "$1"
  else
    printf '%s' "$1"
  fi
}

COMPOSE_FILE="$(native_path "$ROOT/scripts/services/integrations.yml")"

STATE="${NAVREE_INT_STATE:-$(native_path "$ROOT/target/integration-services")}"
export NAVREE_INT_STATE="$STATE"

SMTP_USER=navree
SMTP_PASSWORD=navree-secret
LDAP_ADMIN_DN=cn=admin,dc=acme,dc=test
LDAP_ADMIN_PASSWORD=admin-secret

# The services of a group.
group_services() {
  case "$1" in
    mail) echo "mailpit-starttls mailpit-tls localstack greenmail pubsub" ;;
    identity) echo "keycloak openldap" ;;
    *) return 1 ;;
  esac
}

ALL_GROUPS=(mail identity)
COMMAND="${1:-}"
shift || true
WANTED_GROUPS=("$@")
if [ "${#WANTED_GROUPS[@]}" = 0 ]; then
  WANTED_GROUPS=("${ALL_GROUPS[@]}")
fi

SKIP=" ${NAVREE_INT_SKIP:-} "

# True when the caller leaves the service out.
left_out() {
  case "$SKIP" in
    *" $1 "*) return 0 ;;
    *) return 1 ;;
  esac
}

SELECTED=()
for group in "${WANTED_GROUPS[@]}"; do
  if ! services="$(group_services "$group")"; then
    printf 'unknown group %s (groups: %s)\n' "$group" "${ALL_GROUPS[*]}" >&2
    exit 2
  fi
  for service in $services; do
    if ! left_out "$service"; then
      SELECTED+=("$service")
    fi
  done
done

# True when the service is among those this run starts.
selected() {
  local service
  for service in "${SELECTED[@]}"; do
    if [ "$service" = "$1" ]; then
      return 0
    fi
  done
  return 1
}

say() { printf '%s\n' "$*" >&2; }

compose() { docker compose --project-name navree-int --file "$COMPOSE_FILE" "$@"; }

# A file path as the suites will see it.
consumer_path() {
  local path=$1
  if [ "${NAVREE_INT_CONSUMER:-}" = wsl ]; then
    printf '%s' "$path" | sed -E 's#^([A-Za-z]):/#/mnt/\L\1/#'
  else
    printf '%s' "$path"
  fi
}

# The variables the suites read.
variables() {
  cat <<EOF
NAVREE_TEST_REQUIRE_SERVICES=1
NAVREE_TEST_TLS_SMTP_CA=$(consumer_path "$STATE/mailpit/cert.pem")
NAVREE_TEST_TLS_SMTP_STARTTLS_URL=smtp://$SMTP_USER:$SMTP_PASSWORD@localhost:13027?tls=required
NAVREE_TEST_TLS_SMTP_STARTTLS_API=http://127.0.0.1:18027
NAVREE_TEST_TLS_SMTP_TLS_URL=smtps://$SMTP_USER:$SMTP_PASSWORD@localhost:13465
NAVREE_TEST_TLS_SMTP_TLS_API=http://127.0.0.1:18465
NAVREE_TEST_LOCALSTACK_URL=http://127.0.0.1:14566
NAVREE_TEST_GREENMAIL_SMTP_URL=smtp://127.0.0.1:13025
NAVREE_TEST_GREENMAIL_IMAP=127.0.0.1:13143
NAVREE_TEST_GREENMAIL_POP3=127.0.0.1:13110
NAVREE_TEST_PUBSUB_URL=http://127.0.0.1:18085
NAVREE_TEST_KEYCLOAK_URL=http://127.0.0.1:18081
NAVREE_TEST_LDAP_URL=ldap://127.0.0.1:13389
NAVREE_TEST_LDAPS_URL=ldaps://127.0.0.1:13636
NAVREE_TEST_LDAP_CA=$(consumer_path "$STATE/openldap/certs/ldap.crt")
NAVREE_TEST_LDAP_BIND_DN=$LDAP_ADMIN_DN
NAVREE_TEST_LDAP_PASSWORD=$LDAP_ADMIN_PASSWORD
EOF
}

# `ready <service> <container> <command...>`: runs the command every two
# seconds (up to three minutes) until it succeeds; on a timeout shows the
# container's log.
ready() {
  local service=$1 container=$2
  shift 2
  if ! selected "$service"; then
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

# LocalStack has run its start-up scripts and answers.
localstack_ready() {
  curl --fail --silent http://127.0.0.1:14566/_localstack/init/ready | grep -Eq '"READY": ?true'
}

# GreenMail answers on SMTP, IMAP and POP3.
greenmail_ready() {
  tcp 13025 && tcp 13143 && tcp 13110
}

# OpenLDAP has loaded the directory of integrations/openldap/base.ldif.
openldap_ready() {
  docker exec navree-int-openldap ldapsearch -x -H ldap://localhost -D "$LDAP_ADMIN_DN" -w "$LDAP_ADMIN_PASSWORD" \
    -b dc=acme,dc=test "(cn=support)" dn | grep -q '^dn: cn=support'
}

# 600 more people for the LDAP suite, to pass the server's size limit of 500.
bulk_people() {
  local given=(Gurpreet Harpreet Simran Manpreet Jaspreet Amandeep Navdeep Rajinder Balwinder Kuldeep)
  local family=(Singh Kaur Sandhu Gill Dhillon Grewal Sidhu Bajwa Randhawa Brar)
  local i uid first last
  for i in $(seq 1 600); do
    uid=$(printf 'user%04d' "$i")
    first=${given[$((i % 10))]}
    last=${family[$(((i / 10) % 10))]}
    printf 'dn: uid=%s,ou=bulk,dc=acme,dc=test\nobjectClass: inetOrgPerson\nuid: %s\ncn: %s %s\ngivenName: %s\nsn: %s\nmail: %s@acme.test\n\n' \
      "$uid" "$uid" "$first" "$last" "$first" "$last" "$uid"
  done
}

load_bulk_people() {
  if ! selected openldap; then
    return 0
  fi
  bulk_people | docker exec -i navree-int-openldap ldapadd -x -H ldap://localhost -D "$LDAP_ADMIN_DN" -w "$LDAP_ADMIN_PASSWORD" >/dev/null
}

# Retries a pull that fails (a registry hiccup) twice more.
pull() {
  local _try
  for _try in 1 2 3; do
    if compose pull --quiet --ignore-buildable "${SELECTED[@]}"; then
      break
    fi
    if [ "$_try" = 3 ]; then
      say "could not pull the images"
      return 1
    fi
    sleep 5
  done
  compose build --quiet "${SELECTED[@]}"
}

# A self-signed certificate for localhost and the password file of the Mailpit
# servers (in $STATE/mailpit, which the compose file mounts).
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

  # OpenLDAP's certificate, its own authority, and the Diffie-Hellman
  # parameters its TLS setup asks for.
  local ldap="$STATE/openldap/certs"
  mkdir -p "$ldap"
  if [ ! -s "$ldap/ldap.crt" ] || [ ! -s "$ldap/ldap.key" ]; then
    openssl req -x509 -newkey rsa:2048 -nodes -days 30 \
      -keyout "$ldap/ldap.key" -out "$ldap/ldap.crt" -subj "/CN=localhost" \
      -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" >/dev/null 2>&1
    cp "$ldap/ldap.crt" "$ldap/ca.crt"
    openssl dhparam -dsaparam -out "$ldap/dhparam.pem" 2048 >/dev/null 2>&1
  fi
  chmod -R a+rwX "$ldap"
}

start() {
  prepare_state
  # Start from nothing: leftovers from an earlier run keep old data and ports.
  stop
  pull
  compose up --detach "${SELECTED[@]}"

  local mailpit
  for mailpit in mailpit-starttls mailpit-tls; do
    ready "$mailpit" "navree-int-$mailpit" docker exec "navree-int-$mailpit" /mailpit readyz
  done
  ready localstack navree-int-localstack localstack_ready
  ready greenmail navree-int-greenmail greenmail_ready
  ready pubsub navree-int-pubsub curl --fail --silent http://127.0.0.1:18085/
  ready openldap navree-int-openldap openldap_ready
  load_bulk_people
  # Keycloak answers once the realm is imported.
  ready keycloak navree-int-keycloak curl --fail --silent http://127.0.0.1:18081/realms/navree/.well-known/openid-configuration

  variables
  if [ -n "${GITHUB_ENV:-}" ]; then
    variables >>"$GITHUB_ENV"
  fi
}

# Removes the containers of this script and their volumes.
stop() {
  compose down --volumes --remove-orphans >/dev/null 2>&1 || true
  local leftovers
  leftovers="$(docker ps --all --quiet --filter 'name=^navree-int-' 2>/dev/null || true)"
  if [ -n "$leftovers" ]; then
    # shellcheck disable=SC2086 # one container id per line
    docker rm --force --volumes $leftovers >/dev/null 2>&1 || true
  fi
}

status() {
  local service container down=0
  for service in "${SELECTED[@]}"; do
    container="navree-int-$service"
    if [ "$(docker inspect --format '{{.State.Running}}' "$container" 2>/dev/null)" = "true" ]; then
      printf '%-18s running\n' "$service"
    else
      printf '%-18s NOT RUNNING\n' "$service"
      down=1
    fi
  done
  return "$down"
}

case "$COMMAND" in
  pull) pull ;;
  start) start ;;
  env) variables ;;
  status) status ;;
  stop) stop ;;
  *)
    say "usage: $0 pull|start|env|status|stop [GROUP...]"
    exit 2
    ;;
esac
