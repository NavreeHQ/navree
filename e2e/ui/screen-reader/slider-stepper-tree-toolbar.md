# Stepper, slider, tree view and toolbar screen reader script

## Purpose

These four modules are the keyboard widgets of the toolkit. The stepper is a list of steps with their states in words. The slider is a native range input with its value written as text. The tree view is a list that becomes a WAI-ARIA tree: one tab stop, arrow keys, open and closed branches, choices that can be half chosen, and children fetched when a branch opens. The toolbar is a group of tools with one tab stop and a More menu. This script settles what a reader says for each role, name, state and position, what is said once and not twice (a slider's value, a stepper's current step), the words a tree says while it loads, fails or has more to show, and what each module is without JavaScript.

## Page to open

The UI proof app, http://127.0.0.1:8090/flow. See [Start the proof apps](README.md#start-the-proof-apps). The page has four sections under level 2 headings: Stepper, Slider, Tree and Toolbar. Jump between them with the headings list; the addresses `/flow#steppers`, `/flow#sliders`, `/flow#trees` and `/flow#toolbars` also work.

## Before you start

- JavaScript on, window at least 1200 px wide, browser zoom 100%. Step 26 zooms the page and steps 27 to 29 turn JavaScript off; put both back afterwards.
- Open the speech viewer (NVDA) or caption panel (VoiceOver).
- VoiceOver: keep Quick Nav off. Pressing the Left and Right arrow keys together turns it on and off; with it on, the arrow keys do not reach the page.
- NVDA: let it switch to focus mode by itself when you Tab to a slider, tree or toolbar. In browse mode the arrow keys read; in focus mode they drive the widget. If a widget key reads the page instead of working, press NVDA+Space once.
- Use the headings list to move between sections. NVDA: H or NVDA+F7. VoiceOver: VO+U, Headings.
- Reload the page before you begin, so that every tree is closed as the page drew it.

## Steps

### Stepper

**1. Find the sections and read the first stepper**
- Keys: NVDA: H to the heading "Stepper", then Down arrow. VoiceOver: VO+U, Headings, "Stepper", then VO+Right.
- The page: the first stepper is a list named "Checkout" with five steps, in order Cart, Address, Payment, Review, Done. The headings in the page are "Stepper", "Slider", "Tree" and "Toolbar".
- The reader says: a list named "Checkout" with 5 items. Then each step's name followed by its state in words: "Cart (completed)", "Address (completed)", "Payment (needs attention)", "Review (current step)", "Done (not available yet)". The state words come after the name and are spoken once. The line "Step 4 of 5", which shows on a narrow window, is hidden from readers and is not spoken.

**2. Links, and the one current step**
- Keys: Both readers: Tab to the first step of the "Checkout" list, then Tab again until focus leaves the list.
- The page: "Cart", "Address" and "Payment" are links. "Review" is the current step and is not a link. "Done" is plain text.
- The reader says: three stops only, each spoken as a link: "Cart (completed)", "Address (completed)", "Payment (needs attention)". "Review" and "Done" are not stops. In browse mode "Review" is spoken once as the current step. If the reader says "current step" twice in one phrase (once from the words and once from its own report of the current step), that is a failure.

