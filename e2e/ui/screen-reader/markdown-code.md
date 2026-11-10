# Markdown and code blocks screen reader script

## Purpose

Markdown draws text from a model or a visitor as a document: headings, lists, tables, links, task lists, and pictures that are never drawn from untrusted text. Code blocks show code with its colours already in the markup, and a copy button that says "Copied" from a live region. This script settles what a reader says for each of those: the heading list and levels, the host that follows a link to another site, the words for a link that opens a new tab, a table that scrolls, a code block that takes focus, the copy confirmation, and text that must be read as text and never as a control.

## Page to open

The UI proof app, http://127.0.0.1:8090 (see [Start the proof apps](README.md#start-the-proof-apps)).

- `/markdown`: every section of the page. Each step names the section; a section's address is the page address followed by `#reply`, `#table`, `#links`, `#images`, `#raw`, `#trusted`, `#headings`, `#languages`, `#long` or `#rtl`. Open the address with the section's name after the `#` so that the reader starts there, and one press of Tab then reaches the first control in the section.
- `/markdown/render?text=...&tab=1`: any text, drawn. The addresses used are given in the steps.

## Before you start

- JavaScript is on, except in step 19. The window is at least 1000 px wide, except in step 11.
- NVDA: speech viewer open; browse mode for reading, focus mode (NVDA+Space) for keys that the page must handle. VoiceOver: caption panel on, and "Press Tab to highlight each item" on in Safari.
- The page is on 127.0.0.1, so the browser lets the copy button write to the clipboard.
- The first heading on the page is the page's own title. The section headings are level 2.

## Steps

### Headings, lists and tasks

**1. The list of headings**
- Keys: NVDA: open `/markdown`, press NVDA+F7 and choose Headings (or press H to move from heading to heading). VoiceOver: VO+U, then Left or Right arrow to Headings (or VO+Command+H for the next heading).
- The page: ten sections, each with a level 2 heading.
- The reader says: in page order, a level 1 heading "Markdown", then level 2 headings "A reply", "A table", "Links", "Pictures", "Raw HTML", "Trusted text", "Headings", "Code in each language", "A long block" and "Right to left". Under "A reply" there is a level 3 heading "Weekly summary". Under "Headings" there are four more headings, "one" at level 3, "two" at level 4, "three" at level 5 and "six" at level 6. No text from the document is missing from the list, and no section heading is missing.

**2. A reply read in order**
- Keys: NVDA: open `/markdown#reply`, browse mode, Down arrow from the heading "A reply" to the code block. VoiceOver: VO+Right arrow the same way.
- The page: a heading, a paragraph, a numbered list, a quotation, a list of two tasks, and a code block.
- The reader says: the heading "Weekly summary", level 3; the paragraph "UPI payments from Amritsar grew 12% this week. Three things need you:"; an ordered list of 3 items, numbered 1 to 3, where item 1 contains the link "Harleen Kaur", item 2 contains the link "full report", and item 3 reads "Check the ₹1,00,000 refund for order-1042." (the amount is one number with the rupee sign); a block quote with "The next settlement runs on Monday at 10:30 IST."; then a list of 2 items (step 3); then the code block (step 12). The list and the block quote are entered and left with the reader's own words ("list with 3 items", "block quote" or "quote").

**3. Task list check boxes**
- Keys: both readers: read on from step 2 to the list of two tasks. Then open `/markdown#reply` again and press Tab three times to see where Tab goes.
- The page: two items, each a check box and a sentence. The first check box is checked and the second is not. Neither can be changed.
- The reader says: for the first item, "Done", check box, checked, and that it is unavailable ("dimmed" or "unavailable"), then the sentence "Send the invoices". For the second, "Not done", check box, not checked, unavailable, then "Call Gurpreet Singh". The name of each box is "Done" or "Not done", not the sentence. Tab does not stop on either box; the next stop after "full report" is the "Copy code" button.

### Links, pictures and raw text

**4. Links on this site and a mail link**
- Keys: both readers: open `/markdown#reply` and press Tab.
- The page: the first control in the reply is a mail link, then a link to a page on this site.
- The reader says: "Harleen Kaur", link; then "full report", link. Neither is followed by an arrow, a host, or the words "opens in a new tab".

**5. Links to other sites**
- Keys: both readers: open `/markdown#links` and press Tab five times, reading each stop.
- The page: five links in a paragraph: "the report", "GST portal", "https://yourbank.example/login", "bank" and "write". A sixth item, "click me", is not a link.
- The reader says, stop by stop: "the report", link, with nothing after it. "GST portal", link, then the host it goes to, www.gst.gov.in, after the link's words (the page draws an arrow and the host in brackets after the words, and the reader may say the arrow as "arrow" or leave it out). "https://yourbank.example/login", link, then the host attacker.example: the words of the link say one site, and the host that follows says where it really goes. "bank", link, then attacker.example (the user name in the address is not said, only the host the browser goes to). "write", link, with no host (it is a mail link). Reading the paragraph with the arrow keys, "click me" is plain text: the reader does not say "link", and Tab skips it.

