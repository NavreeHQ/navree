# Upload screen reader script

## Purpose

An upload is a file control with a drop area around it, a list of the files, and a status line that says what is happening to each file. A person using a screen reader cannot drag a file, so everything here is done with the file chooser. This script settles what a reader says for the control and its limits, when a file is sent, sent or refused (in words), when a file is stopped or taken out, how the buttons on a row are named, where focus goes, and what the page without script says.

## Page to open

The UI proof app, see [Start the proof apps](README.md#start-the-proof-apps) (http://127.0.0.1:8090).

- `/upload`: sections headed "A photo", "Documents, with large files in chunks", "Sent by hand", "A field in a form", "A slow upload", "States" and "Right to left".

## Before you start

- Prepare a folder of files:
  - `Simran-Kaur.png`: any small PNG, under 1 MB.
  - `photo.png`: a plain text file with the name changed to `photo.png`.
  - `big.png`: a PNG larger than 1 MB (2 MB is enough).
  - `blank.png`: an empty file (0 bytes).
  - `Salary-slip.pdf` and `Loan-form.pdf`: small PDF files.
  - `rates.csv`: a small CSV file.
- The file chooser filters by the kinds a control accepts. To pick `photo.png` and `blank.png` choose "All files" in the chooser if it hides them.
- Start with a fresh server so that the list of stored files is empty (restart the proof app). The server keeps the files in memory.
- JavaScript on, except in steps 20 to 23.
- NVDA: browse mode to find the controls. The file chooser itself is the browser's: when it closes, focus returns to the control.
- VoiceOver: Tab to move between controls and VO+Space to press a button.
- Keep the speech viewer (NVDA) or the caption panel (VoiceOver) open. The status line is a live region: its words come a moment after the action.
- Words in quotation marks are the toolkit's own.

## Steps

### One photo, sent at once

**1. Find the control**
- Keys: both readers: move to the heading "A photo", read on, then Tab to the file control. NVDA: H, then Down arrow, then Tab. VoiceOver: VO+Right arrow, then Tab.
- The page: the label "Profile photo", the prompt "Choose a file or drop it here" and the limits "PNG or JPG, up to 1 MB" are shown over a drop area. The file control covers the whole area.
- The reader says: in browse mode the three texts are read as plain text. On the control: the name "Profile photo", that it opens a file chooser (a button), and the description "PNG or JPG, up to 1 MB". There is no separate "drop area" control.

**2. Choose a good file**
- Keys: both readers: press the control (NVDA: Enter or Space; VoiceOver: VO+Space) and choose `Simran-Kaur.png`.
- The page: the file is sent at once. A row appears and ends as sent. Focus stays on the control, which is empty again.
- The reader says: the status "Simran-Kaur.png · Sent" within about two seconds, once. The earlier line "Sending Simran-Kaur.png" may be spoken before it, or replaced before it can be spoken, because a small file is quick. No percentage is spoken.

**3. Read the file row**
- Keys: both readers: move forward from the control to the list "Files". NVDA: Down arrow. VoiceOver: VO+Right arrow.
- The page: one row with the file name and its size and state, for example "2.9 KB · Sent". A sent row has no buttons.
- The reader says: a list named "Files" with one item (VoiceOver in Safari may drop list wording from a list without bullets; if so it must still say the item), then "Simran-Kaur.png" and "<size> · Sent". It reads the row once.

**4. A file of the wrong kind**
- Keys: both readers: press the control and choose `photo.png`.
- The page: the server reads the kind from the content and refuses the file. The row turns to the failed state, in red, with the reason as its text, and shows two buttons.
- The reader says: one status, "photo.png is not an accepted kind of file. Accepted: PNG or JPG.", said once. In the list, the new row has the same words.

**5. A file that is too large**
- Keys: both readers: press the control and choose `big.png`.
- The page: the page refuses it at once, without sending it.
- The reader says: one status and the row text "big.png is larger than 1 MB.", said once.

**6. An empty file**
- Keys: both readers: press the control and choose `blank.png`.
- The page: refused at once.
- The reader says: one status and the row text "blank.png is empty.", said once.

**7. Name the buttons on a row**
- Keys: both readers: Tab from the control through the buttons of the three failed rows.
- The page: each failed row shows "Try again" and "Remove". The name of the file is part of each button's name but not shown.
- The reader says: "Try again photo.png", "Remove photo.png", "Try again big.png", "Remove big.png", "Try again blank.png", "Remove blank.png", each as a button. The order follows the rows from top to bottom.

**8. Try again**
- Keys: both readers: Tab to "Try again photo.png" and press it. When the row fails again, press Shift+Tab once. (NVDA: also NVDA+Tab to ask what has focus.)
- The page: the row goes to sending and back to failed, because the server refuses the file again. The button that was pressed is hidden while the row is sending.
- The reader says: the status "Sending photo.png", then "photo.png is not an accepted kind of file. Accepted: PNG or JPG." again, each once. The reader still reports a control inside this upload ("Profile photo", or a row button) as having focus. If it reports the document, the page or nothing, mark the step failed: focus was lost with the hidden button.

**9. Take a file out**
- Keys: both readers: Tab to "Remove photo.png" and press it (NVDA: Enter; VoiceOver: VO+Space). Then do the same for "Remove blank.png", using the Delete key instead of Enter.
- The page: the row is removed and focus goes back to the file control.
- The reader says: the status "photo.png was removed", said once, then the control "Profile photo". The removal words are not said a second time when focus reaches the control (the control is described by the status line, so a reader may read the words again as its description: if it does, mark the step failed). The second removal says "blank.png was removed" in the same way.

### Stopping a file, and several files

**10. Stop a file on its way**
- Keys: both readers: go to the heading "A slow upload". Choose `rates.csv` in its control, then at once Tab to "Cancel rates.csv" and press it. The page keeps a file in the sending state for about two seconds, so be quick; if the row has already ended as sent, remove it and try again.
- The page: while sending, the row shows a progress bar and a "Cancel" button. Cancel stops the file and removes the row. Focus goes to the file control.
- The reader says: the status "Sending rates.csv" once; the button "Cancel rates.csv"; after pressing it, "rates.csv was removed" once, then the control "A slow upload". The reader does not speak each change of the percentage. If the row ended before you could cancel in three tries, mark the step n/a: step 9 covers the same code.

**11. Two files at once**
- Keys: both readers: go to "Documents, with large files in chunks". Its control is named "Documents" and the limits are "PDF, images or CSV, up to 20 MB, up to 5 files". Press the control and choose `Salary-slip.pdf` and `Loan-form.pdf` together.
- The page: both rows appear. The files are sent one after the other.
- The reader says: the status lines in order, "Salary-slip.pdf · Sent" and then "Loan-form.pdf · Sent" (the "Sending ..." lines may be replaced before they are spoken). No line is spoken twice. The list "Files" holds two items.

**12. The list of stored files, and a reload**
- Keys: both readers: read on from the "Documents" list to the text under it. Then reload the page and read the "Documents" list again.
- The page: when the last file is over, the text below the control is drawn again and lists the files the server holds, as "name (kind, size)". After a reload, the rows of the "Documents" list show the stored files, each with a button "Remove" and the file's name.
- The reader says: the stored list changes without a spoken message (it is not a live region) and without a move of focus. After the reload: "Remove Salary-slip.pdf" and "Remove Loan-form.pdf" are buttons, and the other files stored are listed in the same way.

**13. Remove a stored file**
- Keys: both readers: press "Remove Salary-slip.pdf" in the "Documents" list.
- The page: the server removes the file and the page is drawn again with a toast.
- The reader says: the toast "The file was removed." once, as a status. The list no longer has "Salary-slip.pdf".

### Sent by hand, and a field in a form

**14. A file chosen but not sent**
- Keys: both readers: go to "Sent by hand". The control is named "Price list (CSV)". Press it and choose `rates.csv`.
- The page: the file is listed as chosen, not sent. A button "Send" is shown under the list.
- The reader says: the control "Price list (CSV)" with the limits (CSV, up to 1 MB). The page makes no spoken message when the file is chosen; browse to the list "Files": it has one item, "rates.csv" and its size, and a button "Remove rates.csv". Then the button "Send". Write in the Notes whether the reader said anything on its own when the file was chosen (this is not a failure).

**15. Send it**
- Keys: both readers: press "Send".
- The page: the file is sent and the row ends as sent. The "Remove" button goes.
- The reader says: the status "rates.csv · Sent", once.

**16. A file in a form, taken out with Delete**
- Keys: both readers: go to "A field in a form". The control is named "Papers for the loan application". Choose `Salary-slip.pdf` and `Loan-form.pdf`. Tab to "Remove Loan-form.pdf" and press Delete. Then Tab to "Apply" and press it.
- The page: two rows are listed as chosen. Delete takes the second out and focus returns to the control. "Apply" sends the form with the file that is left, and the page loads again with a toast.
- The reader says: the control is described by "PDF, images or CSV, up to 20 MB, up to 5 files" and then "Gurpreet Singh, Ludhiana branch.". After Delete: the status "Loan-form.pdf was removed" once, and then "Papers for the loan application". After "Apply": the new page and the toast "1 file sent." once, as a status.

### States

**17. A field with an error**
- Keys: both readers: go to the heading "States" and the control "With an error". Tab into it.
- The page: the field is required and marked invalid, with the message under it.
- The reader says: the name "With an error", required, invalid entry, and the description with the limits ("up to 10 MB") and "Choose a photo under 1 MB." The message is not spoken when the page loads.

**18. A disabled field**
- Keys: both readers: Tab from "With an error" onward.
- The page: the control "Disabled" cannot take focus.
- The reader says: Tab skips it. In browse mode "Disabled" is read as unavailable (dimmed).

**19. Names that are not text**
- Keys: both readers: go to the list under "Hostile names" and read it.
- The page: two rows. The first file is named `<img src=x onerror=alert(1)>.pdf` and is a link. The second is named in Gurmukhi (ਗੁਰਪ੍ਰੀਤ-ਸਿੰਘ-ਆਧਾਰ.pdf) with a size of 240 KB.
- The reader says: the first name as plain text, character by character as the reader chooses to speak it, as a link, with "1.5 KB"; no dialog opens and nothing runs. The second name, then "240 KB". If no Gurmukhi voice is installed, the reader may spell the name or skip it: that is not a failure.

### JavaScript off

**20. Choose and send, without script**
- Keys: turn JavaScript off and reload `/upload`. Both readers: go to "A photo", Tab to the control, choose `Simran-Kaur.png`, then Tab to "Send" and press it.
- The page: the button "Send" is shown (it is hidden when script runs). The form is sent and the page loads again at "A photo" with a toast.
- The reader says: the control "Profile photo" with its description, then the button "Send". After the press, the new page. Find the toast in browse mode (NVDA: D to move by landmark to the region "Notifications"; VoiceOver: the web rotor, Landmarks). It says "1 file sent." as a status. Write in the Notes whether the reader spoke it on its own.

**21. Send with no file, without script**
- Keys: both readers: press "Send" in "A photo" with nothing chosen.
- The page: the page loads again with a toast.
- The reader says: in the region "Notifications": "Choose a file to send."

**22. A refused file, without script**
- Keys: both readers: choose `photo.png` and press "Send".
- The page: the page loads again with a toast that gives the reason.
- The reader says: in the region "Notifications": "photo.png is not an accepted kind of file. Accepted: PNG or JPG." This toast is an alert; write in the Notes whether the reader spoke it on its own at page load.

**23. Remove a stored file, without script**
- Keys: both readers: in the "Documents" list press "Remove" for a stored file (for example "Remove Simran-Kaur.png"). Turn JavaScript on again afterwards.
- The page: the page loads again at "Documents" with a toast.
- The reader says: in the region "Notifications": "The file was removed." The file is gone from the list.

## Results

| | |
|---|---|
| Script | `upload.md` |
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
| 1 | "Profile photo", opens a chooser, described by "PNG or JPG, up to 1 MB" | | | |
| 2 | "Simran-Kaur.png · Sent" once within about 2 seconds | | | |
| 3 | List "Files", one item with name and "<size> · Sent" | | | |
| 4 | "photo.png is not an accepted kind of file. Accepted: PNG or JPG." once | | | |
| 5 | "big.png is larger than 1 MB." once | | | |
| 6 | "blank.png is empty." once | | | |
| 7 | Buttons named with the file: "Try again photo.png", "Remove photo.png" | | | |
| 8 | "Sending photo.png", then the refusal; focus not lost | | | |
| 9 | "photo.png was removed" once; focus on "Profile photo" | | | |
| 10 | "Sending rates.csv", "Cancel rates.csv", then "rates.csv was removed" once | | | |
| 11 | Each file's "Sent" line in order, none repeated | | | |
| 12 | Stored list changes silently; "Remove Salary-slip.pdf" after reload | | | |
| 13 | Toast "The file was removed." once | | | |
| 14 | Chosen row and "Remove rates.csv" reachable; "Send" button | | | |
| 15 | "rates.csv · Sent" once | | | |
| 16 | Delete removes a file: "Loan-form.pdf was removed"; toast "1 file sent." | | | |
| 17 | "With an error": required, invalid entry, error as description | | | |
| 18 | Disabled control skipped; read as unavailable | | | |
| 19 | Hostile name read as plain text and a link; nothing runs | | | |
| 20 | "Send" shown; toast "1 file sent." found in "Notifications" | | | |
| 21 | Toast "Choose a file to send." | | | |
| 22 | Toast with the refusal reason (alert) | | | |
| 23 | Toast "The file was removed." | | | |

Overall: see [results-template.md](results-template.md) for the sign-off checklist.
