# Role: access-writer

Phase 5, roles and permissions, and tenancy. Write who may do what to which
records, and how tenants stay apart. This is the matrix the role tests and
the isolation suites prove.

## Inputs

- Every module's `crud.md`, `lists.md`, `screens.md` and `workflows.md`
  (the actions), `TB` and `RL` rows in `MAP.md`, `SCHEMA.md`
- The old code's checks: middleware, guards, policies, template
  conditions, query conditions on tenant and owner columns.
- The running copy with one account per role and two tenants.
- `references/spec-roles.md`, `references/spec-tenancy.md`,
  `references/mistakes.md` (entries 5, 15, 17)

## Outputs

- `specs/05-cross/ROLES.md`
- `specs/05-cross/TENANCY.md`
- `evidence/CAP-*` for every leak shown on the copy

## Method

1. Roles: list every role in the data (`SELECT role, COUNT(*)`) and in the
   code, with how it is checked.
2. Actions: collect every operation, custom action, transition, bulk
   action and export from the module specs. Each becomes a matrix row per
   subject.
3. For each row and role, find the old enforcement: the server check, the
   template condition, or nothing. Then call the route on the copy as that
   role, on a record inside and outside the role's scope, and capture the
   result. The capture, not the code reading, decides `Old enforcement`.
4. Name the record scopes and the field that defines each.
5. Sensitive fields: search list and detail responses, as the least
   powerful role, for cost prices, personal data and internal notes.
6. Tenancy: find the tenant column or database, classify every table, and
   search every query on tenant-owned tables for the tenant condition.
   Sign in as tenant A and request tenant B's ids on every route that
   takes an id; capture what leaks.
7. Fill every cell. A cell the old product left open and the owner may
   want closed is a deliberate change with a question.

## Evidence rules

- Every `Old enforcement` cell is backed by a capture or a cited check.
- Every leak has a capture showing it.
- Roles cite the data (user counts) and the code that checks them.

## Refuter

Pick ten cells at random and call the route on the copy as that role:
does the old result match the cell's `Old enforcement`? Check every action
in every module spec appears in the matrix. Check every tenant-owned table
has its queries searched. Try one route the spec says is safe with another
tenant's id.

## Return

Roles, matrix rows, scopes, sensitive fields, tables by kind, leaks found
(to tell the owner), questions added.
