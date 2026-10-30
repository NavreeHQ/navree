---
name: navree-deployment
description: "Deploys a Navree app: the release build, what to copy, production settings (APP_KEY, migrations, trusted proxies, metrics token), running `serve`, `queue:work` and `schedule:work` under systemd, migrations as a deploy step, nginx in front, health checks and containers. Use when shipping a Navree app to a server or container, writing systemd units, an nginx config or a Dockerfile, choosing production settings, or preparing a deploy runbook."
---

# Deployment

A Navree app deploys as **one binary**. The same executable serves HTTP,
runs queue workers and the scheduler, and migrates the database; the first
argument picks the job. It reads `.env` and `config/app.toml` from its
**working directory**.

To have all of this done for you on Ubuntu servers (provisioning, systemd
units, Caddy with TLS, blue-green releases with health checks and
rollbacks), load the `navree-harbor` skill and use `navree harbor:provision`
and `navree deploy`; a load balancer in front of Harbor's Caddy goes in
`deploy.trusted_proxies` (Caddy keeps its `X-Forwarded-For`, and the app's
`http.trusted_proxies` lists it after loopback). This skill is for running
the app on your own setup.

## Steps

1. Build: `cargo build --release` (binary: `target/release/<package>`).
   Build on the target platform (or cross-compile). No OpenSSL or SQLite
   library is needed at runtime (SQLite is compiled in only with the
   `sqlite` feature). Until the crates are published, the machine that
   builds the release needs the Navree checkout at the path in `Cargo.toml`
   (or point the dependency at the Git repository).
2. Copy to the server: the binary, `config/app.toml` and a writable
   `storage/` (plus `resources/views/` if you use template files).

```text
/srv/myapp/
  myapp              # the release binary
  config/app.toml
  storage/           # writable by the app's user (SQLite file, uploads, logs)
```

3. Write the environment file (production settings below), readable only by
   root and the app's user.
4. Migrate: `./myapp migrate` (with the production environment), as its own
   step, before restarting.
5. Start or restart the processes (`serve`, `queue:work`, and
   `schedule:work` when the app has `ScheduleModule`).
6. Check `GET /health/ready` and `./myapp routes`.

## Production settings

```env
# /etc/myapp/myapp.env  (environment variables override config/app.toml)
APP_KEY=base64:...                          # the SAME key for every process
APP_HTTP__ADDR=127.0.0.1:8080               # 0.0.0.0:8080 in a container
APP_HTTP__TRUSTED_PROXIES=127.0.0.1,::1     # the reverse proxy's addresses
APP_DATABASE__MIGRATE_ON_BOOT=false         # migrate as a deploy step
APP_APP__URL=https://myapp.example.com      # absolute links in mail
APP_MAIL__TRANSPORT=smtps://user:password@smtp.example.com:465
APP_MAIL__FROM="My App <hello@example.com>"
APP_OBSERVE__METRICS_TOKEN=a-long-random-string   # /metrics is public otherwise
# APP_DATABASE__URL=postgres://...          # with the `postgres` feature of navree
# APP_DATABASE__URL=mysql://...             # with the `mysql` feature (MySQL or MariaDB)
```

| Setting | Why |
|---|---|
| `APP_KEY` | without it encryption fails and every restart signs users out; different keys break tokens and relayed broadcasts |
| `database.migrate_on_boot=false` | a failing migration should stop the deploy, not the server |
| `http.trusted_proxies` | otherwise every client shares the proxy's IP for the per-IP limit (600/min) and the login throttle |
| `app.url` | Navree doesn't build URLs from `Host` or `X-Forwarded-Proto` |
| `observe.metrics_token` | protect `GET /metrics` (or block it at the proxy) |
| `auth.registration` | turn public sign-up off if accounts are created by admins |

## Processes

| Process | Command | When |
|---|---|---|
| web | `./myapp serve` (or no command) | always |
| queue worker | `./myapp queue:work` | jobs, queued mail, queued listeners or notifications |
| scheduler | `./myapp schedule:work` (or `./myapp schedule:run` from cron every minute) | `ScheduleModule` with tasks |

