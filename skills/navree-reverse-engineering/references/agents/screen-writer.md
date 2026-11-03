# Role: screen-writer

Phase 4, the screen layer (the top of the stack), one module per dispatch.
Describe every page and dialog: purpose, contents, actions, states, who
sees it, and the journeys through it.

## Inputs

- Scope: one module.
- `specs/04-specs/<module>/MODULE.md`, `crud.md`, `lists.md` (write after
  them, or point at their row ids as they appear)
- The module's `SC` and `RT` rows in `MAP.md`; the running copy, signed in
  as each role.
- `references/spec-screen.md`

## Outputs

- `specs/04-specs/<module>/screens.md`
- New journeys added to the module's screens and, for each journey that
  spans modules, a note in `MODULES.md`

## Method

1. Visit each screen as each role on the copy. Screenshot it (`SHOT-`),
   note what each role sees and what is hidden.
2. List every button, link, menu item and keyboard shortcut. For each,
   find what it calls (the request it sends, the handler) and the state
   that enables it. A control that does nothing, or only shows a message,
   is recorded as such.
3. Force the states: an empty table (a tenant with no rows on the copy),
   a slow or failed request (stop the database briefly, or block the
   endpoint), a forbidden screen (open it by URL as a role without access,
   and as another tenant). Record what the old product shows.
4. Write the journeys: ask the owner for the tasks people do daily and
   monthly; confirm each by walking it on the copy and capturing its
   requests.
5. Check the screen at a phone's width (375 pixels) and record what
   breaks; the owner decides whether phones matter.

## Evidence rules

- Every action cites the control in the template and the handler it
  reaches.
- Every state cites a screenshot or capture of the old product.
- Every journey is walked on the copy, not only described.

## Refuter

Walk each journey on the copy following only the spec. Compare one
screenshot per screen with the spec's contents and actions. Search the
module's templates for buttons and links missing from the spec.

## Return

Screens and journeys written, dead controls found, states the old product
lacked, questions added.
