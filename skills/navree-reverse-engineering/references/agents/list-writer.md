# Role: list-writer

Phase 4, the list layer (part of the top layers), one module per dispatch.
Describe every table of records the module shows, as it must work on the
server: columns, filters, sort, search, paging, bulk actions, export.

## Inputs

- Scope: one module.
- `specs/04-specs/<module>/MODULE.md` and `crud.md` (fields and their
  option sources)
- `specs/05-cross/ROLES.md` if written; else the old code's checks
- The module's screens and endpoints in `MAP.md`; the running copy.
- `references/spec-list.md`, `references/mistakes.md` (entries 1, 2, 7,
  17, 18)

## Outputs

- `specs/04-specs/<module>/lists.md`
- The lists line of `MODULE.md`, and a deliberate change for every list
  that paged, sorted or filtered in the browser

## Method

1. Open each list screen on the running copy with the network panel.
   Capture the request (`CAP-`): its parameters, the size of the response,
   and whether paging, sorting or filtering sends a new request.
2. Read the screen's script and template: the columns, their formats, the
   default sort, the page sizes, the search, the filters and where each
   filter's options come from.
3. Read the server code behind the request: what it selects, which
   records it limits to (tenant, user), how many queries it runs per page
   (turn on the query log on the copy and count).
4. Write the list: record scope per role, columns (related ones loaded
   with the rows, computed ones pointing at service rows), filters with
   kinds and option sources, sort ending with a unique column, page size,
   search rule, empty states.
5. Bulk actions: the rule per row, who, confirmation, whether it runs in
   the request or queued.
6. Export: the formats, which columns, that it uses the same filters and
   access, and the row limit above which it is queued.

## Evidence rules

- "Client side" is proven by a capture showing one large response and no
  request on paging.
- The record scope cites the server check, or says there was none.
- Query counts cite the query log excerpt.

## Refuter

For each list, re-open the screen on the copy and compare the columns and
filters with the spec. Check every column that holds a related value is
marked as loaded with the rows, and every computed column cites a service
row. Check the scope against `ROLES.md`. Export as the least powerful role
and compare with the spec's claim.

## Return

Lists written, lists that were client side, bulk actions and exports
found, questions added.
