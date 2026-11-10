# Dates screen reader script

## Purpose

The calendar, the date picker, the date range picker and the time picker are widgets built on a grid, a popover and a list of options, and each has keys of its own. This script settles what a reader says as you move through a calendar, open a picker, choose a value, type a wrong value, and close the picker again: day names, selected and unavailable states, the month that is announced when the calendar turns, the error that is read when a typed date is refused, and where focus goes after the picker closes.

## Page to open

The UI proof app, see [Start the proof apps](README.md#start-the-proof-apps) (http://127.0.0.1:8090).

- `/dates` for steps 1 to 29. Its sections are headed "Calendars", "Date pickers", "Range pickers" and "Time pickers". On this page today is fixed at Tuesday 3 November 2026, so the days named in the steps are always right.
- `/dates/form` for steps 30 and 31 (a booking form).
- `/dates/report` for step 31 (a report with a date range).

## Before you start

- JavaScript on, except in step 31.
- Reload `/dates` at the start of each group (steps 1, 11, 21 and 25) so that no picker is left open or changed.
- NVDA: start in browse mode. On a day button, the keys of the calendar (arrows, Home, End, Page Up, Page Down) must reach the page. NVDA should switch to focus mode on its own; if the arrow keys move NVDA's cursor instead of the focus, press NVDA+Space and write "switched by hand" in the Notes of that step.
- VoiceOver: use Tab to move focus, then press the calendar keys by themselves, without the VO keys.
- Keep the speech viewer (NVDA) or the caption panel (VoiceOver) in view. Words in quotation marks are the toolkit's own and must match; the reader adds the role names itself.
- Names of days are in English, for example "Thursday, November 12, 2026".

## Steps

### The calendar

**1. Find the first calendar**
- Keys: both readers: from the top of the page, move to the heading "Calendars", then on to the first calendar. NVDA: H, then Down arrow. VoiceOver: VO+Right arrow.
- The page: the calendar "Single date" shows November 2026 with 12 November selected.
- The reader says: a group named "Single date"; the buttons "Previous month" and "Next month"; the text "November 2026"; a grid or table named "November 2026" with 7 columns whose headers are the weekdays (Su to Sa, or their full names); and a button "Today" after the grid.

**2. Tab into the grid**
- Keys: both readers: Tab from "Next month".
- The page: focus goes to 12 November, the only day in the Tab order. A second Tab leaves the grid for the "Today" button.
- The reader says: "Thursday, November 12, 2026", that it is a button, and that it is selected. It does not read the whole month or the other days.

**3. Move by day and by week**
- Keys: both readers: Right arrow, Left arrow, Down arrow, Up arrow, then Home, then End.
- The page: focus moves one day or one week. Home goes to the first day of the week (Sunday 8 November) and End to the last (Saturday 14 November).
- The reader says: each time, only the new day in full: "Friday, November 13, 2026", then "Thursday, November 12, 2026" (selected), "Thursday, November 19, 2026", "Thursday, November 12, 2026", "Sunday, November 8, 2026", "Saturday, November 14, 2026". Days other than the 12th are not "selected". No month name is repeated.

**4. Cross into another month with the page keys**
- Keys: both readers: from Saturday 14 November, Page Down; then Page Up.
- The page: the grid turns to December 2026 and focus lands on 14 December. Page Up returns to 14 November.
- The reader says: "December 2026" once, from the live caption (within about 2 seconds), and the focused day "Monday, December 14, 2026". Going back says "November 2026" once and "Saturday, November 14, 2026". The month is not said twice for one key press.

**5. Choose a day**
- Keys: both readers: Down arrow to Thursday 19 November, then Enter (or Space). Then Up arrow, then Down arrow.
- The page: 19 November becomes the selected day and 12 November stops being selected. The calendar stays as it is (it is not in a popover).
- The reader says: after Enter, focus stays on "Thursday, November 19, 2026". Moving up, "Thursday, November 12, 2026" is no longer selected; moving down, "Thursday, November 19, 2026" is selected. Enter once more on the 19th clears the selection (the day is no longer "selected").

**6. Turn the month with the buttons**
- Keys: reload the page. Both readers: in "Single date", Tab to "Next month" (the second button) and press it twice (NVDA: Enter; VoiceOver: VO+Space). Then Shift+Tab to "Previous month" and press it once.
- The page: the month changes each time, and focus stays on the button that was pressed.
- The reader says: after the first press "December 2026", after the second "January 2027", after "Previous month" "December 2026". Each is said once. The reader does not announce a new focus target (focus did not move) and does not read a day.

**7. Days that cannot be chosen**
- Keys: both readers: move to the group "With rules", then Tab three times (Previous month, Next month, the grid). Focus lands on Tuesday 3 November. Press Right arrow four times to Saturday 7 November and press Enter. Then Left arrow, Down arrow (Friday 13 November), and Left arrow twice (Wednesday 11 November).
- The page: weekends, 11 November and every 13th are unavailable. They can take focus but cannot be chosen.
- The reader says: for 7, 11 and 13 November, the full date and that the day is unavailable (NVDA "unavailable", VoiceOver "dimmed"). Enter on an unavailable day does nothing: no "selected", no change of focus, no message.

**8. Several dates, at most three**
- Keys: both readers: move to the group "Several dates, at most three", then Tab three times to the grid. It holds 3 and 10 November, and focus lands on Tuesday 3 November. Press Right arrow and Enter on 4 November; Right arrow and Enter on 5 November; then Left arrow.
- The page: 4 November is added (three days are chosen). 5 November is refused, because the limit is three.
- The reader says: 3, 4 and 10 November are "selected" when read. 5 November is not selected, and nothing is said about a limit (the page makes no spoken message; the day just does not take the choice).

**9. A range of days**
- Keys: both readers: move to the group "Date range" (two months side by side, 9 to 12 November chosen), then Tab three times to the grid. Focus lands on Monday 9 November. Press Down arrow twice (23 November) and Enter. Then Right arrow three times (26 November) and Enter.
- The page: the first Enter begins a new range and clears the old one; the second Enter completes it, from 23 to 26 November.
- The reader says: the two grids are named "November 2026" and "December 2026". After the first Enter, 9 to 12 November are no longer selected and 23 November is selected. After the second Enter, 23, 24, 25 and 26 November are each "selected" when read, and 27 November is not.

**10. Month and year lists**
- Keys: both readers: move to the group "Date of birth" (it shows May 1990), then Tab to "Previous month", "Next month", the month list and the year list. In the month list press Down arrow to change the month (VoiceOver: VO+Space opens the list, Down arrow chooses, Enter confirms).
- The page: these are native lists named "Month" and "Year", holding "May" and "1990". The caption here is not a live region: the new month is shown by the list itself.
- The reader says: "Month", the list role and its value ("May"); after the change, the new month ("June"). The reader does not say the month a second time from a separate announcement.

### The date picker

**11. Find a date picker**
- Keys: reload the page. Both readers: move to the heading "Date pickers", then to the first field "Basic date". NVDA: H, then Tab. VoiceOver: VO+Right arrow until "Basic date".
- The page: a text box, a button that opens the calendar, and (not shown to a reader) a hidden native date field.
- The reader says: "Basic date", the role of a text box, blank (its placeholder is "MM/DD/YYYY"); the next stop is a button "Choose date" that has a popup (a dialog). There is only one field named "Basic date": the hidden native date field is never read and never reached by Tab.

**12. Open the picker from the keyboard**
- Keys: both readers: with focus in the "Basic date" box, press Down arrow.
- The page: a popover opens under the field and focus moves to Tuesday 3 November (today, because nothing is chosen).
- The reader says: a dialog named "Choose date", then the focused day "Tuesday, November 3, 2026" with today conveyed (for example "current date"). The calendar group inside the dialog is also named "Choose date": record in the Notes how many times "Choose date" is spoken. Pass: it is said for the dialog and the day follows. Fail: it is said more than twice for the one opening.

**13. Choose a day and come back**
- Keys: both readers: Right arrow, then Enter.
- The page: the dialog closes, the box holds "11/04/2026", and focus returns to the box.
- The reader says: focus goes back to "Basic date", a text box, with the value "11/04/2026". The reader does not say "selected" or a day name after the dialog closes.

**14. Close without choosing, and close from the button**
- Keys: both readers: with focus in the "Basic date" box, Down arrow to open, Right arrow, then Escape. Then Tab to the button "Choose date", press it (NVDA: Enter; VoiceOver: VO+Space), and press Escape.
- The page: Escape closes the dialog and leaves the value as it was ("11/04/2026"). When the picker was opened from the box, focus returns to the box; when it was opened with the button, focus returns to that button.
- The reader says: after the first Escape, the box "Basic date" with "11/04/2026". After the second, the button "Choose date" with its popup state. Focus is never lost to the top of the page.

**15. The Today and Clear buttons**
- Keys: both readers: go to "Date with a value" (it holds 11/12/2026). Tab from the box to "Clear" and press it. Then press Down arrow in the box to open the dialog, Tab from the focused day to "Today", and press it.
- The page: Clear empties the box and focus goes to the box. In the dialog, Today chooses 3 November and closes the dialog; focus returns to the box.
- The reader says: after Clear, "Date with a value", a text box, blank. In the dialog the foot buttons are "Today" and "Clear". After Today, the box says "11/03/2026".

**16. A typed date that is not a date**
- Keys: both readers: go to "Weekdays in the window" (the last field of "Date pickers"). Type `soon`, then press Tab. Then Shift+Tab.
- The page: a message appears under the field and the box is marked invalid. Leaving the box with a wrong value does not hold focus in it.
- The reader says: at once after Tab, the alert "Enter a date like 11/30/2026." (spoken once, even though focus has moved on; it may come before or after the name of the next control). After Shift+Tab: "Weekdays in the window", a text box, invalid entry, the text `soon`, and the message as its description.

**17. A date that exists but is not allowed**
- Keys: both readers: with focus in the same box, select all (Ctrl+A) and type `11/07/2026` (a Saturday), then Tab. Then Shift+Tab, select all, type `11/09/2026` (a Monday), Tab, and Shift+Tab.
- The page: weekends are not allowed in this window. The Saturday is refused and the box stays invalid; the Monday is accepted, the message goes and the box is valid.
- The reader says: after the Saturday, the alert "That date isn't available." once. After the Monday, no message is spoken, and when you return to the box it is not "invalid entry"; the value is "11/09/2026".

**18. A required field with help**
- Keys: both readers: go to "Due date" (the third field) and Tab into its box.
- The page: the box is required and described by the help text under it.
- The reader says: "Due date", text box, required, and the description "Type it as 11/30/2026, or pick it." The help text is said once, not twice.

**19. An error drawn by the server**
- Keys: both readers: go to "Late date" (the fourth field) by Tab. Do not type.
- The page: the box holds `garbage`, marked invalid, with the message "Choose a weekday in the next three months." under it. This is plain text from the server, not an alert.
- The reader says: nothing about this error while the page loads. On focus: "Late date", text box, invalid entry, `garbage`, and the description "Choose a weekday in the next three months."

**20. Disabled, read only and button style**
- Keys: both readers: Tab from "Late date" onward through "Disabled", "Read only" and "Button style".
- The page: "Disabled" is skipped by Tab. "Read only" is reached; its calendar button is disabled and skipped. "Button style" is a single button.
- The reader says: in browse mode, "Disabled" and its value are read as unavailable (dimmed). "Read only": text box, read only, "11/12/2026". "Button style": a button named by its text "Pick a day", with a dialog popup; after you choose a day in its dialog, focus returns to the button, whose text is now the chosen date.

### The date range picker

**21. Open a range picker**
- Keys: reload the page. Both readers: go to the heading "Range pickers" and the first button, whose text begins "Created". Press it (NVDA: Enter; VoiceOver: VO+Space).
- The page: a dialog opens with a row of presets, then two months, and focus moves to Tuesday 3 November.
- The reader says: the button "Created" followed by "Pick a date range", with a popup. Then the dialog "Pick a date range", and the day "Tuesday, November 3, 2026". Shift+Tab until the group "Presets": its buttons are "Today", "Yesterday", "Last 7 days", "Last 30 days", "This month", "Last month" and "Custom range", each a toggle button that is not pressed.

**22. Choose a preset and see it pressed**
- Keys: both readers: with focus in the presets, press "Last 7 days". Then open the picker again and Shift+Tab to the presets.
- The page: the range is chosen, the dialog closes, and focus returns to the "Created" button, whose text now shows the range (28 October to 3 November). When reopened, "Last 7 days" is pressed.
- The reader says: after choosing, the "Created" button again, with the new range as part of its name. After reopening: "Last 7 days", toggle button, pressed. In the calendar the days of the range are "selected" when read.

**23. Build a range by keys, and give it up**
- Keys: both readers: open "Created" again. On the focused day press Enter, press Right arrow three times, and press Enter. Then open it again, press Enter on a day (a start only), then press Escape.
- The page: the first Enter picks a start and the dialog stays open; the second Enter picks the end, closes, and returns focus to the button. In the second run Escape leaves the earlier range as it was.
- The reader says: after the start, that day is "selected" and the days around it are not. After the end, focus is on the "Created" button with the new range. After Escape, the button reads the earlier range and focus is on the button.

**24. Apply and Cancel, and presets that cannot be used**
- Keys: both readers: open the third range picker, "Window" (it has Cancel and Apply). On the focused day press Enter, then Right arrow and Enter (the dialog stays open). Tab to "Cancel" and press it. Open it again, choose two days the same way, Tab to "Apply" and press it. Then open "Limited" and Shift+Tab to its presets.
- The page: nothing is applied until "Apply". "Cancel" closes and keeps the old range. In "Limited" a one-day range is too short, so "Today" cannot be used.
- The reader says: the foot of the dialog has "Clear", "Cancel" and "Apply". After Cancel, focus is on the "Window" button and its range has not changed; after Apply, it has changed. In "Limited", the preset "Today" is unavailable (dimmed) and cannot be pressed.

### The time picker

**25. Open the list of times**
- Keys: reload the page. Both readers: go to the heading "Time pickers" and the field "Twelve hour". With focus in the box press Down arrow.
- The page: the box holds "2:30 PM". A popover opens and focus moves into a list of times, with 2:30 PM the active one.
- The reader says: before opening: "Twelve hour", text box, "2:30 PM", and the button "Choose time" with a popup. After opening: a dialog "Choose time", then a list box "Choose time" (the same name twice; record how many times it is said, as in step 12), and the active option "2:30 PM", selected, "30 of 48".

**26. Move in the list and choose**
- Keys: both readers: Down arrow, Page Down, Home, End, then Enter.
- The page: the active option moves; Enter chooses it, closes the list and returns focus to the box.
- The reader says: each move says only the new option and its place: "3:00 PM" "31 of 48"; "7:00 PM" "39 of 48"; "12:00 AM" "1 of 48"; "11:30 PM" "48 of 48". Only the option that was the value when the list opened (2:30 PM) is "selected". After Enter the box says "Twelve hour" and its new value "11:30 PM".

**27. A list with gaps**
- Keys: both readers: go to "Office hours" (empty) and press Down arrow in the box to open the list. Press Down arrow 11 times (11:45 AM), and once more. Press Escape.
- The page: the list shows only 9:00 AM to 5:00 PM in steps of 15 minutes, with 12:00 to 12:59 left out (29 options). Escape closes it and focus returns to the box.
- The reader says: the first option "9:00 AM", "1 of 29"; "11:45 AM", "12 of 29"; then "1:00 PM", "13 of 29". No lunchtime option is ever said.

**28. Typed times that are refused**
- Keys: both readers: in "Office hours" type `soon` and press Tab. Shift+Tab, select all, type `12:30 pm`, and Tab. Shift+Tab, select all, type `2:15 pm`, and Tab. Then go to "Invalid" (a field of the same section) and Tab into its box.
- The page: a wrong time and a lunchtime each show a message; a good time clears it. "Invalid" shows a message drawn by the server.
- The reader says: the alerts "Enter a time like 2:30 PM." and then "That time isn't available.", each once. After "2:15 pm" no message is spoken and the box is not invalid. In "Invalid": text box, invalid entry, `soon`, described by "Enter a time like 2:30 PM." (read on focus, not at page load).

**29. Stepping a time with the arrow keys**
- Keys: both readers: go to "Every minute" (the last time field, empty). It has no list button. In the box press Up arrow, then Up arrow again, then Down arrow.
- The page: Up arrow sets the box to the next time (12:00 AM, then 12:01 AM); Down arrow goes back a minute.
- The reader says: after each key, the new value of the box ("12:00 AM", "12:01 AM", "12:00 AM"). If the reader says nothing when the value changes, mark the step failed: the page makes no other announcement for it.

### Forms, and JavaScript off

**30. The booking form**
- Keys: open `/dates/form`. Both readers: Tab into "Start date", type `11/07/2026`, press Tab (the alert is read). Shift+Tab, select all, type `11/09/2026`, Tab. Move to "Arrives at", type `2:15 pm`, Tab. Then Tab to the button "Book" and press it.
- The page: the fields are "Start date", "End date", "Arrives at" and "Note". The Saturday is refused with a message under the box. A good date and a good time let the form send, and the page "Booked" opens with a toast.
- The reader says: for "Start date", the help "A weekday in the next three months." as its description; after the Saturday, the alert "That date isn't available." once; after the fix, no alert. After "Book": the new page title (it contains "Booked") once, and the toast text "Booked" once as a status.

**31. JavaScript off: booking and report**
- Keys: turn JavaScript off and reload `/dates/form`. Both readers: Tab to "Start date" and type the date in the field's own format for 7 November 2026 (in US English, month, day and year as digits), Tab to "Arrives at" and enter `12:30 PM`, then press "Book". Then open `/dates/report` and look at the fields "From" and "To".
- The page: the native date and time fields are shown (no box, no calendar button). The server answers with the form again, with a message under each refused field, and keeps what was sent. The report has two native date fields and an "Apply" button.
- The reader says: the label "Start date" with the browser's own date field (a reader may read its month, day and year parts one by one; that is the browser's field). After the answer, each refused field is invalid and described by a message that says the date (or the time) isn't available. The report fields are named "From" and "To". Turn JavaScript on again afterwards.

