# Select, combobox, multi-select and command palette screen reader script

## Purpose

Select, Combobox and MultiSelect are pickers: a control that opens a list of options. The command palette is a search box over a list of commands, in a dialog. All four use the combobox pattern with `aria-activedescendant`: focus stays on the control and the reader follows the highlighted option. The browser suites check the roles and attributes. This script settles what a reader says as the highlight moves, how it speaks counts ("10 results available"), additions ("Japan added"), errors and empty lists, and where focus is when a list closes.

## Page to open

The UI proof app, http://127.0.0.1:8090 (see [Start the proof apps](README.md#start-the-proof-apps)).

- `/select` for steps 1 to 27 (title "Select and Combobox"; its sections are Select, Combobox, MultiSelect and Command palette).
- `/select/form` for step 28 (a form with pickers).
- `/select/form` and `/select` with JavaScript off for steps 29 and 30.

## Before you start

- JavaScript on, except steps 29 and 30. Use a window wide enough for the desktop layout (about 1200 px).
- Load `/select` fresh.
- Speech viewer (NVDA) or caption panel (VoiceOver) visible, so that you can copy what was said.
- NVDA: stay in focus mode while a picker has focus. NVDA switches to it by itself when focus lands on a combobox or an edit field; if you hear the browse-mode sound, press NVDA+Space.
- VoiceOver: use Tab to put focus on a control, then press arrow keys by themselves (no VO keys), as the README says.
- Find a control by its label. NVDA: NVDA+F7 lists form fields. VoiceOver: VO+U, then Form Controls.
- Each picker has a hidden status line that the reader speaks (a polite live region). A count such as "5 results available" is spoken about a quarter of a second after a list opens or its text changes, and only the last count is spoken if you type quickly. A message about what you just did ("Japan added") always comes before a count and is never followed by one straight away.
- To re-read the focused control: NVDA: NVDA+Tab. VoiceOver: VO+Left arrow, then VO+Right arrow.

## Steps

### Select (a button that opens a list)

**1. The closed select**
- Keys: Both readers: Tab to the field labelled "Stage" (placeholder "Choose a stage").
- The page: the focus ring is on the Stage button. Nothing is open.
- The reader says: the name "Stage", the role combobox (some readers say "pop-up button" or "menu button"; either is fine if the reader also says a list is available), the state collapsed, and the text "Choose a stage" as its value.

**2. Open the list**
- Keys: Both readers: Down arrow. (Enter and Space also open it.)
- The page: a list of five options opens under the button: Lead, Qualified, Proposal, Won, Lost. The first, Lead, is highlighted. Focus stays on the Stage button.
- The reader says: expanded, then the highlighted option "Lead" with its position, "1 of 5". About a quarter of a second later it says "5 results available", once. It does not say the count again until the list is closed and opened again.

**3. Move through the options, then type ahead**
- Keys: Both readers: Down arrow, Down arrow, Up arrow, End, Home, then type the letter w.
- The page: the highlight follows each key and ends on Won.
- The reader says: at each key the new option's name and position: "Qualified" 2 of 5, "Proposal" 3 of 5, "Qualified" 2 of 5, "Lost" 5 of 5, "Lead" 1 of 5, then "Won" 4 of 5. Each option is spoken once per key. The count is not repeated.

**4. Choose with Enter**
- Keys: Both readers: Enter. Then re-read the control.
- The page: the list closes, the button shows "Won", and focus stays on the Stage button.
- The reader says: collapsed. On the re-read: the name "Stage", the role combobox, collapsed, and the value "Won" (not "Choose a stage"). Record whether the reader spoke the new value by itself after Enter.

**5. Open again, then Escape**
- Keys: Both readers: Down arrow, then Escape.
- The page: the list opens with Won highlighted. Escape closes it. The value stays "Won". Focus is still on the Stage button.
- The reader says: on opening, "Won", selected, 4 of 5. After Escape, collapsed. Focus has not moved: Tab goes to the next field, "Chosen".

**6. Groups and a disabled option**
- Keys: Both readers: Tab to "Time zone" (placeholder "Choose a zone"; "Chosen" comes before it), Down arrow to open, then Down arrow until the end of the list.
- The page: the groups are Europe (London, Paris, Berlin) and Asia (Tokyo, Delhi, Seoul), with a divider between them. Delhi is dimmed. The highlight goes from Tokyo to Seoul and never stops on Delhi.
- The reader says: each option's name and position as it is highlighted. Delhi is never spoken as the highlighted option. Some readers say the group name ("Europe", "Asia") as the highlight enters a group; record whether yours does. Close the list with Escape.

