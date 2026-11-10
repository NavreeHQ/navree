# Error pages, sign-in and states screen reader script

## Purpose

When something is missing, refused or broken, the toolkit draws a page, a state inside a page, a toast, or a message in a region, always in the same words. A reader user depends on three things here: that an error page is a real page with a title, one heading and a way on; that a page which replaces another in place is announced by its title, once; and that a failure in a region or an action is spoken once, without the app's secrets and without repeating on every poll. This script settles those spoken parts for every status page, the states inside cards, tabs, lists and tables, regions that fail and recover, posts that fail, sign-in with its way back, and the same pages with script off.

## Page to open

The UI proof app, http://127.0.0.1:8090 (see [Start the proof apps](README.md#start-the-proof-apps)). Routes used:

| Route | Shows |
|---|---|
| `/pages/status/{code}` | a page that ends in that status (401, 403, 404, 409, 429, 500 and others) |
| `/pages/leak` | a failure with secrets inside, which must not be shown |
| `/pages/user` | a page that needs a signed-in user |
| `/pages` | the states, in every kind, in cards, in a tab, empty, and links to error pages |
| `/pages/rows` | a table with a locked row, a missing row and an empty row |
| `/pages/regions` | regions whose fetches end in each status |
| `/pages/actions` | forms whose posts end in each status |
| `/sign-in` | the sign-in page, which reads `next` and goes back there |

## Before you start

- Use two browser windows. A private window is a guest (steps 1 to 6, 23, 24 and 25). A normal window is signed in once you finish step 7: the sign-in sets a cookie for one hour. To be a guest again, use a new private window.
- The app must be started without `UI_PROOF_DEBUG`, so error pages show no development details.
- Before steps 16 to 19, reset the counts the regions use, in a terminal: `curl -X POST http://127.0.0.1:8090/pages/reset` (on Windows, `curl.exe -X POST http://127.0.0.1:8090/pages/reset`). The flaky region fails twice and then works, once per reset.
- NVDA: speech viewer open, and clear its log before the steps that count what was said (16, 20 to 22). VoiceOver: caption panel on, and "Press Tab to highlight each item" on in Safari.
- JavaScript is on, except in steps 25 and 26.
- An address ending in `#main`, `#inline`, `#empty` or `#links` puts the reader at that part of the page, and one press of Tab then reaches the first control after it.

## Steps

### Error pages with no shell

**1. A missing page**
- Keys: NVDA: in a private window open `/pages/status/404`, NVDA+T for the title, Down arrow from the top, then H for headings. VoiceOver: open it in a private Safari window, VO+Right arrow from the top, then VO+Command+H.
- The page: a page on its own, with no sidebar. It has the code 404, a headline, an explanation, one way on and a reference.
- The reader says: the title "Page not found". A main landmark. A group whose name is the headline "We can't find that page". Inside it: "404"; a heading, level 1, "We can't find that page"; the sentence "The address may be wrong, or the page may have moved or been deleted."; the link "Go to home"; then "Reference" and a code of letters and digits. There is exactly one heading on the page. The icon is not spoken, and there is no "Copy reference" button on this page. The headline can be spoken as the group's name and again as the heading: each at most once. The same words a third time is a failure.

