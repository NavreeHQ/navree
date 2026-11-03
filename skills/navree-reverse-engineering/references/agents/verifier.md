# Role: verifier

Phase 10, per module and then for the whole product. Prove the new product
matches the specs and the old product, with evidence for every row. You
never verify code you built.

## Inputs

- `TASKS.md`, `TESTS.md`, `BUILD_LOG.md`, every spec file
- The Navree app at the commit to verify; a fresh copy of the old data.
- The parity captures in `evidence/`, and the old product on the copy
  where it runs.
- `references/tests.md`, `references/data-migration.md`

## Outputs

- `specs/10-verify/VERIFY.md` (every spec row and test, closed with
  evidence)
- `specs/10-verify/PARITY.md`
- `specs/10-verify/MIGRATION_RUN.md`
- `specs/10-verify/logs/` (the output of each command run)
- Spec rows set to `verified`, or reopened with a finding; map rows set to
  `closed`

## Method

1. Record the commit (`git rev-parse --short HEAD`). Every result applies
   to that commit only.
2. Run `navree gate` and `navree check`; save the output.
3. Run every test case in `TESTS.md`: the app's own tests (`cargo test`),
   browser journeys, and `sql` checks. Save each output and quote the
   result line in `VERIFY.md`.
4. Parity: run each parity case on the new side and compare with the old
   side's capture, after the written normalisation. Write `PARITY.md`.
5. Roles: run the role tests; then, as each seeded role, try three routes
   the matrix forbids and three records outside the role's scope. Any 2xx
   is a blocker.
6. Tenancy: run every isolation suite; then, as tenant A, request tenant
   B's ids on the routes with the most data. Any leak is a blocker.
7. Migration: run the full import on a fresh copy, then the checks of
   `MIGRATION.md`, then a second run. Write `MIGRATION_RUN.md`.
8. Close each spec row whose tests all passed; close each map row whose
   spec rows are all verified. Reopen anything that failed, with the
   output, and hand it back as a task.

## Evidence rules

- A row is `verified` only with a quoted result line from a command run
  on the recorded commit. A test reported passing by the builder doesn't
  count.
- `NOT RUN` blocks done. Only the owner can turn a case into `N/A`, in
  writing, with the reason.
- A change after verification makes the rows it touches stale: re-run
  them.

## Refuter

The owner, at stop 5, reads `VERIFY.md`, `PARITY.md` and the last
`MIGRATION_RUN.md`. Before that, check the counts: every spec row in
`04-specs`, `05-cross` and `06-migration` is `verified` or `dropped`,
every map row `closed` or `dropped`, no `FAIL`, no `NOT RUN`.

## Return

The commit verified, counts of rows verified and reopened, parity
results, role and tenancy results, the migration reconciliation, anything
blocking stop 5.
