---
name: navree-harbor
description: "Deploys a Navree app to Ubuntu servers with Harbor: `harbor.toml` (servers, roles, processes, `[env]`, encrypted `[secrets]`), `navree harbor:init`, `navree harbor:provision` (Caddy with TLS, PostgreSQL, Redis, ufw, the `navree` user, systemd units), `navree deploy` (build, upload, migrate, blue-green switch after a health check, automatic revert, pruning), `navree deploy:rollback`, `navree harbor:secrets`, environments (`--env`, `NAVREE_HARBOR_ENV`), several servers, a front end, `navree build:image` and `navree harbor:ci github`. Use when shipping a Navree app to a VPS, adding a server or worker, storing a production secret, rolling back, debugging a failed deploy or health check, building a container image, or setting up push-to-deploy."
---

# Harbor: servers and zero-downtime deploys

Harbor (crate `navree-harbor`, built into the `navree` CLI) turns Ubuntu
22.04/24.04 servers into ones that run a Navree app, and ships releases to
them over SSH. Everything is described in `harbor.toml` next to the app's
`Cargo.toml`. It shells out to the local `ssh` (and `cargo`, `git`,
`docker` or `podman`); nothing is installed on the servers but the app and
its services.

## Steps

1. From the app's directory (the package with `[package]` in `Cargo.toml`):

   ```sh
   navree harbor:init     # harbor.toml, harbor.key (never commit it), .gitignore line
   ```

2. Edit `harbor.toml`: `domain`, the server's `host` and its `roles`
   (template below). For PostgreSQL add `features = ["postgres"]` under
   `[build]` and the `postgres` role.
3. Store the secrets (encrypted into `harbor.toml`):

   ```sh
   navree harbor:secrets set APP_KEY --generate
   navree harbor:secrets set APP_MAIL__TRANSPORT     # prompts without echo
   ```

4. Provision, then deploy:

   ```sh
   navree harbor:provision    # as root; records host_key, APP_DATABASE__URL and REDIS_URL
   navree deploy
   ```

5. Commit `harbor.toml` (with the `host_key` lines and the encrypted
   secrets). Share `harbor.key` through a password manager.

## harbor.toml template

```toml
app = "crm-api"                  # the package to build (default: the package next to the file)
domain = "crm.example.com"       # required with a `web` server
# aliases = ["www.crm.example.com"]   # redirected to domain

[build]
features = ["postgres"]          # Cargo features of the release build
# method = "auto"                # auto | native | container
# target = "x86_64-unknown-linux-musl"

[deploy]
keep_releases = 5
health_check = "/health/ready"
health_timeout_secs = 60
# migrate = true
# include = ["config", "resources"]   # shipped with the binary
# shared = ["storage"]                # kept across releases
# ports = [8081, 8082]                # blue and green
# trusted_proxies = ["10.0.0.0/24"]   # a load balancer's network

[processes]
web = { command = "serve" }
worker = { command = "queue:work", instances = 2 }
scheduler = { command = "schedule:work" }

[servers.app-1]
host = "203.0.113.10"
roles = ["web", "worker", "scheduler", "postgres", "redis"]
# private_ip = "10.0.0.11"
# host_key = "ssh-ed25519 AAAA..."    # written by harbor:provision

[env]
APP_APP__URL = "https://crm.example.com"
APP_LOG__LEVEL = "info"

[secrets]
# written by `navree harbor:secrets set` and `harbor:provision`
```

Other tables: `[ssh]` (`user = "navree"`, `provision_user = "root"`,
`port = 22`, `key`, `authorized_keys`), `[web]` (`metrics_allow`,
`proxy_protocol`, and `[web.custom_domains]` with `enabled`,
`ask_path = "/_domains/allow"` and `to = "app"`/`"frontend"`: Caddy's
on-demand TLS for workspaces' own domains, asking navree-domains' ask
endpoint through the internal entry), `[postgres]` (`version = 17`, `database`,
`backup_days = 7`), `[redis]` (`maxmemory_mb = 256`), `[frontend]`
(`path`, `build`, `command`, `health_check = "/"`, `ports = [3001, 3002]`,
`node_version = 24`) and `api_domain`. Unknown keys are errors.

Roles: `web` (Caddy and the `web` process in blue/green slots), `worker`,
`scheduler`, `postgres`, `redis`. A server without `roles` gets
`["web", "worker", "scheduler"]`. At most one `postgres` and one `redis`
server.

## Commands

```sh
navree harbor:provision [server]           # idempotent; run again after any change to servers or roles
navree deploy [--server app-1] [--no-migrate] [--maintenance] [--release <id>]
navree deploy:rollback [--to <release>]
navree deploy:releases [--server app-1]
navree harbor:status [--server app-1]
navree harbor:logs [--server app-1] [--process web] [--lines 200] [--follow]
navree harbor:run migrate:status           # an app command on the first web server, from current
navree harbor:ssh [server]                 # a shell as the deploy user
navree harbor:backup [--download]          # pg_dump now on the postgres server
navree harbor:secrets list                 # names only
navree harbor:secrets reveal APP_MAIL__TRANSPORT
navree harbor:secrets unset STRIPE_SECRET
navree harbor:secrets export               # [env] + decrypted [secrets] as an env file
navree harbor:secrets rotate-key           # re-encrypt under a new harbor.key
navree harbor:ci github --branch main      # .github/workflows/deploy.yml
navree build:image --tag registry.example.com/crm-api:1.4.0 --push
navree build:image --dockerfile            # write Dockerfile + ignore file to customise
```

