# Timeline, activity feed and live updates screen reader script

## Purpose

The timeline, the activity feed, the fact cards and the outcome chips show what happened and when. Most of them are plain pages. The live feed is the part that speaks: it adds new rows by itself over a server-sent event stream and says how many came, it shows whether it is connected, and it keeps a notice of rows left out. This script settles what a reader says for the lists, times, chips and cards, and, above all, that a live update is announced once, in one sentence, a moment after the rows stop coming, and that a dropped or paused connection is said without noise.

## Page to open

The UI proof app, see [Start the proof apps](README.md#start-the-proof-apps) (http://127.0.0.1:8090).

- `/display`: badges, outcome chips, fact cards, timelines and activity feeds (steps 1 to 9).
- `/display?case=empty`, `/display?case=loading`, `/display?case=failed`, `/display?case=hostile`: single states (steps 10 to 13).
- `/display/live`: a live feed and a live table on one stream (steps 14 to 22 and step 24).
- `/display/people/0`: a recipient's page (step 23).

## Before you start

- The live steps need a way to add rows from outside the page. In a second window (not the browser) run these with `curl` (on Windows use `curl.exe`). They are the hooks the proof app offers for this and take no sign-in:
  - Add rows: `curl -X POST "http://127.0.0.1:8090/display/test/emit?n=3"` (n is how many).
  - Add rows that are left out of the feed (a mail scanner's visit): `curl -X POST "http://127.0.0.1:8090/display/test/emit?n=1&kind=scanner"`.
  - Cut the open connections: `curl -X POST "http://127.0.0.1:8090/display/test/drop"`.
  - Make the next connections be refused (n times): `curl -X POST "http://127.0.0.1:8090/display/test/refuse?n=2"`.
  - Start over with the first 14 events: `curl -X POST "http://127.0.0.1:8090/display/test/reset"`.
- Run the reset command before step 1 and again before steps 14, 20, 21 and 23. Reload the page after a reset.
- JavaScript on, except in step 24.
- NVDA: browse mode throughout. VoiceOver: VO+Right arrow to read, Tab to move between controls.
- Keep the speech viewer (NVDA) or the caption panel (VoiceOver) open. The live steps depend on timing: write down how long after the action each message came.
- All times on these pages are in India (Asia/Kolkata), for example "09:41:08". Words in quotation marks are the toolkit's own.

## Steps

### Badges, chips and fact cards

**1. Badges**
- Keys: open `/display`. Both readers: move to the heading "Badges" and read the first row. NVDA: H, then Down arrow. VoiceOver: VO+Right arrow.
- The page: ten badges: "Queued", "Delivered", "Opened", "Delayed", "Bounced", "Replied", "Forwarded", "Complained", "Won" and "Draft". Some have a coloured dot or an icon.
- The reader says: only the words, in that order. The dots and icons are not spoken. The meaning does not depend on the colour: "Bounced" is read as the word "Bounced".

**2. Outcome chips**
- Keys: both readers: read the second row of the same section.
- The page: five chips: "Delivered", "Opened", "Clicked", "Replied" and "Unsubscribed". The first three are reached; the last two are missed (drawn with a dash).
- The reader says: the name and then the word "yes" or "no" as two separate words: "Delivered yes", "Opened yes", "Clicked yes", "Replied no", "Unsubscribed no". It must not run them into one word ("Deliveredyes"). The check and dash icons are not spoken.

**3. Fact cards**
- Keys: both readers: move to the heading "Fact cards" and read the five cards, then the single card under them.
- The page: cards with a label, a value, and lines of detail: "Outcome", "Clicked", "After 5 min 11 s"; "Device", "MacBook Air", "Safari on macOS", "1440 x 900"; "Place", "Mumbai, India", "Jio Fiber"; "Speed", "5 min 11 s", "Faster than 72% of recipients"; "Consent", "Marketing emails", "Given 12 Sep 2026".
- The reader says: for each card the label, then the value, then each detail line, in that order. The card's icon is not spoken. The reader may announce the card as a term and its descriptions; it must convey which words are the label and which are the value.

### The timeline

**4. Find a timeline**
- Keys: both readers: move to the heading "Timeline" and then to the timeline "Autumn launch email to Maya Patel".
- The page: a group named after the title; the line "Times in Asia/Kolkata (GMT+05:30)"; a day heading "Monday, November 2, 2026"; the events; and a closing note "Nothing else happened."
- The reader says: a group named "Autumn launch email to Maya Patel"; the zone line; the day as a heading (level 3); then a list of events. The note is read after the last event.

**5. Read the events**
- Keys: both readers: move through the six events.
- The page: each event shows its time, its title, a chip with the wait since the step before (for example "+1 min 34 s"), and its facts (the device and the place).
- The reader says: the list as "list with 6 items" (or the reader's words for six list items). Each item in this order: the time ("09:41:08"), the title ("Delivered", "Opened the email", "Clicked the pricing link", "Replied"), the wait chip, then the facts ("Safari on macOS", "Mumbai"). The coloured dot and the line joining the events are not spoken. The last event is "09:50:47", "Replied".

**6. Rows left out, and the notice**
- Keys: both readers: move to the timeline titled "Dev Kapoor" (just below). Tab to the check box in its notice.
- The page: the notice reads "1 item was hidden." and has a check box. Five events are shown and one is left out. Pressing Space shows the sixth event, in its place in time.
- The reader says: the check box "Show them", not checked, described by "1 item was hidden." After Space: checked; the list now has six items and the extra event, "Checked by a mail scanner", is read between the events around it. The check box's name is now "Hide them" (said when you move off and back).

**7. Dismiss the notice**
- Keys: both readers: Tab to the button "Dismiss" and press it (NVDA: Enter or Space; VoiceOver: VO+Space). Then press Tab.
- The page: the notice goes and the revealed event stays.
- The reader says: the button "Dismiss" before the press. After it, the reader does not report the document or the page as having focus, and Tab goes to the next control after the notice. If the reader goes quiet or reports the document, mark the step failed and write what it said: the button that had focus has gone.

### The activity feed

**8. Find the feed and read a row**
- Keys: both readers: move to the heading "Activity feed" and the feed "Recent activity" under it.
- The page: a group with the title as its heading, the summary "14 events", the line "Times in Asia/Kolkata (GMT+05:30)", a select "Period", the notice "1 item was hidden.", and the list of rows, newest first.
- The reader says: the heading "Recent activity" (level 3); "14 events"; the zone line; then the select named "Period" with the value "Since launch". The first row reads, in order: the person as a link ("Tejás García"), the verb ("replied to"), the object ("the Autumn launch email"), the tag ("Autumn launch"), the device and place ("Chrome on Android", "Berlin"), and the time ("09:50:47"). The row's round icon is not spoken.

**9. Keyboard stops in the feed**
- Keys: both readers: Tab from the heading through the feed.
- The page: the stops are the "Period" select, the check box in the notice, the button "Dismiss", the scrolling list, and then the link of each row.
- The reader says: "Period", a select; "Show them", a check box, described by "1 item was hidden."; "Dismiss", a button; then the list itself, named "Recent activity", as a list of 13 items (14 events less the one left out). The Apply button next to the select is not a stop when script runs.

### States

**10. Empty**
- Keys: open `/display?case=empty`. Both readers: read the page.
- The page: a timeline "Nothing" and a feed "Recent activity", both with no rows.
- The reader says: the timeline group "Nothing" with the text "No events yet"; the feed "Recent activity" with "Nothing yet". No live state is shown.

**11. Loading**
- Keys: open `/display?case=loading`. Both readers: read the page.
- The page: the timeline "Loading" and the feed "Recent activity" show grey placeholders.
- The reader says: each group is busy and carries the word "Loading" (a status). The grey placeholders are not spoken. Write in the Notes whether the reader spoke "Loading" on its own when the page opened.

**12. Failed**
- Keys: open `/display?case=failed`. Both readers: read the page.
- The page: the feed shows an alert and a link "Try again".
- The reader says: an alert with the title "The activity could not load." and then the link "Try again". Write in the Notes whether the alert was spoken on its own when the page opened.

**13. Hostile text**
- Keys: open `/display?case=hostile`. Both readers: read the page.
- The page: titles and values are made of tags, quotes and script text.
- The reader says: the characters as words (the reader may spell or skip the symbols). No dialog opens, and the text is never read as a button, a link or an image of its own.

### The live feed

**14. The state of the connection**
- Keys: run the reset command and open `/display/live`. Both readers: read the heading "Recent activity" at the top of the feed.
- The page: next to the title a small status shows "Connecting" for a moment and then "Live".
- The reader says: the status "Live" (it may also say "Connecting" first). It must not say "Reconnecting" or "Not live". The fact cards on the page read "Events 14", "Recipients 8" and "Clicked 1".

**15. One new row**
- Keys: both readers: Tab to the button "Simulate activity" and press it (NVDA: Enter or Space; VoiceOver: VO+Space).
- The page: a toast appears. A moment later one row appears at the top of the feed. Focus stays on the button.
- The reader says: first, at once, the toast "Activity added", once. Then, between one and two seconds after the row arrives, one message, "1 new item", once. Nothing is said for the table beside it. Focus stays on "Simulate activity".

**16. A burst of rows**
- Keys: in the other window run `curl -X POST "http://127.0.0.1:8090/display/test/emit?n=3"`. Do nothing in the browser.
- The page: three rows arrive at the top of the feed, newest first, none twice.
- The reader says: one message, "3 new items", said once about a second after the rows arrive (and at most five seconds after the first one when rows keep coming). It is not "1 new item" three times.

**17. No repeat**
- Keys: run `curl -X POST "http://127.0.0.1:8090/display/test/emit?n=1"`.
- The page: one more row.
- The reader says: "1 new item", once. It does not say "4 new items" and does not repeat the earlier count.

**18. A row left out is silent**
- Keys: run `curl -X POST "http://127.0.0.1:8090/display/test/emit?n=1&kind=scanner"`. Then move to the check box in the notice of the feed (it follows the select "Period").
- The page: the new row is not shown. The notice now counts the rows left out.
- The reader says: nothing at all when the row arrives. On the check box: "Show them", described by "2 items were hidden." (not "1 item").

**19. Focus is not moved by an update**
- Keys: Tab to the select "Period" and leave focus there. Run `curl -X POST "http://127.0.0.1:8090/display/test/emit?n=2"`. Then press Tab once.
- The page: two rows arrive above the list; the select keeps focus and its value until you press Tab.
- The reader says: one message, "2 new items". It does not read the select again on its own. After Tab, the next stop is the check box "Show them" of the notice, not the top of the page or the first row.

**20. Change the period**
- Keys: run the reset command and reload `/display/live`. Tab to "Period" and choose "Last 5 events" (NVDA: Down arrow twice; VoiceOver: VO+Space, Down arrow twice, Enter). Move to the list. Then run `curl -X POST "http://127.0.0.1:8090/display/test/emit?n=1"`.
- The page: the feed is drawn again in place with five rows. The feed keeps following the stream under the new period.
- The reader says: the select reads its new value, "Last 5 events". The list reads as five items. The page makes no spoken message for the change itself. After the emit: "1 new item", once.

**21. The connection is cut**
- Keys: run the reset command and reload `/display/live`. Then run this as one line, so that the timing is right. macOS or Git Bash: `curl -X POST "http://127.0.0.1:8090/display/test/refuse?n=2" && curl -X POST "http://127.0.0.1:8090/display/test/drop" && sleep 2 && curl -X POST "http://127.0.0.1:8090/display/test/emit?n=2"`. Windows PowerShell: the same four commands with `curl.exe`, separated by `;` and with `Start-Sleep 2` in place of `sleep 2`.
- The page: the status changes to "Reconnecting". The next two attempts to connect are refused (after about 1 and 3 seconds) and the third works, about 7 seconds after the cut. The status returns to "Live". The two rows added while it was down arrive.
- The reader says: "Reconnecting" once (it does not repeat for each refused attempt), then "Live" once, about 7 seconds later. Then one message, "2 new items", once. It never says "Not live".

**22. The tab is hidden**
- Keys: switch to another tab of the browser. Run `curl -X POST "http://127.0.0.1:8090/display/test/emit?n=2"`. Wait five seconds. Switch back.
- The page: while the page is out of sight the status shows "Paused" and no connection is held. When it is shown again the page connects and catches up.
- The reader says: on return, "Live" (it may say "Connecting" first), and then one message, "2 new items", once. It does not read the old rows again.

### Links, and JavaScript off

**23. Follow a row's link**
- Keys: run the reset command and reload `/display/live`. Both readers: Tab to the link "Maya Patel" in the feed and press Enter (VoiceOver: VO+Space). Read the new page.
- The page: the recipient's page opens in place, with chips, cards and a timeline.
- The reader says: the page title, once. Then the chips: "Delivered yes", "Opened yes", "Clicked yes", "Replied no". Then the cards "Status", "Clicked" and the email address; "Device", "Safari on macOS"; "Place", "Mumbai", "Asia/Kolkata"; "Speed", "310 s". Then a timeline "Steps for Maya Patel" with three events and the note "Nothing else happened."

**24. JavaScript off**
- Keys: turn JavaScript off and open `/display/live`. Both readers: read the feed header and Tab through it. Open `/display` and Tab to the check box in the notice of "Dev Kapoor". Turn JavaScript on again afterwards.
- The page: there is no connection and no live status. The select "Period" is followed by a button "Apply". The notice has the check box but no "Dismiss" button. New rows do not arrive on their own.
- The reader says: no "Live", "Connecting" or any other state word. After "Period": a button "Apply". The notice offers "Show them" only, and Space on it shows the extra event.

## Results

| | |
|---|---|
| Script | `timeline-activity-live.md` |
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
| 1 | Ten badge words in order; dots and icons silent | | | |
| 2 | "Delivered yes" ... "Replied no" as two words each | | | |
| 3 | Label, value, then details for each card; icon silent | | | |
| 4 | Group named by title; zone line; day heading; closing note | | | |
| 5 | List of 6 items; time, title, wait chip, facts in order | | | |
| 6 | "Show them" check box described by "1 item was hidden."; Space adds an event | | | |
| 7 | Dismiss removes notice; focus not reported as the document | | | |
| 8 | Feed heading, summary, zone line, "Period" select; first row in order | | | |
| 9 | Stops in order; list "Recent activity" with 13 items | | | |
| 10 | "No events yet" and "Nothing yet"; no list | | | |
| 11 | "Loading" conveyed; placeholders silent | | | |
| 12 | Alert "The activity could not load." and link "Try again" | | | |
| 13 | Hostile text read as plain text; nothing extra | | | |
| 14 | "Live" conveyed; never "Not live" | | | |
| 15 | Toast "Activity added" once; "1 new item" once; focus stays | | | |
| 16 | "3 new items" once, about a second after the burst | | | |
| 17 | "1 new item" once; no running total | | | |
| 18 | Silence for a row left out; notice says "2 items were hidden." | | | |
| 19 | "2 new items" once; select keeps focus and is not re-read | | | |
| 20 | Select reads "Last 5 events"; list of 5; "1 new item" once | | | |
| 21 | "Reconnecting" once, "Live" once, then "2 new items" once | | | |
| 22 | On return "Live" and "2 new items" once; no old rows re-read | | | |
| 23 | Page title once; chips yes/no; cards; timeline "Steps for Maya Patel" | | | |
| 24 | No state words; "Apply" button; notice without "Dismiss" | | | |

Overall: see [results-template.md](results-template.md) for the sign-off checklist.