**7. Options with a description and a picture**
- Keys: Both readers: Tab to "Owner" (placeholder "Choose an owner"). Down arrow to open, Down arrow, Enter. Then Down arrow to open again, Home, Enter.
- The page: three options each show a name and a second line: Anu Lakhotia (Engineering), Arun Tiwari (Research), Gauri Hundal (Compilers). After the first Enter the button shows "Arun Tiwari". After the second it shows "Anu", which is shorter than the option's text.
- The reader says: each option as its name followed by its second line ("Anu Lakhotia Engineering"), with its position, "1 of 3" and so on. The small picture beside Anu Lakhotia has no text of its own: it must not be spoken as "image" or as a file name. The final value is "Anu".

**8. Clear a chosen value**
- Keys: Both readers: move focus to "Clearable" (it shows "Lead"; use the form field list to jump there). With the list closed, press Delete.
- The page: the value goes back to the placeholder "None". Focus stays on the control.
- The reader says: "Selection cleared", once, as a live announcement. The control's value is then "None".

**9. Error, read-only, disabled and empty states**
- Keys: Both readers: move focus to "Required, with an error", then to "Read only", then to "No options", then to "Invalid" in the Combobox section (use the form field list to jump, or Tab). "Disabled" is skipped by Tab: read it with the reader's cursor (NVDA: browse mode, Down arrow; VoiceOver: VO+Right arrow).
- The page: "Required, with an error" shows "Choose a stage." under it, and "Invalid" shows "Choose a country." under it. "Read only" shows "Lead" and does not open. "No options" opens an empty list. "Disabled" shows "Lead" and cannot take focus.
- The reader says: for "Required, with an error" and for "Invalid": the name, the role, invalid entry (or the reader's word for invalid), and the text "Choose a stage." or "Choose a country." as the description, spoken once as part of arriving on the field. For "Read only": read only is conveyed and Down arrow opens nothing. For "No options": Down arrow opens the list and "No options" is spoken once through the live status. For "Disabled": unavailable or dimmed.

### Combobox (a text box with a list)

**10. The closed combobox**
- Keys: Both readers: Tab to "Country" (placeholder "Search countries").
- The page: a text box with a small arrow button on its right. The arrow button is not a tab stop.
- The reader says: the name "Country", the role (combobox, or edit with a list), collapsed, and the placeholder as a hint if the reader reads placeholders. The arrow button is not spoken.

**11. Type to filter**
- Keys: Both readers: type an at normal speed.
- The page: the list opens and shows the countries that contain "an". The first match is highlighted.
- The reader says: expanded, then "10 results available", once, about a quarter of a second after the last key, and the first option with its position. If you type slowly an earlier count may be spoken; the final count must be 10.

**12. Narrow the list and choose**
- Keys: Both readers: type ad (the text is now "anad"), then Enter.
- The page: only Canada is left. Enter chooses it, the list closes, and the box shows "Canada".
- The reader says: "1 result available", once, and the option "Canada", 1 of 1. After Enter: collapsed, and the value "Canada".

**13. No match**
- Keys: Both readers: Ctrl+A, then type zzz.
- The page: the list is empty and a note "No results" shows under the box.
- The reader says: "No results", once, through the live status. No option is spoken.

**14. Escape in two stages**
- Keys: Both readers: with "zzz" still in the box press Escape. Then Tab to "Chosen and clearable" (it shows "India") and press Escape with the list closed.
- The page: the first Escape closes the list and the box goes back to "Canada". In "Chosen and clearable" the second Escape clears the value.
- The reader says: after the first Escape, collapsed, and focus stays in the box. After the second, "Selection cleared", once, and the box is empty.

**15. Create from typed text**
- Keys: Both readers: Tab to "Tag (any text)" (placeholder "Pick or type"). Type Prospect. Press Enter.
- The page: no option matches, so one row appears that reads Create, then the typed text in quotation marks. It is the highlighted row. Enter chooses it, the box shows "Prospect" and the list closes.
- The reader says: "No results" (no existing option matches), and the highlighted row read as an option with its text: Create "Prospect". After Enter: "Prospect added", once. Focus stays in the box.

**16. A create row that is off**
- Keys: Both readers: Tab to "Team (not allowed to create)". Type Yellow. Press Down arrow.
- The page: a Create row shows dimmed, with the second line "Only admins can add teams." No row is highlighted and Down arrow does not land on the dimmed row.
- The reader says: "No results" once. Down arrow speaks no option. If you move the reader's cursor to the row (NVDA: browse mode; VoiceOver: VO+Right arrow), it reads the Create text, "Only admins can add teams.", and unavailable or dimmed. Press Escape and clear the box when you are done.

### MultiSelect (chips and a text box)

**17. The chips and the box**
- Keys: Both readers: Tab to "Countries" (it holds France and Germany). Then read the chips: NVDA: NVDA+Space for browse mode, Up arrow. VoiceOver: VO+Left arrow.
- The page: two chips, France and Germany, each with a small x button, then the text box with the placeholder "Add countries".
- The reader says: on Tab, the name "Countries", combobox or edit with a list, collapsed. On the read: a list of two items, "France" and "Germany", and a button after each named "Remove France" and "Remove Germany" (not "x", and not "button" alone).

**18. Add a country**
- Keys: Both readers: NVDA: NVDA+Space back to focus mode if needed. Type jap, then Enter.
- The page: Japan becomes a third chip, the box empties, and the list stays open with Japan marked as selected.
- The reader says: "Japan added", once, as a live announcement. No "results available" count follows it straight away. In the list, Japan is spoken as selected and the other options as not selected (a multiple-choice list).

**19. Remove with Backspace**
- Keys: Both readers: with the box empty press Backspace, then Left arrow, then Backspace.
- The page: the first Backspace puts focus on the last chip, Japan. Left arrow moves to Germany. The second Backspace removes Germany and focus moves to the next chip, Japan.
- The reader says: on the first Backspace, "Japan focused. Press Backspace to remove.", once (the reader may read the chip's own text first). On Left arrow, the chip "Germany". After the second Backspace, "Germany removed", once.

**20. Remove with the button**
- Keys: NVDA: NVDA+Space for browse mode, press Shift+B or B until the reader says "Remove France", then Enter. VoiceOver: VO+Right arrow or VO+Left arrow to "Remove France", then VO+Space.
- The page: France goes. Focus moves to the text box.
- The reader says: "France removed", once. Then the text box is spoken as the focused control (the name "Countries").

**21. A limit**
- Keys: Both readers: Tab to "At most three" (France and Germany chosen). Type ita, then Enter. Then press Down arrow, Down arrow.
- The page: Italy is the third chip. The other options in the open list are dimmed. Down arrow visits only the three chosen options.
- The reader says: "Italy added", once. Down arrow speaks France, Germany and Italy, each as selected, and never an unchosen option. Optional, with a pointer: click a dimmed option, and "You can choose at most 3." is spoken.

### Pickers that search on the server

**22. A list that is fetched**
- Keys: Both readers: Tab to "Person (searched on the server)" (it holds "Gauri Hundal 3"). Press Down arrow. Then Ctrl+A and type manjeet.
- The page: the list fills with the first 20 people, with a "Show more" row at the end. After "manjeet" one person is left, whose name is made of the text `<img src=x onerror=alert(1)> Manjeet`.
- The reader says: expanded, then "20 results available", once, when the list lands. The "Show more" row is spoken as an option. After "manjeet": "1 result available", once, and the name read as plain text (a reader may spell out the angle brackets), with no image and no alert. Press Escape.

**23. A minimum number of letters**
- Keys: Both readers: Tab to "Person (2 characters first)", type a, then d.
- The page: after one letter the list is empty and the note reads "Type at least 2 characters to search." After the second letter people appear.
- The reader says: "Type at least 2 characters to search.", once, after the first letter. After the second letter a count of results, once.

**24. A slow search**
- Keys: Both readers: first slow the network (Chrome or Firefox: developer tools, Network, throttling set to a slow profile; in Safari there is no built-in throttle, so mark VoiceOver n/a unless you have one). Tab to "Person (searched on the server)", press Ctrl+A, type gauri.
- The page: while the answer is on its way the list is marked busy and the note reads "Searching...".
- The reader says: "Searching...", once, then a count when the answer lands. Turn the throttle off afterwards.

### Command palette

**25. Open the palette**
- Keys: NVDA: Ctrl+K from the page (click the page first if the browser takes the key). VoiceOver: Control+K. You can also press the button "Search..." with Enter.
- The page: a dialog opens with its text box focused. The page behind it cannot be reached.
- The reader says: the dialog's name "Command palette" and the role dialog, then the focused text box: the name "Type a command or search...", combobox, expanded. Focus is in the text box without another key.

**26. Move through the commands**
- Keys: Both readers: Down arrow, Down arrow, Down arrow, Down arrow.
- The page: the highlight moves through Deals, New deal, Gallery (group Navigate), then Open the dialog (group Actions). The shortcut written at the end of a row (such as Alt+D) is drawn but is not for the reader.
- The reader says: each command's name and position, once per key. "Gallery" is spoken with its second line, "Every component in its states". The shortcut keys are not spoken. The list is named "Results", and the groups "Navigate" and "Actions" are spoken where your reader speaks group names.

**27. Filter, a dimmed command, no results, close**
- Keys: Both readers: type deal, then Escape. Open the palette again, move the reader's cursor to the row "Billing" (NVDA: NVDA+Space then Down arrow; VoiceOver: VO+Right arrow), then type zzzzqq, then Escape.
- The page: "deal" leaves Deals and New deal, and the first is highlighted. Escape closes the palette and focus goes back to where it was before it opened. "Billing" is dimmed with the line "Only owners can open billing." "zzzzqq" leaves nothing and shows "No results".
- The reader says: "2 results available", once. If the same count is spoken a second time about half a second later (when the palette's search for people answers with nothing), that is a repeat and the step fails. After Escape the reader speaks the control that had focus before. "Billing" reads with "Only owners can open billing." and unavailable or dimmed. "zzzzqq" gives "No results", once.

### A required picker in a form

**28. The error on submit**
- Keys: Both readers: open `/select/form`. Move to the button "Send" (the form field list, or Tab) and press Enter with nothing chosen. Focus lands on "Stage": press Down arrow, then Enter to choose "Lead". Then Tab to "Country".
- The page: the form is not sent. Focus goes to "Stage" and a message shows under Stage and under Country. The text is the browser's own validation message, or "Choose an option." when the browser gives none. Choosing a stage removes only Stage's message.
- The reader says: on the focus move, the name "Stage", invalid entry, and the message as the description, once. The message is not repeated as a separate announcement on top of the focus announcement. On Tab to "Country": the same, with its own message.

### JavaScript off

**29. Select with JavaScript off**
- Keys: Both readers: turn JavaScript off and reload `/select/form`. Tab to the first field and press the arrow keys to choose Won. Tab to the second and choose India. Tab to the multiple-choice list and choose two entries with Shift+Down arrow. Turn JavaScript on again after the step.
- The page: the browser's own select boxes show, not the styled buttons. "Send" posts them.
- The reader says: for each box the name that matches its label ("Stage", "Country", "Countries"), the role (combobox or list box, and a multiple-choice list box for "Countries"), and the current option. The same control is not announced twice. If a box has no name, that is a failure of the module.

**30. Palette with JavaScript off**
- Keys: Both readers: turn JavaScript off and reload `/select`. Press the button "Search..." with Enter. Tab to the row "Gallery" and press Enter. Turn JavaScript on again after the step.
- The page: the dialog opens from the button alone, with every command listed. "Gallery" is a link and goes to the Gallery page.
- The reader says: the dialog "Command palette" opens and focus is inside it. The rows are spoken with their names as options or links. After Enter the new page's title is spoken.

## Results

| | |
|---|---|
| Script | select-combobox-command.md |
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
| 1 | Stage, combobox, collapsed, "Choose a stage" | | | |
| 2 | Expanded; Lead, 1 of 5; "5 results available" once | | | |
| 3 | Each option with its position; count not repeated | | | |
| 4 | Closes; Stage, collapsed, value Won | | | |
| 5 | Won selected, 4 of 5; Escape keeps value and focus | | | |
| 6 | Options with positions; Delhi never highlighted | | | |
| 7 | Name plus second line; picture silent; value "Anu" | | | |
| 8 | "Selection cleared" once; value "None" | | | |
| 9 | Invalid with its message once; read only; unavailable; "No options" | | | |
| 10 | Country, combobox, collapsed; arrow button not spoken | | | |
| 11 | "10 results available" once; first match spoken | | | |
| 12 | "1 result available"; Canada, 1 of 1; value Canada | | | |
| 13 | "No results" once | | | |
| 14 | Escape closes; second Escape: "Selection cleared" | | | |
| 15 | Create row as an option; "Prospect added" once | | | |
| 16 | Create row unavailable with "Only admins can add teams." | | | |
| 17 | Countries, collapsed; chips as a list; "Remove France" | | | |
| 18 | "Japan added" once; Japan selected in the list | | | |
| 19 | "Japan focused. Press Backspace to remove."; "Germany removed" | | | |
| 20 | "France removed"; focus back in the box | | | |
| 21 | "Italy added"; Down arrow visits only chosen options | | | |
| 22 | "20 results available" once; "1 result available"; plain text name | | | |
| 23 | "Type at least 2 characters to search." once | | | |
| 24 | "Searching..." once, then the count | | | |
| 25 | Command palette dialog; text box, combobox, expanded | | | |
| 26 | Command name and position; shortcuts not spoken | | | |
| 27 | "2 results available"; Billing unavailable; "No results"; focus returns | | | |
| 28 | Stage invalid with its message once; Country the same | | | |
| 29 | Native boxes each with a name, role and value | | | |
| 30 | Dialog opens from the button; link row goes | | | |

## Overall result

- [ ] Every step passed in NVDA with Firefox.
- [ ] Every step passed in NVDA with Chrome.
- [ ] Every step passed in VoiceOver with Safari.
- [ ] Every failed step has an issue, with the module name and the step number.
- [ ] Every n/a has a reason in the notes.

Signed off by: ______________________  Date: ______________
