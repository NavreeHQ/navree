# One-time code field screen reader script

## Purpose

The one-time code field is one real text input that the stylesheet draws as boxes. A reader must hear one field with its name and help, not six separate boxes. This script settles what is spoken when a person types, deletes, pastes and reviews a code, how the field's states (read-only, disabled, invalid) are spoken, what is said when the code is whole or the form sends itself, and that the field works the same with JavaScript off. The field has no live region of its own, so a step also checks that nothing extra is spoken.

## Page to open

The UI proof app, http://127.0.0.1:8090/otp. See [Start the proof apps](README.md#start-the-proof-apps).

The page has these fields, in this order:

| Label | What it is |
|---|---|
| Verification code | Six digits, with the help "Six digits, sent to +91 98765 43210." |
| Transaction PIN | Six digits, drawn in two groups of three |
| Voucher | Eight letters or digits |
| Four-digit PIN | Four digits |
| Twelve characters | Twelve digits |
| Verify | The button that sends the five fields above |
| Code from the bank (sent when whole) | Six digits, in a form that sends itself |
| Saved code | Six digits, already filled in: 482915 |
| Read-only code | 482915, read-only |
| Disabled code | 482915, disabled |
| Expired code | 123456, with the error "That code has expired. Ask for a new one." |
| the Urdu field | A six-digit field in a right-to-left block |

## Before you start

- JavaScript on, window at least 1200 px wide, browser zoom 100%. Steps 20 and 21 turn JavaScript off; turn it on again afterwards.
- Start the reader with its speech viewer (NVDA) or caption panel (VoiceOver) open.
- Keep typing echo on. NVDA: Settings > Keyboard > "Speak typed characters" on. VoiceOver: VoiceOver Utility > Verbosity > Typing, characters on. The scripts say when a spoken character is the reader's own echo of a key.
- Put the text `482 915` (with the space) on the clipboard from a text editor before step 8.
- To read a value one character at a time, press Home, then the Right arrow key; the reader speaks each character. Steps use this to see exactly what is in the field.
- Reader mode: browse mode (NVDA) to find fields, focus mode inside them. NVDA switches itself when you Tab into a field. VoiceOver: Tab between fields, then type.
- Reload the page before you begin, so that every field is as the page drew it.

## Steps

### Finding the fields

**1. Open the page and list the form fields**
- Keys: NVDA: NVDA+T for the page title, then NVDA+F7 and choose Form fields. VoiceOver: VO+U, then the Left and Right arrows to Form Controls.
- The page: it is titled "One-time codes". There is one text field for each code, and one Verify button.
- The reader says: the title "One-time codes". In the list of form fields each code appears once, by its label, as an edit or text field. There is no list of single-digit boxes, and no field called "Digit 1".

### The six-digit field

**2. Reach Verification code**
- Keys: Both readers: Tab until the reader says "Verification code".
- The page: focus is in the field. It is empty.
- The reader says: the name "Verification code", the role (edit or text field), that it is empty (NVDA "blank"), and the help "Six digits, sent to +91 98765 43210." as its description. The name and the help are each said once. It does not say "required" and does not say "invalid".

