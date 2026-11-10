# Role: cross-cutting-writer

Phase 5, what every module shares: authentication, audit, notifications,
settings, files, search, languages and time zones.

## Inputs

- `MAP.md` (`NT`, `FL`, `ST` rows), `INTAKE.md`, `SCHEMA.md`,
  `DATA_PROFILE.md`
- The old code's sign-in, mail, upload, settings, search and localization
  code; the mail catcher's messages from runs on the copy.
- `references/spec-cross-cutting.md`, `references/mistakes.md` (entries 9,
  14, 21, 23)

## Outputs

- `specs/05-cross/CROSS_CUTTING.md`, rows `draft` or `question`, with its
  closing sections (`spec-module.md`)

## Method

1. Authentication: sign-in methods, the password hash scheme (count users
   per scheme with a query on the hash prefix), session length, lockout,
   two-factor, reset and invitations. Note what must happen to passwords
   at migration and ask the owner.
2. Audit: what is logged today (log tables, `modified_by` columns), what
   the owner needs, and who reads it.
3. Notifications: every mail, SMS and message: the trigger, recipients,
   template, language, opt-out. Compare the code with what the mail
   catcher received during the screen and worker runs.
4. Settings: every settings table row, config key and constant that
   changes behaviour; its level (product, organization, tenant, user) and
   who changes it.
5. Files: upload handlers, where files are stored and named, who may
   download them; files reachable without signing in are a finding.
6. Search: every search box: what it searches, how it matches, and whether
   it respects tenant and role scope.
7. Languages and formats; time zones: the server's and database's zones,
   how each date column was written, the zone per tenant and per user.

## Evidence rules

- The hash scheme and the server zone come from queries, cited.
- Each notification cites its send call and, where it ran, the caught
  message.
- Each file finding cites the path and a capture of the unauthenticated
  download if one works.

## Refuter

Send each notification's trigger on the copy and compare the caught
message with the spec. Check every section is filled or `none` with a
reason. Re-run the hash scheme and zone queries.

## Return

Sections written, decisions the owner must make (passwords, zones,
retention), findings to tell the owner, questions added.