Every command takes `--env <name>` (`harbor.<name>.toml` and
`harbor.<name>.key`); `NAVREE_HARBOR_ENV` does the same. Harbor looks for
the file in the current directory, then the ones above it.

## What a deploy does

1. Builds once (natively on Linux with the servers' architecture and a
   glibc no newer than theirs, otherwise in a container: Docker or Podman
   with Harbor's Ubuntu 22.04 builder image).
2. Uploads `releases/<YYYYMMDD-HHMMSS-shortsha>/` to every server in
   parallel; `deploy.shared` directories link to `shared/`.
3. Writes `/etc/<app>/env`: Harbor's settings, then `[env]`, then
   `[secrets]` (later wins). Secrets are decrypted only on the deploying
   machine and travel base64-encoded on SSH's stdin.
4. Runs `migrate` once from the new release on the first `web` server. A
   failed migration stops the deploy before anything switches.
5. Per web server, one at a time: start the idle slot (blue 8081 / green
   8082), health-check `deploy.health_check` until 200, rewrite the Caddy
   site and reload, move `current` atomically (`ln -sfn` + `mv -T`), stop
   the old slot (drain). A failed health check stops the new slot, prints
   its last log lines, and moves already-switched servers back.
6. Restarts workers and the scheduler, then prunes beyond `keep_releases`
   (never a release `current`, `blue` or `green` points at).

## Checks

- `navree harbor:provision` ends with `Provisioned <server> in ...` and
  `harbor.toml` now has `host_key` for each server and, with the data
  roles, `APP_DATABASE__URL` and `REDIS_URL` under `[secrets]`.
- `navree deploy` ends with `Released <id> in ...`; `navree harbor:status`
  shows the release as current and the web slot running.
- `navree deploy:releases` lists the release with `current`.
- The app answers `GET /health/ready` with 200 locally before deploying (it
  is what Harbor waits for).

## Using the library

`navree::harbor` (facade feature `harbor`, opt-in, not in `full`) or the
`navree-harbor` crate. `Project::load(dir, Environment::production())`
reads and validates `harbor.toml`; `HarborConfig::parse(text, path)` and
`config.validate()` work on text. Every program goes through an
`Executor` (`SystemExecutor`, or `FakeExecutor` in tests); the release state
machine (`deploy::Deployer`) runs against the `Host` trait (`SshHost`, or
`fake::FakeHost` in memory).

```rust
use navree_harbor::exec::{Cmd, Executor, FakeExecutor, Output};
use navree_harbor::{HarborConfig, Role};

let config = HarborConfig::parse(
    "app = \"crm-api\"\ndomain = \"crm.example.com\"\n\n[servers.app-1]\nhost = \"203.0.113.10\"\n",
    std::path::Path::new("harbor.toml"),
)?;
assert!(config.validate().is_empty());
assert_eq!(config.servers_with(Role::Web).count(), 1);

let fake = FakeExecutor::new(|cmd| match cmd.program.as_str() {
    "ssh" => Output::ok("arch=x86_64\n"),
    _ => Output::failed(127, "not found"),
});
fake.run(&Cmd::new("ssh").arg("navree@203.0.113.10").arg("bash -s"))?;
assert_eq!(fake.calls()[0].program, "ssh");
```

## Gotchas

- **`harbor.key` missing**: `no secrets key: harbor.key does not exist and
  NAVREE_HARBOR_KEY is not set`. Get the key from the team, or set
  `NAVREE_HARBOR_KEY` (CI). Provisioning a `postgres` or `redis` server
  needs the key, since it stores the generated URLs.
- **`servers.<name>.host_key is not set`**: deploys and every other server
  command need the pinned key; run `navree harbor:provision <name>` first.
  If a server is rebuilt, delete its `host_key` line and provision again.
- **`is not provisioned yet`**: the server has no `/srv/<app>/releases`;
  provision it before deploying.
- **Health check fails**: the error prints the slot's last 20 log lines.
  Common causes: `APP_KEY` missing from `[secrets]`, the database not
  reachable (readiness checks it), a migration the new code expects but
  `--no-migrate` skipped, or a slow boot (raise
  `deploy.health_timeout_secs`). The old release keeps serving.
- **Migrations must suit both releases**: the new release migrates while
  the old one serves. Rename or drop columns over two deploys, or deploy
  with `--maintenance`. Rollbacks never undo migrations.
- **Redis on another server** needs `private_ip` on both sides (Redis has no
  TLS here); `harbor.toml` is refused otherwise. PostgreSQL across public
  addresses requires TLS (`sslmode=require`).
- **Non-Ubuntu or old Ubuntu** is refused before anything changes; Harbor
  supports Ubuntu 22.04+ on x86_64 or aarch64.
- **macOS/Windows deploys** need Docker or Podman running (the build runs
  in a container). `build.method = "native"` only works on Linux.
- **Provisioning as a non-root user** runs scripts with `sudo -n`: that user
  needs password-less sudo.
- **Values with newlines** can't go into `harbor:secrets export` (an env
  file). Deploys carry any value, since the env file travels base64-encoded.
- **`harbor:ci github`** never overwrites an existing workflow, and needs
  the CI key's public half in `ssh.authorized_keys` plus a re-provision. It
  writes at the repository root, except when that repository ignores the
  app's directory: then the app's own `.github/workflows`.
- **A kit's `harbor.toml` without `harbor.key`**: `navree new` writes the
  key; otherwise `navree harbor:init` writes just the key (not when the file
  has encrypted `[secrets]`).

## Docs

- https://navree.com/docs/tools/harbor/
- https://navree.com/docs/getting-started/deployment/