## Results

| | |
|---|---|
| Script | `dates.md` |
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
| 1 | Group "Single date", month buttons, "November 2026", named grid, "Today" | | | |
| 2 | "Thursday, November 12, 2026", button, selected; one tab stop | | | |
| 3 | Only the new day said on each move; Home and End reach Sunday and Saturday | | | |
| 4 | "December 2026" said once; then "Monday, December 14, 2026" | | | |
| 5 | Chosen day becomes selected, old one not; Enter again clears | | | |
| 6 | Month said once per press; focus stays on the button | | | |
| 7 | 7, 11 and 13 November said as unavailable; Enter does nothing | | | |
| 8 | Third day selected; fourth refused; no spoken message | | | |
| 9 | New range replaces old; days in range read as selected | | | |
| 10 | "Month" and "Year" lists with values; no second announcement | | | |
| 11 | "Basic date" text box, blank, "Choose date" button; one field only | | | |
| 12 | Dialog "Choose date", then "Tuesday, November 3, 2026", today conveyed | | | |
| 13 | Dialog closes; box "Basic date" says "11/04/2026" | | | |
| 14 | Escape keeps value; focus back on box or on the button | | | |
| 15 | Clear empties box; Today sets "11/03/2026"; focus in box | | | |
| 16 | Alert "Enter a date like 11/30/2026." once; box invalid | | | |
| 17 | Alert "That date isn't available." once; valid date clears it | | | |
| 18 | "Due date" required, described by the help text once | | | |
| 19 | Server error read on focus only: "Choose a weekday in the next three months." | | | |
| 20 | Disabled skipped; read only said; button style named by its text | | | |
| 21 | "Created" button; dialog "Pick a date range"; presets group, not pressed | | | |
| 22 | Preset closes dialog, focus on button; "Last 7 days" pressed on reopening | | | |
| 23 | Start then end chosen; Escape keeps earlier range; focus on button | | | |
| 24 | Cancel keeps range, Apply changes it; "Today" unavailable in "Limited" | | | |
| 25 | "Twelve hour" box; dialog and list "Choose time"; "2:30 PM", "30 of 48" | | | |
| 26 | Each option with its place; only the opening value selected; Enter returns focus to box | | | |
| 27 | "9:00 AM" "1 of 29"; no lunchtime options; "1:00 PM" "13 of 29" | | | |
| 28 | Alerts "Enter a time like 2:30 PM." and "That time isn't available."; server error on focus | | | |
| 29 | Each arrow key makes the reader say the new time | | | |
| 30 | Help and alert once; "Booked" title and toast said once each | | | |
| 31 | Native fields labelled; server errors as descriptions; "From" and "To" named | | | |

Overall: see [results-template.md](results-template.md) for the sign-off checklist.
