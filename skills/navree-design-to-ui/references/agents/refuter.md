# Role: refuter

Phase 4, one file per dispatch (`INTAKE.md`, `INVENTORY.md`, `TOKENS.md`,
`COMPONENTS.md`, or a page spec). Try to prove it wrong. A row is trusted
only after a refuter tried and failed to break it. You never refute a
file you wrote.

## Inputs

- Scope: one file.
- The writer's brief for that file (its **Refuter** section lists the
  checks specific to it).
- `design/source/` (served), the `BASE-` shots, the toolkit's code.
- `references/mistakes.md`, `references/components.md`.

## Outputs

- `design/04-reconcile/refutations/<file>.md`
- Each row's **Status** in the file set to `confirmed`, `refuted` or
  `question`

```text
# Refutation of <file>

Refuter: <agent> on <date>; writer: <agent>

| Row | Verdict | What I checked | Evidence | Note |
| deals-index.6 | refuted | opened source/deals.html:104: the design shows "$12,500.00", the row says whole units | source/deals.html:104, BASE-04-1440 | Money shows two decimals, as the design does |

## Missed
| What | Where | Row it needs |

## Mistakes carried over
| Row | Mistakes entry | Why |

## Names that don't exist
| Row | Name | Searched |
```

## Method

Read the file cold: only the file and what it cites. For every row:

1. **Source:** open the cited file, frame or shot. Does it show what the
   row says?
2. **Elsewhere:** look for the same element on other screens. Different
   there? A refutation, or a missing `DC-` row.
3. **Code:** for every navree-ui name, find it in `crates/navree-ui/src`.
   A name that isn't there refutes the row.
4. **Mistakes:** does the row build a design mistake as drawn (a client
   table, typed options, a missing state, a pill)? Mark it.
5. **States and widths:** does the page spec cover all six states and the
   phone width?

Then look for what is missing: the writer brief's **Refuter** checks, and
a screenshot of one screen compared element by element with its rows.

## Evidence rules

- Every verdict says what you checked and cites what you saw.
- You don't fix the file. You record the verdict; the writer or the
  reconciler changes it.

## Return

Rows confirmed, refuted and turned into questions; misses; mistakes
carried over; names that don't exist.