**3. Type more digits than fit**
- Keys: Both readers: type `4829157` (seven digits).
- The page: the field takes `482915` and stops. The seventh digit does not appear.
- The reader says: each digit as you type it (the reader's own typing echo, including the seventh key). Then press Home and the Right arrow six times: the characters spoken are 4, 8, 2, 9, 1, 5 in that order. A seventh Right arrow press says nothing more, because the field ends. No space or dash is in the value.

**4. Type characters a code cannot hold**
- Keys: Both readers: Ctrl+A (Command+A on macOS) to select the code, Backspace to clear it, then type `4a8 2-9x1 5`.
- The page: letters, the space and the dash are dropped as they are typed. The field ends up holding `482915`.
- The reader says: the reader may echo each key you press, including `a`, `space` and `dash`; that is its own typing echo and not a failure. Then press Home and the Right arrow six times: the characters are 4, 8, 2, 9, 1, 5. A letter or a space in the value is a failure.

**5. Finish the code, and hear nothing more**
- Keys: Both readers: clear the field, then type `482915` and wait three seconds without pressing anything.
- The page: when the sixth digit goes in, the field is whole. It fires a `change` event for the page, and it does not send this form, because this field did not ask for that. The page stays at `/otp`.
- The reader says: the sixth digit as the typing echo. After that, nothing for three seconds. There is no "code complete" announcement, and no other text is spoken. If the reader speaks a new sentence after the sixth digit, that is a failure.

**6. Delete and move through the code**
- Keys: Both readers: Backspace once, then the Left arrow twice, then Home.
- The page: Backspace takes off the last digit, leaving `48291`. The arrow keys move the caret one character at a time.
- The reader says: on Backspace, the deleted character (the reader's own echo; NVDA says "5"). On each Left arrow, the character the caret passes. On Home, the first character, 4. Then Tab away to the next field and press Shift+Tab to come back: the reader says "Verification code", the role, and the value `48291`.

**7. Type to full again**
- Keys: Both readers: press End, then type `5`.
- The page: the code is `482915` again, so it is whole again.
- The reader says: the digit 5 as the typing echo, and nothing else. A code that was whole, lost a digit and became whole again is not announced either time.

**8. Paste a code with a space in it**
- Keys: NVDA: Ctrl+A to select the code, then Ctrl+V. VoiceOver: Command+A, then Command+V. The clipboard holds `482 915`.
- The page: the whole paste lands, with its space taken out. The field holds `482915`. The sixth digit is not lost to the length limit.
- The reader says: nothing is required for the paste itself (readers differ on whether they say "selected" and the pasted text). Press Home and the Right arrow six times: the characters are 4, 8, 2, 9, 1, 5. There is no space.

### The other fields of the first form

**9. Transaction PIN (drawn in groups)**
- Keys: Both readers: Tab to the next field.
- The page: the field is the same single input; the heavier line after the third box is drawn by the stylesheet only.
- The reader says: "Transaction PIN", the role, and that it is empty. It has no help text, so no description. The reader does not mention groups, boxes or a dash. Type `482915`; the six digits are echoed, and Home then Right arrow six times speaks 4, 8, 2, 9, 1, 5.

**10. Voucher takes letters and digits**
- Keys: Both readers: Tab to "Voucher", then type `ab-12 cd34`.
- The page: letters become capitals, the dash and the space are dropped. The field holds `AB12CD34`.
- The reader says: "Voucher", the role, empty. After typing, press Home and the Right arrow eight times: the characters are A, B, 1, 2, C, D, 3, 4 (a reader may say "capital A" or raise its voice; the letters must be capitals and in that order).

**11. Four-digit PIN stops at four**
- Keys: Both readers: Tab to "Four-digit PIN", type `12345`.
- The page: the field holds `1234`; the fifth digit does not appear.
- The reader says: "Four-digit PIN", the role, empty. Then the digits as echoed. Home and Right arrow four times speak 1, 2, 3, 4. A fifth Right arrow says nothing more.

**12. Send a code that is too short**
- Keys: Both readers: Shift+Tab until the reader says "Verification code", clear it and type `4829`. Tab forward to the "Verify" button, and press Enter (VoiceOver: VO+Space).
- The page: the form is not sent. The browser refuses it because four digits do not match six, and puts focus in "Verification code". The address stays `/otp` and the title stays "One-time codes".
- The reader says: focus lands on "Verification code" and the reader says its name, the role and the value `4829`, with the invalid state if the reader reports it. The browser may also show a message; its words are the browser's own (Chrome and Firefox differ) and the reader may speak them. Write the words in the notes. The step fails if nothing at all is spoken, or if focus stays on the Verify button.

**13. Send a whole code**
- Keys: Both readers: in "Verification code" type or paste so it holds `482915` (Ctrl+A, then type it). Tab to "Verify" and press Enter (VoiceOver: VO+Space).
- The page: the browser sends the form, and a new page loads. It is titled "Posted" and shows two blocks of plain text and a "Back" button. The second block says what the server made of each code, and includes the line `code: ok 482915`.
- The reader says: the new page's title, "Posted". In browse mode, the lines `code=482915` and `code: ok 482915` can be read, and there is a button "Back". Press Back (it is a link styled as a button) to return to `/otp` before the next step.

### The field that sends itself

**14. Code from the bank (sent when whole)**
- Keys: Both readers: Tab to "Code from the bank (sent when whole)", type `48291`, wait two seconds, then type `5`.
- The page: after five digits nothing happens. When the sixth digit goes in, the form is sent at once and the "Posted" page loads. This page also shows `auto: ok 482915`.
- The reader says: the label, including the words "(sent when whole)", is the warning that the form will send itself, so the name must include them. After the fifth digit, nothing extra. After the sixth, the reader announces the new page's title "Posted" within about two seconds. Press Back to return.

### States

**15. A code that is already filled in**
- Keys: Both readers: Tab to "Saved code".
- The page: the field holds `482915`. It belongs to no form, so nothing is sent by any key.
- The reader says: "Saved code", the role, and the value `482915`. Press Backspace: the value is `48291`, and nothing but the deleted character is spoken. Type `5`: the value is whole again, and nothing is announced.

**16. A read-only code**
- Keys: Both readers: Tab to "Read-only code", then type `999999` and press Backspace.
- The page: the code stays `482915`.
- The reader says: "Read-only code", the role, the value, and that it is read-only (NVDA "read only"; VoiceOver "read only" or "not editable"). Typing changes nothing and no digit is spoken as accepted.

**17. A disabled code**
- Keys: Both readers: Tab from the field before it. In browse mode find "Disabled code" with the arrow keys.
- The page: Tab skips the field, because it is disabled.
- The reader says: Tab does not stop on "Disabled code". In browse mode the reader says its name, the role and that it is unavailable (NVDA "unavailable"; VoiceOver "dimmed").

**18. An invalid code, with its error**
- Keys: Both readers: Tab to "Expired code".
- The page: the field holds `123456`. It is marked invalid, and the message "That code has expired. Ask for a new one." sits under it. The message is tied to the field as its description. The page does not announce it when it loads.
- The reader says: "Expired code", the role, that the entry is invalid (NVDA "invalid entry"; VoiceOver "invalid"), the value, and the description "That code has expired. Ask for a new one." The description is spoken once. Nothing about the error was spoken when the page first loaded; if the reader speaks it before you reach the field, that is a failure.

**19. A field in a right-to-left block**
- Keys: Both readers: Tab to the last field, whose label is in Urdu. Type `123456`, then Home and the Right arrow six times.
- The page: the digits stay left to right inside the boxes, and the Right arrow moves forward through them.
- The reader says: the Urdu label (if the voice has no Urdu, it may spell or skip it; mark the step n/a for the label only, and write that in the notes), the role, and empty. The characters spoken are 1, 2, 3, 4, 5, 6 in that order. If the order is reversed, that is a failure.

### JavaScript off

**20. The same field without script**
- Keys: Turn JavaScript off in the browser, reload `/otp`. Both readers: Tab to "Verification code". Type `482 915` (with the space), Tab to "Verify" and press Enter.
- The page: with no script, nothing cleans the input, so the space stays in the field, and the browser's own length limit of six characters cuts what you typed to `482 91`. The browser refuses to send, because that is not six digits, and puts focus back in "Verification code". Now clear the field, type `482915`, and press Enter on "Verify": the "Posted" page loads with `code: ok 482915`.
- The reader says: "Verification code", the role and the help "Six digits, sent to +91 98765 43210." exactly as with script on. After the refusal, focus is in the field and the reader speaks its name; Home and the Right arrow then speak the characters 4, 8, 2, space, 9, 1 (the space is in the value, which the script would have taken out). After the good send, the reader announces the title "Posted".

**21. The self-sending field without script**
- Keys: Both readers: Back to `/otp` (JavaScript still off). Tab to "Code from the bank (sent when whole)", type `482915` and wait three seconds. Then press Enter.
- The page: with no script, completing the code does not send the form. Enter does, because the form has one field. The "Posted" page then loads.
- The reader says: after the sixth digit, only the typing echo and nothing else for three seconds. After Enter, the title "Posted". Turn JavaScript on again.

## Results

| | |
|---|---|
| Script | otp.md |
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
| 1 | One entry per code, by label; no per-digit fields | | | |
| 2 | Name, edit role, empty, help read once; not required or invalid | | | |
| 3 | Seventh digit refused; characters read 4 8 2 9 1 5 | | | |
| 4 | Letters, space, dash dropped; value 482915 | | | |
| 5 | Nothing spoken after the sixth digit; form not sent | | | |
| 6 | Backspace and arrows read characters; value 48291 kept | | | |
| 7 | Whole again: no announcement | | | |
| 8 | Pasted 482 915 lands as 482915 | | | |
| 9 | Transaction PIN: name only, no group words | | | |
| 10 | Voucher holds A B 1 2 C D 3 4 in capitals | | | |
| 11 | Four-digit PIN stops at 1 2 3 4 | | | |
| 12 | Short code refused; focus on Verification code; something spoken | | | |
| 13 | Posted page announced; code: ok 482915 readable | | | |
| 14 | Label has "(sent when whole)"; sends on sixth digit; title announced | | | |
| 15 | Saved code read as 482915; edits silent | | | |
| 16 | Read-only state spoken; typing changes nothing | | | |
| 17 | Disabled code skipped by Tab; unavailable in browse mode | | | |
| 18 | Invalid state and error description spoken once on focus only | | | |
| 19 | Right-to-left field: digits read 1 to 6 in order | | | |
| 20 | Same name and help with script off; space visible; refusal then send | | | |
| 21 | Self-sending field does not send without script; Enter does | | | |

## Overall result

- [ ] Every step passed in NVDA with Firefox.
- [ ] Every step passed in NVDA with Chrome.
- [ ] Every step passed in VoiceOver with Safari.
- [ ] Every failed step has an issue, with the module name and the step number.
- [ ] Every n/a has a reason in the notes.

Signed off by: ______________________  Date: ______________

Reviewed by: ______________________  Date: ______________