On SIGTERM: `serve` drains requests for `http.shutdown_grace_secs` (30 s),
`queue:work` finishes running jobs, then background tasks and shutdown hooks
get up to 30 s each. Give the process manager a stop timeout above that
(`TimeoutStopSec=100`). Restart workers after every deploy (they keep the
old code). With `RealtimeModule`, only `serve` holds sockets: set
`APP_REALTIME__RELAY_URL=auto` so workers and the scheduler relay their
broadcasts.

```ini
# /etc/systemd/system/myapp.service (copy for myapp-queue with `queue:work`)
[Unit]
Description=My App API
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=myapp
Group=myapp
WorkingDirectory=/srv/myapp
EnvironmentFile=/etc/myapp/myapp.env
ExecStart=/srv/myapp/myapp serve
Restart=on-failure
RestartSec=5
TimeoutStopSec=100

[Install]
WantedBy=multi-user.target
```

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now myapp myapp-queue
cd /srv/myapp && sudo -u myapp sh -c 'set -a; . /etc/myapp/myapp.env; set +a; ./myapp migrate'
sudo systemctl restart myapp myapp-queue
```

For a migration the running version can't handle: `./myapp down` (maintenance
mode, 503 except health probes and `/metrics`), migrate, restart, `./myapp up`.
Requests during a restart fail (the socket isn't handed over): run two
instances behind the proxy and restart them in turn when that matters.

## Reverse proxy (nginx)

```nginx
server {
    listen 443 ssl;
    server_name myapp.example.com;
    ssl_certificate     /etc/ssl/certs/myapp.pem;
    ssl_certificate_key /etc/ssl/private/myapp.key;
    client_max_body_size 2m;                 # match http.body_limit_bytes

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    location = /metrics { deny all; }

    location /app/ {                         # WebSockets (RealtimeModule)
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_read_timeout 300s;
    }
}
```

Serve over HTTPS: Navree sends `Strict-Transport-Security` by default.

## Health checks

`GET /health/live` → 200 while the process serves (liveness).
`GET /health/ready` → 200 when every check (database, cache, storage, yours)
is up, 503 otherwise (readiness). Both are public and keep answering in
maintenance mode; don't send an `Authorization` header to them.

## Containers

Same rules: the binary plus `config/` in the working directory, settings
from the environment, `APP_HTTP__ADDR=0.0.0.0:8080`. A two-stage build
(`rust:1-bookworm` to build, `debian:bookworm-slim` to run) works; copy the
Navree checkout into the build stage while the app depends on it by path.

## The database

A new app runs on TimescaleDB (`features = ["postgres"]` on `navree`): in
production use a TimescaleDB server, or PostgreSQL with the `timescaledb`
extension, create the extension once, and set `APP_SCHEMA__TIMESCALEDB=require`
so a migration stops on a server without it instead of creating regular
tables. Plain PostgreSQL works too (hypertables become regular tables).
An app built without the driver of its `database.url` stops at boot with a
message naming the feature to enable.

SQLite (`features = ["sqlite"]` on `navree`) suits one server: WAL mode, a 5 s busy timeout; keep
`storage/` on a persistent disk and back the file up. For several servers
use PostgreSQL (`features = ["postgres"]` on `navree`, a `postgres://`
URL) or MySQL/MariaDB (`features = ["mysql"]`, a `mysql://` URL; TLS is
used when the server offers it, `ssl-mode=VERIFY_IDENTITY&ssl-ca=...` checks
the certificate). The modules' test suites run on SQLite (navree-db's own
paths also run against MySQL 8.4), so exercise database-specific paths (row
locks, the queue's `SKIP LOCKED`) in staging first.

## Common mistakes

- Running from another working directory (config and `storage/` not found).
- `migrate:fresh` anywhere near production (drops every table, no prompt).
- Different `.env` files for `serve` and workers.
- Forgetting to restart `queue:work` after a deploy.
- Exposing `/metrics` publicly.

## Docs

- https://navree.com/docs/getting-started/deployment/
- https://navree.com/docs/digging-deeper/health-and-metrics/
- https://navree.com/docs/digging-deeper/maintenance-mode/
