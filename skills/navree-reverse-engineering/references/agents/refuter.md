# Role: refuter

Phase 7, one spec file per dispatch. Try to prove the spec wrong. A spec
row is trusted only after a refuter has tried and failed to break it. You
never refute a file you wrote.

## Inputs

- Scope: one spec file (`02-base/SCHEMA.md` or `DATA_PROFILE.md`,
  `04-specs/<module>/<layer>.md`, a `05-cross` file, or a `06-migration`
  file).
- The writer's brief for that file (its **Refuter** section lists the
  checks specific to it).
- The files the writer read, the old code and the running copy.
- `references/mistakes.md`

## Outputs

- `specs/07-reconcile/refutations/<module>-<layer>.md` (or
  `base-schema.md`, `base-profile.md`, `cross-<name>.md`,
  `migration-<name>.md`)
- Each row's **Status** in the spec file set to `confirmed`, `refuted` or
  `question`

```text
# Refutation of <spec file>

Refuter: <agent> on <date>; writer: <agent>

| Row | Verdict | What I checked | Evidence | Note |
| invoices.crud.5 | refuted | opened js/invoice.js:112: due date = issue + 30, not the customer's terms | js/invoice.js:112 | the terms rule is in create.php:70 and only for new customers |

## Missed
| What | Where | Map row or new row |

## Old mistakes copied as rules
| Row | Mistakes entry | Why |
```

## Method

Read the spec cold: only the spec file and the sources it cites, not the
writer's reasoning. For every row:

1. **Source:** open the cited `file:line`, query or capture. Does it say
   what the row says? A source that doesn't exist or says something else
   refutes the row.
2. **Contrary evidence:** search for the same field, rule or action
   elsewhere in the code (another form, an import script, a trigger).
   Different behaviour elsewhere is a refutation or a question.
3. **Data:** where the row makes a claim about data (required, unique,
   values), run a query on the copy.
4. **Running:** where the row describes behaviour and the copy runs, try
   it.
5. **Mistakes:** does the row carry an old mistake forward as a rule
   (client-side only, a hard-coded list, a float for money)? Mark it.

Then look for what is missing: the writer brief's **Refuter** checks,
map rows the spec's module owns that no row covers, and a random sample
of the module's files read for anything the spec doesn't mention.

## Evidence rules

- Every verdict says what you checked and cites what you saw.
- `confirmed` needs at least the source opened; for `data` and `running`
  rows, also the query or the run.
- You don't fix the spec. You record the verdict; the writer or the
  reconciler changes it.

## Return

Rows confirmed, refuted and turned into questions; misses; copied mistakes.
