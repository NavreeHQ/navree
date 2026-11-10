# Context menu, menu bar and navigation menu screen reader script

## Purpose

A context menu opens where a person asks for it with a right click, a long press, Shift+F10 or the menu key. A menu bar is a row of menus with one tab stop and arrow keys between them. A navigation menu is a different thing again: a list of links and buttons that disclose panels of links, which is not a menu to a reader. This script settles that each has the right role, name and state, that a reader can open the context menu without a mouse, that the arrow keys and Escape move focus where they should, and that the navigation menu is not announced as a menu.

## Page to open

The UI proof app, http://127.0.0.1:8090/menubar (title "Menu bars"; see [Start the proof apps](README.md#start-the-proof-apps)). The page has three sections in this order: Context menu, Menu bar, Navigation menu.

Steps 21 and 22 use the same page with JavaScript off.

## Before you start

- JavaScript on, except steps 21 and 22. Window about 1200 px wide.
- Load `/menubar` fresh.
- Speech viewer (NVDA) or caption panel (VoiceOver) visible.
- NVDA: stay in focus mode inside a menu (NVDA switches by itself). VoiceOver: use Tab to put focus on a control, then arrow keys by themselves (no VO keys).
- To open the context menu from the keyboard: Shift+F10 or the Menu (Applications) key. On a Mac laptop use Shift+Fn+F10 (the key has to reach the page as F10 with Shift). VoiceOver's own command VO+Shift+M asks the item for its menu in a different way; if you try it, write what it did in the notes.
- Every menu item's shortcut text (such as "Enter") is part of the item's text. A reader may speak it after the name; that is expected.
- To re-read the focused control: NVDA: NVDA+Tab. VoiceOver: VO+Left arrow, then VO+Right arrow.

## Steps

### Context menu

**1. Reach the region**
- Keys: Both readers: Tab until the reader says "Order actions". It is the card in the Context menu section that reads "Order 2041".
- The page: the card has a focus ring. It is a tab stop on purpose, so that a keyboard user can open its menu.
- The reader says: the name "Order actions" and the role group (some readers say "grouping"). The card's text is read on from there (NVDA: NVDA+Space for browse mode, Down arrow; VoiceOver: VO+Right arrow): "Order 2041", "Gurpreet Singh, Ludhiana. ₹1,24,500 by UPI." and the hint "Right click, long press, or press Shift+F10." Press NVDA+Space again to return to focus mode.

**2. Open the menu with the keyboard**
- Keys: Both readers: Shift+F10 (or the Menu key).
- The page: the menu opens under the card, and focus goes to the first item, "Open order".
- The reader says: the menu's name "Order actions" and the role menu, then "Open order", menu item (the word "Enter", its shortcut, may follow). The menu is not announced as a dropdown or a list of links.

**3. Move through the items**
- Keys: Both readers: Down arrow, Down arrow, Down arrow, Down arrow, Down arrow, Down arrow, Down arrow, Down arrow, Down arrow. Then Up arrow once.
- The page: the highlight goes through: Copy the order number, Mark as paid, Refund (dimmed), Flag for follow-up (a check), Ludhiana, Amritsar, Chandigarh (a group of three, named Ship from), Move to (it opens a submenu), Cancel order. The header "Order 2041" and the dividers are not stops.
- The reader says: the name and role of each: "Copy the order number" menu item; "Mark as paid" menu item; "Refund" menu item, unavailable or dimmed (the arrow keys reach it); "Flag for follow-up", menu item checkbox, checked; "Ludhiana", menu item radio, checked, 1 of 3, with the group name "Ship from" spoken where the reader speaks groups; "Amritsar" and "Chandigarh", radio, not checked, 2 of 3 and 3 of 3; "Move to", menu item, has a submenu, collapsed; "Cancel order", menu item. The danger colour of "Cancel order" has no spoken form.

