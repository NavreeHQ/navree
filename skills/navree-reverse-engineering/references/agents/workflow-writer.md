# Role: workflow-writer

Phase 4, workflows, one module per dispatch. Describe every record that
moves through states: the states, every allowed move, who may make it,
what it does, and the timers.

## Inputs

- Scope: one module.
- `specs/04-specs/<module>/crud.md` (status fields), `service.md`,
  `workers.md`; `DATA_PROFILE.md` (status values)
- The old code that writes each status field.
- `references/spec-workflow.md`, `references/mistakes.md` (entries 5, 19)

## Outputs

- `specs/04-specs/<module>/workflows.md`, rows `draft` or `question`,
  with its closing sections (questions, deliberate changes, new map rows;
  `spec-module.md`)

## Method

1. Find the status fields: enum-like columns that change over a record's
   life (`status`, `stage`, `state`, flags like `approved`).
2. List every value in the data (from the profile) and group spellings
   into states.
3. Find every write to the field in the code (`rg` for the column name
   with `=`), and turn each into a transition: from which states, by which
   action, by whom, with which guard and effects.
4. Find moves that happened in the data but no code path allows: query the
   audit or history tables if any, or look for impossible combinations
   (paid with no payment, shipped before ordered). Each is a question.
5. Find timers: workers and schedules that move or remind based on time in
   a state.
6. Write who may make each move as roles and record scope; add any that
   are missing to the roles matrix as questions.

## Evidence rules

- Every state lists the old values it absorbs, from the profile.
- Every transition cites the code that performs it, or says it is new.
- "Anyone could do this" cites the missing check.

## Refuter

Grep for every write to the status field and match each to a transition.
Check every profile value maps to a state. Check every transition's
roles appear in `ROLES.md` and every screen action that changes status
names a transition.

## Return

Workflows written, states and transitions, moves the old product allowed
by accident, timers, questions added.
