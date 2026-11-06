# Small parts screen reader script

## Purpose

The small parts that sit on the base components: an alert with a close button and actions, a box with a description under its label and a box that is neither checked nor unchecked, a long breadcrumb folded into a list, an input with a clear button, the colour picker, an avatar group, a trace, a questionnaire, a select with a flag in each choice, and a sparkline in a table cell. Most of them hide a decoration (a flag, a mark, a picture) from a reader and say its meaning in words, so the script settles that the words are spoken once and the decorations are not spoken at all.

## Page to open

The UI proof app, `/parts`: http://127.0.0.1:8090/parts (see [README.md](README.md), "Start the proof apps"). The steps go down the page in order.

## Before you start

- JavaScript on, window at least 1200 px wide, browser zoom 100%.
- Open the speech viewer (NVDA) or the caption panel (VoiceOver).
- Jump between sections with the headings: NVDA, H in browse mode; VoiceOver, VO+U, Headings. The headings, in order, are: An alert with actions and a close button, Boxes with a description, A long trail folded, Inputs, A colour, People, A trace, A questionnaire, A flag in each choice, Flags by pointer, A sparkline in each row.

## Steps

### The alert

**1. The alert and its actions**
- Keys: Go to the first heading, then press the down arrow (NVDA browse mode) or VO+Right arrow (VoiceOver).
- The page: a warning alert, "Your trial ends in 3 days", with the line "Add a payment method to keep your workspace.", two buttons under it, and a close button at its corner.
- The reader says: the title and the text, read as an alert (NVDA: "alert"; VoiceOver: the alert role). Then "Add payment, link", "Remind me later, button". Then "Dismiss this notice, button". The icon is not spoken.

**2. Closing it**
- Keys: Tab to "Dismiss this notice" and press Enter.
- The page: the alert disappears.
- The reader says: nothing about the alert, and focus is not lost into the void: write down where the cursor and focus are afterwards (the page does not move focus; the browser usually puts it on the page). If the reader is silent and focus is on the page, the step passes.

### Boxes

**3. A description after the label**
- Keys: Tab to "Product news".
- The page: a checked box labelled "Product news", with "A message a month at most." under it.
- The reader says: "Product news, check box, checked", then the description "A message a month at most." once, after the name (NVDA reads it with the name; VoiceOver after a pause).

**4. The box that is neither checked nor unchecked**
- Keys: Tab to "All notifications". Press Space. Press Space again.
- The page: a box that is drawn with a dash. Space checks it, and the dash goes.
- The reader says: "All notifications, check box, partially checked" (NVDA) or "mixed" (VoiceOver). After the first Space: "checked". After the second: "not checked".

**5. A switch with a description**
- Keys: Tab to "Payment alerts".
- The page: a switch, off, with "An email when a payment fails." under it.
- The reader says: "Payment alerts, switch, off" (NVDA "toggle button"; VoiceOver "switch"), then the description once.

### The trail

**6. A long trail folded**
- Keys: Go to "A long trail folded". Move through the trail with the cursor, then Tab to "Show hidden pages" and press Enter.
- The page: "Home", a button with three dots, "Dhillon Traders" and "Invoices". The button opens a list of "Customers", "Punjab" and "Ludhiana".
- The reader says: the navigation named "Breadcrumb"; "Home, link"; "Show hidden pages", with its role (a disclosure; NVDA: "button, collapsed"; VoiceOver: "button, collapsed" or "disclosure triangle"); after Enter "expanded" and then the three links one by one; "Invoices" as the current page, not a link. The dots are not read.

### Inputs

**7. An input with a clear button**
- Keys: Tab to "Search the catalogue". Tab once more.
- The page: a search field holding "kurta", and a close button at its end.
- The reader says: "Search the catalogue, edit, kurta"; then "Clear the search, button". Press Enter: the field empties, focus returns to it, and the reader says the field's name again with "blank".

**8. A small field**
- Keys: Tab to "A small field".
- The page: a shorter text field.
- The reader says: "A small field, edit". The size is not spoken.

### The colour

