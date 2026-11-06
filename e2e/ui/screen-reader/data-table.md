# Data table screen reader script

## Purpose

The data table is one table with a toolbar, column headers that sort, popovers for filters and columns, row boxes, a bar for actions on the chosen rows, a menu on each row, and a footer with a count and page links. Most of it updates in place without a page load, which is where a reader can go quiet or speak twice. This script settles what is said for the table and its headers, a sort, a filter and a search, the selection count, the confirm dialog, the row menu, paging, the empty, loading and failed states, and the page with JavaScript off.

## Page to open

The data table proof app, which runs on a database (see "Start the proof apps" in [README.md](README.md)):

```sh
DT_DATABASE_URL=postgres://... ui-table-proof seed 1000000     # once
DT_DATABASE_URL=postgres://... PORT=8097 ui-table-proof serve  # http://127.0.0.1:8097
```

| Route | Used for |
|---|---|
| `/deals?per_page=10` | Most steps. 137 deals, 10 to a page, so 14 pages. |
| `/extras` | Keys of the grid, rows that cannot be ticked, an action that is denied. |
| `/empty`, `/one` | No rows, and one row. |
| `/failed`, `/skeleton` | Rows that could not load, and a first load on its way. |

The deal names on these pages come from the proof app's own data (for example "Gulati renewal 10"). The steps name a row by its place, not by its name.

## Before you start

1. Reset the data so the counts below hold. In a terminal, run `curl -X POST http://127.0.0.1:8097/__t/reset`. Run it again before part D and before part H.
2. Use a window at least 1280 pixels wide, so no column is hidden. JavaScript is on, except in part H.
3. Open the speech viewer (NVDA) or the caption panel (VoiceOver). See the README.
4. NVDA: start in browse mode. VoiceOver: start with VO on the page, not interacting.
5. With the data reset, the numbers are: 137 deals; the stage "Won" has 28; a search for "gulati" finds 26.

## Steps

### A. The table and its headers

