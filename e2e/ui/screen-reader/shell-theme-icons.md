# App shell, theme menu, icons and flags screen reader script

## Purpose

The app shell is on every page of every app: its landmarks, the sidebar, the top bar, the account menu, the appearance menu, and the announcement a boosted page change makes. Icons and flags appear inside every other module, so this script also settles that a decorative icon or flag stays silent and a meaningful one is named once. A fault here is heard on every page.

## Page to open

The UI proof app (see [README.md](README.md), "Start the proof apps"):

- `/themes`: http://127.0.0.1:8090/themes. The shell with the appearance button in the top bar and a "Look" group in the sidebar.
- `/themes/layouts/header`: http://127.0.0.1:8090/themes/layouts/header. The same shell with the links in the top bar.
- `/icons-flags/hidden-first`: http://127.0.0.1:8090/icons-flags/hidden-first. Icons and flags, with hidden copies of the flags placed before the visible ones.

## Before you start

- JavaScript on, except where a step says "JavaScript off".
- Window wide enough to show the sidebar (at least 1024 px). Steps 7 and 8 make it narrow.
- Clear the theme cookie so that you start in the default look: open the browser's site data for 127.0.0.1 and remove the cookie `navree_ui_theme`.
- Start in browse mode (NVDA), or with the VoiceOver cursor on the page.
- Open the speech viewer (NVDA) or the caption panel (VoiceOver).
- The proof app has no skip link. Reader users reach the main area with the landmarks list: NVDA, press D in browse mode or NVDA+F7 and choose Landmarks; VoiceOver, VO+U and choose Landmarks.

## Steps

### Landmarks and the sidebar

**1. Landmarks**
- Keys: Open `/themes`. NVDA: NVDA+F7, Landmarks. VoiceOver: VO+U, Left or Right arrow to Landmarks.
- The page: the page has a top bar, a sidebar named "Sidebar" holding a navigation named "Main", a breadcrumb navigation, and the main area. A notifications area named "Notifications" holds toasts and is listed only while one is on show.
- The reader says: a banner (the top bar), a complementary landmark named "Sidebar", a navigation named "Main", a navigation named "Breadcrumb", and a main landmark. Every landmark of the same kind has a different name. If "Notifications" is listed, its name is "Notifications".

**2. The page heading and the main area**
- Keys: NVDA: from the top, press D until "main", then H. VoiceOver: VO+U, Landmarks, choose main, Enter, then VO+Right arrow.
- The page: the main area starts with the level 1 heading "Themes".
- The reader says: entering the main landmark, then the heading "Themes", level 1. The first Tab from the top of the page goes into the sidebar, not to the main area (there is no skip link).

**3. Sidebar links and the current page**
- Keys: Go to the "Main" navigation, then read down. NVDA: browse mode, Down arrow. VoiceOver: VO+Right arrow.
- The page: the groups "Sales", "System" and "Look" hold links. The link for the page you are on, "Themes", is marked as the current page. The "Deals" link has the count "1000" beside it.
- The reader says: each group's name as text, then its links, each as "link" with its name, with no icon name spoken. "Themes" is conveyed as the current page. "Deals" is conveyed with its count, as in "Deals 1000". The word "Deals" is not spoken twice in a row (once as the name and again as a description). If it is, mark the step failed and note the reader and browser.

**4. A boosted page change is announced once**
- Keys: In the sidebar, move to the link "Gallery" (group "System"). Both readers: Enter. Wait two seconds.
- The page: the gallery replaces the page without a full reload. The address changes to `/gallery`, the title changes to "Gallery", and focus moves to the main area. The link "Gallery" is now the current page.
- The reader says: the new title "Gallery" once, from the page's own announcer, within about one second. Focus is in the main area, and the reader may name the main landmark. The sidebar is not read again. If the title is spoken twice, or not at all, mark the step failed.

