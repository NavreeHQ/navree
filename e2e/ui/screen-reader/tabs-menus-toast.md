# Tabs, toggle groups, dropdown menus and toasts screen reader script

## Purpose

Tabs, toggle groups and dropdown menus are keyboard widgets: each has its own arrow keys, and a reader must say which item is selected, checked or expanded, and where it is in the set. Toasts are messages that appear for a few seconds with no focus move, so they depend on live-region roles: a status for most, an alert for errors. This script settles what a reader says for each, whether a toast is spoken once, and where focus goes when a menu or toast closes.

## Page to open

The UI proof app, http://127.0.0.1:8090/menus (title "Menus and toasts"; see [Start the proof apps](README.md#start-the-proof-apps)). The page has four sections in this order: Tabs, Toggle groups, Dropdown menus, Toasts.

Steps 23 and 24 use the same page with JavaScript off. Step 25 stops the app, so it comes last.

## Before you start

- JavaScript on, except steps 23 and 24. Window about 1200 px wide.
- Load `/menus` fresh. Reload it if a step leaves a menu or dialog open.
- Speech viewer (NVDA) or caption panel (VoiceOver) visible. Toasts leave the screen on their own: info and success after 5 seconds, warning and error after 8 seconds, and a toast waits while it has focus or the pointer. The viewer is your record of what was said.
- NVDA: use focus mode for the widgets (NVDA switches by itself on menus and form fields). VoiceOver: put focus on a control with Tab, then press arrow keys by themselves (no VO keys).
- Find a control by its text. NVDA: NVDA+F7 lists buttons and form fields; VoiceOver: VO+U, then Buttons or Form Controls.
- To re-read the focused control: NVDA: NVDA+Tab. VoiceOver: VO+Left arrow, then VO+Right arrow.

## Steps

### Tabs

**1. Arrive at a tab list**
- Keys: Both readers: Tab to the first tab list in the Tabs section, the one that holds Overview, Activity, Files and Settings.
- The page: focus is on the open tab, Overview. Only this one tab of the list is a tab stop.
- The reader says: the list's name "Deal sections" (on arriving in the list), then "Overview", the role tab, selected, and its place, "1 of 4".

**2. Move with the arrow keys**
- Keys: Both readers: Right arrow, Right arrow, Right arrow, End, Home, Left arrow.
- The page: each arrow opens the tab it lands on (automatic activation), and the panel under the list changes. The disabled tab "Files" is skipped. Right arrow from Settings wraps to Overview.
- The reader says: at each key, the tab's name, tab, selected, and its place: "Activity 12" 2 of 4 (the count 12 is part of the name), then "Settings" 4 of 4 (Files is skipped), then "Overview" 1 of 4, "Settings" 4 of 4, "Overview" 1 of 4, "Settings" 4 of 4. The panel's text is not spoken by itself. With the reader's cursor, "Files" reads as unavailable or dimmed.

