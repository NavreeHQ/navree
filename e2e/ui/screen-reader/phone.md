# Phone input screen reader script

## Purpose

The phone input is a number box with a country picker beside it. The picker is a button that opens a searchable list of countries, and the box formats the number as you type and picks the country when you type or paste a number that starts with a country code. This script settles what a reader says for the button, the list, the search, the choice of a country, the announcement that a typed number chose a country, the count of countries left in a search, and the errors on the number.

## Page to open

The UI proof app, see [Start the proof apps](README.md#start-the-proof-apps) (http://127.0.0.1:8090).

- `/phone`: a card "New contact" with the fields "Name", "Mobile" and "Office", then the section "States" and a right to left example.

## Before you start

- JavaScript on, except in step 24.
- Reload `/phone` before step 1. The form starts with the United Kingdom chosen for both numbers.
- NVDA: use browse mode to find the controls; switch to focus mode (NVDA+Space) before typing in a box if NVDA has not done it for you. In the search box and in the number box NVDA should switch by itself.
- VoiceOver: use Tab to move between controls and VO+Space to press a button.
- Keep the speech viewer (NVDA) or the caption panel (VoiceOver) open. The count of countries and the "Country set to" message are spoken by a live region (a status), so they come a moment after the key that caused them.
- Words in quotation marks are the toolkit's own.

## Steps

### The form and the country list

**1. Find the form**
- Keys: both readers: move to the card titled "New contact". NVDA: H or Down arrow. VoiceOver: VO+Right arrow.
- The page: a card with the text "Numbers are checked on the server against each country's numbering plan." and the fields "Name", "Mobile" and "Office", and a button "Save contact".
- The reader says: the title and the description, then each label. "Mobile" has the help "Type it as you would dial it, or paste it with its country code."

**2. Tab order and one country control per number**
- Keys: both readers: Tab from "Name" through "Mobile" and "Office" to "Save contact".
- The page: for each number the Tab stops are the country button, then the number box. The hidden list of countries that a form without script would use is not a stop.
- The reader says: "Name", text box; then a button "Country: United Kingdom +44" that has a popup (a list) and is collapsed; then "Mobile", an edit box, with the placeholder "07400 123456" (spoken if the box is empty) and the description with the help text; then a button "Country: United Kingdom +44" for "Office"; then "Office"; then "Save contact". Only one control named "Country" is reachable for each number (never a second, plain list).

**3. Open the list of countries**
- Keys: both readers: Tab to the button "Country: United Kingdom +44" for "Mobile". NVDA: Enter or Space. VoiceOver: VO+Space.
- The page: a list opens under the button. Focus moves to a search box inside it. The button is now expanded.
- The reader says: the search box named "Search countries", as a combo box that is expanded; the active option "United Kingdom +44", selected. About half a second later, one status: the number of countries in the list followed by "countries" (for example "<n> countries", where n is the number of options in the list). The status is said once.

**4. Search**
- Keys: both readers: type `fra` in the search box, quickly, and stop.
- The page: the list narrows to the countries whose names (or codes) start with "fra", and the first one becomes active.
- The reader says: the first match as the active option, with its name and dial code, for example "France +33" (the other matches are not read). After you stop typing (about half a second), one status: the number of matches followed by "countries". It is said once, not after each letter you typed.

**5. Move through the matches**
- Keys: both readers: Down arrow, Up arrow, Page Down, Page Up.
- The page: the active option moves; the list scrolls to keep it in view.
- The reader says: each time only the new option, its name and dial code, and its place among the countries now listed ("2 of 4"). The option for the country that is chosen now (United Kingdom) is "selected" only if it is listed. Nothing is said about the count again while you move.

**6. Choose a country**
- Keys: both readers: move to "France +33" and press Enter. Then press Shift+Tab.
- The page: the list closes, "France" is chosen and focus goes to the number box ("Mobile"), not back to the button. The placeholder of the box becomes "06 12 34 56 78".
- The reader says: after Enter, focus is on "Mobile", an edit box, blank, with the new placeholder. No status message is spoken for a choice made in the list (the page makes none). After Shift+Tab: the button "Country: France +33", collapsed.

**7. Close without choosing**
- Keys: both readers: press Enter (NVDA) or VO+Space (VoiceOver) on "Country: France +33", type `ger`, wait for the count to be spoken, then press Escape.
- The page: the list closes and focus returns to the button. The country stays France.
- The reader says: the button "Country: France +33", collapsed. It does not say "Germany".

**8. A key on the closed button**
- Keys: both readers: with focus on the button, press the digit `9`. NVDA: first press NVDA+Space if arrow keys and digits do not reach the page. VoiceOver: no VO keys.
- The page: the list opens with `9` already typed in the search box, showing the countries whose dial code starts with 9.
- The reader says: "Search countries", combo box, expanded, with the text `9`; the first match as the active option; about half a second later the status with the number of matches and "countries". Press Escape to close.

**9. A search that finds nothing**
- Keys: both readers: open the list again, type `zzzz`, and stop.
- The page: the list shows the line "No country found." and no option is active.
- The reader says: about half a second after you stop, the status "No country found." once. No option is read. Press Escape to close.

### The number

**10. A number with a country code picks the country**
- Keys: both readers: Tab to the "Mobile" box, select all (Ctrl+A) and type `+91 98765 43210` at a normal speed.
- The page: the box formats the number as you type. When `+91` is complete, the country changes to India without leaving the box.
- The reader says: each typed character echoed once. The reader does not read the whole number again after each key. At `+91`, one status: "Country set to India +91", said once, and not again as you type the other digits.

**11. The same country is not announced again**
- Keys: both readers: in the same box select all and type `+91 99999 11111`. Then Shift+Tab to the button.
- The page: the country is India already, so nothing changes.
- The reader says: no "Country set to" status at all while typing. The button reads "Country: India +91", collapsed.

**12. Another country code**
- Keys: both readers: Tab to the box, select all and type `+44 7400 123456`. Then Shift+Tab to the button.
- The page: the country changes from India to the United Kingdom.
- The reader says: one status, "Country set to United Kingdom +44". The button reads "Country: United Kingdom +44".

**13. A pasted number**
- Keys: both readers: type `+91 98765 43210` in "Name", select all, cut (Ctrl+X). Tab to the "Mobile" box, select all and paste (Ctrl+V).
- The page: a pasted number chooses its country in the same way as typed digits.
- The reader says: "Country set to India +91" once. The button reads "Country: India +91".

### Errors

**14. A number that is wrong for the country**
- Keys: both readers: in "Name" type `Gurpreet Singh`. Choose France in the "Mobile" list (as in steps 3 to 6), select all in the "Mobile" box, type `07400 123456`, and activate "Save contact" (NVDA: Enter or Space; VoiceOver: VO+Space).
- The page: the form is sent without leaving the page. The server refuses the number. A message appears under the field and focus moves to the "Mobile" box.
- The reader says: one status, "1 field needs attention" (said once). Focus is on "Mobile", an edit box, invalid entry, with the text `07400 123456`, and the description "Mobile: enter a phone number, with its country." The error text is read as the description of the box, not as a separate alert.

**15. A good number saves**
- Keys: both readers: choose India in the "Mobile" list, select all in the box, type `098765 43210`, and press "Save contact".
- The page: the page comes back with a card "Saved Gurpreet Singh" that lists what the server read: "E.164", "International", "National", "Type" and "Country".
- The reader says: the page title once; then, in browse mode, the card title and the terms and values: "E.164" "+919876543210", and "Country" "India".

### The states

**16. Empty with a country, and a saved number**
- Keys: both readers: go to the heading "States" and the fields "Empty, United Kingdom" and "A saved number, United States".
- The page: the first has a country and no number. The second holds a saved United States number.
- The reader says: for the first: the button "Country: United Kingdom +44" and the box "Empty, United Kingdom", blank. For the second: the button "Country: United States +1" and the box "A saved number, United States" holding the digits 650 253 0000 (in the written form the box shows).

**17. No country chosen**
- Keys: both readers: go to "No country chosen".
- The page: the button has no country, and the box has no example.
- The reader says: the button named only "Country" (no country name), collapsed. The box "No country chosen", blank, with no placeholder spoken.

**18. An invalid number**
- Keys: both readers: Tab to the box "Invalid".
- The page: the country is France, the box holds `06 12`, and a message sits under the field.
- The reader says: "Invalid", an edit box, invalid entry, `06 12`, and the description "Enter a valid number for France." The message is not spoken when the page loads.

**19. Disabled**
- Keys: both readers: Tab from "Invalid" onward.
- The page: the "Disabled" number and its country button cannot take focus.
- The reader says: Tab skips them. In browse mode they are read as unavailable (dimmed).

**20. Suggested countries first**
- Keys: both readers: go to "Suggested countries first", open its list (Enter or VO+Space on the button) and press Down arrow four times.
- The page: five countries come first in this order: India, United Arab Emirates, Saudi Arabia, United Kingdom, United States. India is the chosen country.
- The reader says: the active option "India +91", selected; then "United Arab Emirates +971", "Saudi Arabia +966", "United Kingdom +44", "United States +1", each with its place in the list ("2 of n", "3 of n", and so on, n being the number of countries listed). The word "Suggested" is not spoken (it is only a group in the version without script). Press Escape.

**21. Right to left**
- Keys: both readers: go to the field named in Arabic (رقم الجوال) at the end of the page. Tab to its country button and open the list.
- The page: the labels are in Arabic. The button reads the Arabic word for "country", then the English name of the country, United Arab Emirates +971.
- The reader says: the button with an Arabic name "الدولة" followed by "United Arab Emirates +971"; the search box named "ابحث عن دولة"; after a search, the status ends with the Arabic word for countries ("{n} دولة"). If no Arabic voice is installed, mark the step n/a and say so in the Notes.

### JavaScript off

**22. Without script: the plain list**
- Keys: turn JavaScript off and reload `/phone`. Both readers: Tab from "Name".
- The page: the country button is hidden. A plain list named "Country" is shown, with "United Kingdom +44" chosen, then the number box.
- The reader says: "Country", a combo box (VoiceOver: a pop-up button) with the value "United Kingdom +44"; then "Mobile". The list has a first empty entry "Country" and the countries, each as name and dial code.

**23. Without script: choose and save**
- Keys: both readers: in the "Mobile" list choose "India +91" (type `India` in the list, or use the arrow keys), type `098765 43210` in the box, enter `Gurpreet Singh` as the name, and press "Save contact".
- The page: the page loads again with the card "Saved Gurpreet Singh" and the values read by the server.
- The reader says: the new page is read from its title. In the card: "E.164" "+919876543210", "Country" "India".

**24. Without script: an error**
- Keys: both readers: choose "France +33", type `12` in "Mobile", and press "Save contact". Then Tab to the "Mobile" box. Turn JavaScript on again afterwards.
- The page: the page loads again with the number and the country kept, and an error under "Mobile".
- The reader says: no live message (the page is simply read again). On focus, "Mobile", edit box, invalid entry, `12`, with the error text as its description. The list still reads "France +33".

## Results

| | |
|---|---|
| Script | `phone.md` |
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
| 1 | Title, description, labels; help on "Mobile" | | | |
| 2 | Country button then number box; no second Country control | | | |
| 3 | "Search countries" combo expanded; chosen option selected; count status once | | | |
| 4 | First match read; one "n countries" status after typing stops | | | |
| 5 | Each option with name, dial code and place; no repeated count | | | |
| 6 | Focus to the number box; button name "Country: France +33" | | | |
| 7 | Escape: focus on button, country unchanged | | | |
| 8 | Digit on button opens search with that digit; count status | | | |
| 9 | "No country found." once; no option read | | | |
| 10 | Characters echoed once; "Country set to India +91" once | | | |
| 11 | No second announcement for the same country | | | |
| 12 | "Country set to United Kingdom +44" once | | | |
| 13 | Pasted number: "Country set to India +91" once | | | |
| 14 | "1 field needs attention"; box invalid with error as description | | | |
| 15 | Saved card read: E.164 "+919876543210", Country "India" | | | |
| 16 | Button names with country; box names and values | | | |
| 17 | Button named only "Country"; box blank | | | |
| 18 | "Invalid" box: invalid entry, "Enter a valid number for France." | | | |
| 19 | Disabled number skipped by Tab; read as unavailable | | | |
| 20 | India, United Arab Emirates, Saudi Arabia, United Kingdom, United States in order | | | |
| 21 | Arabic names conveyed; "{n} دولة" status (or n/a) | | | |
| 22 | Plain list "Country" with "United Kingdom +44"; no button | | | |
| 23 | Saved card read after reload | | | |
| 24 | Error on "Mobile" as description; no live message | | | |

Overall: see [results-template.md](results-template.md) for the sign-off checklist.