**5. Back returns and is announced**
- Keys: Alt+Left (Windows), Command+Left bracket (Safari).
- The page: the themes page returns, with the title "Themes".
- The reader says: the title "Themes" once. Focus is in the main area or where the browser puts it; the reader does not announce the whole page.

**6. Fold the sidebar**
- Keys: Tab to the button "Toggle sidebar" in the top bar. Both readers: Space or Enter. Press it again to unfold. (Ctrl+B, or Command+B on a Mac, does the same from anywhere on the page.)
- The page: the sidebar folds to a column of icons, and unfolds again. The button's expanded state follows.
- The reader says: before: "Toggle sidebar", "button", "expanded". After folding: "collapsed". After unfolding: "expanded". Each state is spoken once, on the button, right after the press. While the sidebar is folded, move back to the "Main" navigation: the links are still there, each named by its name only ("Overview", "Deals", "Gallery"), with no count and no group names. A link with no name, or named "link", fails.

**7. The sidebar on a narrow window**
- Keys: Make the window narrower than 768 px, or zoom the page to 400 percent (Ctrl+Plus, Command+Plus in Safari). Reload. Read the landmarks list, then press the button "Toggle sidebar".
- The page: the sidebar is hidden until you press the button. The button opens it as a sheet over the page, and focus moves to its first link.
- The reader says: before the press, the landmarks list has no "Sidebar" and no "Main" navigation. After the press, focus is on the first link in the sidebar (the app name), and the reader says it as a link. Reading continues down the links.

**8. Close the sheet**
- Keys: Press Escape. Then open the sheet again and activate a link, for example "Gallery".
- The page: Escape closes the sheet and returns focus to "Toggle sidebar". Choosing a link closes the sheet and shows the new page.
- The reader says: after Escape, "Toggle sidebar" and its state. After a link, the new page's title once (see step 4). The sheet's links are not reachable again until it is reopened.

### The account menu

**9. The account button**
- Keys: Make the window wide again (zoom back to 100 percent) and reload `/themes`. Tab or move to the button at the foot of the sidebar that shows "Ada Lovelace" and "ada@navree.test".
- The page: a button with a person icon, the name and the email.
- The reader says: the name "Ada Lovelace", the role "button", and that it opens a dialog or popup (NVDA: "opens dialog" or "has popup"; VoiceOver: "popup"). The email is conveyed. "Ada Lovelace" is not spoken three times in a row; once as part of the name is right, a repeat from the icon's label and from the title is not. Note any repeat.

**10. Open the account menu**
- Keys: Both readers: Enter (or Space). VoiceOver: VO+Space.
- The page: a panel opens beside the button with a link "Settings" and a button "Sign out". Focus moves into the panel.
- The reader says: the panel's name "Account" and its role "dialog". Then Tab: "Settings", "link"; Tab: "Sign out", "button". Do not press "Sign out".

**11. Close the account menu**
- Keys: Escape.
- The page: the panel closes.
- The reader says: focus returns to the button "Ada Lovelace", and the reader says that button again (name and role). Nothing else is spoken.

### The appearance menu

**12. The appearance button**
- Keys: Tab to the icon button in the top bar named "Appearance".
- The page: a sun and moon icon button.
- The reader says: "Appearance", "button", and that it opens a dialog or popup. No icon name.

**13. Open the menu and read the themes**
- Keys: Enter. Then Tab once.
- The page: a panel named "Appearance" opens with a group of eight themes, one chosen, a group of three modes, and an "Apply" button. The first Tab moves to the chosen theme.
- The reader says: on opening, "Appearance", "dialog". On Tab, the group name "Theme", then the chosen theme's name and its description (the colour swatches are not spoken), the role "radio button", the state "checked" and its place ("1 of 8" for the default theme, or the number of the one chosen).

**14. Change the theme with the arrow keys**
- Keys: Down arrow, then Up arrow.
- The page: the next theme is chosen, and the whole page changes to that theme at once, with no reload. The menu stays open.
- The reader says: the next theme's name, "radio button", "checked", and its place ("2 of 8"). The page does not announce the change in any other way; that is expected. Focus stays in the menu.

