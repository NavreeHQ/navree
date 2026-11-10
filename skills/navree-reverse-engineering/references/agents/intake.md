# Role: intake

Phase 0. Learn what the product is and set up a safe way to read and run
it. Everything later depends on this file being true.

## Inputs

- The owner's description of the product and why it is being rebuilt.
- Access to the old code (a clone, read only) and a database backup.
- Any documents: manuals, help pages, old specs, tickets, screenshots.

## Outputs

- `specs/00-intake/INTAKE.md`
- `specs/STATUS.md` (created, phase 0 filled in)

`INTAKE.md` sections:

```text
# Intake: <product>

## The product           purpose, users and roles in plain words, size (users, tenants, rows)
## Why rebuild           the owner's reasons; what must not change
## Stacks                languages, frameworks, versions, database engine and version, servers, cron, queues
## Where things are      code repository and branch, backup and its date, documents, logs
## Running it safely     the copy, masked data, mail catcher, payment sandbox, blocked network, schedulers
## What can't run        each part and why; what replaces it (captures, the owner's screenshots)
## Accounts              one test account per role on the copy, who made them
## The spec folder       its path, its repository
## Risks seen            secrets in the code, known leaks, data the owner worries about
```

`STATUS.md`:

```text
| Phase | State | Role | Files | Gate | Owner sign-off |
| 0 Intake | done | intake | 00-intake/INTAKE.md | runs: yes (CAP-001) | |
```

## Method

1. Read the repository's top level, its dependency files (`composer.json`,
   `package.json`, `Gemfile`, `requirements.txt`, `*.csproj`, `pom.xml`),
   its configuration and deployment files, and its README. Record versions.
2. Restore the backup into a local database server of the same engine and
   major version. Create a read-only user for the profiler and the import.
   Mask personal data if the owner asks (names, emails, phones), keeping
   formats and duplicates intact so the profile stays honest.
3. Start the old product against the copy, with mail sent to a local
   catcher (Mailpit from `navree dock` works), payment keys swapped for
   sandbox ones, outbound network blocked or pointed at nothing, and cron
   and queue workers off until a test needs them.
4. Sign in as each role (create test accounts on the copy if needed) and
   capture one request per role (`CAP-001` ...).
5. Write what can't run and what stands in for it.
6. Search the code for secrets (`mistakes.md`, entry 14) and note where
   they are, never their values.

## Evidence rules

- Versions come from files or commands, cited.
- "It runs" is proven by a capture or screenshot per role.
- Nothing is copied from production; the copy's backup date is recorded.

## Refuter

Check that the copy is not production (host, credentials), that mail and
payments can't leave the machine (send a test mail and find it in the
catcher), that each role's capture exists, and that every "can't run" has a
reason.

## Return

The files written, the stacks found, what runs and what can't, any
secret or leak to tell the owner.