**4. Open a submenu and close it level by level**
- Keys: Both readers: with the highlight on "Move to" press Right arrow. Press Down arrow twice (to "Zones"), then Right arrow. Then Left arrow. Then Escape. Then Escape again.
- The page: "Move to" opens a submenu with Jalandhar, Patiala and Zones. Zones opens another with Doaba and Malwa. Left closes the inner submenu and returns to "Zones". The first Escape closes the open submenu and returns to "Move to". The second Escape closes the menu and returns focus to the card.
- The reader says: "Move to" expanded and "Jalandhar", menu item. On "Zones": has a submenu, collapsed, then expanded with "Doaba" after Right arrow. After Left arrow, "Zones" collapsed. After the first Escape, "Move to" collapsed. After the second, the card "Order actions", group, and no menu.

**5. Choose items**
- Keys: Both readers: Shift+F10, Down arrow four times (to "Flag for follow-up"), Space. Then Shift+F10 again and press Down arrow four times to hear "Flag for follow-up" again. Escape. Then Shift+F10, Down arrow twice (to "Mark as paid"), Enter.
- The page: Space on the check turns it off and closes the menu, and focus returns to the card. "Mark as paid" closes the menu, sends a request and shows the toast "Posted: paid".
- The reader says: the second time "Flag for follow-up" is read as menu item checkbox, not checked. After Enter on "Mark as paid", "Posted: paid" once, as a live announcement, and focus on the card "Order actions".

**6. Tab leaves the menu**
- Keys: Both readers: Shift+F10, then Tab.
- The page: the menu closes and focus moves on to the next control after the card. Focus does not fall to the top of the page.
- The reader says: the name and role of the next control after the menu (a link, a field or a button), never "document" or the page title alone, and the menu is gone.

**7. A menu from a link, and a field that keeps the browser's menu**
- Keys: Both readers: Tab to the link "Open the profile" (in the card "Harleen Kaur"). Press Shift+F10. Press Escape. Then Tab to the field "Notes" and press Shift+F10.
- The page: from the link, a menu "Contact actions" opens with "Call +91 98765 43210" and "Write an email". Escape returns focus to the link. In the field, the page's menu does not open: the browser's own menu is left alone, with copy and paste.
- The reader says: from the link: the menu "Contact actions", menu, and "Call +91 98765 43210", menu item. After Escape, "Open the profile", link. In the field: "Notes", edit, and no "Contact actions" is spoken. On Windows the browser's own context menu (copy and paste) opens; close it with Escape.

### Menu bar

**8. One tab stop**
- Keys: Both readers: Tab to the menu bar "Application", the row File, View, Reports, Admin, Help. Then Tab once more, then Shift+Tab.
- The page: the whole bar is one tab stop. Focus is on "File".
- The reader says: the bar's name "Application" and the role menu bar, then "File", menu item, has a menu, collapsed, 1 of 5. After the second Tab, focus has left the bar (to the next control of the page). Shift+Tab returns to the bar's one stop, "File".

**9. Move along the bar**
- Keys: Both readers: with focus on "File" press Right arrow five times, then Left arrow, then Home, then End.
- The page: the arrows move focus without opening a menu. They go File, View, Reports, Admin, Help, File; Left goes back to Help; Home to File; End to Help. The disabled "Admin" is reached.
- The reader says: at each key, the name, menu item and its place: "View" 2 of 5, "Reports" 3 of 5, "Admin" 4 of 5, unavailable or dimmed, "Help" 5 of 5 (it has no menu, so no "collapsed"), "File" 1 of 5, "Help" 5 of 5, "File" 1 of 5, "Help" 5 of 5. No menu opens, and the reader does not say expanded.

**10. Open a menu**
- Keys: Both readers: Home, then Down arrow. Escape. Then Up arrow. Escape. Then Enter. Escape. Then Space.
- The page: Down arrow opens "File" with focus on its first item, "New invoice". Up arrow opens it with focus on the last item, "Export". Enter and Space open it with the first item focused. Escape closes it and returns focus to "File".
- The reader says: "File" expanded, then "New invoice", menu item (with the shortcut text "Ctrl+N" possibly after it), 1 of 4. After Up arrow, "Export", menu item, has a submenu, collapsed. After each Escape, "File", menu item, collapsed. The menu "File" is named after its button.

