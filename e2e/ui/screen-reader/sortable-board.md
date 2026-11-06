# Sortable list and board screen reader script

## Purpose

A sortable list lets a person reorder items, and a board lets them move cards between columns. Dragging with a pointer is out of reach for a reader user, so every card has a handle that is moved with the keys, and every step of a move is said in an assertive live region. This script settles what a reader says for the columns and cards, for each key of a move and the words it brings, for a column that refuses a card, for a move the server refuses or never hears, for a move by someone else, and for the form every card has when JavaScript is off.

## Page to open

The board demo app on a free port (see "Start the proof apps" in [README.md](README.md)). It keeps its data in a small file and needs no database server:

```sh
PORT=8098 BOARD_SIMULATOR=off BOARD_TEST_HOOKS=1 cargo run --release -p board-demo
```

Open `http://127.0.0.1:8098/`. The test routes let you do what a second person would do: `curl -X POST http://127.0.0.1:8098/__test/reset` puts the cards back, and `curl -X POST "http://127.0.0.1:8098/__test/move?item=7&to=review"` moves a card as someone else (add `&quiet=1` so no open page hears of it).

## Before you start

1. Reset the cards: `curl -X POST http://127.0.0.1:8098/__test/reset`. Use a window at least 1280 pixels wide, JavaScript on, except in part E.
2. Open the speech viewer (NVDA) or the caption panel (VoiceOver). See the README.
3. The arrow keys must reach the page while a card is lifted. NVDA: browse mode keeps them, so use focus mode (NVDA+Space) while you move a card. VoiceOver: keep Quick Nav off (press Left and Right arrow together to toggle it; with it off the arrow keys go to the page).
4. The first card in Backlog is "Offline order sheets for the dyeing unit" and the second is "Export a shipment as CSV". Backlog is a long column; call the number of cards in it N (the first phrase you hear says it).
5. The live region is assertive and read as a whole each time. A phrase may cut off the one before it when keys are pressed fast. Press one key and wait for the phrase before the next.

## Steps

### A. The board at rest

**1. Find the board and its five columns**
- Keys: NVDA: browse mode, press D (next landmark), or NVDA+F7 and Landmarks. VoiceOver: VO+U, choose Landmarks.
- The page: five columns side by side: Backlog, Ready, In progress, Review, Done.
- The reader says: a group named "Taana 3.2 board", and five regions named "Backlog", "Ready", "In progress", "Review" and "Done". Each column has a heading with its name. Each name is said once for each element.

**2. Hear a column's count**
- Keys: NVDA: browse mode, press H to the heading "In progress" and read on with Down arrow. VoiceOver: VO+U, Headings, "In progress", then VO+Right arrow.
- The page: In progress has a limit of 3 cards and starts full. The visible count reads 3/3.
- The reader says: after the heading, "3 of at most 3" (the page hides the short "3/3" from readers and gives the words instead). For Backlog or Ready, a bare number. The cards of a column are a list named by its heading, "In progress", with 3 items.

**3. Read one card**
- Keys: NVDA: browse mode, Down arrow through the first card of Backlog. VoiceOver: VO+Right arrow through it.
- The page: nothing changes.
- The reader says, for the card: a button "Move Offline order sheets for the dyeing unit" with the description "Press Space to pick it up, the arrow keys to move it, Space again to drop it, Escape to cancel."; then the card's labels, its title as a second button "Offline order sheets for the dyeing unit" with a dialog popup (it opens the card), the assignee's first name, the checklist count and the comment count where there are any. The card's title is said by two different controls, not twice by one. The reader's own list position counts the cards drawn (the first 20 in Backlog); the live phrases below count the whole column.

### B. Moving a card with the keys

**4. Reach the handle**
- Keys: both readers: Tab until the focus is on the button "Move Offline order sheets for the dyeing unit". NVDA: then press NVDA+Space to enter focus mode (write in Notes whether NVDA went there by itself).
- The page: the handle shows a focus mark. The card is not lifted yet.
- The reader says: "Move Offline order sheets for the dyeing unit", button, and the help text as its description, once ("Press Space to pick it up, the arrow keys to move it, Space again to drop it, Escape to cancel."). Where the reader leaves descriptions out, re-read the focus (NVDA+Tab; VoiceOver: VO+Left then VO+Right) and write in Notes what you did.

**5. Pick the card up and move it down**
- Keys: both readers: press Space, wait for the phrase, then Down arrow.
- The page: the card is lifted and, after Down arrow, sits one place lower in Backlog. Focus stays on its handle.
- The reader says: after Space, "Picked up Offline order sheets for the dyeing unit. Position 1 of N in Backlog." After Down arrow, "Position 2 of N in Backlog." Each once.

**6. Move to the next column and pass a full one**
- Keys: both readers: press Right arrow, wait, then Right arrow again.
- The page: the first Right arrow puts the card in Ready at the same place. The second skips In progress, which is full, and puts it in Review.
- The reader says: after the first Right arrow, "Position 2 of 5 in Ready." After the second, "In progress holds at most 3. Position 2 of 2 in Review." The reason for passing a column comes before the position, in one phrase.