**2. Keyboard on the missing page**
- Keys: both readers: reload `/pages/status/404`, press Tab, then Enter; afterwards go Back (NVDA: Alt+Left arrow; VoiceOver: Command+[).
- The page: the first stop on the page is the main action, with a visible focus ring. Enter goes to the home page of the app.
- The reader says: on Tab, "Go to home", link. After Enter, the new page's title as for any new page. After Back, the title "Page not found" again.

**3. Sign in required, and no access**
- Keys: both readers: open `/pages/status/401`, read down, press Tab. Then open `/pages/status/403` and read down.
- The page: two pages on their own, each with a code, a headline and one link.
- The reader says: for 401, the title "Sign in required"; "401"; a heading "Sign in to continue"; "You need to be signed in to see this. After you sign in, you come back here."; the link "Sign in" (the one stop). For 403, the title "No access"; "403"; a heading "You don't have access to this"; "Your account can't open this page. If you think it should, ask an admin to give you access."; the link "Go to home".

**4. A failure with secrets inside**
- Keys: NVDA: open `/pages/leak` and use Say all (NVDA+Down arrow). VoiceOver: VO+A. Look at the speech log afterwards.
- The page: a server failure whose message holds a password, a query, a token and a file path. None of it is on the page.
- The reader says: the title "Something went wrong"; "500"; a heading "Something went wrong on our side"; "That wasn't your doing. Try again in a moment. If it keeps happening, tell support the reference below."; the links "Try again" and "Go to home"; then "Reference" and a code. The log holds none of these words: "hunter2", "password", "SELECT", "token", "ubuntu", "db.rs", "Development only".

**5. A page that asks you to wait**
- Keys: both readers: open `/pages/status/429` and read down, then press Tab.
- The page: a page asking the visitor to wait, with one link.
- The reader says: the title "Too many requests"; "429"; a heading "You're going too fast"; "Wait a moment, then try again."; then "Try again in 12 seconds."; the link "Try again" as the only stop. There is no "Go to home".

**6. A clash with another change**
- Keys: both readers: open `/pages/status/409` and read down, then press Tab twice.
- The page: a page with an extra line from the server.
- The reader says: the title "Conflict"; "409"; a heading "That clashes with another change"; "Someone changed this while you were working on it. Reload to see the latest version, then try again."; then the extra line "Someone else renamed this deal a minute ago."; then the links "Reload" and "Go to home", in that order.

### Sign in

**7. The sign-in form**
- Keys: both readers: open `/sign-in`, read down, then Tab through the form. Press Enter on "Sign in".
- The page: a form with one field and one button. The field holds the name "Anu". Enter signs in and goes to the home page.
- The reader says: the title "Sign in"; a heading "Sign in"; the field "Name", edit, required, with the value "Anu"; the button "Sign in". After Enter the new page's title is spoken as for any new page. You are signed in for the next steps.

### Errors in the shell (signed in)

**8. The error inside the app**
- Keys: both readers: signed in, open `/pages/status/500#main`, press Tab three times, then read the page from the top (H for headings).
- The page: the sidebar and top bar stay. The page's heading is the short title, and the state is drawn under it. The first Tab after the start of the main area goes to the state's first link.
- The reader says: the title "Something went wrong"; one level 1 heading, "Something went wrong" (the page's own heading); the state's headline "Something went wrong on our side" is read as text, not as a heading. The Tab stops, in order: "Try again", link; "Go to home", link; "Copy reference", button. The line "Reference" and a code of letters and digits sits before the button.

**9. Copy the reference**
- Keys: both readers: on the button "Copy reference", press Enter (VoiceOver: VO+Space).
- The page: the reference is put on the clipboard.
- The reader says: "Copied", once, within about a second. The button's name stays "Copy reference", and focus stays on it.

**10. A link to an error keeps the shell**
- Keys: both readers: signed in, open `/pages#links`, press Tab three times to the link named "404", then press Enter.
- The page: the address changes to `/pages/status/404`. The sidebar and top bar stay and the page does not reload. Focus moves to the main area, and the error is shown inside the shell with the heading "Page not found".
- The reader says: the page title "Page not found", once, from the page's live region, within about a second. The reader may also name the main landmark as focus enters it: that is not a repeat of the title. Reading down: the heading "Page not found", then the state "We can't find that page". Tab from there goes to "Go to home".

**11. Back from the error**
- Keys: NVDA: Alt+Left arrow. VoiceOver: Command+[.
- The page: the earlier page returns in place, with no reload, and focus moves to the main area.
- The reader says: the page title "States", once.

### States inside a page

**12. Cards that cannot show their content**
- Keys: both readers: signed in, open `/pages#inline`, read down, press Tab three times.
- The page: five cards. "Revenue" and "Contracts" refuse access, "Owner" is missing, "Your deals" asks for sign-in.
- The reader says: in "Revenue", "You don't have access to this report." In "Contracts", "You don't have access to this contract." and a link "Request access". In "Owner", "This deal doesn't exist, or it was removed." In "Your deals", "Sign in to see your deals." and a link "Sign in". None of the headlines is a heading. Each headline is spoken as the card state's group name and as its text: each at most once. The three Tab stops are the link "Request access", the link "Sign in", and the selected tab "Overview" of the tab list "A record".

**13. A tab that holds a state**
- Keys: both readers: continue from step 12, on the tab list "A record": Right arrow to "Notes" (press Enter or Space if the panel does not open by itself), then Right arrow to "Files".
- The page: the panel of "Notes" shows a refusal, and the panel of "Files" shows a missing item.
- The reader says: the tab "Notes", selected; the panel says "You don't have access to this notes." Then the tab "Files", selected; the panel says "This file list doesn't exist, or it was removed." (The words follow the names the proof page gives the things.)

**14. Empty lists**
- Keys: both readers: open `/pages#empty`, read down, press Tab.
- The page: three empty states with a different way on each.
- The reader says: "No deals yet", then "Create a deal to start tracking your pipeline.", then the button "New deal". Then "No results", "Nothing matches your search or filters. Change them, or clear them to see everything.", and the link "Clear filters". Then "You're all caught up" and "There is nothing left to do here." No status code is spoken for any of them.

**15. Locked and missing rows in a table**
- Keys: NVDA: open `/pages/rows`, browse mode, move into the table and use Ctrl+Alt+Down arrow. VoiceOver: VO+Shift+Down arrow to interact with the table, then VO+Down arrow.
- The page: a table of deals where two rows cannot show their deal. Each such row is one cell across both columns.
- The reader says: the caption "Deals, one of which you can no longer open"; a table of 2 columns; the headers "Name" and "Amount". Rows in order: "Agra", "1,200.00"; then one cell, "You no longer have access to this row."; "Gulati", "3,400.00"; one cell, "This deal doesn't exist, or it was removed."; one cell beginning "No results"; "Ilaichi", "9,900.00". None of the three single cells is spoken as a link, button or check box.

### Regions

**16. A page of regions answers at once**
- Keys: both readers: after the reset in "Before you start", clear the speech log, open `/pages/regions`, then wait 15 seconds without touching a key. Read the log.
- The page: cards that each fetch a fragment, several of them every second or so. They answer with each kind of refusal and failure.
- The reader says: the page's own announcer says each state's headline when it arrives. A headline is not spoken more often than the number of cards in the table shows. After the first few seconds nothing more is spoken, even though some regions keep asking.

| Headline | Cards that show it |
|---|---|
| Sign in to continue | 1 (Region 401) |
| You don't have access to this | 2 (Region 403, Door) |
| We can't find that page | 2 (Region 404, Polling) |
| This is gone for good | 1 (Region 410) |
| You're going too fast | 2 (Region 429, Poll 429) |
| Something went wrong on our side | 3 (Region 500, Poll 500, and the box under "Any failure") |
| We'll be right back | 2 (Region 503, Flaky) |

Not every headline may be heard (the announcer is one line, and a later one can replace an earlier one before it is spoken). Write which were heard in Notes.

**17. Read a failed region**
- Keys: both readers: open `/pages/regions`, read down from "Region 403" to "Region 503".
- The page: each card holds a state where its content was, in place.
- The reader says: for "Region 403": "403", "You don't have access to this", "Your account can't open this page. If you think it should, ask an admin to give you access."; no button. For "Region 429": "429", "You're going too fast", "Wait a moment, then try again.", "Try again in 12 seconds.", and the button "Try again". For "Region 500": "500", "Something went wrong on our side", the explanation, the button "Try again", and "Reference" with a code. For "Region 401": "401", "Sign in to continue", and the link "Sign in".

**18. Try again until it works**
- Keys: both readers: open `/pages/regions#region-flaky`, press Tab, Enter, and Enter again.
- The page: the "Flaky" card fails twice, then works. Focus is on the "Try again" button.
- The reader says: at load, "We'll be right back", "Not just yet.", the button "Try again". After the first Enter: the same state, and the headline "We'll be right back" is spoken once for that press. Focus stays on "Try again". After the second Enter: "It worked on the third try." is spoken as a status, once, and the state is gone. The button no longer exists, so focus has nowhere to stay: write in Notes where it went (use NVDA+Tab, or press Tab and note what is reached).

**19. A failure that is not "offline"**
- Keys: both readers: open `/pages/regions#region-http`, read down.
- The page: two cards fetch the same failing answer. One only knows the "You're offline" words, and the other has a general failure message.
- The reader says: in "Offline wording", nothing is said about being offline: the offline words are hidden because the server did answer. In "Any failure", an alert: "Something went wrong on our side" with its explanation. The reader does not say "You're offline" anywhere in this section.

### Posts that fail

**20. A refusal is one toast**
- Keys: both readers: signed in, open `/pages/actions#act-403`, press Tab to the field "Note 403", press End and type " more", press Tab to "Post 403", then Enter.
- The page: a toast appears at a corner. The field keeps its text "typed text more". The page stays.
- The reader says: right away, once: "You don't have permission to do that." (a toast raised as an alert, so it is spoken at once). Focus is not moved to the toast. The field still reads "typed text more". Nothing else is spoken, and the text is not repeated.

**21. A clash is a polite toast**
- Keys: both readers: open `/pages/actions#act-409`, Tab to "Note 409", Tab to "Post 409", Enter.
- The page: a toast in a different tone, with the same behaviour.
- The reader says: "Someone else renamed this deal a minute ago." once, as a status, within about a second. The field keeps "typed text".

**22. A failure toast with a reference**
- Keys: both readers: open `/pages/actions#act-500`, Tab to "Note 500", Tab to "Post 500", Enter.
- The page: a toast with the reference at the end. No secret appears.
- The reader says: once: "Something went wrong on our side. Try again in a moment. Reference" followed by a code of letters and digits and a full stop. The log holds none of the words "hunter2", "SELECT", "token", "ubuntu".

### Sign in and the way back

**23. After a session ended**
- Keys: both readers: in a private window open `/sign-in?next=%2Fpages`, read down, press Enter on "Sign in".
- The page: a notice sits above the form. Enter signs in and goes back to the page `/pages`.
- The reader says: the title "Sign in"; the notice "You've been signed out" and "Sign in again to carry on where you were." (a status); the field "Name", required, "Anu"; the button "Sign in". After Enter, the title "States".

**24. An address that would leave the site**
- Keys: both readers: in a private window open `/sign-in?next=%2F%2Fevil.example%2Fx`, press Enter on "Sign in". Then read the address (Ctrl+L; VoiceOver: Command+L).
- The page: the form works as before. The address after sign-in is on this site, at its home page, never on another site.
- The reader says: the address begins `http://127.0.0.1:8090/` and not `evil.example`. The new page's title is spoken as for any page.

### With script off

**25. A guest is sent to sign in and back**
- Keys: turn script off in the browser (see the README). In a private window open `/pages/user`. Tab to "Sign in" and press Enter. On the sign-in page Tab to the field "Name", Tab to the button "Sign in", and press Enter.
- The page: the page that needs a user shows a sign-in page. After the form, the guest comes back to the page they asked for.
- The reader says: on `/pages/user`: the title "Sign in required", a heading "Sign in to continue", the link "Sign in". On the sign-in page: the notice "You've been signed out", the field, and the button. Then the title "A page" and the text "You are on /pages/user." Each page is read as a new page load.

**26. A failed post is a page**
- Keys: with script off and signed in (sign in first, as in step 7), open `/pages/actions#act-403`, Tab to "Note 403", Tab to "Post 403", Enter. Then Back.
- The page: with no script the post is a plain form, so its failure is a whole page. There is no "Try again" for a post.
- The reader says: after Enter, a page with the title "No access", inside the shell, with a heading "No access", the text "You don't have access to this" and the link "Go to home" as the main action. After Back, the actions page returns, with the title "Actions".

## Results

| | |
|---|---|
| Script | pages-errors.md |
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
| 1 | Title, one level 1 heading, explanation, "Go to home", reference; no third repeat | | | |
| 2 | Tab reaches "Go to home" first; Enter goes home; Back returns | | | |
| 3 | 401 offers "Sign in"; 403 offers "Go to home"; both headings read | | | |
| 4 | Generic 500 words and reference; none of the secret words | | | |
| 5 | "Try again in 12 seconds." and one "Try again" link | | | |
| 6 | Extra line, then "Reload" and "Go to home" | | | |
| 7 | Field "Name", required, "Anu"; button "Sign in"; signed in after | | | |
| 8 | Shell stays; heading is short title; Tab: Try again, Go to home, Copy reference | | | |
| 9 | "Copied" once; button name and focus stay | | | |
| 10 | Title "Page not found" once; shell stays; focus in main | | | |
| 11 | Back: title "States" once | | | |
| 12 | Four card states in the same words; two links; no headings | | | |
| 13 | Tabs "Notes" and "Files" show their states | | | |
| 14 | Three empty states with their words, button and link | | | |
| 15 | Table of 2 columns; three single-cell rows read as text | | | |
| 16 | Each headline at most its card count; silence after the first seconds | | | |
| 17 | Regions show code, headline, explanation, buttons as listed | | | |
| 18 | "Try again" twice: headline once each; success words once | | | |
| 19 | No "offline" words; "Any failure" alert read | | | |
| 20 | Alert toast once; field keeps text; focus stays | | | |
| 21 | Status toast once; field keeps text | | | |
| 22 | Toast with reference; no secret words | | | |
| 23 | Notice "You've been signed out"; back at "States" | | | |
| 24 | Address stays on 127.0.0.1:8090 home page | | | |
| 25 | No script: sign in page, back to "You are on /pages/user." | | | |
| 26 | No script: "No access" page in the shell; Back to "Actions" | | | |
