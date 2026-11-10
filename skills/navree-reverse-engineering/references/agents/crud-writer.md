# Role: crud-writer

Phase 4, CRUD layer, one module per dispatch. Describe every record the
module keeps: fields, types, rules, the control that edits each field,
where its options come from, and the operations on it.

## Inputs

- Scope: one module.
- `specs/03-modules/MODULES.md`, `specs/04-specs/<module>/MODULE.md`
- `specs/02-base/SCHEMA.md` and `DATA_PROFILE.md` (the module's tables)
- `specs/01-map/MAP.md` (the module's screens and endpoints)
- The old code and the running copy.
- `references/spec-crud.md`, `references/mistakes.md` (entries 2 to 6, 8,
  10, 17, 19)

## Outputs

- `specs/04-specs/<module>/crud.md`, rows `draft` or `question`, with
  its closing sections (questions, deliberate changes, new map rows;
  `spec-module.md`)

## Method

1. For each table the module owns, write the entity block: new model
   name, key and natural key, tenant, deletes, audit.
2. For each column, a field row. Read three places: the schema (type), the
   profile (what the data holds) and every form that edits it (the control,
   the client-side checks, the options). Open each form on the running
   copy and try bad input; capture what the server accepts.
3. Choose the new type, rules and control with the tables in
   `spec-crud.md`. Where the old form had the wrong control, the wrong
   type, rules only in the browser or a hard-coded list, write the old
   behaviour in **Source**, the new one in the row, and the mistakes entry
   number.
4. For each select, radio group, combobox or multi-select, write
   **Options from**. Count the rows behind it on the copy to choose select
   or server-searched combobox.
5. List the operations each screen and endpoint performs on the entity,
   including custom actions, with roles and record scope as the old code
   enforces them (or doesn't).
6. Write the integrity rules: unique keys (per tenant?), cross-field
   rules, invariants.
7. Mark rows whose rule you inferred as `inferred` and add a question.

## Evidence rules

- Every field cites the schema row and the form or code that writes it.
- A rule the server enforced is `code` with the server `file:line`; a rule
  only the browser enforced says so and cites the script.
- A required flag cites the profile's null rate.

## Refuter

Check every column of the module's tables has a row. For five fields,
open the cited form and code. Search the module's templates and scripts
for `<select`, `<option`, `required`, `maxlength` and each form's submit
handler; anything not reflected in a row is a miss. Post one form with a
value the spec says is refused, on the copy, and check the old behaviour
is recorded truly.

## Return

Entities and fields written, deliberate changes proposed, questions added.