**7. Go to the start and drop**
- Keys: both readers: press Home, wait, then Space.
- The page: the card is first in Review. Space drops it. Focus is still on the card's handle. The move is saved without a toast.
- The reader says: after Home, "Position 1 of 2 in Review." After Space, "Dropped Offline order sheets for the dyeing unit at position 1 of 2 in Review." Once. The focus is still read as "Move Offline order sheets for the dyeing unit", button. Reload the page: the card is first in Review.

**8. Cancel a move with Escape**
- Keys: both readers: Tab to the handle of "Export a shipment as CSV" (now first in Backlog). Press Enter, then Right arrow, then Escape.
- The page: Enter lifts the card, Right arrow moves it to Ready, Escape puts it back in Backlog at the same place.
- The reader says: after Enter, "Picked up Export a shipment as CSV. Position 1 of " followed by Backlog's count and " in Backlog." After Right arrow, "Position 1 of 5 in Ready." After Escape, "Move cancelled. Export a shipment as CSV is back at position 1 of " followed by Backlog's count and " in Backlog."

**9. Tab away cancels too**
- Keys: both readers: press Space, then Down arrow, then Tab.
- The page: the card is lifted and moved one place, then Tab puts it back where it began and moves focus on to the next control.
- The reader says: the same cancelled phrase as in step 8, said again, once. Nothing is saved: reload and the card is first in Backlog.

**10. Pick up and drop with two presses**
- Keys: both readers: with the cursor on the handle in browse mode (NVDA: leave focus mode with NVDA+Space; VoiceOver: VO+Right arrow onto the button), press Enter (VoiceOver: VO+Space). Press it again without moving anything.
- The page: the first press lifts the card, the second drops it where it is. This is the way to start and finish a move when the arrow keys are not available.
- The reader says: after the first press "Picked up Export a shipment as CSV. Position 1 of " and Backlog's count " in Backlog." After the second, "Dropped Export a shipment as CSV at position 1 of " and the count " in Backlog."

### C. Refusals and failures

**11. A column that refuses a card**
- Keys: both readers: Tab to the handle of the only card in Review. NVDA: focus mode. Press Space, then Right arrow, then Escape.
- The page: the board is shown as a member. A member may not move cards to Done. The card stays in Review.
- The reader says: after Space, the "Picked up" phrase with "in Review". After Right arrow, "Only a lead can move cards to Done." After Escape, "Move cancelled." followed by the card's name, "is back at position 1 of 1 in Review."

**12. Change who you view as**
- Keys: both readers: Tab to the button "Viewing as Gurpreet Singh" and press Enter (VoiceOver: VO+Space). Choose "Harleen Kaur (lead)".
- The page: a menu opens with the team. Choosing the lead posts and the board is drawn again.
- The reader says: the button with a menu popup, collapsed; the menu items by name, with "(lead)" on the lead; after the choice a toast, once: "Viewing as Harleen Kaur, a lead." Repeat step 11: this time Right arrow says a position in Done, not a refusal. Escape to cancel.

**13. A move the server refuses**
- Keys: both readers: reset the cards. In a terminal, run `curl -X POST "http://127.0.0.1:8098/__test/move?item=7&to=backlog&quiet=1"` (the page does not hear of it). Then Tab to the handle of "Retry uploads on a weak signal" in Ready; press Space, Down arrow, Space.
- The page: the card is lifted, moved and dropped; the server refuses it because the card was moved already. A toast says why, the card goes back, and the board is drawn again without it in Ready.
- The reader says: the toast once: "Someone else moved it first. It is in Backlog now." Then, once the board is drawn again, "Retry uploads on a weak signal is no longer shown here." (the card is further down Backlog than the page draws). The earlier phrases ("Position", "Dropped") may be cut off by the later ones. Focus lands on the handle of the card that took its place in Ready, which the reader says by its "Move ..." name.

**14. A move while offline**
- Keys: both readers: reset the cards and reload. Turn the network off (Chrome and Firefox: developer tools, Network, Offline; Safari: stop the app with Ctrl+C). Tab to the handle of the first card in Ready, press Space, Down arrow, Space. Turn the network on (or start the app) afterwards.
- The page: the move cannot be sent and the board cannot be fetched. The card goes back where the server last had it.
- The reader says: a toast, once: "No connection. That move may not have been saved." and the phrase "wasn't moved. It is back at position 1 of 4 in Ready." with the card's name before it.

### D. Other changes to the board

**15. Another person moves a card**
- Keys: both readers: reset the cards, reload, and put the cursor on the first card of Review. In a terminal run `curl -X POST "http://127.0.0.1:8098/__test/move?item=7&to=review"`. Wait 3 seconds.
- The page: the board is drawn again from the server and card 7 ("Retry uploads on a weak signal") slides into Review. A status line near the top of the board reads "Live: moves by your team show up here".
- The reader says: nothing is announced for the other person's move: the board has no live phrase for it. Pass if the reader stays quiet and your reading place does not jump; write in Notes if it says anything. Read Review again: the card is there and the counts changed.

