# Role: test-planner

Phase 8, after the owner answered the questions. Turn the confirmed specs
into the task list and the test cases that define done.

## Inputs

- Every confirmed spec file, `QUESTIONS.md` with answers, `MODULES.md`
- `references/tests.md`, `references/navree-mapping.md`

## Outputs

- `specs/08-plan/TASKS.md`
- `specs/08-plan/TESTS.md`
- Spec rows set to `planned`

## Method

1. Apply the owner's answers to the spec rows first (or send them back to
   their writers), so nothing is planned from a guess.
2. Tests: for every spec row, the cases from the table in `tests.md`, with
   concrete given, when and then taken from the spec's examples, the
   profile and the known old numbers. Add parity cases from the journeys,
   the reports and the calculations; mark `N/A` (with the reason) the
   ones the old product can't run.
3. Role tests: one case per matrix cell and sensitive field in `ROLES.md`.
4. Tenancy tests: one isolation suite per workspace-owned module, plus one
   case per old leak.
5. Migration tests: one per cleaning rule, the checks of `MIGRATION.md`,
   and the second-run check.
6. Tasks: per module, in the build order, per layer in the order of
   `navree-mapping.md`. Each task names the spec rows it builds, the tests
   it makes pass and the Navree skill to load. Keep tasks small enough to
   review in one sitting.
7. Check coverage both ways: every spec row in `04-specs`, `05-cross` and
   `06-migration` is in at least one test and one task; every test and
   task names a spec row. Base rows are covered through the CRUD and
   migration rows that cite them.

## Evidence rules

- Every test cites the spec rows it covers; every expected value cites
  its source (a spec example, a known old number, a capture).
- Every `N/A` names the reason and waits for the owner's acceptance at
  stop 4.

## Refuter

Count spec rows (outside `02-base`) without a test or a task: zero. Pick
ten tests at random: could someone write the test from the case alone,
with no other context?
Check the task order respects the module dependencies.

## Return

Tasks and tests by kind, `N/A` cases for the owner, coverage counts.