**3. Read the vertical stepper**
- Keys: NVDA: Down arrow in browse mode from the first list to the list named "Setup". VoiceOver: VO+Right.
- The page: the "Setup" list has four steps, each with a line under its name.
- The reader says: a list named "Setup" with 4 items: "Create the account (completed)" with "Your name and email"; "Invite the team (current step)" with the text "Optional" (the demo says "Optional" twice, once as its description and once as the optional mark; that is the page's data and not a failure); "Connect the mailbox (needs attention)" with "Needs an administrator"; "Launch (not available yet)". Only "Create the account" is a link.

### Slider

**4. Reach a slider**
- Keys: NVDA: H to the heading "Slider", then Tab. VoiceOver: VO+U, Headings, "Slider", then Tab.
- The page: the first slider is "Volume". It is a native range input with the value 40 and the unit "%".
- The reader says: "Volume", slider, "40%". The name, the role and the value with its unit are each spoken once. The value is spoken as "40%", not as the bare "40".

**5. Move it with the keys**
- Keys: Both readers: Right arrow; Left arrow twice; Home; End; Page Down; Shift+Left arrow.
- The page: the value moves by 5 and the filled part follows. Right: 45%. Left twice: 40%, then 35%. Home: 0%. End: 100%. Page Down: 90%. Shift+Left: 80%.
- The reader says: after each key, the new value with its unit, spoken once: "45%", "40%", "35%", "0%", "100%", "90%", "80%". For one key press, the same value must not be spoken twice in a row. The page also shows the value beside the slider in a status area. If you hear the value once from the slider and again from that area for a single press ("45%" then "45%"), that is a failure. Write what you heard.

**6. A value written with a decimal comma**
- Keys: Both readers: Tab until the reader says "Menge" (four presses from "Volume"), then Right arrow.
- The page: this slider writes numbers with a comma. It starts at 0,50 and moves by 0,25. It has no unit and no step names, so the page writes no separate spoken text for it. The value shown beside it reads "0,50", then "0,75".
- The reader says: "Menge", slider, and its value. Write down exactly what you heard for the value (the input's own "0.5" and "0.75", or the page's "0,50" and "0,75", or both). The step passes when one form of the value is spoken once per press. It fails if the reader says both forms for one press.

**7. A range with two sliders**
- Keys: Both readers: Tab to "Price minimum", then End. Tab to "Price maximum", then Home, then Right arrow.
- The page: the range "Price" is two sliders with a gap of two steps of 10 between them. It starts at $200 and $800. End on the minimum stops at $780. Home on the maximum stops at $800. Right on the maximum then makes $810.
- The reader says: "Price minimum", slider, "$200" on the first, and "Price maximum", slider, "$800" on the second. After End the minimum says "$780", not "$1000". On the maximum, Home says "$800" and Right says "$810". The currency sign is part of the spoken value. Each press speaks one value. The words "minimum" and "maximum" are part of the name.

**8. A slider with names for its steps**
- Keys: Both readers: Tab to "Level". Right arrow, then Right arrow again, then Left arrow twice.
- The page: "Level" has three steps and a name for each: Low, Medium, High. It starts on Medium. The value stops at each end.
- The reader says: "Level", slider, "Medium". Then "High". At the end the second Right arrow changes nothing; the reader says nothing new or repeats "High". Then "Medium", then "Low". The names are spoken, not the numbers 0, 1 and 2.

**9. Disabled, read-only and invalid sliders**
- Keys: Both readers: Tab onward from "Level". Note which sliders Tab reaches. Then in browse mode read the sliders "Disabled" and "Read only".
- The page: Tab reaches "Twelve marks", "A wide track", "Big step of 25" and "Marked invalid". "Disabled" and "Read only" are disabled inputs, so Tab skips them. "Marked invalid" is marked invalid and holds 90.
- The reader says: Tab does not stop on "Disabled" or "Read only". In browse mode, "Disabled", slider, 30, and unavailable (NVDA "unavailable"; VoiceOver "dimmed"). "Read only" is spoken as two sliders (minimum and maximum) and unavailable. On "Marked invalid": "Marked invalid", slider, 90, and that the entry is invalid (NVDA "invalid entry"; VoiceOver "invalid").

### Tree view

**10. Enter the first tree**
- Keys: NVDA: H to the heading "Tree", then Tab. VoiceOver: VO+U, Headings, "Tree", then Tab.
- The page: the first tree is named "Files" and has four rows: Documents, Music, README, LICENSE. Documents and Music are branches and are closed. Focus lands on the first row, which is the tree's only tab stop.
- The reader says: the tree's name "Files" and its role (tree view; VoiceOver may say outline). Then the row: "Documents", tree item, collapsed, level 1, 1 of 4. The caret drawn next to the name is not spoken as a separate control.

**11. Open a branch whose children come from the server**
- Keys: Both readers: Right arrow on "Documents".
- The page: the branch opens at once and asks the server for its children: Plans, Archive and Notes. A status area says "Loading", and when the children arrive, "3 items loaded". Focus stays on "Documents".
- The reader says: "expanded" for Documents. Then "3 items loaded", spoken once within about two seconds. The word "Loading" may be spoken first or may be skipped when the answer is quick; write down which. No line is spoken twice.

**12. Move around the tree**
- Keys: Both readers: Right arrow (into the first child); Down arrow twice; Left arrow; End; Home; then type the letter `n`.
- The page: Right on an open branch moves to its first child. Down moves one row. Left on a child with no children moves to its parent. End goes to the last row shown, Home to the first. A letter moves to the next row whose name starts with it.
- The reader says: for each row reached, its name, tree item, its level and its place among its siblings: "Plans", level 2, 1 of 3; "Archive", level 2, 2 of 3, collapsed (it is a branch); "Notes", level 2, 3 of 3. Left says "Documents", expanded, level 1. End says "LICENSE", level 1, 4 of 4. Home says "Documents". The letter `n` moves to "Notes". Every move speaks one row, once.

**13. A branch inside a branch**
- Keys: Both readers: Up arrow to "Archive", then Right arrow, wait one second, then Right arrow again.
- The page: "Archive" asks the server for its children, 2024 and 2025. A status says "2 items loaded".
- The reader says: "Archive", expanded, then "2 items loaded" once. The second Right arrow moves to "2024", tree item, level 3, 1 of 2.

**14. A tree of choices that cascade**
- Keys: Both readers: press Tab from the "Files" tree to the next tree, named "Folders to share".
- The page: the tree has six top-level rows. "Engineering" is open and has three children: Web (not chosen), API (chosen) and Retired (not chosen and not allowed to change). Because only some of its children are chosen, "Engineering" is half chosen.
- The reader says: the tree's name "Folders to share", then "Engineering", tree item, expanded, level 1, 1 of 6, and its choice state as half chosen (NVDA "half checked"; VoiceOver "mixed"). Down arrow: "Web", not checked, level 2, 1 of 3. Down: "API", checked, 2 of 3. Down: "Retired", not checked, 3 of 3. Down: "Operations", collapsed, level 1, 2 of 6.

**15. Choose with the keys**
- Keys: Both readers: Up arrow to "Retired", then Space. Up arrow twice to "Web", then Space. Left arrow to the parent.
- The page: Space on "Retired" changes nothing, because that row cannot be changed. Space on "Web" chooses it. "Engineering" is still half chosen, because "Retired" is not chosen and cannot be.
- The reader says: on "Retired", nothing is spoken for the Space press (the page does not mark this row as unavailable to the reader; write down what you hear). On "Web", "checked", once. On the parent, "Engineering", half checked (mixed).

**16. A branch that takes a while**
- Keys: Both readers: Down arrow until the reader says "Slow folder", then Right arrow. Wait for the answer, about three seconds.
- The page: the branch opens at once. A row says "Loading" inside it, and the status area says "Loading". About 2.5 seconds later two rows appear, Slow A and Slow B, and the status says "2 items loaded".
- The reader says: "expanded" and then "Loading" within about a second of the key. About three seconds later, "2 items loaded", once. The row may be spoken as busy while it loads.

**17. A branch with nothing in it**
- Keys: Both readers: End (to "Miscellaneous"), then Up arrow to "Folder with nothing", then Right arrow.
- The page: the branch opens with no rows. The status says "0 items loaded".
- The reader says: "Folder with nothing", collapsed, then after Right "expanded" and "0 items loaded", once. Nothing else is said about it being empty.

**18. A branch that fails, and Try again**
- Keys: Both readers: Up arrow to "Folder that fails", then Right arrow. Wait a second. Down arrow onto the new row, then Enter.
- The page: the branch shows one row, "Couldn't load this. Try again", and the status says "Couldn't load this." Enter on that row asks the server again, and it fails again with the same words. The row that had focus is replaced by a new one, so focus may fall back to the page; write down where focus is.
- The reader says: "Couldn't load this.", once for the first ask, and the row "Couldn't load this. Try again". After Enter, "Couldn't load this." once more. Then press Shift+Tab or Tab and write down where focus goes. If focus was lost, move the reading cursor to the end of this tree before the next step.

**19. A tree where one row is chosen**
- Keys: Both readers: press Tab to the next tree, named "Owner". Down arrow to "Ada", then Enter. Down arrow to "Grace".
- The page: "Owner" lets one row be chosen. "Sales" is open with Ada and Grace; Grace is chosen at the start. Enter on Ada chooses Ada and unchooses Grace.
- The reader says: "Owner", then "Sales", expanded, level 1. "Ada", level 2, 1 of 2, not selected. After Enter, "selected". "Grace", level 2, 2 of 2, not selected now.

**20. A long list in pages**
- Keys: Both readers: press Tab to the next tree, named "Everyone". Down arrow once, then End, then Enter.
- The page: the tree has one open branch, "Everyone (10,000)", with its first 50 rows. At the end of them is a row "Show more (9950)". Enter on it fetches the next 50 rows and moves focus to the first new row.
- The reader says: "Everyone (10,000)", expanded, level 1, 1 of 1. Down arrow: "Person 0", level 2, 1 of 10,000 (the reader writes the number its own way). End: "Show more (9950)", tree item, level 2. After Enter, "50 items loaded" once, and focus is on "Person 50", level 2, 51 of 10,000. The row now at the end says "Show more (9900)".

### Toolbar

**21. Enter the toolbar**
- Keys: NVDA: H to the heading "Toolbar", then Tab. VoiceOver: VO+U, Headings, "Toolbar", then Tab.
- The page: the first toolbar is named "Text formatting". It has one tab stop. Focus lands on its first tool, "Bold".
- The reader says: the toolbar's name "Text formatting" and its role (toolbar). Then "Bold", check box, not checked. A toggle in this toolbar is a check box, not a button that says "pressed".

**22. Go through the tools with the arrow keys**
- Keys: Both readers: Right arrow, one press at a time, until the reader says "Always shown". Then Left arrow back to "Italic".
- The page: the arrow keys move focus one tool at a time; Home and End go to the first and last.
- The reader says: in order, "Italic", check box, checked; "Underline", check box, not checked; "Left", radio button, checked, 1 of 3; "Center", radio button, not checked, 2 of 3; "Right", radio button, not checked, 3 of 3; "Copy", button; "Help", link; "Archive", button; "Share", button with a menu, collapsed; "Always shown", button. "Disabled" is skipped by the arrow keys. The demo names both the group and the radio group "Alignment"; the reader may say "Alignment" twice on entering the radios, and that is the demo's own data and not a failure.

**23. Toggle and choose**
- Keys: Both readers: Home, then Space on "Bold"; Space again. Then Right arrow until the reader says "Center", and Space.
- The page: Space on a check box toggles it. An arrow key on a radio only moves focus and does not choose it; Space on the radio chooses it.
- The reader says: "checked", then "not checked", each once. Moving to "Center" says "Center", radio button, not checked. After Space, "checked", once. Move Left to "Left": it now says not checked.

**24. Run a tool**
- Keys: Both readers: Right arrow until the reader says "Copy", then Enter (VoiceOver: VO+Space).
- The page: a toast with the text "Copied." shows. Focus stays on "Copy".
- The reader says: "Copied.", once, within about two seconds. Nothing says that focus moved.

**25. One tab stop**
- Keys: Both readers: on "Copy" press Tab, then Shift+Tab.
- The page: Tab leaves the toolbar in one press, to the "Post the toolbar" button. Shift+Tab comes back to the tool last used.
- The reader says: after Tab, "Post the toolbar", button. After Shift+Tab, "Copy", button, and the toolbar's name "Text formatting" is spoken again as focus re-enters it.

**26. The More menu at a narrow width**
- Keys: Zoom the page to 300% (Ctrl and + several times, or Command and +). Both readers: with focus in the toolbar press End.
- The page: the tools that no longer fit move into a menu opened by a button named "More". "Always shown" stays in the bar.
- The reader says: End goes to the last stop, "More", button with a menu, collapsed. Enter opens it, and focus is on an item. Down arrow through the items: each is named for a tool that moved (such as "Archive" or "Help"), with the right role: menu item; for a toggle, check box menu item with its state; for a choice, radio menu item; "Share" as a menu item with a submenu. Escape closes the menu, and focus is back on "More". Put the zoom back to 100%.

### JavaScript off

**27. The tree without script**
- Keys: Turn JavaScript off and reload `/flow`. NVDA: H to "Tree", then Down arrow. VoiceOver: VO+U, Headings, "Tree", then VO+Right.
- The page: the trees are plain nested lists. In "Files" the caret of a closed branch is a link. In "Folders to share" the rows are check boxes and every level shows. No row has a tree role.
- The reader says: lists with a count of items, not a tree. In "Files": a link "Expand Documents". Press Enter on it: the page loads again with Documents open, and the link now says "Collapse Documents"; Plans, Archive and Notes are listed. In "Folders to share": "Web", check box, not checked; "API", check box, checked. After the page load, the reader announces the page's own title.

**28. The slider without script**
- Keys: Both readers: Tab to "Volume", then Right arrow twice.
- The page: the slider is a native range input, so it still moves with the arrow keys and posts. The text that says "40%" is written by the server and is changed only by script, so it stays "40%" while the real value goes to 45 and then 50.
- The reader says: "Volume", slider. After two presses the value is 50. The reader must give the real value, 50 or "50%". If it says "40%" after the value has moved, it is giving a wrong value: mark the step fail and write what you heard.

**29. The toolbar without script**
- Keys: Both readers: H to "Toolbar", then Tab and keep pressing Tab.
- The page: the toolbar is a labelled group. Every tool is its own tab stop, except the three radios, which are one stop. There is no "More" button.
- The reader says: the group's name "Text formatting" and its role as a group, not a toolbar. Tab reaches in turn: "Bold", "Italic", "Underline", the radio "Left" (the checked one; the arrow keys move among the radios), "Copy", "Help", "Archive", "Share", "Always shown", then "Post the toolbar". "Disabled" is skipped. There is no "More". Turn JavaScript on again.

## Results

| | |
|---|---|
| Script | slider-stepper-tree-toolbar.md |
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
| 1 | List "Checkout", 5 items; state words once; count line not spoken | | | |
| 2 | Three link stops; current step spoken once | | | |
| 3 | List "Setup", 4 items; descriptions read; one link | | | |
| 4 | Volume, slider, 40% | | | |
| 5 | One value with unit per key; no repeat from the status area | | | |
| 6 | Menge: one form of the value per press | | | |
| 7 | Price minimum and maximum with $; gap kept | | | |
| 8 | Level: Medium, High, Medium, Low as names | | | |
| 9 | Tab skips disabled; invalid entry spoken | | | |
| 10 | Tree "Files"; Documents collapsed, level 1, 1 of 4; one tab stop | | | |
| 11 | Expanded; "3 items loaded" once | | | |
| 12 | Each move: name, level, position; type-ahead works | | | |
| 13 | Archive: "2 items loaded"; 2024 level 3, 1 of 2 | | | |
| 14 | Engineering half checked; Web, API, Retired states and positions | | | |
| 15 | Retired unchanged; Web checked once; parent still half checked | | | |
| 16 | Loading, then "2 items loaded" after about 2.5 seconds | | | |
| 17 | Folder with nothing: expanded, "0 items loaded" | | | |
| 18 | "Couldn't load this." and Try again row; where focus goes | | | |
| 19 | Owner: Ada not selected, then selected; Grace not selected | | | |
| 20 | Person 0 of 10,000; Show more (9950); 50 items loaded; Person 50 | | | |
| 21 | Toolbar "Text formatting"; Bold, check box, not checked | | | |
| 22 | Ten tools in order with roles and states; Disabled skipped | | | |
| 23 | Check box toggles; radio chosen by Space only | | | |
| 24 | "Copied." once; focus stays on Copy | | | |
| 25 | One tab stop; Shift+Tab returns to Copy | | | |
| 26 | More, collapsed; menu items named with right roles; Escape returns | | | |
| 27 | Lists, not a tree; Expand Documents link; check boxes | | | |
| 28 | Real value spoken after moving, not the starting "40%" | | | |
| 29 | Group, not toolbar; each tool a tab stop; no More | | | |

## Overall result

- [ ] Every step passed in NVDA with Firefox.
- [ ] Every step passed in NVDA with Chrome.
- [ ] Every step passed in VoiceOver with Safari.
- [ ] Every failed step has an issue, with the module name and the step number.
- [ ] Every n/a has a reason in the notes.

Signed off by: ______________________  Date: ______________

Reviewed by: ______________________  Date: ______________