**11. Move between menus with one open**
- Keys: Both readers: Escape if a menu is open, then Home, Down arrow (opens "File"), then Right arrow, Right arrow, Right arrow, Left arrow, Left arrow.
- The page: with a menu open, Right and Left move to the next and previous button and open its menu with the first item focused. "Admin" has no menu, so the open menu closes and focus is on "Admin".
- The reader says: after the first Right arrow: "View" expanded and "Show the grid", menu item checkbox, checked. After the second: "Reports" expanded and "Sales in Ludhiana". After the third: "Admin", unavailable, and no menu. After the first Left arrow: "Reports", collapsed (with no menu open an arrow only moves). After the second: "View", collapsed.

**12. A submenu in the bar**
- Keys: Both readers: Home, then Up arrow (to "Export"), Right arrow, Left arrow, Escape.
- The page: Right arrow opens the submenu "Export" with PDF and CSV and focus on "PDF". Left closes only the submenu and returns to "Export" while the "File" menu stays open. Escape closes the menu and returns focus to "File".
- The reader says: "Export" expanded and "PDF", menu item; after Left arrow, "Export", collapsed; after Escape, "File", collapsed.

**13. Checkbox and radio items in the bar**
- Keys: Both readers: Escape if a menu is open, then Home, Right arrow (to "View"), Down arrow, Space. Then Down arrow, Down arrow, Down arrow, Space.
- The page: "Show the grid" is checked at first. Space turns it off and closes the menu, with focus on "View". Down arrow opens the menu again on "Show the grid", two more reach "125%", and Space chooses it and closes the menu.
- The reader says: "Show the grid", menu item checkbox, checked; after Space, the menu closes and "View" is read, collapsed. Then "Show the grid", not checked. For the zoom group: "Zoom" (group), "100%", radio, checked, 1 of 3; "125%", radio, not checked, 2 of 3; after Space, "125%" is the checked one.

**14. A disabled menu**
- Keys: Both readers: move to "Admin" with the arrow keys and press Enter, then Down arrow.
- The page: nothing opens.
- The reader says: "Admin", menu item, unavailable or dimmed, and no change of state and no menu after Enter and Down arrow.

### Navigation menu

**15. Find the navigation**
- Keys: Both readers: Tab to "Products" in the Navigation menu section. (NVDA: D moves between landmarks; VoiceOver: VO+U, then Landmarks.)
- The page: a row of two buttons and two links: Products, Company, Pricing, Docs.
- The reader says: the landmark "Main", navigation, and a list of four items. "Products" is read as a button and collapsed (not as a menu item and not as "menu"). With the reader's cursor (NVDA: browse mode and Down arrow; VoiceOver: VO+Right arrow), "Pricing" and "Docs" are read as links.

**16. Open a panel**
- Keys: Both readers: with focus on "Products" press Enter.
- The page: a panel opens under the button with a card "New: UPI collect" and a list of links: Invoices, Payments, Payroll. Focus stays on the button.
- The reader says: "Products", button, expanded. Nothing is said about a menu. Focus has not moved into the panel.

