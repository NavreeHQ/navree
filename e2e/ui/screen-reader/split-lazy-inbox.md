# Split panes, lazy lists and the inbox screen reader script

## Purpose

Split panes are regions with a separator between them that a person moves with the arrow keys. A lazy list loads more of itself as you read down it, and says so in a polite live region. The support inbox puts both together: a list of conversations, a thread of messages that grows as customers write, and a customer pane. This script settles what a reader says for the separators and their values, for pages that load more rows or messages, for new messages that arrive while you read (said once, in one phrase), for errors, for the single-key shortcuts, and for the stacked panes and the page with JavaScript off. Two short steps at the end check a lazy region and a polling region on the proof app's overview page.

## Page to open

The inbox demo app on its own port. Start it with a small desk, nothing happening by itself, and the test routes on (see "Start the proof apps" in [README.md](README.md)):

```sh
INBOX_SCALE=2000 INBOX_SIMULATOR=off INBOX_TICK=off INBOX_TEST_HOOKS=1 cargo run --release -p inbox-demo
```

Open `http://127.0.0.1:8096/inbox`. The test routes let you make a customer write while you listen; they are `curl -X POST "http://127.0.0.1:8096/__test/step?what=customer&on=<id>"` (a customer writes again in conversation `<id>`) and `...?what=new` (a new conversation starts). The id is the number at the end of the address when a conversation is open (`/inbox/<id>`).

Steps 25 and 26 use the UI proof app instead, at `http://127.0.0.1:8090/` (`cargo run --release -p ui-proof -- serve`).

## Before you start

1. Reset the desk: `curl -X POST http://127.0.0.1:8096/__test/reset`. The app has no sign-in; you work as one of the team.
2. Use a window at least 1440 pixels wide, JavaScript on, except in part G.
3. Open the speech viewer (NVDA) or the caption panel (VoiceOver). See the README.
4. Pick a conversation to work with: open `/inbox` and open the first row that has no "Pending" or "Closed" badge. Note its id from the address. Call it the open conversation below.
5. NVDA: start in browse mode. VoiceOver: start with VO on the page, not interacting.

## Steps

### A. The panes and their separators