**3. From the tab to its panel**
- Keys: Both readers: with focus on a tab, press Tab once.
- The page: focus moves to the open panel, which is itself a tab stop (there is no other control in most panels). On Overview the next Tab goes to the button "A button in the panel".
- The reader says: the panel named after its tab ("Overview" or "Settings", role tab panel or the reader's word for a labelled region), and then, with Down arrow in browse mode, the panel's text, "The overview panel." or "The settings panel."

**4. Other kinds of tab list**
- Keys: Both readers: Tab to the Range tabs (Day, Week, Month 3). Press Right arrow, then Enter, then Right arrow, then Space. Then Tab to the Settings sections tabs (Profile, Billing, Team) and press Right arrow, Down arrow, Up arrow. Then Tab to the Many tabs list and press End.
- The page: in Range, the arrow keys move focus only (manual activation) and Enter or Space opens the tab. In Settings sections the list is vertical: Down and Up arrow change tab, and Right arrow does nothing. In Many tabs, End goes to the last of 24.
- The reader says: in Range, "Week", tab, not selected, 2 of 3 after the first Right arrow, and "selected" after Enter. In Settings sections, "Billing" tab selected 2 of 3 after Down arrow and nothing for Right arrow. In Many tabs, "Section number 24", tab, selected, 24 of 24.

**5. A tab that loads on demand**
- Keys: Both readers: Tab to the tab list "Loaded on demand" (Fast, Slow, Fresh, Given). Press Right arrow to "Slow", then Tab to its panel, then read it (NVDA: NVDA+Space for browse mode, Down arrow; VoiceOver: VO+Right arrow).
- The page: the panel loads from the server the first time the tab opens and shows the text "The slow tab, loaded N time(s)." (N is how often this app has been asked since it started) and a button "A button in the loaded panel". While it loads, the panel is marked busy. Go back to Fast and to Slow again: the text does not change, so the tab was not asked for again. (The tab "Fresh" loads again each time it opens.)
- The reader says: the tab "Slow" selected, 2 of 4. The panel's text and button as above. A reader may say "busy" while the panel loads. Opening Slow again says the same text, not a higher number.

### Toggle groups

**6. A group where one item is on**
- Keys: Both readers: move focus to the toggle group "Show as" (List, Grid, Chart; use the form field list or Tab). Press Right arrow, then Right arrow, then Left arrow.
- The page: the items are radio buttons. Right arrow checks the next one. The line below ("Showing as grid.") and the address (`?view=grid`) change in place.
- The reader says: on arriving, the group's name "Show as" (radio group), then "List", radio button, checked, 1 of 3. At each key, the new item's name, radio button, checked, and place: "Grid" 2 of 3, "Chart" 3 of 3, "Grid" 2 of 3. The reader says "checked" and not "pressed". The changed line below is not a live region, so no announcement is required for it; record whether the reader says anything about it.

**7. A group where any items are on**
- Keys: Both readers: move focus to the group "Working days" (Mon, Tue, Wed, Thu, Fri; Mon and Wed on). Press Right arrow, Space, Enter, Right arrow, Right arrow.
- The page: the items are checkboxes with one tab stop. Right arrow moves focus only. Space or Enter toggles. Thu is dimmed and skipped. Pressing Enter does not send the form.
- The reader says: on arriving, "Working days" (group) and "Mon", checkbox, checked. Then "Tue", checkbox, not checked; after Space "checked"; after Enter "not checked". The arrows then go to "Wed" and then "Fri", never to "Thu". With the reader's cursor, "Thu" reads as unavailable.

**8. Icon-only items and turning one off**
- Keys: Both readers: move focus to the group "Layout" (three icon items). Press Down arrow, then Space, then Space.
- The page: the items have no visible text. Down arrow checks "As a grid". Space on the item that is on turns it off, and Space again turns it on.
- The reader says: on arriving, the group "Layout" (radio group) and "As a list", radio button, checked. Each item is named by its hidden text ("As a list", "As a grid", "Settings"), never "radio button" alone. After Down arrow, "As a grid" checked. After the first Space, "not checked"; after the second, "checked".

### Dropdown menus

**9. Open a menu with the keyboard**
- Keys: Both readers: Tab to the button "Actions" (in the Dropdown menus section). Press Enter.
- The page: the menu opens under the button and focus moves to its first item, "Edit".
- The reader says: on the button, "Actions", button, with a note that it opens a menu, collapsed. After Enter: expanded, then the menu's name "Actions" (menu), then "Edit", menu item. The item's shortcut text may be spoken with its name ("Edit E"); that is the item's own text.

**10. Move through the items**
- Keys: Both readers: Down arrow (to Duplicate), Down arrow (Archive), Enter, Down arrow (Rename), Down arrow (Copy link), then type e, then End.
- The page: Archive is dimmed but the arrow keys reach it, and Enter on it does nothing and leaves the menu open. Copy link and Email are in a group named "Share". Typing e jumps to the next item that starts with it, "Email". End goes to "Delete" (shown in a danger colour).
- The reader says: at each key, the item's name, menu item, and its place in the menu. "Archive" is read as unavailable or dimmed, and Enter on it says nothing and the menu stays open. On entering "Copy link" the reader says the group name "Share" where it speaks groups. After the letter e: "Email", menu item. "Delete" is read like any other item: the danger colour has no spoken form.

**11. Choose an item, and Escape**
- Keys: Both readers: Home, Down arrow (Duplicate), Enter. Then Enter on "Actions" again, and Escape.
- The page: choosing "Duplicate" closes the menu, returns focus to "Actions" and shows a toast "Duplicated." Escape also closes the menu and returns focus to "Actions".
- The reader says: after Enter, "Duplicated." once, as a live announcement, and focus on "Actions" (button, collapsed). After Escape, collapsed and "Actions" again. Neither the menu's items nor the toast are spoken a second time.

**12. Checkbox and radio items**
- Keys: Both readers: Tab to the button "View" and press Down arrow. Press Space. Open it again (Down arrow). Press Down arrow, Enter, Down arrow, Down arrow, Enter.
- The page: the first item "Show archived" is checked. Space turns it off and closes the menu. "Show owner" toggles on Enter and the menu stays open (it asks to stay open). The group "Sort by" holds three radio items: Name (on), Date, Size. Enter on Date chooses it and closes the menu.
- The reader says: "Show archived", menu item checkbox (or "check menu item"), checked, then not checked when reopened. "Show owner", checked after Enter, with the menu still open. For the radio items the group name "Sort by", then "Name", radio menu item, checked, 1 of 3; and "Date" 2 of 3 as the highlight reaches it; after Enter "Date" is checked.

**13. Submenus**
- Keys: Both readers: Tab to the button "Move" and press Down arrow (opens it on "Rename"), Down arrow ("Move to"), Right arrow, Down arrow, Down arrow, Right arrow, Left arrow, Escape, Enter (opens "Move to" again), Enter (on "Inbox").
- The page: "Move to" opens a submenu with Inbox, Archive and "Label as", which opens another with Urgent and Later. Left closes the inner level and returns to "Label as". Escape closes the open submenu and returns to "Move to". Enter on Inbox closes everything and shows the toast "Moved to Inbox."
- The reader says: "Move to", menu item, has a submenu, collapsed. After Right arrow: expanded, and "Inbox", menu item. On "Label as": has a submenu, collapsed, then expanded after Right arrow with "Urgent". After Left arrow, "Label as" collapsed. After Escape, "Move to" collapsed. After the last Enter, "Moved to Inbox." once, and focus on the button "Move".

**14. A menu on a table row**
- Keys: Both readers: Tab to the first row's menu button in the table captioned "Rows with a menu", press Enter, Down arrow, then Escape.
- The page: each row has an icon-only button. The menu holds "Open" (a link) and "Remove" (a post, shown in a danger colour).
- The reader says: the button's name "Actions for deal 1" (not "button" alone), collapsed, then expanded and "Open", menu item, 1 of 2. After Down arrow, "Remove", menu item, 2 of 2. After Escape, focus on "Actions for deal 1".

**15. A menu inside a dialog**
- Keys: Both readers: open "Actions" with Enter, press Down arrow three times (to "Rename") and press Enter. In the dialog, focus is on "Menu in a dialog": press Enter. Press Escape. Press Escape again.
- The page: "Rename" opens the dialog "Rename deal" ("A menu item opened this."). The menu inside the dialog opens above it. The first Escape closes only the menu, the second closes the dialog.
- The reader says: on opening the dialog, its name "Rename deal", the role dialog and its description. After the first Escape, focus on "Menu in a dialog" and the dialog still open. After the second Escape, the dialog is gone. Record where focus lands: it must be on a control on the page (the reader says a button or a link, not "document" alone).

### Toasts

**16. Four tones from a button**
- Keys: Both readers: Tab to the buttons Info, Success, Warning and Error in the Toasts section and press Enter on each, one after the other.
- The page: each shows a toast at the bottom end of the window with a Dismiss button. Focus stays on the button you pressed.
- The reader says: after Info: "Info from a command"; Success: "Success from a command"; Warning: "Warning from a command"; Error: "Error from a command", each once, within about two seconds, with no focus move. Error is an alert (spoken at once, over other speech); the others are polite status messages. Focus is still on the button.

**17. A toast with a title and an action**
- Keys: Both readers: Tab to "Rich toast" and press Enter. Then press Alt+T (Option+T on a Mac).
- The page: a toast appears with the title "Archived", the text "The deal moved to the archive.", a button "Undo" and a Dismiss button. Alt+T moves focus to the first link or button inside the toasts, "Undo".
- The reader says: after Enter, "Archived" and "The deal moved to the archive.", once, as one message. After Alt+T, "Undo", button, in the region "Notifications". If the browser opens its own menu on Alt+T instead (some Windows browsers), write it in the notes.

**18. Use the action, then Escape**
- Keys: Both readers: with focus on "Undo" press Enter. Wait a second. Then press Alt+T again and press Escape.
- The page: "Undo" posts and the toast with the action goes. A new toast says "Undone." Alt+T moves focus into it. Escape takes the toast away and focus goes back to the button that had it before Alt+T ("Rich toast").
- The reader says: "Undone.", once. After Escape, focus on "Rich toast" (button), and no toast remains.

**19. Errors: a toast that stays, and a refused post**
- Keys: Both readers: Tab to "Sticky toast" and press Enter. Wait ten seconds. Press Alt+T, then Space. Then Tab to "Refused" and press Enter.
- The page: the sticky toast, titled "Import failed" with "The import stopped at row 40.", stays until it is dismissed. Space on its Dismiss button removes it and returns focus to "Sticky toast". "Refused" shows "That name is taken."
- The reader says: "Import failed" and "The import stopped at row 40." once, as an alert. After ten seconds the toast is still there. After Alt+T, "Dismiss", button (a close control that is a button to the reader, not a link). After Space, focus on "Sticky toast". For "Refused": "That name is taken." once, as an alert.

**20. A message that is replaced**
- Keys: Both readers: Tab to "Saving then saved" and press Enter.
- The page: one toast shows. A toast with the same name replaces the one before it, so it ends as "Saved." and never two toasts.
- The reader says: "Saved.", once. If "Saving..." is also spoken, it is spoken first and "Saved." follows; neither is spoken twice, and only one toast is in the region.

**21. A toast after a redirect**
- Keys: Both readers: Tab to "Post and redirect" and press Enter.
- The page: the form is sent, the page moves to the address with `landed=1`, and the toast "Saved, and moved on." shows on it. Focus moves to the page's main area.
- The reader says: the page title "Menus and toasts", then "Saved, and moved on.", each once. The toast text is not spoken twice.

**22. A toast over a dialog**
- Keys: Both readers: open "Actions", choose "Rename" (Enter). In the dialog press Tab to "Raise a toast" and press Enter. Then press Alt+T, then Escape, then Escape.
- The page: the toast "Raised over the dialog." shows above the dialog. Alt+T moves focus to its Dismiss button. The first Escape removes the toast and the dialog stays. The second closes the dialog.
- The reader says: "Raised over the dialog.", once, although a dialog is open. After Alt+T, "Dismiss", button. After the first Escape, focus back on "Raise a toast" and the dialog still open.

### With JavaScript off

**23. Tabs, toggle group and menu with JavaScript off**
- Keys: Both readers: turn JavaScript off and reload `/menus`. Tab to the tab "Settings" in "Deal sections" (each tab is a link and a tab stop now) and press Enter. Then Tab to "Chart" in "Show as", press Space, Tab to "Apply" and press Enter. Then Tab to "Actions", press Enter, then Escape. Turn JavaScript on again afterwards.
- The page: each tab is a link to the same page with `?tab=...`; the page loads showing that tab. The toggle group is plain radio buttons sent by "Apply". The menu opens and closes with the browser's own popover behaviour.
- The reader says: tabs as links or tabs, with the new page's tab "Settings" selected. "Chart" radio button checked after Apply. The menu button "Actions" expanded when open, and its items spoken as menu items. The arrow-key movement of the tab list is not expected without script; Tab moves between the tabs instead.

**24. A message after a redirect with JavaScript off**
- Keys: Both readers: with JavaScript off, Tab to "Post and redirect, no script" and press Enter. Then move to the region "Notifications" (NVDA: D for the next landmark; VoiceOver: VO+U, then Landmarks) and press Tab to the link "Dismiss" and Enter.
- The page: the new page shows the toast "Saved, and moved on." in a region named "Notifications". The Dismiss link takes the toast away.
- The reader says: the region "Notifications" (landmark) and the text "Saved, and moved on." as a status. Whether the reader speaks it by itself on page load is for the notes: it must at least be found in the region. After Dismiss the toast is gone.

### With the app not answering

**25. A tab and a post that cannot be reached**
- Keys: Both readers: with JavaScript on, reload `/menus`, then stop the proof app (Ctrl+C in its terminal) and leave the page open. Tab to the tab list "Loaded on demand" and press Right arrow to "Slow". Then Tab to "Server toast" (in the Toasts section) and press Enter. Start the app again afterwards.
- The page: the tab panel shows an alert "This tab didn't load." with a "Retry" button, and the busy mark goes. "Server toast" shows an error toast "No connection. Try again."
- The reader says: "This tab didn't load.", once, as an alert, and a "Retry" button after it. "No connection. Try again." once, as an alert. After starting the app, press "Retry": the panel loads and the text "The slow tab, loaded 1 time(s)." replaces the alert. Record where focus is after Retry.

## Results

| | |
|---|---|
| Script | tabs-menus-toast.md |
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
| 1 | List "Deal sections"; Overview, tab, selected, 1 of 4 | | | |
| 2 | Each tab selected with place; Files skipped; wrap | | | |
| 3 | Panel named after its tab; its text readable | | | |
| 4 | Manual: not selected until Enter; vertical; Section number 24 of 24 | | | |
| 5 | Slow selected; loaded text; not asked again | | | |
| 6 | Show as radio group; radio checked, place, not "pressed" | | | |
| 7 | Working days checkboxes; Thu skipped; Space and Enter toggle | | | |
| 8 | Icon-only items named; Space turns one off and on | | | |
| 9 | Actions collapsed, then expanded; Edit menu item | | | |
| 10 | Items with place; Archive dimmed, Enter does nothing; group Share | | | |
| 11 | "Duplicated." once; focus back on Actions | | | |
| 12 | Checkbox checked state; radio group Sort by, 1 of 3 | | | |
| 13 | Submenu collapsed, expanded; Left returns; "Moved to Inbox." once | | | |
| 14 | Actions for deal 1; Open, menu item; focus returns | | | |
| 15 | Rename deal dialog; Escape closes menu first; focus on a control | | | |
| 16 | Four texts once each; Error as alert | | | |
| 17 | Archived and its text once; Alt+T reaches Undo | | | |
| 18 | "Undone." once; Escape returns focus to Rich toast | | | |
| 19 | Import failed stays; Dismiss is a button; "That name is taken." | | | |
| 20 | "Saved." once; one toast only | | | |
| 21 | Page title, then "Saved, and moved on." once | | | |
| 22 | Toast spoken over a dialog; Escape keeps the dialog | | | |
| 23 | Tabs as links; radio Apply; menu opens without script | | | |
| 24 | Region Notifications holds "Saved, and moved on." | | | |
| 25 | "This tab didn't load." alert with Retry; "No connection. Try again." | | | |

## Overall result

- [ ] Every step passed in NVDA with Firefox.
- [ ] Every step passed in NVDA with Chrome.
- [ ] Every step passed in VoiceOver with Safari.
- [ ] Every failed step has an issue, with the module name and the step number.
- [ ] Every n/a has a reason in the notes.

Signed off by: ______________________  Date: ______________
