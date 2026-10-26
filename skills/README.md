# Navree agent skills

Agent skills teach coding agents how to build with Navree. Each skill is a
folder with a `SKILL.md`: YAML front-matter (`name` and a `description` that
says when to use it), then the steps, the CLI commands, code templates, the
checks to run and the common mistakes for one task. An agent loads a skill's
description up front and reads the body only when the task matches, so a
project can carry all of them at little cost.

Every skill describes features that exist in the framework, and every API it
names is checked against the code. The code templates compile against the
current `navree` crate.

## The skills

Start with **`navree`**: it explains the framework, the app layout and the
conventions, and says which skill to load for each task.

| Skill | Use it to |
|---|---|
| [`navree`](navree/SKILL.md) | Understand a Navree app and pick the right skill |
| [`navree-new-app`](navree-new-app/SKILL.md) | Create an app with `navree new`, set the key, run `navree dev` |
| [`navree-modules-and-di`](navree-modules-and-di/SKILL.md) | Define modules and providers, wire dependencies, fix graph errors |
| [`navree-controllers-and-routing`](navree-controllers-and-routing/SKILL.md) | Add controllers, routes, extractors and access attributes |
| [`navree-validation`](navree-validation/SKILL.md) | Validate request bodies and query strings |
| [`navree-errors-and-responses`](navree-errors-and-responses/SKILL.md) | Return errors in Navree's JSON shape, map error types, shape responses |
| [`navree-database`](navree-database/SKILL.md) | Entities, `Repo`, the unit of work, migrations, seeders, factories, pagination |
| [`navree-auth`](navree-auth/SKILL.md) | JWT sign-in, refresh tokens, the current user, account flows, two-factor |
| [`navree-access-control`](navree-access-control/SKILL.md) | Roles, permissions, abilities, record checks and SQL list filters |
| [`navree-queues-and-scheduling`](navree-queues-and-scheduling/SKILL.md) | Jobs, workers, retries, failed jobs and cron-style schedules |
| [`navree-events-and-broadcasting`](navree-events-and-broadcasting/SKILL.md) | Events and listeners; real-time channels over WebSockets |
| [`navree-mail-and-notifications`](navree-mail-and-notifications/SKILL.md) | Mail with templates and transports; notifications over several channels |
| [`navree-storage`](navree-storage/SKILL.md) | Disks, uploads, downloads and signed temporary URLs |
| [`navree-cache-and-http-client`](navree-cache-and-http-client/SKILL.md) | The cache, locks, and outgoing HTTP with retries and an SSRF guard |
| [`navree-openapi`](navree-openapi/SKILL.md) | The OpenAPI 3.1 document, schemas and typed clients |
| [`navree-config`](navree-config/SKILL.md) | `.env`, `config/app.toml`, `APP_*` variables, the app key, typed sections |
| [`navree-testing`](navree-testing/SKILL.md) | `TestApp`, HTTP tests, database tests and fakes |
| [`navree-cli-generators`](navree-cli-generators/SKILL.md) | The `navree` CLI, every `make:*` generator and console commands |
| [`navree-deployment`](navree-deployment/SKILL.md) | Release builds, processes, systemd, proxies and health checks |
| [`navree-security-checklist`](navree-security-checklist/SKILL.md) | What to check before shipping |
| [`navree-workspaces`](navree-workspaces/SKILL.md) | Multi-tenancy at the workspace level |
| [`navree-dock`](navree-dock/SKILL.md) | Run PostgreSQL, Redis, Meilisearch, Mailpit, MinIO and NATS locally and in CI with `navree dock` |
| [`navree-social`](navree-social/SKILL.md) | Sign in with Google, GitHub, Apple and other providers; linked accounts |
| [`navree-webhooks`](navree-webhooks/SKILL.md) | Signed outgoing webhooks with retries, and verified incoming ones |
| [`navree-flags`](navree-flags/SKILL.md) | Feature flags: define, check, roll out by percentage or plan, kill, test |
| [`navree-helm`](navree-helm/SKILL.md) | The Helm admin panel: resources, fields, filters, actions, metrics, custom pages |

## Installing

Pick one:

- **With the skills CLI:** `npx skills add NavreeHQ/navree` installs every
  skill of this repository for the agents it finds in your project.
- **By hand:** copy the folders of this directory into your project's
  `.claude/skills/` (Claude Code) or `.agents/skills/` (other agents). Copy
  only the ones you need if you like, but keep `navree`: it indexes the
  rest.
- **With the navree CLI:** `navree skills:install` (part of Navigator,
  Navree's MCP server for coding agents) copies them into the current
  project.

Apps created with `navree new` also get an `AGENTS.md` with the project's
conventions (and a `CLAUDE.md` that imports it), so an agent knows the
layout, the generators and the checks to run before it loads any skill.

## Docs for agents

The skills link to the documentation at https://navree.com/docs/. Models
can read it as plain text: https://navree.com/llms.txt indexes every page,
https://navree.com/llms-full.txt holds all of them, and every page is
available as Markdown at `https://navree.com/docs/<section>/<page>.md`.

## Writing a skill

Keep a skill to one task. The `description` says what the skill does and
exactly when to use it, with the words a user would say. The body gives the
steps in order, the commands, a template that compiles, the checks to run,
the mistakes to avoid and links to the docs. Name only APIs that exist:
grep for each one in `crates/` before writing it down.