**17. Walk the panel**
- Keys: Both readers: Tab, Tab, Tab, Tab, then Shift+Tab.
- The page: Tab goes through the links of the panel in order and then on to "Company", closing the panel behind it. Shift+Tab goes back to "Products", with its panel shut.
- The reader says: "Invoices", link, with its description "Send GST invoices from Ludhiana to Kochi in one click." (the description is part of the link's text); "Payments", link, current page, with "Collect by UPI, card or bank transfer."; "Payroll", link; then "Company", button, collapsed. Each link is spoken once. After Shift+Tab: "Products", button, collapsed.

**18. Close a panel with Escape**
- Keys: Both readers: with focus on "Products" press Enter, Tab to "Invoices", then Escape.
- The page: the panel closes and focus returns to the button "Products".
- The reader says: after Escape, "Products", button, collapsed.

**19. Move along the top level**
- Keys: Both readers: with focus on "Products" press Enter, then Right arrow, Right arrow, Right arrow, Right arrow, End, Home.
- The page: Right arrow closes an open panel and moves focus to the next top-level item without opening it: Company, Pricing, Docs, Products. End goes to "Docs", Home to "Products".
- The reader says: after the first Right arrow, "Company", button, collapsed, and the panel of Products is gone. Then "Pricing", link; "Docs", link; "Products", button, collapsed; "Docs", link; "Products", button, collapsed. No panel opens on an arrow key.

**20. The navigation in the current page**
- Keys: Both readers: open "Products" with Enter and read the panel with the reader's cursor (NVDA: NVDA+Space for browse mode, Down arrow; VoiceOver: VO+Right arrow).
- The page: the link "Payments" marks the current page.
- The reader says: the featured card's text "New: UPI collect" and "Ask a customer to pay in seconds." as text, then the three links. Only "Payments" is read as the current page.

### With JavaScript off

**21. Menu bar and navigation menu with JavaScript off**
- Keys: Both readers: turn JavaScript off and reload `/menubar`. Tab through the menu bar and note how many stops there are. Press Enter on "File", then Escape. Tab to "Products" in the Navigation menu section, press Enter, Tab to "Invoices" and press Enter. Turn JavaScript on again afterwards.
- The page: without script every button of the bar is a tab stop (five) and the arrow keys do not move between them. "File" opens its menu natively and Escape closes it. The navigation menu opens its panel and the link goes to its page.
- The reader says: five stops, each read as a menu item. "File" expanded when open. For "Products": button, with expanded when open if the reader says it. After Enter on "Invoices", the new page is "Went somewhere" and the text "You went to invoices." The roles and names are the same as with script.

**22. The context menu with JavaScript off**
- Keys: Both readers: with JavaScript off, Tab to "Order actions" and press Shift+F10 (or the Menu key). Turn JavaScript on again afterwards.
- The page: the card is still a tab stop, because its markup makes it one, but the page's own menu never opens. The browser keeps its own context menu.
- The reader says: "Order actions", group, on arriving. After the key, the browser's own context menu if it opens one (on Windows, items such as copy), and never a menu called "Order actions". Write in the notes that a real page must offer the same actions another way.

## Results

| | |
|---|---|
| Script | menubar-context-navigation.md |
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
| 1 | "Order actions", group, then the card text | | | |
| 2 | Order actions, menu; Open order, menu item | | | |
| 3 | Menu items with states; Refund dimmed; radio 1 of 3; Move to collapsed | | | |
| 4 | Submenu expanded, collapsed; Escape one level at a time | | | |
| 5 | Check not checked after Space; "Posted: paid" once | | | |
| 6 | Menu closes; next control named; focus not lost | | | |
| 7 | Contact actions from a link; field keeps the browser's menu | | | |
| 8 | Application, menu bar; File, 1 of 5; one tab stop | | | |
| 9 | Arrows move with places 1 to 5; Admin unavailable; no menu | | | |
| 10 | File expanded; New invoice or Export first; Escape returns | | | |
| 11 | Right and Left open the next menu; Admin closes the open one | | | |
| 12 | Export expanded; Left closes the submenu only | | | |
| 13 | Checkbox state; radio checked changes; menu closes | | | |
| 14 | Admin unavailable; nothing opens | | | |
| 15 | Main, navigation; Products button collapsed; links | | | |
| 16 | Products expanded; focus stays on the button | | | |
| 17 | Panel links with descriptions; Payments current page | | | |
| 18 | Escape closes; focus on Products, collapsed | | | |
| 19 | Arrows move along; no panel opens; panel closes | | | |
| 20 | Featured text read; only Payments is current | | | |
| 21 | Five stops; File and Products open; link goes | | | |
| 22 | Browser's own context menu; not Order actions | | | |

## Overall result

- [ ] Every step passed in NVDA with Firefox.
- [ ] Every step passed in NVDA with Chrome.
- [ ] Every step passed in VoiceOver with Safari.
- [ ] Every failed step has an issue, with the module name and the step number.
- [ ] Every n/a has a reason in the notes.

Signed off by: ______________________  Date: ______________