**1. Find the table**
- Keys: NVDA: open `/deals?per_page=10`, then press T (next table). VoiceOver: open the page, press VO+U, choose Tables with the arrow keys, then press Enter.
- The page: the cursor is on the table. The table sits in a scrollable box that is a tab stop.
- The reader says: a table named "Deals in the pipeline", with its row and column count (the reader's own wording). The scroll box has the same name and may be said as a region. Each element's name is said once, not three times.

**2. Read the column headers**
- Keys: NVDA: Ctrl+Alt+Right arrow along the header row, starting from the first header. VoiceOver: VO+Shift+Down arrow to interact with the table, then VO+Right arrow along the header row.
- The page: nothing changes.
- The reader says, in order: "Select all on this page", a check box, not checked; then "Deal", "Company", "Stage", "Amount", "Closed", "Active" and "Actions" (the narrow column of details links has no header). Each is said as a column header. The page's default sort is Closed, newest first, then Deal: "Closed" conveys descending with rank 1 and "Deal" conveys ascending with rank 2 (the rank number may be part of the name or said after it). The other headers convey no sort state. Each sortable header also holds a button such as "Deal options", which the reader may say after the name.

**3. Read one row**
- Keys: NVDA: Ctrl+Alt+Down arrow to the first data row, then Ctrl+Alt+Right arrow across it. VoiceOver: VO+Down arrow once, then VO+Right arrow across the row.
- The page: nothing changes.
- The reader says, across the row: a check box named "Select " followed by the deal's name, not checked; a link "Show details", collapsed; the deal's name as a link; then the company, stage, amount, closed date and active value, each with its column header (the reader says the header when the column changes). The deal's name is said once for its cell, not twice.

### B. Sorting

**4. Sort by Amount from the keyboard**
- Keys: both readers: Tab to the link "Amount" in the header row, then Enter. VoiceOver: VO+Space also works.
- The page: the rows are replaced in place with the amount order, the address gets `sort=amount`, and focus stays on the "Amount" link. While the answer is on its way the table is marked busy.
- The reader says: the page announces nothing about the sort: no live region speaks and the page title is not read again. Pass if the reader is quiet, or says only its own short phrase for a pressed link. Fail if the whole table, the page title or the count is read out again, or the same phrase is said twice.

**5. Hear the sort state in the header**
- Keys: NVDA: browse mode, Ctrl+Alt+Up arrow from a cell of the "Amount" column to its header. VoiceOver: interact with the table, VO+Up arrow to the header row, then VO+Left or Right arrow to "Amount".
- The page: the "Amount" header now has an ascending sort state. "Deal" and "Closed" have none, and no rank numbers are shown.
- The reader says: "Amount", column header, sorted ascending. "Deal" and "Closed" no longer say a sort state.

**6. Reverse the sort**
- Keys: both readers: Tab (or Shift+Tab) back to the "Amount" link, then Enter.
- The page: the rows reverse, the address changes to `sort=-amount`, and focus stays on the link.
- The reader says: as in step 4, nothing extra. Moving to the "Amount" header as in step 5 now conveys sorted descending.

### C. Filters and search

**7. Open the options of a column**
- Keys: both readers: Tab to the button "Stage options" in the "Stage" header, then Enter (VoiceOver: VO+Space).
- The page: a popover opens under the button and focus moves into it. It holds the links "Sort ascending", "Sort descending", "Add to sort" (the table is already sorted by Amount) and "Hide column", and the stage choices.
- The reader says: the button "Stage options" with a popup (and collapsed, where the reader reports it). After it is pressed, a dialog named "Stage options" is spoken. Moving with Tab inside it says each link by name, then check boxes named "Lead", "Qualified", "Proposal", "Won" and "Lost", none checked.

**8. Filter by a stage and hear the new count once**
- Keys: both readers: Tab to the check box "Won", then Space.
- The page: after a short pause the rows behind the popover are replaced. A chip "Stage: Won" appears above the table. The popover stays open. The count under the table reads "Showing 1 to 10 of 28 results".
- The reader says: "Won", check box, checked. Then, within about two seconds, the count "Showing 1 to 10 of 28 results", once. It is a polite live region, so it may follow whatever is being said. Fail if it is never said, or is said twice. VoiceOver may hold it until the table's busy mark ends; it must still be said.

**9. Close the popover and take the filter off**
- Keys: both readers: Escape. Then Tab to the chip link and press Enter.
- The page: Escape closes the popover and focus returns to the "Stage options" button. The chip is a link; pressing it removes the filter and the count returns to 137.
- The reader says: after Escape, "Stage options", button. The chip link is named "Remove filter Stage: Won". After it is pressed, "Showing 1 to 10 of 137 results" is said once.

**10. Search and hear the result count**
- Keys: both readers: Tab to the search box, named "Search". Type `gulati` in one go. NVDA switches to focus mode in the box by itself; if the letters are not typed, press NVDA+Space.
- The page: after the typing pauses (a tenth of a second or more), the rows are replaced with the matches. Focus stays in the box and the typed text is kept. A chip "Search: gulati" appears. The count reads "Showing 1 to 10 of 26 results".
- The reader says: "Search", search edit, and each typed letter. After the pause, the count "Showing 1 to 10 of 26 results" is said once. If you type slowly, the count may be said for the letters typed so far; each different count is said once, the last one is 26, and none is repeated.

**11. Search with no match, then clear**
- Keys: both readers: in the search box, select all (Ctrl+A) and type `zzz`. Then, with the cursor on the table (NVDA: T; VoiceOver: VO+U, Tables), read the message. Then Tab to the link "Clear filters" and press Enter.
- The page: the table body shows one cell with "No results match these filters.", the line "Change or clear the search and filters." and a link "Clear filters" (the same link is also over the table). The count under the table is gone, because an empty result has no count. After "Clear filters", the box empties, the chip goes and 10 rows return.
- The reader says: the code has no live region for an empty result, so the reader is not required to speak by itself. Pass if the message, the hint and the link are each read as text and the link is reachable. Write in Notes whether anything was said without being asked. If nothing was, the user gets no sign that the search ended; report it as a gap in the module. After "Clear filters": the link is named, then the count "Showing 1 to 10 of 137 results", once.

### D. Selecting rows and acting on them

Run the reset command from "Before you start" first, then reload `/deals?per_page=10`.

**12. Tick one row**
- Keys: both readers: Tab to the first row's check box, named "Select " and the deal's name, then Space.
- The page: the box is checked and the row is marked. A bar appears over the table with a count, a button "Clear selection", and the buttons "Archive" and "Delete".
- The reader says: the check box is checked. Then "1 selected", once, from the bar's polite live region. The bar was hidden until this tick, and some readers lose an update that arrives as its region appears. If "1 selected" is not said but the next count in step 13 is, mark this step fail and say so in Notes.

**13. Tick two more rows**
- Keys: both readers: Tab to the next row's box, then Space; again for the third.
- The page: the count changes.
- The reader says: "2 selected", then "3 selected", each once and in that order, with no repeats.

**14. Select the whole page, then every result**
- Keys: both readers: go back to the header check box "Select all on this page" (Shift+Tab, or the table keys of step 2), then Space. Then Tab to the new button "Select all 137 results" and press Enter.
- The page: the header box is checked and all 10 row boxes are checked. The bar reads "All 10 on this page are selected." and shows the button "Select all 137 results". After the button is pressed, the bar reads "All 137 results are selected." and focus moves to the button "Clear selection".
- The reader says: "Select all on this page", check box, checked. Then "All 10 on this page are selected." once. After Enter: "All 137 results are selected." once, and the focus is read as "Clear selection", button.

**15. Take one row out**
- Keys: both readers: Tab to a row box in the table and press Space to uncheck it.
- The page: the bar reads "136 of 137 results are selected." The header box is neither checked nor unchecked.
- The reader says: the row box unchecked; "136 of 137 results are selected." once. The header check box "Select all on this page" is conveyed as partly checked (mixed), not as checked.

**16. Keep the selection on the next page**
- Keys: both readers: Tab to the link "Page 2" in the pagination and press Enter.
- The page: page 2 loads in place; its row boxes are all checked, because every result was chosen. The count under the table reads "Showing 11 to 20 of 137 results". The bar still reads "136 of 137 results are selected."
- The reader says: "Page 2", link; then the footer count "Showing 11 to 20 of 137 results" once. The row boxes read as checked. The bar's count is not said again, because its text did not change.

**17. Ask before a destructive action, and cancel**
- Keys: both readers: press the button "Clear selection", tick one row box, then Tab to the button "Delete" and press Enter.
- The page: a modal dialog opens. Its title is "Are you sure?" and its text is "Delete 1 deals? This can't be undone." Focus is on the button "Cancel". The page behind cannot be reached.
- The reader says: an alert dialog named "Are you sure?", described by "Delete 1 deals? This can't be undone."; then focus on "Cancel", button. Tab reaches "Yes, go ahead", then stays inside the dialog. Press Escape (or Enter on Cancel): the dialog closes, nothing is deleted, and focus returns to the button "Delete", which the reader says.

**18. Run a bulk action and hear the result once**
- Keys: both readers: with one row still ticked, Tab to the button "Archive" and press Enter.
- The page: a toast appears in the Notifications area with the text "Done: 1 deals (selected)." The rows are drawn again. The selection ends and the bar hides.
- The reader says: the toast text "Done: 1 deals (selected).", once, within about two seconds, as a status and not as an error alert. The bar's count "0 selected" is not required to be said, as the bar hides. Write in Notes where focus is afterwards (NVDA+Tab); Tab must move on to a control of the page.

### E. Row details and the row menu

**19. Open the details of a row**
- Keys: both readers: Tab to the link "Show details" in the first row and press Enter.
- The page: a row opens under it with the deal's name in bold and its notes. The link now reads "Hide details". Focus stays on it.
- The reader says: before: "Show details", link, collapsed. After: "Hide details", link, expanded. The reader may say the change by itself; if it is quiet, re-reading the focus (NVDA+Tab; VoiceOver: VO+Left then VO+Right) must say "Hide details" and expanded. The notes text can then be read in the next row.

**20. Open a row's menu and move in it**
- Keys: both readers: Tab to the button "Actions for " followed by the deal's name, then Enter (VoiceOver: VO+Space). Press Down arrow twice, then Up arrow once.
- The page: a menu opens under the button and focus moves to its first item. The items are "View", "Archive" and "Delete". Down arrow moves to the next item.
- The reader says: the button as "Actions for <deal name>", with a menu popup, collapsed. After Enter, the menu is entered (NVDA switches to focus mode) and "View", menu item, is said. Down arrow: "Archive", menu item; then "Delete", menu item; Up arrow: "Archive". The reader may add a position such as "2 of 3".

**21. Close the menu and choose an item**
- Keys: both readers: press Escape. Then open the menu again (Enter on the button), press Down arrow once to "Archive", and Enter.
- The page: Escape closes the menu and focus returns to the button. After the second opening, "Archive" is chosen, the menu closes, and a toast reads "Archived."
- The reader says: after Escape: "Actions for <deal name>", button, collapsed. After the choice: the toast "Archived.", once. The menu is not said to be still open.

### F. Paging

**22. Page links, the disabled link and a page change**
- Keys: both readers: Tab to the link "Page 1" and press Enter (the table was on page 2); the count reads "Showing 1 to 10 of 137 results". In browse mode find the pagination (NVDA: D until "Pagination"; VoiceOver: VO+U, Landmarks, "Pagination") and read through it with the arrow keys. Then Tab to "Page 3" and press Enter.
- The page: a navigation named "Pagination" holds a list. On page 1 the first link, "Previous page", is disabled. After "Page 3" is pressed the rows change in place, the address gets `page=3`, and focus stays on the link, which is now the current page.
- The reader says: a navigation landmark "Pagination". Then, in a list: "Previous page", link, unavailable; "Page 1", link, current page; "Page 2"; "Page 14"; "Next page". The gap between pages is not read, and the numbers are read as "Page N". After the change: "Page 3", link, current page, and the footer count "Showing 21 to 30 of 137 results" said once. "Page 1" and "Page 2" no longer say a current-page state.

**23. Change the rows per page**
- Keys: both readers: Tab to the list "Rows per page". NVDA: Alt+Down arrow to open it, Down arrow to 25, Enter. VoiceOver: VO+Space, Down arrow to 25, Enter.
- The page: the table is drawn with 25 rows and the footer count is redrawn for the rows now shown.
- The reader says: "Rows per page", with the current value "10", then "25". The footer count is said once, and matches the rows now shown (for example "Showing 1 to 25 of 137 results").

### G. Other keys and states

**24. Resize a column from the keyboard**
- Keys: both readers: Tab to the separator named "Resize Stage" in the "Stage" header. Press Right arrow twice. If the arrow keys read instead of acting, NVDA needs NVDA+Space to enter focus mode; write that in Notes.
- The page: the column grows by 16 pixels for each press. The address gets a `w=` entry after a short pause. Focus stays on the handle.
- The reader says: "Resize Stage", separator, with its value (the width in pixels) changing by 16 for each press (for example 160, 176, 192), if the reader reports values for a separator. At least the name and the separator role must be conveyed, and the focus must stay on it.

**25. Move with the grid keys**
- Keys: both readers: open `/extras`. Tab into the table until focus lands on a cell (one body cell is the only tab stop). Press Down arrow, Right arrow twice, then Home. Press Enter on the first cell of a row whose box is not dimmed, then Escape. If the arrow keys read instead of moving, press NVDA+Space once and write that in Notes.
- The page: the table acts as a grid. Arrow keys move one cell; Home goes to the first cell of the row; Enter puts focus on the control inside the cell; Escape returns to the cell. Only one cell is a tab stop.
- The reader says: a grid named "Everything else", then each cell with its column header and its text as you move. After Enter, the control in the cell (the row's check box) is named. After Escape, the cell is said again.

**26. Rows that cannot be chosen, and an action that is denied**
- Keys: both readers: on `/extras`, tick the box of a row that is not dimmed. Then Tab to the buttons over the table, and look for a dimmed row box with the table keys.
- The page: for deals that are not active, the row box is disabled. The button "Archive" is disabled and has the tooltip text "Your role can't archive.". The button "Mark" is enabled. The bar shows only after a box is ticked.
- The reader says: a disabled row box as "Select <deal name>", check box, unavailable (dimmed). The button "Archive" as unavailable; the reason "Your role can't archive." may be said as its description. "Mark" as an ordinary button.

**27. No rows, and one row**
- Keys: both readers: open `/empty`, then `/one`. Find the table (NVDA: T; VoiceOver: VO+U, Tables).
- The page: `/empty` shows one message row, "Nothing here yet.", and no count. `/one` shows one row and the count "1 result".
- The reader says: on `/empty`, the table named "No rows" and the text "Nothing here yet." read as an ordinary cell. On `/one`, the footer says "1 result" (not "Showing 1 to 1 of 1 results").

**28. A table that could not load, and one still loading**
- Keys: both readers: open `/failed`, find the table and read it. Then open `/skeleton` and do the same.
- The page: `/failed` shows the message "The deals couldn't load." and a link "Try again" in the table. `/skeleton` shows six rows of grey shapes.
- The reader says: on `/failed`, the message "The deals couldn't load." and the link "Try again". On `/skeleton`, the grey shapes are not read (they are hidden from readers) and the six rows read as empty cells. The code adds no word such as "Loading" here, so none is expected. Write in Notes how the reader describes the table.

**29. A refresh that is refused**
- Keys: both readers: with `/deals?per_page=10` open, run `curl -X POST http://127.0.0.1:8097/__t/permission/no-read` in a terminal. Back in the browser, Tab to the "Amount" sort link and press Enter (the answer is refused). Afterwards run `curl -X POST http://127.0.0.1:8097/__t/permission/ok`.
- The page: the rows stay as they were, and a message appears over the table with a link "Try again": "You no longer have access to this." If the app cannot be reached at all, the message reads "That didn't load. The rows below may be out of date."
- The reader says: the message at once, as an alert, within about a second, and only once: "You no longer have access to this." Then the link "Try again" is reachable with Tab.

### H. JavaScript off

Turn JavaScript off in the browser, run the reset command from "Before you start", and open `/deals?per_page=10`.

**30. Sort, page, search and act with full page loads**
- Keys: both readers: Tab to the link "Amount" and press Enter. Then reach "Page 2" and press Enter. Then Tab to the search box, type `gulati`, Tab on to the button "Apply" and press Enter. Then Tab to the first row's box and press Space, and Tab to the button "Archive" and press Enter. Turn JavaScript on again afterwards.
- The page: each link loads a whole new page, with no live update and no busy state. Without script, nothing is sent while you type: the button "Apply" (shown only without script) sends the form and the page reloads with the 26 results. The bar over the table shows as soon as a box is ticked, with an empty count; "Archive" posts the form and the page loads again with a toast.
- The reader says: after each load, the reader's own page-load speech (the title, and where it starts). The sort state is still conveyed by the "Amount" header (sorted ascending) when you read the headers as in step 5. The button "Apply" is named and reachable. The count under the table is ordinary text, once, when you reach it: "Showing 1 to 10 of 26 results". The row box is checked and "Archive" is named. After the post, the toast is found as text in the region named "Notifications" and reads "Done: 1 deals (selected)."

## Results

| | |
|---|---|
| Script | data-table.md |
| Tester | |
| Date | |
| Operating system and version | |
| Browser and version | |
| Screen reader and version | |
| Speech settings changed from the defaults | none |
| Build under test (commit) | |
| Proof app and port | table proof, 8097 |
| Steps run | |
| Steps failed | |
| Issues opened | |

| Step | Expected | NVDA heard / pass | VoiceOver heard / pass | Notes |
|---|---|---|---|---|
| 1 | Table and scroll box named "Deals in the pipeline", each said once | | | |
| 2 | Column headers by name; Closed and Deal convey the default sort | | | |
| 3 | Row cells with header names; deal name said once | | | |
| 4 | Sort by Amount: focus stays, nothing extra announced | | | |
| 5 | Amount header: column header, sorted ascending | | | |
| 6 | Reverse sort: descending on the header | | | |
| 7 | "Stage options" button, then dialog and stage check boxes | | | |
| 8 | "Won" checked; "Showing 1 to 10 of 28 results" said once | | | |
| 9 | Escape returns to "Stage options"; chip link named; count 137 once | | | |
| 10 | Search box; count "Showing 1 to 10 of 26 results" once after the pause | | | |
| 11 | Empty message, hint and "Clear filters" readable; note if silent | | | |
| 12 | Box checked; "1 selected" said once | | | |
| 13 | "2 selected", then "3 selected", once each | | | |
| 14 | "All 10 on this page are selected." then "All 137 results are selected." | | | |
| 15 | "136 of 137 results are selected."; header box mixed | | | |
| 16 | Page 2 count once; boxes checked; bar count not repeated | | | |
| 17 | Alert dialog "Are you sure?" with description; Cancel returns focus to Delete | | | |
| 18 | Toast "Done: 1 deals (selected)." once | | | |
| 19 | "Show details" collapsed becomes "Hide details" expanded | | | |
| 20 | "Actions for <deal>" menu button; items View, Archive, Delete | | | |
| 21 | Escape returns to the button; toast "Archived." once | | | |
| 22 | Pagination landmark; Previous page unavailable; Page 3 current; count once | | | |
| 23 | "Rows per page" list; new count said once | | | |
| 24 | "Resize Stage" separator; value changes by 16 | | | |
| 25 | Grid cells with headers; Enter enters the cell; Escape returns | | | |
| 26 | Disabled row box and "Archive" button said as unavailable | | | |
| 27 | "Nothing here yet."; "1 result" | | | |
| 28 | "The deals couldn't load." and "Try again"; skeleton shapes not read | | | |
| 29 | Alert "You no longer have access to this." once | | | |
| 30 | No-script sort, page, Apply, tick and Archive all usable by name | | | |

Overall result:

- [ ] Every step passed in NVDA with Firefox.
- [ ] Every step passed in NVDA with Chrome.
- [ ] Every step passed in VoiceOver with Safari.
- [ ] Every failed step has an issue, with the module name and the step number.
- [ ] Every n/a has a reason in the notes.

Signed off by: ______________________  Date: ______________

Reviewed by: ______________________  Date: ______________
