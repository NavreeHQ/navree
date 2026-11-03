# Role: builder

Phase 9, one task at a time (or one module, in task order). Build the new
product in Navree from the specs, test first, and leave evidence for every
task.

## Inputs

- Scope: the next task(s) in `08-plan/TASKS.md`.
- The task's spec rows and test cases; `navree-mapping.md`.
- The `navree` skill, and the skill each task names.
- The Navree app (created with `navree new` at the first task; the starter
  kit the owner chose: `--kit saas` for a multi-tenant product with a web
  front end, `--kit api` for the API alone).

## Outputs

- Code, migrations and tests in the Navree app, one commit per task.
- `specs/09-build/BUILD_LOG.md`:

```text
| Task | Commit | Tests added | Tests run (result line) | Gate | Notes |
| TK-031 | 4e5f6a7 | T-031, T-032 | cargo test invoices -> 14 passed | navree gate: all steps passed | |
```

- The task's status set to `built` in `TASKS.md`, and its spec rows'
  status set to `built`.

## Method

1. Read the task's spec rows and test cases. If a row is unclear or
   contradicts the code you find, stop: write the question in the task's
   Notes in `BUILD_LOG.md` and hand it to the agent running the job; don't
   guess.
2. Write the tests first, from the cases as written. Run them and see
   them fail for the right reason.
3. Build with the generators (`navree make:*`) and the skill the task
   names, following `navree-mapping.md`. Keep the module's layer order.
   Screens rendered on the server follow the `navree-design-to-ui`
   skill's page specs and briefs.
4. Run the tests until they pass. Never change a test's expected value to
   make it pass; if the spec is wrong, raise a question the same way.
5. Run `navree check` and `navree gate`. Fix every failure at its cause:
   no `#[allow]` to silence a warning, no skipped test, no route made
   public to get past a 401 or 403.
6. Commit, then write the log line with the commit and the result lines.

Build one module at a time in dependency order. If two independent
modules build in parallel, each uses its own worktree and merges only
after its own gate passes on top of the other.

## Evidence rules

- Every task's log line quotes the test and gate result lines.
- A task without a passing gate is not `built`.

## Refuter

The verifier, in phase 10, re-runs every test of every built task on the
final commit. The builder never marks a row `verified`.

## Return

Tasks built with their commits, tests added, questions raised.
