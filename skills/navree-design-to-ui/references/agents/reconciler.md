# Role: reconciler

Phase 4, after every refuter. Bring the files together, find what nobody
covered, and prepare stop 2.

## Inputs

- Every file in `01-inventory`, `02-tokens` and `03-mapping`, and every
  refutation.
- `INTAKE.md`, `references/preservation.md`, `references/components.md`.

## Outputs

- `design/04-reconcile/QUESTIONS.md`: every open question, with options
  and a recommendation.
- `design/04-reconcile/DEVIATIONS.md`: every deviation, with its picture
  and an empty Owner column.
- `design/04-reconcile/GAPS.md`: every gap, by module of the "Coming"
  table, with the pages it touches and the interim form recommended.
- `design/04-reconcile/RECONCILE.md`: contradictions and uncovered rows.

```text
# Gaps

| ID | Module coming | Pages | What the design needs | Interim form | Owner |
| GAP-01 | Select, Combobox, Command | deals-new, deals-edit | company picker over 3,000 companies | wait; or a search page that links back | |
| GAP-04 | HTML error pages | every page | forbidden and not-found states inside the shell | Navree's JSON error until it lands | |
```

## Method

1. Collect every `## Questions`, `## Deviations` and `## Gaps` section.
   Give each item its id; merge duplicates.
2. Tick off the inventory: every screen, field, table, action, figure
   and raw value maps to a row in a page spec, `COMPONENTS.md` or
   `TOKENS.md`. List what doesn't.
3. Compare the page specs with each other: the same component mapped two
   ways, the same data from two sources, the same action on two routes.
4. Cut each deviation's picture from the shots (`04-reconcile/dv/`).
5. Write a recommendation for every question and gap.

## Evidence rules

- Every contradiction cites both rows.
- Every gap names its module in the "Coming" table, checked against the
  toolkit's code on this date.

## Refuter

The owner, at stop 2. Before that, check the counts: no row `refuted`,
every inventory row covered, every deviation with a picture.

## Return

Questions, deviations and gaps by count; uncovered rows; contradictions.