**16. Read down a long column**
- Keys: NVDA: browse mode, press I (next list item) until you are past the 20th card of Backlog. VoiceOver: VO+Right arrow repeatedly, or Tab (two stops for each card).
- The page: when the end of the column comes into sight, its "Load more" button is pressed by the page and the next 20 cards are added at the end.
- The reader says: nothing is announced for the cards that were added (the column has no live phrase for it), and you can read on into the new cards. The button "Load more" is a real button the reader can reach and press. The list's count in the reader's own words (for example "1 of 20") grows to 40.

**17. Open a card and close it**
- Keys: both readers: Tab to a card's title button (the second button of the card) and press Enter. Read the dialog. Press Escape.
- The page: a drawer opens on the right with the card, the board still showing beside it. Escape closes it.
- The reader says: a dialog named with the card's title. After Escape, focus returns to the title button that opened it, and the reader says that card's title and its dialog popup.

### E. JavaScript off

Turn JavaScript off, run the reset command, and open the board again.

**18. The move form on every card**
- Keys: both readers: Tab through the first card. There is no handle. Tab to the button "Move" (a disclosure), press Enter, then Tab to the list "To", choose "Ready" (NVDA: Alt+Down arrow, Down arrow, Enter; VoiceOver: VO+Space, Down arrow, Enter), Tab to "Position", type `2`, Tab to the button "Move" and press Enter.
- The page: the "Move" disclosure opens a small form: a list "To", a number "Position" starting at the card's place, and a button "Move". The handle is not shown and cannot be reached. Posting loads the board again with the card in Ready at place 2.
- The reader says: "Move", button, collapsed (or its own word for a closed disclosure), and expanded once it is open. The form fields are named "To" and "Position", with the current place as the number's value; in the list "To", the column "In progress (full)" is unavailable, and when the board is shown as a member (before step 12) "Done" is unavailable too. After the post, the reader announces the new page, and the card is found in "Ready".

**19. Loading more with no script**
- Keys: both readers: browse mode, reach the end of the Backlog cards and find the button "Load more" (NVDA: B; VoiceOver: VO+U, Buttons). Press Enter.
- The page: the board loads again with 20 more cards in Backlog; the address says how many. Nothing is added by itself.
- The reader says: "Load more", button. After the load, the reader announces the new page and the list of Backlog has 40 cards. Turn JavaScript on again afterwards.

## Results

| | |
|---|---|
| Script | sortable-board.md |
| Tester | |
| Date | |
| Operating system and version | |
| Browser and version | |
| Screen reader and version | |
| Speech settings changed from the defaults | none |
| Build under test (commit) | |
| Proof app and port | board demo, 8098 |
| Steps run | |
| Steps failed | |
| Issues opened | |

| Step | Expected | NVDA heard / pass | VoiceOver heard / pass | Notes |
|---|---|---|---|---|
| 1 | Group "Taana 3.2 board"; five named column regions | | | |
| 2 | "3 of at most 3" for In progress; list with 3 items | | | |
| 3 | Handle button named "Move <card>" with help text; title button separate | | | |
| 4 | Handle focused: name and description, once | | | |
| 5 | "Picked up ... Position 1 of N in Backlog." then "Position 2 of N" | | | |
| 6 | "Position 2 of 5 in Ready."; then full column passed with reason | | | |
| 7 | "Position 1 of 2 in Review."; "Dropped ... at position 1 of 2 in Review." | | | |
| 8 | Escape: "Move cancelled. ... is back at position 1 of N in Backlog." | | | |
| 9 | Tab away says the cancelled phrase again, once | | | |
| 10 | Two presses: "Picked up ..." then "Dropped ..." | | | |
| 11 | "Only a lead can move cards to Done."; then "Move cancelled." | | | |
| 12 | Viewer menu; toast "Viewing as Harleen Kaur, a lead." once | | | |
| 13 | Toast "Someone else moved it first. It is in Backlog now."; "no longer shown here" | | | |
| 14 | Toast "No connection. That move may not have been saved."; "wasn't moved" phrase | | | |
| 15 | Nothing announced for another person's move; place kept | | | |
| 16 | Nothing announced when cards load; "Load more" reachable | | | |
| 17 | Dialog named by the card; focus returns to its title button | | | |
| 18 | Move disclosure, "To" and "Position" fields; full column unavailable | | | |
| 19 | "Load more" button works with no script | | | |

Overall result:

- [ ] Every step passed in NVDA with Firefox.
- [ ] Every step passed in NVDA with Chrome.
- [ ] Every step passed in VoiceOver with Safari.
- [ ] Every failed step has an issue, with the module name and the step number.
- [ ] Every n/a has a reason in the notes.

Signed off by: ______________________  Date: ______________

Reviewed by: ______________________  Date: ______________
