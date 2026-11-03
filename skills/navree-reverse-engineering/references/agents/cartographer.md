# Role: cartographer

Phase 1. List everything the product has, as checklists with ids, so later
phases can't skip anything. You map; you don't judge.

## Inputs

- `specs/00-intake/INTAKE.md`
- The old code (read only) and the running copy.

## Outputs

- `specs/01-map/MAP.md`

```text
# Map of <product>

Commit or snapshot: <...>

## Routes (RT), <N> total
Command: <the command that listed them>
| ID | Method | Path | Handler | Screen | Status | Owner |

## Screens (SC)          | ID | Name | Route | Template or component | Status | Owner |
## Endpoints (EP)        | ID | Method | Path | Handler | Called from | Status | Owner |
## Tables (TB)           | ID | Table | Rows | Status | Owner |
## Jobs (JB)             | ID | Job, script or trigger | Started by | Status | Owner |
## Schedules (SH)        | ID | Expression | Command | Server zone | Status | Owner |
## Integrations (IN)     | ID | System | Direction | Code | Status | Owner |
## Roles (RL)            | ID | Role | Where defined | Users | Status | Owner |
## Reports and exports (RP), Notifications (NT), File stores (FL), Settings (ST)

## Added later
| ID | Added by phase | Why it was missed |
```

**Status** is `open` here; later phases set `owned`, `closed` or
`dropped`. **Owner** is filled by the module planner.

## Method

Derive every list from a command and keep the command with its count:

- Routes: the framework's route list (`php artisan route:list --json`,
  `rails routes`, `python manage.py show_urls`, `dotnet` endpoint
  discovery), else a search for route definitions, else the web server's
  rewrite rules and the PHP files under the document root.
- Screens: templates and front-end pages; each route that renders HTML.
- Endpoints: routes answering JSON or forms; AJAX calls found in scripts
  (`fetch(`, `$.ajax`, `axios.`) matched to routes.
- Tables: `information_schema.tables` on the copy, with row counts.
- Jobs: queue consumers, daemons, database triggers and events, scripts in
  `bin/`, `cron/`, `scripts/`.
- Schedules: crontab files, the framework's scheduler, Windows tasks, the
  database's event scheduler. Ask the owner for crontabs on servers.
- Integrations: outgoing HTTP calls, SDK clients, SFTP, SMTP, inbound
  webhook routes (`mistakes.md`, entry 13 searches).
- Roles: role columns and tables (`SELECT DISTINCT`), groups, hard-coded
  checks.
- Reports, notifications, files, settings: report pages and export
  buttons; every mail template and send call; upload handlers and storage
  paths; settings tables and config keys.

Sort each list. Compare with the running copy's menus: every menu item
reaches a mapped route; a route nothing links to is still mapped.

## Evidence rules

- Every section has the command that produced it and its count. A list
  made from memory or by reading "the main files" is not a map.
- A section that comes out empty says why ("no queue: no consumer,
  library or table found; searches listed").

## Refuter

Re-run each command and compare counts. Sample ten files at random from
the code and check that each one's routes, queries and calls appear in the
map. Click through the running copy's menus as the most powerful role and
check every page is mapped.

## Return

Counts per section, empty sections with their reasons, anything
surprising to show the owner at stop 1.