**1. Find the three panes**
- Keys: both readers: open `/inbox/<id>` for the open conversation. NVDA: press D (next landmark) until you have passed the three panes, or NVDA+F7 and choose Landmarks. VoiceOver: VO+U, choose Landmarks.
- The page: three panes side by side: the conversations, the open conversation, and the customer.
- The reader says: three regions named "Conversations", "Conversation" and "Customer" (the reader's word for a named section, such as "region"). Each pane is a region once.

**2. Find a separator and hear its value**
- Keys: both readers: the separator comes in the tab order between the list pane and the conversation pane. Move the cursor to the region "Conversation" (as in step 1), press Tab once to land on its first control, then Shift+Tab once. (If Tab does not start from where the cursor is, Tab on from the link "Show more conversations" at the end of the list: the separator is the next stop after it.) If the arrow keys later move NVDA's reading cursor instead of the separator, press NVDA+Space for focus mode and write that in Notes.
- The page: the separator has a visible focus mark.
- The reader says: "Conversations", separator, with the value 30 (the pane's width as a share of the window). Where the reader reports it, a least and a most are given (22 and 44). It is an upright separator (vertical).

**3. Move the separator with the keys**
- Keys: both readers: with focus on the separator press Right arrow, then Shift+Right arrow, then End, then Home, then Left arrow.
- The page: the list pane widens by 2 points, then by 10 points, then to its widest, then to its narrowest; Left at the narrowest changes nothing.
- The reader says the new value after each key, once: 32, then 42, then 44, then 22, and for the last Left arrow either the same value 22 again or nothing. After about a second the sizes are saved with no sound; reload the page (F5) and the separator says 22.

**4. Collapse and restore the customer pane**
- Keys: both readers: reach the second separator the same way, from the region "Customer": Tab once to its first control, then Shift+Tab once. Press Left arrow once, then Enter, then Enter again.
- The page: Left grows the customer pane (26 to 28). Enter collapses it: the pane disappears and the others fill the window. Enter again brings it back at 28.
- The reader says: "Customer", separator, 26; then 28. After the first Enter the value is spoken as "Collapsed" (the page gives that word as the value's text), not as 0. After the second Enter the value is 28 again. While collapsed, the "Customer" region is gone from the landmarks list.

**5. Use the Customer link**
- Keys: both readers: Tab to the link "Customer" among the controls over the conversation (after "Snooze" and the close button), then Enter, then Enter again.
- The page: Enter shows the customer pane when it is hidden and hides it when it is shown, with no page load. Focus stays on the link.
- The reader says: "Customer", link, and its state: expanded when the pane is shown and collapsed when it is hidden, changing with each Enter. The state is spoken after the change, or when you re-read the focus (NVDA+Tab; VoiceOver: VO+Left then VO+Right).

### B. The lazy list of conversations

**6. Open a conversation from the list**
- Keys: both readers: Tab into the list of conversations and press Enter on the link of a row that is not the open one.
- The page: the conversation opens in the middle pane without a full page load. The address becomes `/inbox/<that id>`. Focus moves to the main area of the page. The list keeps its rows and its place, and the link of the open row is marked as the current page.
- The reader says: the new page's title, once (it is spoken by a polite live region and may follow the focus phrase). When you return to the list, the open row's link is spoken as "current page". The rows are not read from the top again.

**7. Read the list and one row**
- Keys: NVDA: browse mode, press L (next list) to reach the list named "Conversations", then Down arrow through two rows. VoiceOver: VO+U, Landmarks or Lists, choose the list, then VO+Right arrow through two rows.
- The page: nothing changes.
- The reader says: a list named "Conversations" with 50 items on the first page. Each row is a link whose name has the customer's name, the subject, how long ago it moved, and the badges (for example "Unassigned" or a name; "Urgent"; "3 new"). Above the list, a line such as "2,000 conversations" is read as ordinary text.

**8. Load the next fifty by reading down**
- Keys: NVDA: browse mode, press I (next list item) or Down arrow repeatedly until you are past the last of the 50 rows. VoiceOver: VO+Right arrow repeatedly.
- The page: before you reach the last row the next 50 rows are added at the end. No row appears twice.
- The reader says: "50 more conversations", once, from a polite live region. Your place is kept and you can keep reading on into the new rows. The list is not read again from the start, and the same phrase is not said twice.

**9. Use the link at the end**
- Keys: both readers: after step 8, Tab to the link "Show more conversations" (it is the last link after the rows) and press Enter.
- The page: the next rows are added in place. The link opens a page of its own only with JavaScript off.
- The reader says: the link by its name; after Enter, the live phrase once again, for example "50 more conversations". Focus stays on the link.

**10. A page that does not come**
- Keys: both readers: turn the network off (Chrome and Firefox: open the developer tools, Network tab, choose Offline; Safari: stop the app in its terminal with Ctrl+C). Press Enter on "Show more conversations" again. Turn the network back on (or start the app again) afterwards.
- The page: nothing is added.
- The reader says: "That didn't load. Try again", once, from the same live region. The link stays reachable so you can try again.

### C. New messages arriving while you read

Back online, open the open conversation (`/inbox/<id>`) and put the cursor in the thread.

**11. One new message**
- Keys: both readers: with the cursor in the thread, run in a terminal `curl -X POST "http://127.0.0.1:8096/__test/step?what=customer&on=<id>"`.
- The page: a message from the customer is added at the end of the thread. The list pane moves that conversation up.
- The reader says: "New message from " followed by the customer's name, once, within about two seconds of the command (the page waits a second to see whether more are coming). Your reading position does not move.

**12. Three messages at once**
- Keys: both readers: run the command from step 11 three times in a row on one line.
- The page: three messages are added.
- The reader says: one phrase, "3 new messages", once. It does not say "New message from" three times.

**13. Your own reply is not announced**
- Keys: both readers: move to the field named "Message" (NVDA: E or F in browse mode until you reach it; VoiceOver: VO+U, Form Controls, "Message"), type `On it.`, press Tab twice (past "Saved replies") to the button "Send" and press Enter.
- The page: your message is added to the thread and the field empties. A toast appears with the title "Reply sent" and the text "Your reply is on its way to " followed by the customer's name.
- The reader says: the toast, once: "Reply sent", then the text (a status, not an error alert). The thread's live region says nothing about your own message, so "A new message" or "New message from" is not spoken.

**14. A new conversation arrives**
- Keys: both readers: run `curl -X POST "http://127.0.0.1:8096/__test/step?what=new"`.
- The page: a conversation is added at the top of the list and the line over the list changes (the count goes up).
- The reader says: the whole line over the list as one phrase, once (for example the count of conversations, then "new" and "overdue" counts where shown). The list itself is silent: no phrase is spoken for the new row, because the code gives that list no announcement. Write in Notes if the reader says anything else.

### D. The thread and the composer

**15. Read the messages**
- Keys: NVDA: browse mode, press L to reach the list named "Messages", then Down arrow through three messages. VoiceOver: VO+U, Lists, "Messages", then VO+Right arrow.
- The page: nothing changes.
- The reader says: a list named "Messages". Each message is an article named "Message from " followed by the sender, or "Internal note by " followed by the sender for a note. Each reads its sender, its text and its time. Attachments, where there are any, are read as links with their file names.

**16. Load earlier messages**
- Keys: both readers: open `/inbox/1008?view=closed` (a conversation of 100 messages). Move up to the first message shown (NVDA: browse mode, Up arrow repeatedly; VoiceOver: VO+Left arrow repeatedly). Stay there a second.
- The page: the 30 newest messages show first. The link "Show earlier messages" is at the top. As it nears view the older messages are added above, and your place is kept.
- The reader says: after the load, "30 more messages", once. The message you were on is still the one under the cursor, and the reader does not jump to the top of the thread. The link "Show earlier messages" is also reachable with Tab.

**17. An empty reply**
- Keys: both readers: back on the open conversation, move to the field "Message" as in step 13, leave it empty, press Tab twice to "Send" and press Enter.
- The page: the text "Write a message first." appears under the field, the field is marked invalid, and focus moves to the field.
- The reader says: "1 field needs attention", once (a polite announcement). On the field: "Message", an edit box for several lines, invalid, with the description "Write a message first." Re-read the focus (NVDA+Tab; VoiceOver: VO+Left then VO+Right) to hear the field alone. The error text must be on the field, and the announcement must be said once.

**18. Send an internal note**
- Keys: both readers: choose "Internal note" in the group "Send as" (the group is just before the field "Message"), move to "Message", type a line, press Tab twice to "Send" and press Enter.
- The page: the note is added to the thread, drawn as a note. A toast appears with the title "Note added" and the text "Only the team can see it."
- The reader says: the group "Send as" with two options, "Reply" and "Internal note", and which is chosen ("Internal note" selected or pressed after your choice). After sending, the toast once, then the new item in the thread as "Internal note by " and your name.

**19. Change the status from its menu**
- Keys: both readers: Tab to the button "Open status, change it", press Enter (VoiceOver: VO+Space), press Down arrow to "Mark closed" and Enter.
- The page: the menu opens with the items "Mark pending" and "Mark closed" (the current status is not offered). Choosing one posts it. A toast appears with the title "Closed" and the subject in quotes followed by "is closed." The status button is drawn again and now reads "Closed status, change it".
- The reader says: the button as "Open status, change it", with a menu popup, collapsed; a menu named like its button, with "Mark pending" and "Mark closed" as menu items; after the choice the toast once, and the button's new name when you reach it. Write in Notes where focus is after the choice (NVDA+Tab). The priority menu next to it shows its current priority as an unavailable item.

### E. Single-key shortcuts

**20. Open the list of keys**
- Keys: both readers: Tab to the button "Keyboard shortcuts" (an icon button beside "Actions" at the top of the page), press Enter. Read the dialog. Then press Escape.
- The page: a modal dialog opens, with a table of eight rows: j, k, r, e, /, Ctrl K, ?, Esc and what each does. The page behind cannot be reached. Escape closes it and focus returns to the button.
- The reader says: a dialog named "Keyboard shortcuts", described by "Single keys work while you aren't typing in a field; e works only inside the open conversation. Turn them off if you use speech input or press keys by accident." Then a table named "Keyboard shortcuts" with the column headers "Key" and "Does", the key cells read as keys ("Ctrl" and "K"), and the button "Turn single keys off". After Escape: "Keyboard shortcuts", button.

**21. Turn single keys off**
- Keys: both readers: open the dialog again, Tab to "Turn single keys off" and press Enter. Then, with the cursor on the page and not in a field, press `j`. Then press Ctrl+K and then Escape.
- The page: the page is drawn again with no single-key shortcuts. A toast appears: "Single-key shortcuts are off. Ctrl+K still opens the command palette." Pressing `j` now does nothing. Ctrl+K opens the command palette.
- The reader says: the toast text, once, as a status. After `j`, nothing is spoken and the page does not change (in NVDA browse mode `j` is not a page key anyway; try it in focus mode with NVDA+Space as well). Ctrl+K: a dialog named "Command palette". Turn the keys on again afterwards with the same dialog ("Turn single keys on"; the toast reads "Single-key shortcuts are on.").

### F. The panes stacked

**22. One pane at a time on a narrow window**
- Keys: both readers: make the window 390 pixels wide (or zoom the page to 400 percent on a 1440 pixel screen). Open `/inbox`. Open a row. Tab to the link "Customer", press Enter. Then Tab to the link "Conversation", press Enter. Then the link "Conversations", press Enter.
- The page: only one pane is shown at a time. There are no separators. The link "Customer" goes to a page that shows the customer pane. "Conversation" and "Conversations" go back.
- The reader says: only the pane in view is a region ("Conversations", then "Conversation", then "Customer"). The separators are not reachable. Each link is named as above, and each move is a page change that says the new page's title once. The link "Customer" may be spoken as collapsed (it is a link here, and the code still sets its state); write in Notes what the reader says.

### G. JavaScript off

Turn JavaScript off in the browser, make the window wide again, run `curl -X POST http://127.0.0.1:8096/__test/reset`, and open `/inbox`.

**23. Panes are lines, and lists page by links**
- Keys: both readers: Tab through the first screen. Tab to "Show more conversations" and press Enter. Then open `/inbox/1008?view=closed`, find the link "Show earlier messages" and press Enter.
- The page: the separators are plain lines with no tab stop. The three panes keep their sizes. "Show more conversations" loads a new page with the next 50 rows and a link "Show newer conversations" at the top. "Show earlier messages" loads a page with 30 earlier messages.
- The reader says: the panes are still regions "Conversations", "Conversation" and "Customer". No separator is reached by Tab or listed as a control. After each link, the reader announces the new page in its own way (title, then where it starts), and the list is named "Conversations" or "Messages" again.

**24. Reply and mark as read with no script**
- Keys: both readers: open a conversation that has a badge "new" in the list. Find the button "Mark as read" and press Enter. Then Tab to the field "Message", type `Answered with no script.`, and press the button "Send".
- The page: "Mark as read" is shown only without script and reloads the page with the badge gone. "Send" posts the form and the page loads again with the reply in the thread and the text "Your reply is on its way to " and the customer's name in the Notifications area.
- The reader says: "Mark as read", button. After the page loads, the reply is found in the list "Messages" and the toast text is found in the region named "Notifications" with browse mode. Turn JavaScript on again afterwards.

### H. Lazy and polling regions on the overview page

Use the UI proof app at `http://127.0.0.1:8090/`, JavaScript on.

**25. A region that loads after the page**
- Keys: both readers: in browse mode (VoiceOver: VO+Right arrow) read down the page to the card titled "Revenue by month" and the item after it. NVDA: G jumps to the next graphic.
- The page: the card first holds a placeholder marked busy, and then the chart is drawn in it.
- The reader says: nothing is announced as the chart loads. When you read the card after it loaded, the chart is an image named "Revenue by month, rising from 12 to 61".

**26. A region that refreshes every five seconds**
- Keys: both readers: move the cursor to the first line of the card "Activity" ("Anu moved Agra renewal to Proposal"). Do not press any key for 30 seconds.
- The page: the region fetches the same two lines every five seconds. The text does not change.
- The reader says: nothing. The lines are not read again, the cursor stays on the first line, and no live phrase is spoken.

## Results

| | |
|---|---|
| Script | split-lazy-inbox.md |
| Tester | |
| Date | |
| Operating system and version | |
| Browser and version | |
| Screen reader and version | |
| Speech settings changed from the defaults | none |
| Build under test (commit) | |
| Proof app and port | inbox demo 8096 (steps 25 and 26: UI proof 8090) |
| Steps run | |
| Steps failed | |
| Issues opened | |

| Step | Expected | NVDA heard / pass | VoiceOver heard / pass | Notes |
|---|---|---|---|---|
| 1 | Three regions: Conversations, Conversation, Customer | | | |
| 2 | "Conversations" separator, vertical, value 30 | | | |
| 3 | Values 32, 42, 44, 22 after each key; saved silently | | | |
| 4 | Customer separator 26, 28, "Collapsed", back to 28 | | | |
| 5 | "Customer" button expanded or collapsed after each Enter | | | |
| 6 | New page title said once; open row is "current page" | | | |
| 7 | List "Conversations", 50 items; row link names customer, subject, badges | | | |
| 8 | "50 more conversations" once; place kept | | | |
| 9 | "Show more conversations" link; phrase once after Enter | | | |
| 10 | "That didn't load. Try again" once | | | |
| 11 | "New message from <name>" once, within about two seconds | | | |
| 12 | "3 new messages" once, not three phrases | | | |
| 13 | Toast "Reply sent" once; no thread announcement for own reply | | | |
| 14 | Count line read whole, once; list silent | | | |
| 15 | List "Messages"; articles "Message from" or "Internal note by" | | | |
| 16 | "30 more messages" once; reading place kept | | | |
| 17 | "1 field needs attention" once; field invalid with error | | | |
| 18 | "Send as" group; toast "Note added"; note article | | | |
| 19 | Status menu items; toast "Closed" once; new button name | | | |
| 20 | Dialog "Keyboard shortcuts" with description and table | | | |
| 21 | Toast "Single-key shortcuts are off..." once; j does nothing | | | |
| 22 | One pane at a time; no separators; links named | | | |
| 23 | No separator stops; paging links; regions kept | | | |
| 24 | "Mark as read" button; reply toast text found | | | |
| 25 | Nothing said on load; image "Revenue by month, rising from 12 to 61" | | | |
| 26 | Silence while the region refreshes unchanged | | | |

Overall result:

- [ ] Every step passed in NVDA with Firefox.
- [ ] Every step passed in NVDA with Chrome.
- [ ] Every step passed in VoiceOver with Safari.
- [ ] Every failed step has an issue, with the module name and the step number.
- [ ] Every n/a has a reason in the notes.

Signed off by: ______________________  Date: ______________

Reviewed by: ______________________  Date: ______________
