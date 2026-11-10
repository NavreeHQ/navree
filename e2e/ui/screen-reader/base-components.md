# Base components screen reader script

## Purpose

The base components are the parts every other module is built from: buttons, links, toggles, text fields with their help and error, checkboxes, switches, radio groups, alerts, badges, progress, breadcrumbs, pagination, avatars, cards, empty states and plain tables. If one of these is wrong, every module that uses it is wrong. This script settles what a reader says for each one: its name, its role, its state, and whether an icon or a decoration is wrongly spoken.

## Page to open

The UI proof app, `/gallery`: http://127.0.0.1:8090/gallery (see [README.md](README.md), "Start the proof apps"). Use the first four sections, Buttons, Inputs, Display and Commands. The Overlays section is covered in `overlays.md`.

## Before you start

- JavaScript on. The page is a full page of the proof app, with the sidebar on its left; the steps below stay in the main area.
- Window wide enough to show the sidebar (at least 1024 px).
- Start in browse mode (NVDA) or with the VoiceOver cursor on the page. Steps that use form fields say when to use Tab and focus mode.
- Open the speech viewer (NVDA) or the caption panel (VoiceOver).
- In NVDA, set Settings > Object presentation > Progress bar output to "Speak" for this script, or read the progress bars with the cursor as step 21 says. Put it back afterwards.
- To jump between sections: NVDA, press H in browse mode (heading by heading) or NVDA+F7 and choose Headings. VoiceOver, VO+U, then Left or Right arrow to Headings, Up or Down arrow to choose, Enter to go.

## Steps

### Page and headings

**1. Page title and headings**
- Keys: NVDA: NVDA+T, then NVDA+F7, Headings tab. VoiceOver: VO+U, Left or Right arrow to Headings.
- The page: the document title is "Gallery". The page has one level 1 heading, "Gallery", and level 2 headings "Buttons", "Inputs", "Display", "Overlays" and "Commands", in that order.
- The reader says: the title "Gallery" (VoiceOver reads it from the window or tab). The headings list shows the level 1 heading and the five level 2 headings, each with its level.

### Buttons

**2. Button variants**
- Keys: Go to the "Buttons" heading. NVDA: browse mode, press B (next button). VoiceOver: VO+Right arrow.
- The page: the first row is six buttons, each labelled "Save" with a plus icon, in six looks (primary, secondary, outline, ghost, destructive, link).
- The reader says: for each, the name "Save" and the role "button". No icon name is spoken (not "plus", not "image", not "graphic"). The look is not spoken.

**3. Disabled and pending buttons**
- Keys: Skip the four rows of "Save" buttons: NVDA, NVDA+F7 and choose Buttons, then pick "Disabled"; VoiceOver, VO+U, Left or Right arrow to Form Controls (or Buttons), pick "Disabled". Then read on with B (NVDA) or VO+Right arrow (VoiceOver) to "Link" and "Pending". Then Tab back through the row.
- The page: "Disabled" is a disabled button. "Link" is a link that looks like a button. "Pending" is drawn as busy but is an ordinary button.
- The reader says: "Disabled" with its role and the state "unavailable" (NVDA) or "dimmed" (VoiceOver). Tab skips it. "Link" is spoken as a link, not a button. "Pending" is spoken as a button, with the name "Pending" only; the busy look has no spoken word. If a reader says "busy" or "loading" here, note it.

