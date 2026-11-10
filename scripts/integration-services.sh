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
#   scripts/integration-services.sh stop [GROUP...]
#                                       remove the containers and their volumes
#                                       (those of the named groups, or all)
#
# The groups (all of them when none is named), one per job of the workflow:
#
#   mail      Mailpit over TLS, Moto (SES), GreenMail (SMTP, IMAP, POP3),
#             the Pub/Sub emulator
#   identity  Keycloak (SAML and OpenID Connect), OpenLDAP
#   infra     an OpenTelemetry Collector, CoreDNS, a DNS-over-HTTPS server
#   harbor    an Ubuntu host with systemd and sshd
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
    mail) echo "mailpit-starttls mailpit-tls ses greenmail pubsub" ;;
    identity) echo "keycloak openldap" ;;
    infra) echo "otel-collector coredns doh" ;;
    harbor) echo "harbor-host" ;;
    *) return 1 ;;
  esac
}

ALL_GROUPS=(mail identity infra harbor)
COMMAND="${1:-}"
shift || true
WANTED_GROUPS=("$@")
NAMED_GROUPS=$#
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
NAVREE_TEST_TLS_SMTP_CA=$(consumer_path "$STATE/mailpit/ca.pem")
NAVREE_TEST_TLS_SMTP_STARTTLS_URL=smtp://$SMTP_USER:$SMTP_PASSWORD@localhost:13027?tls=required
NAVREE_TEST_TLS_SMTP_STARTTLS_API=http://127.0.0.1:18027
NAVREE_TEST_TLS_SMTP_TLS_URL=smtps://$SMTP_USER:$SMTP_PASSWORD@localhost:13465
NAVREE_TEST_TLS_SMTP_TLS_API=http://127.0.0.1:18465
NAVREE_TEST_SES_URL=http://127.0.0.1:14566
NAVREE_TEST_GREENMAIL_SMTP_URL=smtp://127.0.0.1:13025
NAVREE_TEST_GREENMAIL_IMAP=127.0.0.1:13143
NAVREE_TEST_GREENMAIL_POP3=127.0.0.1:13110
NAVREE_TEST_PUBSUB_URL=http://127.0.0.1:18085
NAVREE_TEST_KEYCLOAK_URL=http://127.0.0.1:18081
NAVREE_TEST_LDAP_URL=ldap://127.0.0.1:13389
NAVREE_TEST_LDAPS_URL=ldaps://127.0.0.1:13636
NAVREE_TEST_LDAP_CA=$(consumer_path "$STATE/openldap/certs/ca.crt")
NAVREE_TEST_LDAP_BIND_DN=$LDAP_ADMIN_DN
NAVREE_TEST_LDAP_PASSWORD=$LDAP_ADMIN_PASSWORD
NAVREE_TEST_OTEL_HTTP_URL=http://127.0.0.1:14318
NAVREE_TEST_OTEL_GRPC_URL=http://127.0.0.1:14317
NAVREE_TEST_OTEL_FILE=$(consumer_path "$STATE/otel/traces.json")
NAVREE_TEST_DNS_SERVER=127.0.0.1:15353
NAVREE_TEST_DOH_URL=http://127.0.0.1:18053/dns-query
NAVREE_TEST_HARBOR_HOST=127.0.0.1
NAVREE_TEST_HARBOR_PORT=12222
NAVREE_TEST_HARBOR_KEY=$(consumer_path "$STATE/harbor/id_ed25519")
EOF
}

# The services that did not start, named by `ready`.
FAILED=()

# What a server that did not start leaves to look at: the memory of the
# machine and of each container, and the service's own report if it has one
# (a function named diagnose_<service>, dashes as underscores).
diagnose() {
  local service=$1
  say "memory of the machine:"
  free -m >&2 2>/dev/null || true
  docker stats --no-stream --format '{{.Name}} {{.MemUsage}}' >&2 2>/dev/null || true
  if declare -F "diagnose_${service//-/_}" >/dev/null; then
    "diagnose_${service//-/_}" || true
  fi
}

# True when `ready` named the service as one that did not start.
failed() {
  case " ${FAILED[*]:-} " in
    *" $1 "*) return 0 ;;
    *) return 1 ;;
  esac
}

# The Harbor host's report: its units that failed and its log.
diagnose_harbor_host() {
  docker exec navree-int-harbor-host systemctl --failed --no-pager >&2 2>/dev/null || true
  docker exec navree-int-harbor-host journalctl --no-pager --lines 60 >&2 2>/dev/null || true
}