**6. A link that opens a new tab**
- Keys: both readers: open `/markdown/render?text=%5Baway%5D(https%3A%2F%2Fexample.com)%20and%20%5Bhere%5D(%2Freports%2Fweekly)&tab=1#out` and press Tab twice. Do not press Enter.
- The page: two links in a paragraph, "away" (another site) and "here" (this site).
- The reader says: "away", link, then "opens in a new tab", then the host example.com. "here", link, with nothing after it.

**7. Pictures from untrusted text**
- Keys: both readers: open `/markdown#images` and read down.
- The page: three pictures in untrusted text, none drawn: one from another site, one from this site, and one inline. In their place stand the pictures' alternative words in italics.
- The reader says: "A picture from another site: chart of orders"; "One from this site: orders"; "One as data: dot". The three alternative words are read as ordinary text. None is said to be a graphic or an image. The words "Image not shown" may be said as a description: write in Notes if they are.

**8. Pictures and links in trusted text**
- Keys: both readers: open `/markdown#trusted`, read down, then press Tab.
- The page: a logo from this site, drawn; a picture from another site, not drawn; and one link.
- The reader says: "A logo from this site:", then a graphic named "Navree logo". Then "A picture from another site:" followed by the word "x" as plain text, not as a graphic. Then the link "Docs", with no host after it (trusted text shows the arrow only). Tab goes to "Docs", link.

**9. Raw HTML is shown as text**
- Keys: both readers: open `/markdown#raw` and read down. NVDA: press NVDA+P to raise the symbol level to "most" or "all" to hear the angle brackets; put it back afterwards. VoiceOver: read by line with VO+Right arrow.
- The page: a paragraph that begins "Raw HTML:" followed by the characters of a script tag, an image tag and a bold tag, and then a second paragraph with the characters of a division tag. Nothing runs and nothing is drawn.
- The reader says: the characters as words, including "script", "window", "onerror", "bold?" and "block". The reader does not say "graphic", "button" or "link". No dialog, sound or new content appears.

### A table

**10. Read the table by cell**
- Keys: NVDA: open `/markdown#table`, browse mode, Down arrow into the table, then Ctrl+Alt+Right arrow and Ctrl+Alt+Down arrow. VoiceOver: VO+Right arrow to the table, VO+Shift+Down arrow to interact, then VO+arrows; VO+Shift+Up arrow to stop.
- The page: a table of cities in a bordered box.
- The reader says: before the table, a group named "Table". Then the table, with 7 columns and 5 rows. The column headers, in order: City, State, Orders, Revenue, Refunds, Average order, Paid by UPI. The first data row, cell by cell: "Ludhiana", "Punjab", "1,240", "₹18,40,000", "₹42,300", "₹1,484", "61%". Moving across a row says the column header with each value (for example "Orders" and "1,240"). The last row begins "A very long city name that keeps going" and ends "0%". There are no row headers, so the first column is read as a plain cell.

**11. A table that scrolls**
- Keys: make the window about 400 px wide (or zoom the page to 300%). Open `/markdown#table`, press Tab. NVDA: NVDA+Space for focus mode, then Right arrow twice. VoiceOver: Right arrow twice, without VO keys. Then Tab again.
- The page: the table is wider than its box. A focus ring shows around the box. The arrow keys scroll it sideways.
- The reader says: on Tab, "Table", group. The scrolling is not spoken: the box moves and the reader says nothing. The next Tab leaves the box for the next control.

### Code

**12. A code block read in order**
- Keys: both readers: open `/markdown#reply`, then Down arrow (VoiceOver: VO+Right arrow) to the end of the reply.
- The page: a bar with the language name "sql" and a button, then the code on a grey ground with coloured words.
- The reader says: "sql"; then "Copy code", button; then the code as a line of text: "SELECT city, SUM(total) AS revenue FROM orders GROUP BY city;". The code is read as ordinary words, with no letters or fragments of the colours spoken between them (no "k" or "f").

**13. The code box takes focus**
- Keys: both readers: open `/markdown#reply`, press Tab four times: the link "Harleen Kaur", the link "full report", "Copy code", then the code box.
- The page: after the copy button, the code box takes focus, with a focus ring. It has no name of its own.
- The reader says: on the third stop, "Copy code", button. On the fourth, that focus is on the code: the reader reads the code, or part of it, or says its role; it is not silent. Write in Notes exactly what was said (the first line, the whole block, only a role).

**14. Copy the code**
- Keys: NVDA: Tab to "Copy code", then Enter. VoiceOver: VO+Space on "Copy code". Press it a second time.
- The page: the code is put on the clipboard and the page marks the button for about a second and a half. Focus stays on the button.
- The reader says: "Copied", once, within about a second of each press, from the page's live region. The button's name stays "Copy code". Optionally, press Ctrl+L (Command+L), then Ctrl+V (Command+V): the address bar holds the code. Press Escape.

