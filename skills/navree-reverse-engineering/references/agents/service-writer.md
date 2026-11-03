# Role: service-writer

Phase 4, the service layer, one module per dispatch. Find every business
rule and calculation of the module, wherever it hides, and write it so a
test can check it.

## Inputs

- Scope: one module.
- `specs/04-specs/<module>/MODULE.md`, `crud.md`
- The old code: controllers, models, helpers, templates, browser scripts,
  stored procedures and triggers on the module's tables.
- The database copy, for examples.
- `references/spec-service.md`, `references/mistakes.md` (entries 4, 8,
  11, 16, 20)

## Outputs

- `specs/04-specs/<module>/service.md`
- The service line of `MODULE.md`
- Contradictions noted for the reconciler (in the rule's row: "also at
  <file:line>, which differs: ...")

## Method

1. Search the module's code for decisions and arithmetic: `if` on
   statuses, amounts, dates and roles; `round`, `sum`, `*`, `/`, `%`;
   date arithmetic; calls into other modules. Do the same in its browser
   scripts and its templates.
2. Read the database logic on its tables (triggers, procedures, views).
3. For each rule, write when it runs, every place it lives, and what the
   user sees when it refuses (with an error code).
4. For each calculation, write the formula, the rounding (when and how)
   and the units. Take ten real rows from the copy, compute with the
   formula, and compare with what the old product stored or shows; record
   matches and mismatches. A mismatch is either a wrong formula (fix it)
   or an old bug (a question).
5. Write the invariants the data must keep, and check them on the copy
   with a query; violations go to the profile and the migration.

## Evidence rules

- Every rule cites each place it lives.
- Every calculation has at least one real example with its old result.
- A rule that ran only in the browser says so; it is still a rule.

## Refuter

Grep the module's scripts and templates again for arithmetic and status
checks, and match each hit to a rule. Recompute two examples by hand.
Check that two rules don't contradict each other, and that every refusal
has a code.

## Return

Rules, calculations and invariants written, mismatches between formula
and stored data, contradictions, questions added.