# `wait_for <service> <container> <command...>`: runs the command every two
# seconds (up to three minutes) until it succeeds. On a timeout it shows the
# container's state, what the command last said and the container's log.
wait_for() {
  local service=$1 container=$2
  shift 2
  local _attempt answer=""
  for _attempt in $(seq 1 90); do
    if [ "$(docker inspect --format '{{.State.Running}}' "$container" 2>/dev/null)" != "true" ]; then
      break
    fi
    if answer="$("$@" 2>&1)"; then
      say "$service is ready"
      return 0
    fi
    sleep 2
  done
  say "$service did not start:"
  # Why the container stopped, if it did: its exit code, whether the kernel
  # killed it for memory, and Docker's own error.
  docker inspect --format 'state {{.State.Status}}, exit code {{.State.ExitCode}}, killed for memory {{.State.OOMKilled}} {{.State.Error}}' "$container" >&2 2>/dev/null || true
  say "the last answer of the readiness check: $(printf '%s' "$answer" | tail -c 600)"
  docker logs "$container" 2>&1 | tail -n 40 >&2
  diagnose "$service"
  return 1
}

# `ready <service> <container> <command...>`: waits until the server answers.
# One that does not is made again from its image and given the same time
# again; one that still does not is named in FAILED, and `start` goes on to
# the others, so one run shows every server that does not start.
ready() {
  local service=$1
  if ! selected "$service"; then
    return 0
  fi
  if wait_for "$@"; then
    return 0
  fi
  say "starting $service again"
  compose up --detach --force-recreate "$service" >/dev/null 2>&1 || true
  if wait_for "$@"; then
    return 0
  fi
  FAILED+=("$service")
  return 0
}

tcp() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }

# Moto's server answers; its SES v2 API is part of the same process.
ses_ready() {
  curl --fail --silent http://127.0.0.1:14566/moto-api/ >/dev/null
}

# GreenMail answers on SMTP, IMAP and POP3.
greenmail_ready() {
  tcp 13025 && tcp 13143 && tcp 13110
}

# The host's systemd is up and sshd answers on the published port with its
# banner (Docker's port proxy accepts a connection whether or not anything
# listens behind it, so a bare connect proves nothing).
harbor_ready() {
  docker exec navree-int-harbor-host systemctl is-active ssh >/dev/null &&
    (exec 3<>/dev/tcp/127.0.0.1/12222 && head -c 4 <&3 | grep -q '^SSH-') 2>/dev/null
}

install_harbor_key() {
  if ! selected harbor-host; then
    return 0
  fi
  docker exec -i navree-int-harbor-host sh -c 'cat > /root/.ssh/authorized_keys && chmod 600 /root/.ssh/authorized_keys' \
    <"$STATE/harbor/id_ed25519.pub"
}

# CoreDNS answers over TCP (it answers UDP on the same port).
coredns_ready() {
  tcp 15353
}

# The DoH server answers the JSON API with a record of the zone.
doh_ready() {
  curl --fail --silent --header 'accept: application/dns-json' \
    'http://127.0.0.1:18053/dns-query?name=split.acme.test&type=TXT' | grep -q 'part one'
}

# The Collector accepts OTLP on both ports.
otel_ready() {
  tcp 14317 && tcp 14318
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
  # The services with a Dockerfile of their own.
  if selected harbor-host; then
    compose build --quiet harbor-host
  fi
}

# A certificate for localhost and a new authority that signs it. A client
# refuses an authority's own certificate as a server's (rustls says
# CaUsedAsEndEntity, and `openssl req -x509` marks what it makes as an
# authority), so the server shows the certificate and the suites trust the
# authority. Arguments: the authority's certificate and key, the server's
# certificate and key, the authority's name.
signed_certificate() {
  local ca=$1 ca_key=$2 cert=$3 key=$4 name=$5
  openssl req -x509 -newkey rsa:2048 -nodes -days 30 \
    -keyout "$ca_key" -out "$ca" -subj "/CN=$name" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "$key" -out "$cert.csr" -subj "/CN=localhost" >/dev/null 2>&1
  printf '%s\n' "subjectAltName=DNS:localhost,IP:127.0.0.1" "basicConstraints=critical,CA:FALSE" \
    "keyUsage=critical,digitalSignature,keyEncipherment" "extendedKeyUsage=serverAuth" >"$cert.ext"
  openssl x509 -req -in "$cert.csr" -CA "$ca" -CAkey "$ca_key" \
    -CAcreateserial -days 30 -extfile "$cert.ext" -out "$cert" >/dev/null 2>&1
  rm -f "$cert.csr" "$cert.ext" "${ca%.*}.srl"
}