**15. Colours in several languages**
- Keys: both readers: open `/markdown#languages` and read down through the first three blocks (rust, ts, json).
- The page: fourteen blocks, each with a small caption above it in the proof page and a language label in the block's bar.
- The reader says, for each block: the caption ("rust"), then the block's own language label ("rust"), then the code as plain text. The first word is said twice because the page prints the caption and the block prints its label. The code is read without breaks at the colours, and no block has a "Copy code" button.

**16. A long code block**
- Keys: NVDA: open `/markdown#long`, press Tab, NVDA+Space for focus mode, then Down arrow, Page Down and End. VoiceOver: Tab, then Down arrow, Page Down, End, without VO keys.
- The page: a block of 120 long lines in a box with a height limit, with its language label "rust" and no copy button (the block is too long to copy).
- The reader says: on Tab, the first stop after the section's start is the code box (there is no "Copy code" button before it). Write in Notes how much was read when focus arrived. The box scrolls with the keys: Down arrow, Page Down and End move the text. The reader reads the line it lands on, or nothing; it does not read the whole 120 lines on each key.

### Other text

**17. Right to left**
- Keys: both readers: open `/markdown#rtl` and read down.
- The page: a paragraph, a list of two items, and a table in Gurmukhi, inside a right-to-left box.
- The reader says: the structure, whether or not the reader has a voice for the language: a link ("ਪੂਰੀ ਰਿਪੋਰਟ", with no "opens in a new tab", because it goes to a page on this site), a list of 2 items, and a table with 2 columns and 2 rows (headers "ਸ਼ਹਿਰ" and "ਆਰਡਰ"; the row "ਲੁਧਿਆਣਾ" and "1,240"). Mark the words n/a if no voice reads them.

**18. Any text, drawn**
- Keys: both readers: open `/markdown/render?text=%5Bclick%20me%5D(javascript%3Aalert(1))#out`, read down and press Tab. Then open `/markdown/render?text=%23%20Title&level=3#out`, and use the heading key (H, VoiceOver: VO+Command+H).
- The page: the first address shows the words "click me" with no link. The second shows a heading.
- The reader says: first, "click me" as plain text, no "link"; Tab does not stop on it. Second, a heading "Title", level 3.

### With script off

**19. No script**
- Keys: turn script off in the browser (see the README). Open `/markdown`, and repeat steps 1, 5 and 10.
- The page: the same text, headings, links and table. The hosts after links and the colours come from the page, not from script. The copy button needs script and is not tested.
- The reader says: the same as in steps 1, 5 and 10: the same list of headings, the same hosts after the links "GST portal", "https://yourbank.example/login" and "bank", and the same table by cell.

## Results

| | |
|---|---|
| Script | markdown-code.md |
| Tester | |
| Date | |
| Operating system and version | |
| Browser and version | |
| Screen reader and version | |
| Speech settings changed from the defaults | none |
| Build under test (commit) | |
| Proof app and port | UI proof app, 8090 |
| Steps run | |
| Steps failed | |
| Issues opened | |

| Step | Expected | NVDA heard / pass | VoiceOver heard / pass | Notes |
|---|---|---|---|---|
| 1 | Heading list with levels: Markdown, ten level 2 headings, "Weekly summary" level 3, one to six | | | |
| 2 | Reply in order: heading, ordered list of 3, block quote, task list, code | | | |
| 3 | Boxes named "Done" and "Not done", checked state, unavailable; Tab skips them | | | |
| 4 | "Harleen Kaur" and "full report" links, no host, no arrow | | | |
| 5 | Hosts said after links to other sites; "click me" is plain text | | | |
| 6 | "away" says "opens in a new tab" and example.com; "here" does not | | | |
| 7 | Alternative words read as text; no graphic announced | | | |
| 8 | Logo is a graphic named "Navree logo"; other picture is the word "x"; "Docs" has no host | | | |
| 9 | Raw HTML read as text; no graphic, button or link | | | |
| 10 | Group "Table"; 7 columns, 5 rows; headers and cells read by column | | | |
| 11 | Narrow window: "Table" group takes focus, arrows scroll it, nothing spoken | | | |
| 12 | "sql", "Copy code" button, then the code read as plain words | | | |
| 13 | Focus on the code box is not silent; what was said is noted | | | |
| 14 | "Copied" said once for each press; button name stays; focus stays | | | |
| 15 | Caption and label both said; code unbroken by colours; no copy button | | | |
| 16 | Long block: first stop is the code box; keys scroll it; no copy button | | | |
| 17 | Right to left: link, list of 2, table of 2 by 2 | | | |
| 18 | "click me" not a link; heading "Title" level 3 | | | |
| 19 | With script off: same headings, hosts after links, and table | | | |
