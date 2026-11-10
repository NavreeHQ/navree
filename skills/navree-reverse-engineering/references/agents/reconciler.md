# Role: reconciler

Phase 7, after every spec file has a refutation. Read the whole spec set
and find what doesn't fit together: contradictions between files, map rows
nobody owns, guesses, and refuted rows not yet fixed. Write the questions
only the owner can answer.

## Inputs

- Every file under `specs/` and every refutation.
- `references/mistakes.md`

## Outputs

- `specs/07-reconcile/RECONCILE.md`
- `specs/07-reconcile/QUESTIONS.md`, collected from every spec file's
  `## Questions` and from every row left `question`
- Each module's `MODULE.md`: the layers table (status, rows, questions)
  and the deliberate changes, collected from the layer files
- `MAP.md`: the rows from every `## New map rows`, under "Added later";
  `owned` for rows covered by confirmed spec rows

```text
# RECONCILE.md

## Contradictions
| ID | Rows | What disagrees | Evidence | Resolution or question |

## Unmapped
| Map row | Why nobody owns it | Proposed owner or question |

## Guesses
| Row | Known by inferred | What would settle it | Question |

## Refuted rows
| Row | Refutation | Fixed in the spec? |

## Counts
| Module | Rows | Confirmed | Refuted | Question | Deliberate changes |
```

```text
# QUESTIONS.md

| ID | Question | Why it matters | Options | Recommendation | Rows | Asked | Answer | Date |
```

## Method

1. Contradictions: the same field typed differently in two specs; a role
   allowed an action in a screen spec but not in `ROLES.md`; a worker's
   rule unlike the service's; a report metric unlike the service
   calculation; a migration mapping that loses a state the workflow has.
2. Unmapped: every map row still `open`.
3. Guesses: every `inferred` row and every `low` confidence row.
4. Refuted rows: each must be fixed by its writer or become a question.
5. Deliberate changes: list them all, by module, for the owner.
6. Write each question so the owner can answer it in a word: the options,
   what each means for users and data, and your recommendation with its
   reason. Group them by module; put the ones that block the most rows
   first.

## Evidence rules

- Every contradiction cites both rows and their sources.
- Every question names the rows it unblocks.

## Refuter

The owner is this file's refuter at stop 3. Before handing over, check
that every `question` status in the specs has a `Q-` row, and every `Q-`
row names at least one spec row.

## Return

Counts of contradictions, unmapped rows, guesses and questions; the
questions blocking the most rows.
