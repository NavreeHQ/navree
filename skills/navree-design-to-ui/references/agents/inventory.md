# Role: inventory

Phase 1, the whole design or a group of screens per dispatch. List what
the design holds, with ids. Describe; don't judge or fix.

## Inputs

- Scope: the design, or a list of screens.
- `design/source/` (served), `design/00-intake/INTAKE.md`.
- `references/spec-inventory.md`, `references/tokens.md` (the extraction
  commands).

## Outputs

- `design/01-inventory/INVENTORY.md`, rows `draft` or `question`, with
  its closing sections (questions, deviations, gaps). With several
  inventory agents, each writes `INVENTORY-<group>.md`, and one more
  merges them and the repeated components into `INVENTORY.md`.
- `design/01-inventory/shots/BASE-*.png`: every screen, and every state
  the design draws, at 1440 and 390 pixels (`verification.md`, part 2).

## Method

1. Run the commands of `spec-inventory.md` and keep their output and
   counts at the top of the file.
2. Open each screen in the served copy. Screenshot it at both widths.
   Write its row: what it shows, the states it draws, the states it
   lacks.
3. For each screen, list every navigation item, form field, table,
   action, figure and chart. One row each.
4. For each figure and list, write what data it implies. A guess is
   `inferred`, `low` or `medium`, and a question.
5. Find the patterns used on two or more screens and give each a `DC-`
   row.
6. Collect every raw value (colour, font, size, gap, radius, shadow,
   duration) with its count and where it appears.

## Evidence rules

- Every row cites a file and line, a frame name, or a `BASE-` shot.
- Count from commands, never from memory. A number with no command is a
  question.
- Don't invent screens, fields or states the design doesn't show. What
  is missing goes in "States missing".

## Refuter

Re-run the commands and compare counts. Open three screens at random and
list everything on them; every item must have a row. Check that no row
describes something the design doesn't show.

## Return

Counts of screens, components, fields, tables, actions and raw values;
states missing per screen; questions added.