# The Mailpit servers' certificate, its authority (which the suites trust) and
# their password file (in $STATE/mailpit, which the compose file mounts).
prepare_state() {
  local dir="$STATE/mailpit"
  mkdir -p "$dir"
  if [ ! -s "$dir/ca.pem" ] || [ ! -s "$dir/cert.pem" ] || [ ! -s "$dir/key.pem" ]; then
    signed_certificate "$dir/ca.pem" "$dir/ca-key.pem" "$dir/cert.pem" "$dir/key.pem" "Navree test mail servers"
  fi
  printf '%s:%s\n' "$SMTP_USER" "$SMTP_PASSWORD" >"$dir/auth.txt"
  chmod a+r "$dir/ca.pem" "$dir/cert.pem" "$dir/key.pem" "$dir/auth.txt"

  # OpenLDAP's certificate, its own authority, and the Diffie-Hellman
  # parameters its TLS setup asks for. The server in the image uses GnuTLS,
  # which reads only the PKCS#3 form of the parameters ("BEGIN DH
  # PARAMETERS"). `openssl dhparam -dsaparam`, fast as it is, writes the X9.42
  # form, which makes the server stop while it starts (ldapmodify error 80), so
  # the parameters are one of the standard groups (RFC 7919), which OpenSSL 3
  # writes at once in the right form; an older OpenSSL makes its own.
  local ldap="$STATE/openldap/certs"
  mkdir -p "$ldap"
  if [ ! -s "$ldap/ca.key" ] || [ ! -s "$ldap/ldap.crt" ] || [ ! -s "$ldap/ldap.key" ] || ! grep -q 'BEGIN DH PARAMETERS' "$ldap/dhparam.pem" 2>/dev/null; then
    signed_certificate "$ldap/ca.crt" "$ldap/ca.key" "$ldap/ldap.crt" "$ldap/ldap.key" "Navree test directory"
    openssl genpkey -genparam -algorithm DH -pkeyopt group:ffdhe2048 -out "$ldap/dhparam.pem" >/dev/null 2>&1 ||
      openssl dhparam -out "$ldap/dhparam.pem" 2048 >/dev/null 2>&1
  fi
  chmod -R a+rwX "$ldap"

  # The Collector's file exporter (a user of its own in the container) appends
  # to a file made here, so the tests can read it.
  # The SSH key Harbor's test logs in with.
  mkdir -p "$STATE/harbor"
  if [ ! -s "$STATE/harbor/id_ed25519" ]; then
    ssh-keygen -q -t ed25519 -N '' -C navree-integration-test -f "$STATE/harbor/id_ed25519"
  fi
  chmod 600 "$STATE/harbor/id_ed25519"
  mkdir -p "$STATE/otel"
  : >"$STATE/otel/traces.json"
  chmod a+rwx "$STATE/otel"
  chmod a+rw "$STATE/otel/traces.json"
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
  ready ses navree-int-ses ses_ready
  ready greenmail navree-int-greenmail greenmail_ready
  ready pubsub navree-int-pubsub curl --fail --silent http://127.0.0.1:18085/
  ready openldap navree-int-openldap openldap_ready
  ready otel-collector navree-int-otel-collector otel_ready
  ready coredns navree-int-coredns coredns_ready
  ready doh navree-int-doh doh_ready
  ready harbor-host navree-int-harbor-host harbor_ready
  # What follows a server needs the server.
  if ! failed harbor-host; then
    install_harbor_key
  fi
  if ! failed openldap; then
    load_bulk_people
  fi
  # Keycloak answers once the realm is imported.
  ready keycloak navree-int-keycloak curl --fail --silent http://127.0.0.1:18081/realms/navree/.well-known/openid-configuration

  variables
  if [ -n "${GITHUB_ENV:-}" ]; then
    variables >>"$GITHUB_ENV"
  fi
  if [ "${#FAILED[@]}" != 0 ]; then
    say "these servers did not start: ${FAILED[*]}"
    return 1
  fi
}

# Removes the containers of this script and their volumes: those of the groups
# named on the command line, or all of them when none is named. (Runs of this
# script for different groups on one machine leave each other's containers
# alone.)
stop() {
  if [ "$NAMED_GROUPS" -gt 0 ]; then
    local service
    for service in "${SELECTED[@]}"; do
      docker rm --force --volumes "navree-int-$service" >/dev/null 2>&1 || true
    done
    return 0
  fi
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
