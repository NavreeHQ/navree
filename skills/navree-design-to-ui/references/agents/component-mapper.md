# Role: component-mapper

Phase 3. First dispatch: the repeated components, into `COMPONENTS.md`.
Then one dispatch per screen: its page spec.

## Inputs

- Scope: the repeated components, or one screen (`DS-` id) and its page
  id.
- `design/01-inventory/INVENTORY.md`, `design/02-tokens/TOKENS.md`,
  `design/03-mapping/COMPONENTS.md` (for pages), `design/source/`.
- A reverse-engineering job's `screens.md`, `crud.md` and `lists.md` for
  the module, if they exist.
- `references/components.md`, `references/spec-page.md`,
  `references/mistakes.md`, `references/building-pages.md`.
- `crates/navree-ui/README.md`, `src/components/mod.rs` and
  `src/select/mod.rs`, to confirm a component exists before naming it.

## Outputs

- `design/03-mapping/COMPONENTS.md`: one `DC-` row per repeated
  component, with its navree-ui form and its states.
- `design/03-mapping/pages/<page>.md` (`spec-page.md`), rows `draft`,
  `question` or `gap`, with its closing sections.

## Method

1. For each element, find its row in `components.md`, then check the
   component and the builder methods you name in the code. Write the
   Navree form exactly enough to type.
2. Forms: write the `Form` struct's fields (type, doc comment label,
   `#[validate]`, `#[form]`) in the design's order. Tables: write the
   `TableRow` struct's fields and `#[col]` options. Each choice list
   names its source: an enum, or a query.
3. Where no component fits, write `gap GAP-n` with the module from the
   "Coming" table and the interim form you recommend. Never "custom".
4. Check every row against `mistakes.md` and write the entry numbers.
5. Write all six states and the phone section, including the ones the
   design lacks.
6. With reverse-engineering specs, cite their rows for data, actions,
   access and states instead of writing them again; add only the look.

## Evidence rules

- Every row cites the design (file and line, frame or `BASE-` shot) and,
  for data and access, the model, the service or the spec row.
- Every Navree name is one you found in the code.

## Refuter

Grep the toolkit for every component and method the file names. Compare
the page spec with the screen's `BASE-` shots: every visible element has
a row. Check the states and phone sections are complete.

## Return

Rows by status, gaps by module, mistakes found, questions added.