**15. Change the mode**
- Keys: Tab to the group "Mode". Right arrow.
- The page: the modes are Light, Dark and System. The page changes mode at once.
- The reader says: the group name "Mode", the chosen mode's name, "radio button", "checked", and its place (Light is "1 of 3", Dark "2 of 3", System "3 of 3"). After Right arrow, the next mode, with its place. The icons (sun, moon, monitor) are not spoken.

**16. Close the menu**
- Keys: Escape.
- The page: the menu closes.
- The reader says: focus returns to "Appearance", spoken as a button. The page stays in the chosen look.

**17. The appearance menu with JavaScript off**
- Keys: Turn JavaScript off in the browser and reload `/themes`. Tab to "Appearance". Enter. Tab to a theme and press Space to choose it (arrow keys in a radio group still work). Tab to "Mode" and choose one. Tab to "Apply" and press Enter.
- The page: the panel opens natively. Choosing a card changes nothing until "Apply". "Apply" posts the form, and the page loads again in the new look.
- The reader says: the same names, roles and states as steps 12 to 15. "Apply", "button". The reloaded page speaks its title as it loads (the reader's own announcement of a new page). Reopen the menu: the choice you made is the checked one. Turn JavaScript on again.

### The header layout

**18. Links in the top bar**
- Keys: Open `/themes/layouts/header`. NVDA: D to the navigation named "Main", then Tab. VoiceOver: VO+U, Landmarks, choose the navigation named "Main".
- The page: there is no sidebar. The top bar holds the app name, buttons for each group ("Sales", "System", "Look") and the appearance button.
- The reader says: no landmark named "Sidebar" in the list. A navigation named "Main". Buttons named "Sales", "System" and "Look". Each button says what it opens (NVDA: "opens menu" or the expanded state; VoiceOver: "popup"). No chevron icon name is spoken.

**19. Open a group and reach its links**
- Keys: On "Sales", press Enter. NVDA / Chrome / Firefox: Tab. Safari with VoiceOver: VO+Right arrow (Tab reaches the links only when the Safari setting from the README is on).
- The page: a panel opens under the button with a list of links, and focus can move to the first link.
- The reader says: the button's expanded state where the browser gives one. Then each link's name and "link". The link of the page you are on is conveyed as the current page.

**20. Close it**
- Keys: Escape.
- The page: the panel closes.
- The reader says: focus returns to "Sales", spoken as a button; the state is "collapsed" where the browser gives one.

### Icons and flags

**21. Buttons, a badge and a labelled icon**
- Keys: Open `/icons-flags/hidden-first`. NVDA: H to "Icons", then B and Down arrow. VoiceOver: VO+U, Headings, "Icons", then VO+Right arrow.
- The page: under "Icons": a button "New deal" with a plus, a button "Open" with a mail icon, an icon-only button, a badge "Delivered" with a check, and a separate check icon.
- The reader says: "New deal, button" (not "plus"), "Open, button", "Delete, button" for the icon-only one, the badge as the text "Delivered" (not "check"), and the separate check icon as a graphic named "Delivered".

**22. The icon grid is silent**
- Keys: Read on from "Delivered" with Down arrow (NVDA) or VO+Right arrow (VoiceOver) until the next heading.
- The page: after them, a grid of every icon in the toolkit.
- The reader says: nothing for the grid; the next thing read is the heading "Own icons", level 2. No icon names (such as "arrow", "check", "circle") are heard. If the reader reads icon names, mark the step failed.

**23. Own icons**
- Keys: Read down from "Own icons".
- The page: buttons "Launch" and "Orbit", a badge "Verified", each with an icon of the app's own, and one more icon alone.
- The reader says: "Launch, button", "Orbit, button", "Verified" as text. The icons are silent. The lone icon is silent too.

**24. Hidden flags stay silent**
- Keys: Go to the heading "Flags", then read down. NVDA: Down arrow. VoiceOver: VO+Right arrow.
- The page: before the visible flags are a closed section "More flags", a hidden box and a closed dialog, each holding a copy of every flag.
- The reader says: "More flags", role "disclosure" or "button", state "collapsed" (the reader's words). Nothing from the hidden box or the closed dialog. Then the first flags of the visible grid, each a graphic named with its country. A flag name is not heard twice in a row.

**25. Open the disclosure and close it**
- Keys: On "More flags", press Enter or Space. Press it again.
- The page: the section opens and shows the same flags again, then closes.
- The reader says: "expanded", then the flags inside are reached, named like the visible ones; then "collapsed" again and the flags are no longer reachable.

**26. Flag sizes, an unknown flag, flags beside names, and a lone flag**
- Keys: Go past the three grids of flags to the row of five Brazil flags (NVDA: press G to move between graphics, or use the elements list; VoiceOver: VO+Right arrow, or the rotor). Read on to the badges, then use the table: NVDA, T to the table, then Ctrl+Alt+Down arrow; VoiceOver, VO+Shift+Down arrow to interact, then VO+arrow keys.
- The page: after the grids come five flags of Brazil in five sizes and two flags the toolkit does not know. Then come the badges "Germany" and "Japan", each with a flag, and one flag of India. Last is a table with the headings "Country" and "Opens" and the rows "France 12,480", "Brazil 9,215" and "United Arab Emirates 4,310", each with a flag before the name.
- The reader says: "Brazil", as a graphic, five times, one for each size. "Unknown country", as a graphic, twice. "Germany" and "Japan" once each, with no flag name added. The single flag is a graphic named "India". In the table, "France" once (not "France France"), then the cell "12,480" with the column header "Opens".

## Results

Fill in the sign-off block for this run, then one row for each step.

| | |
|---|---|
| Script | shell-theme-icons.md |
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
| 1 | Banner, Sidebar, Main nav, Breadcrumb nav, main; names differ | | | |
| 2 | Main landmark, then h1 "Themes"; no skip link | | | |
| 3 | Group names, links, "Themes" current; "Deals 1000" not repeated | | | |
| 4 | Title "Gallery" spoken once within a second; focus in main | | | |
| 5 | Back: title "Themes" once | | | |
| 6 | Toggle sidebar expanded/collapsed spoken; folded links keep names | | | |
| 7 | Narrow: no Sidebar landmark; button opens sheet, focus on first link | | | |
| 8 | Escape returns to Toggle sidebar; link closes sheet and announces title | | | |
| 9 | "Ada Lovelace", button, has popup; email conveyed; no triple repeat | | | |
| 10 | "Account" dialog; Settings link; Sign out button | | | |
| 11 | Escape returns focus to the account button | | | |
| 12 | "Appearance", button, has popup; no icon name | | | |
| 13 | Dialog "Appearance"; group "Theme"; radio name, checked, place | | | |
| 14 | Arrow keys change theme; new radio spoken with place; menu stays open | | | |
| 15 | Group "Mode"; Light/Dark/System radios with place; icons silent | | | |
| 16 | Escape returns focus to "Appearance" | | | |
| 17 | JavaScript off: same names; Apply posts; choice kept | | | |
| 18 | Header layout: no Sidebar; Main nav; buttons Sales, System, Look | | | |
| 19 | Group opens; links reachable; current page conveyed | | | |
| 20 | Escape returns to "Sales", collapsed | | | |
| 21 | Icon buttons named by text or label; no icon names; "Delivered" graphic | | | |
| 22 | Icon grid silent; next is heading "Own icons" | | | |
| 23 | "Launch", "Orbit", "Verified"; icons silent | | | |
| 24 | "More flags" collapsed; hidden copies silent; flags named once | | | |
| 25 | Disclosure expanded/collapsed spoken; contents reachable only when open | | | |
| 26 | Beside-name flags silent; lone flag "India"; "Unknown country" twice | | | |