**9. The colour picker**
- Keys: Go to "A colour". Tab to "Accent colour".
- The page: a colour field, a hex field, and four round swatches.
- The reader says: "Accent colour, colour picker" or "button" (the platform's own control; write down the words). Tab: "Hex value, edit, #ff9933". Tab: "Swatches, group", then "Saffron, toggle button, pressed" (NVDA "pressed"; VoiceOver "selected" or "pressed"). Tab to "Green" and press Enter: "Green, pressed", and the hex field now holds "#138808". Only one swatch is pressed.

### People

**10. The avatar group and a picture that fails**
- Keys: Go to "People".
- The page: three people and a "+45" tile; and a person icon in a box (the picture at `/parts/missing.png` does not exist).
- The reader says: "Members of the Ludhiana branch, group"; then "Jaspreet Dhillon, graphic", "Navdeep Grewal, graphic", "Rajveer Brar, graphic", "45 more, graphic" (the tile is read as "45 more", not "plus 45"). Then "Gurpreet Kaur, graphic" for the avatar whose picture failed, once.

### A trace

**11. The steps and their states**
- Keys: Go to "A trace" and read down with the cursor.
- The page: four steps down a line: "Read 4,812 entries", "Matched with bank lines" (running, with detail open), "Write the report", "Send it to the owner".
- The reader says: the list "Reconciling the March ledger", with 4 items. Each item starts with its state in words: "Done Read 4,812 entries", "Running Matched with bank lines" (the current step), "Not started Write the report", "Failed Send it to the owner". The marks are not spoken. The detail "Unmatched lines wait for the next pass." is read in the running step. Tab to "Details" and press Enter: the detail closes; the reader says "collapsed".

### A questionnaire

**12. One question**
- Keys: Go to "A questionnaire".
- The page: "Question 2 of 3" with a progress bar, the question "What do you use today?", help "Choose any that apply.", three choices as cards, and the buttons Previous, Skip and Next.
- The reader says: the progress bar as "Question 2 of 3, progress bar" with no percentage; the question as the name of a group ("What do you use today?, group"); "Choose any that apply." once; each choice "Spreadsheets, check box, not checked"; then "Previous, button", "Skip, button", "Next, button". The words "Question 2 of 3" beside the bar are not read a second time.

### Flags

**13. A flag in each choice**
- Keys: Go to "A flag in each choice". Tab to "Country". Open it with Enter or Alt+Down; move through the choices with the arrow keys; choose "Nepal".
- The page: the control shows the flag of India and "India"; the list has three choices, each with a flag.
- The reader says: "Country, India, combo box" (or "pop-up button"); in the list "India", "Nepal", "Serbia" with no flag name and no "graphic"; after choosing: "Nepal". The flag is never spoken, only the country's name.

**14. Flags by pointer**
- Keys: Go to "Flags by pointer" and read down the first four rows.
- The page: forty rows, each a small flag and a country name.
- The reader says: "Serbia", "India", "Serbia", "India": the name once each, and no flag. If a reader says "graphic" or "image" before each name, the flag is not hidden from it; write it down.

### A sparkline in a table

**15. A cell**
- Keys: Go to "A sparkline in each row". Move into the table with the table keys (NVDA: Ctrl+Alt+Down); move to the second column.
- The page: a table of fifty stores, each with a small line in its second cell.
- The reader says: the table's caption "Orders by store, last 30 days", "table with 50 rows and 2 columns"; for a cell: the column header "Orders", then the picture's name: "Orders of Ludhiana 1: " followed by the series' first and last values and its highest and lowest. The words are the summary the page writes, spoken once. No "table" is announced inside the cell.

## Results

| Step | Expected | NVDA heard / pass | VoiceOver heard / pass | Notes |
|---|---|---|---|---|
| 1 | Title, text, actions, close button; no icon | | | |
| 2 | Closing is silent; focus is on the page | | | |
| 3 | Check box checked, description once | | | |
| 4 | Partially checked, then checked, then not checked | | | |
| 5 | Switch off, description once | | | |
| 6 | Trail, button collapsed, expanded, three links, current page | | | |
| 7 | Edit with value, Clear the search button, field empty after Enter | | | |
| 8 | Edit; size not spoken | | | |
| 9 | Colour control, hex edit, swatches pressed state, one pressed | | | |
| 10 | Group, three people, "45 more", failed picture once | | | |
| 11 | State words before each step, current step, detail collapses | | | |
| 12 | Progress named, question group, help once, three buttons | | | |
| 13 | Country named, flags silent, choice spoken | | | |
| 14 | Names only | | | |
| 15 | Caption, header, cell summary once | | | |