**4. Link styled as a button**
- Keys: Move to the "Link" link. Both readers: Enter. Then go back: Alt+Left (Windows), Command+Left bracket (Safari).
- The page: the "Plain page" page opens (title "Plain page", heading "Nothing interactive here"). This page ships no script, so the browser loads it as a whole new page. Going back returns to the gallery.
- The reader says: the link was spoken as a link, with the name "Link", before you pressed Enter. As the new page loads, the reader says its title or its heading (the words and the moment are the reader's own; write them in the notes). After going back, the title "Gallery" is current again and the reader does not announce the title twice. Boosted navigation inside the app shell, which is announced by the page itself, is covered in `shell-theme-icons.md`.

**5. Icon-only button**
- Keys: Find the button after "Pending" with the cursor, or press B / VO+Right arrow.
- The page: a gear icon button with no text.
- The reader says: "Settings", "button". The name comes from the label, so a name is always spoken. Not "unlabelled", not "graphic".

**6. Button group**
- Keys: NVDA: B to "Left" from the previous button. VoiceOver: VO+Right arrow until you reach the group.
- The page: three buttons, "Left", "Centre" and "Right", in a joined group named "Text alignment".
- The reader says: on entering the group the name "Text alignment" and the role "group" once (NVDA: "Text alignment grouping"; VoiceOver: "Text alignment, group"). Then "Left, button", "Centre, button", "Right, button". The group name is not repeated on each button.

**7. Toggles**
- Keys: Tab to "Bold". Press Space. Tab to "Italic", then to "Off" (it will be skipped, see below).
- The page: three toggle buttons made of checkboxes: "Bold" (not pressed), "Italic" (pressed), "Off" (disabled).
- The reader says: "Bold", role "check box", state "not checked". After Space: "checked". "Italic": "check box, checked". "Off": "check box, unavailable" or "dimmed" in the cursor, and Tab skips it. A toggle is spoken as a checkbox, not as a "toggle button".

### Inputs

**8. Field with help text**
- Keys: Go to the "Inputs" heading. Both readers: Tab to the first field.
- The page: the field "Text" has the placeholder "Placeholder" and the help "Help below the input." under it.
- The reader says: the name "Text", role "edit" (NVDA) or "text field" (VoiceOver), and the description "Help below the input." once, after the name. The label is not repeated. In browse mode the help line is also read as its own text.

**9. Field with an error**
- Keys: Tab to the field "Invalid".
- The page: the field shows the error "This needs a value." under it.
- The reader says: the name "Invalid", role "edit", the state "invalid entry" (NVDA) or "invalid" (VoiceOver), and the description "This needs a value." once. Nothing is spoken when you arrive on the page; the error is read when the field takes focus.

**10. Disabled and read-only fields**
- Keys: Tab on from "Invalid" through "Disabled" and "Read only".
- The page: "Disabled" holds "Can't change" and cannot take focus. "Read only" holds "Can't change" and can take focus.
- The reader says: Tab skips "Disabled". In the cursor, "Disabled" is spoken with "unavailable" or "dimmed". "Read only" is spoken with the name, the role, the state "read only", and the value "Can't change".

**11. Required field**
- Keys: Tab to the field "Required".
- The page: the label shows a star after "Required".
- The reader says: the name "Required", role "edit", the state "required". The star is not spoken, not as "star", "asterisk" or "multiply". "required" is said once.

**12. Field with fixed text around it**
- Keys: Tab to the field "In a group". Then in browse mode read the line with NVDA+Up arrow, or VO+Left / VO+Right around the field.
- The page: the field has "₹" before it and "INR" after it.
- The reader says: on focus, the name "In a group" and the role "edit". The "₹" and the "INR" are reached as plain text in reading order, before and after the field. They are not part of the name.

**13. Native select**
- Keys: Tab to the field "Select". NVDA: Down arrow. VoiceOver: VO+Space to open, Down arrow to "One", Return.
- The page: a browser select whose first choice is "Choose", followed by "One" and "Two".
- The reader says: on focus, the name "Select", role "combo box" (NVDA) or "pop up button" (VoiceOver), and the value "Choose". After the change, "One" is spoken, with its place ("2 of 3") where the reader gives one.

**14. Text area**
- Keys: Tab to the field "Textarea".
- The page: a text area holding "Some text".
- The reader says: the name "Textarea", the role "edit" and "multi line" (NVDA) or "text area" (VoiceOver), and the value "Some text".

**15. Checkboxes**
- Keys: Tab to "Unchecked". Press Space. Tab to "Checked", then to "Disabled".
- The page: three checkboxes with labels "Unchecked", "Checked" (ticked) and "Disabled" (disabled).
- The reader says: "Unchecked, check box, not checked"; after Space, the state changes to "checked" and is spoken at once, with the label not repeated. "Checked, check box, checked." "Disabled": "unavailable" or "dimmed", and Tab skips it.

**16. Switches**
- Keys: Tab to the switch labelled "Off". Press Space. Tab to the switch labelled "On".
- The page: two switches. Their labels are the words "Off" and "On"; the first starts off and the second starts on.
- The reader says: the label, the role "switch", and the state. The first reads as label "Off", role "switch", state "off"; after Space, the label is still "Off" and the state is now "on". The second reads label "On", role "switch", state "on". The role must be a switch (NVDA: "switch"; VoiceOver: "switch"), not a plain check box.

**17. Radio group**
- Keys: Tab into the group "Radio group". Press Down arrow, then Up arrow.
- The page: a group of two radios, "One" (chosen) and "Two".
- The reader says: on entering the group, the group name "Radio group" once (NVDA: "Radio group grouping"; VoiceOver: "Radio group, group"), then "One, radio button, checked", with "1 of 2". Down arrow moves to "Two, radio button, checked, 2 of 2", and the choice changes. Tab leaves the group in one press.

### Display

**18. Badges, key and spinner**
- Keys: Go to the "Display" heading, then read down with Down arrow (NVDA) or VO+Right arrow (VoiceOver).
- The page: a row of badges (Neutral, Outline, Success with a check icon, Warning, Danger), a key "K", a spinner named "Loading", and two avatars.
- The reader says: each badge as its text only ("Neutral", "Outline", "Success", "Warning", "Danger"); the check icon is not spoken. "K" as plain text. The spinner is a status named "Loading": say "Loading" once, or say nothing, as the reader decides for an empty status; no icon name is spoken ("loader", "circle"). Record which.

**19. Avatars**
- Keys: Continue reading with Down arrow or VO+Right arrow.
- The page: two person icons, named "Anu Lakhotia" and "Small". The toolkit has no avatar group; each avatar stands alone.
- The reader says: "Anu Lakhotia", role "graphic" (NVDA) or "image" (VoiceOver), then "Small", role "graphic" or "image". The names are spoken once each.

**20. Alerts**
- Keys: Continue reading down.
- The page: four alerts in order: "Information" with "Something to know."; "Saved" with "Your changes are saved."; "Careful" with "This takes effect at once."; "Failed" with "That didn't work.". The last two are alerts (role alert); the first two are status messages.
- The reader says: each alert's title and then its text, in that order, once. The icon is not spoken. When you reload the page, note whether the reader spoke the two alerts as the page loaded; that is the reader's choice for a static alert, so record it in the notes and do not mark it as a failure.

**21. Progress bars**
- Keys: Read down to the progress bars. NVDA: browse mode, Down arrow (do not rely on beeps). VoiceOver: VO+Right arrow.
- The page: a bar named "Upload" at 40 of 100, and a bar named "Working" with no value.
- The reader says: "Upload", role "progress bar", value "40 percent". "Working", role "progress bar", with no percentage (VoiceOver: "indeterminate" or no value). The value is not spoken as "0".

**22. Card**
- Keys: Read on to the card. NVDA: press H to the heading "A card".
- The page: a card with a title, a description, a button "Act", a body and a footer button.
- The reader says: the title "A card" as a heading, level 2. Then "With a description", the button "Act", "Its content." and the button "Footer". Reading order is title, description, action, content, footer.

**23. Empty state**
- Keys: Read on to the empty state.
- The page: an icon, the text "No deals yet", "Create one to start." and a button "New deal".
- The reader says: "No deals yet" as plain text (it is not a heading), "Create one to start.", then the button "New deal". The icon is not spoken.

**24. Item rows**
- Keys: Read on to "An item" and "A link item".
- The page: "An item" has a description "With a description" and a button "Open". "A link item" is a whole-row link.
- The reader says: "An item", "With a description", then "Open, button". "A link item" is spoken once, as a link, with no repeated text inside it.

**25. Breadcrumb**
- Keys: Read on to the breadcrumb. NVDA: D to jump between landmarks. VoiceOver: VO+U, Landmarks.
- The page: a trail named "Example trail": the link "Deals", and the current page "Agra".
- The reader says: a navigation landmark named "Example trail". A list of 2 items. "Deals" as a link. "Agra" is the current page: NVDA says "current page", VoiceOver says "current page" or reads it as the last item. It is not a link.

**26. Pagination**
- Keys: Read on to the pagination, or jump with the landmarks list (NVDA: D; VoiceOver: VO+U, Landmarks).
- The page: a navigation named "Pagination" showing page 5 of 10: "Previous page", 1, a gap, 4, 5, 6, a gap, 10, "Next page".
- The reader says: a navigation landmark named "Pagination". The links are named "Previous page", "Page 1", "Page 4", "Page 5", "Page 6", "Page 10" and "Next page". "Page 5" also conveys "current page". The gaps are not spoken. The links are named "Page 4", not "4".

**27. Skeletons, separators and the ratio box**
- Keys: Read on past the two placeholder lines.
- The page: two grey placeholder lines, a horizontal line, "A", a vertical line, "B", and a wide box containing "21:9".
- The reader says: the placeholders are silent. The horizontal line may be spoken as "separator". Then "A", the vertical line (a separator), "B". Then "21:9". Nothing is read for the skeletons, and no stray word like "skeleton" is heard.

**28. File tile, markers and messages**
- Keys: Read on to "report.pdf", "Today", "Earlier" and the two messages.
- The page: a file tile "report.pdf", "2.4 MB", the markers "Today" and "Earlier", a message from "Anu" with a bubble "Hello" and the time "10:42", and a reply bubble "Hi there".
- The reader says: "report.pdf" as a link, "2.4 MB". "Today" and "Earlier" as plain text. For the message: the avatar "Anu" as a graphic, the header "Anu", the bubble "Hello" and the footer "10:42", in that order. "Hi there" follows. The name "Anu" is spoken twice here, once for the avatar and once for the header; note it, and treat it as a failure only if the text "Hello" or the time is missing or out of order.

**29. Headings, text and a plain table**
- Keys: Read on. NVDA: press T to the table, then Ctrl+Alt+Down arrow and Ctrl+Alt+Right arrow to move by cell. VoiceOver: VO+Right arrow to the table, VO+Shift+Down arrow to interact, then VO+arrow keys.
- The page: a level 3 heading "A heading", some text, code text "code()", a right-to-left phrase, and a table "A plain table" with columns "Name" and "Amount" and rows "Agra 1,200.00" and "Gulati 3,400.00". The second row is drawn selected.
- The reader says: the heading "A heading", level 3. For the table: the caption "A plain table", a table of 2 columns, the column headers "Name" and "Amount". Moving to "1,200.00" says the column header "Amount" with the cell. Moving down to "3,400.00" says the same header. The Gulati row is selected by colour only: the markup carries no selected state on a plain table row, so no reader says "selected", and none should.

### Commands

**30. Commands that change the page with no request**
- Keys: Go to the "Commands" heading. Tab to "Toggle", press Enter or Space. Tab to "Monthly", press Enter, then "Yearly", press Enter. Read the bold text after them.
- The page: "Toggle" hides and shows the line "Toggled by a command, with no request.". "Monthly" and "Yearly" change the bold text between "₹499 a month" and "₹4,999 a year".
- The reader says: the buttons are "Toggle, button", "Monthly, button", "Yearly, button". None of the three is a live region, so nothing is spoken after you press them: that is expected. When you move the cursor to the text afterwards, it reads the new text ("₹4,999 a year" after "Yearly") and "Toggled by a command, with no request." is gone or back. If the reader speaks the new text by itself, note it.

**31. Character counter**
- Keys: Tab to the field "Bio" and type "Namaste". Then move the cursor to the line below the field.
- The page: the counter under the field changes from "0/160" to "7/160" as you type.
- The reader says: while you type, the reader echoes your characters and nothing else. The counter is not announced as you type (it is not a live region). When you move to it, it reads "7/160".

## Results

Fill in the sign-off block for this run, then one row for each step.

| | |
|---|---|
| Script | base-components.md |
| Tester | |
| Date | |
| Operating system and version | |
| Browser and version | |
| Screen reader and version | |
| Speech settings changed from the defaults | none, except NVDA progress bar output set to "Speak" |
| Build under test (commit) | |
| Proof app and port | UI proof app, 8090 |
| Steps run | |
| Steps failed | |
| Issues opened | |

| Step | Expected | NVDA heard / pass | VoiceOver heard / pass | Notes |
|---|---|---|---|---|
| 1 | Title "Gallery"; one h1 and five h2 headings listed with levels | | | |
| 2 | "Save, button" for each look; no icon name | | | |
| 3 | Disabled unavailable and skipped by Tab; Link is a link; Pending is a plain button | | | |
| 4 | Link spoken as a link; Enter opens Plain page; back returns to Gallery | | | |
| 5 | "Settings, button" | | | |
| 6 | Group "Text alignment" spoken once, then three buttons | | | |
| 7 | Toggles are checkboxes; state changes spoken on Space; Off skipped | | | |
| 8 | Name "Text", edit, description "Help below the input." once | | | |
| 9 | Name "Invalid", invalid entry, description "This needs a value." | | | |
| 10 | Disabled skipped; Read only focusable with read-only state and value | | | |
| 11 | "Required", edit, required; the star is not spoken | | | |
| 12 | Name "In a group", edit; "₹" and "INR" read as separate text | | | |
| 13 | "Select", combo box or pop-up, "Choose"; then "One" after the change | | | |
| 14 | "Textarea", multi-line edit, "Some text" | | | |
| 15 | Checkbox states spoken; Disabled skipped; label not repeated | | | |
| 16 | Switch role and state; label "Off" stays, state turns on | | | |
| 17 | Radio group named once; "One" checked 1 of 2; Down moves to "Two" 2 of 2 | | | |
| 18 | Badges as text; no icon names; spinner "Loading" once or silent | | | |
| 19 | "Anu Lakhotia" and "Small" as graphics, once each | | | |
| 20 | Four alerts read as title then text; no icon spoken | | | |
| 21 | "Upload", progress bar, 40 percent; "Working" with no percent | | | |
| 22 | Heading "A card" level 2, then description, Act, content, Footer | | | |
| 23 | "No deals yet" plain text, description, "New deal" button | | | |
| 24 | "An item", description, "Open, button"; "A link item" once as a link | | | |
| 25 | Navigation "Example trail"; list of 2; "Agra" current page, not a link | | | |
| 26 | Navigation "Pagination"; "Page 5" current; gaps not spoken | | | |
| 27 | Skeletons silent; separators; "A", "B", "21:9" | | | |
| 28 | File link, markers, message avatar, header, bubble, time in order | | | |
| 29 | Heading level 3; table caption, headers spoken per cell; no "selected" | | | |
| 30 | Buttons named; no spoken feedback after press; new text found by cursor | | | |
| 31 | Typing echoes only; counter read "7/160" on arrival | | | |
