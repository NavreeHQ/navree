# Overlays screen reader script

## Purpose

Dialog, alert dialog, sheet, drawer, popover, tooltip and hover card are layers that open over a page. Each rests on a native element (a `dialog` or a `popover`), so what a reader says depends on the role, the name and the description the markup gives it, on where focus goes when it opens, and on where focus goes when it closes. This script settles those for every overlay, and checks that a tooltip is not spoken twice and that a modal layer hides the page behind it from the reader.

## Page to open

The UI proof app, http://127.0.0.1:8090/gallery (title "Gallery"; see [Start the proof apps](README.md#start-the-proof-apps)). The overlays are in the section "Overlays". Step 16 uses the section "Commands" of the same page. Step 17 uses http://127.0.0.1:8090/select.

## Before you start

- JavaScript on, except steps 18 and 19. Window about 1200 px wide.
- Load `/gallery` fresh. Use Tab to reach the buttons. In the Overlays section the buttons come in this order: Open dialog, Open alert, Open sheet, Open drawer, Open popover, Hover card (a link), Tooltip.
- Speech viewer (NVDA) or caption panel (VoiceOver) visible.
- NVDA: stay in focus mode inside a dialog. VoiceOver: use Tab to move inside a dialog.
- To re-read the focused control: NVDA: NVDA+Tab. VoiceOver: VO+Left arrow, then VO+Right arrow.
- Opening a dialog is not a live announcement. The reader speaks it because focus moves into it: the dialog's name, its role, its description, and then the control that has focus. The words differ between readers; this script lists what must be conveyed.

## Steps

### Dialog

**1. Open a dialog**
- Keys: Both readers: Tab to "Open dialog" and press Enter.
- The page: a dialog opens in the middle of the window. Focus goes to its first field, "Name", and not to the small close button.
- The reader says: the dialog's name "A dialog", the role dialog, its description "With a description", and the focused field: "Name", edit (or text field), empty. The name and the description are each spoken once.

**2. The page behind is out of reach**
- Keys: NVDA: NVDA+Space for browse mode, Ctrl+Home, then Down arrow again and again to the end. VoiceOver: VO+Right arrow again and again, until the reader would wrap round. Then, in both readers, Tab four times, then Shift+Tab once.
- The page: the dialog is modal. The page behind it is inert.
- The reader says: only the dialog's own content is read: the heading "A dialog", "With a description", the field "Name", the buttons "Cancel", "Save" and "Close". Nothing of the page behind (such as the heading "Overlays" or the other buttons) is read. Tab goes through Name, Cancel, Save and Close and then out of the page (to the browser's own controls) or back to Name; it never lands on a button of the page behind.

**3. Close with Escape**
- Keys: Both readers: type a name in the field, then press Escape.
- The page: the dialog closes. Focus goes back to the button "Open dialog".
- The reader says: the focus announcement of "Open dialog", button. The reader does not say the dialog's name again.

**4. Close with the Close button**
- Keys: Both readers: press Enter on "Open dialog", then Tab three times (Cancel, Save, then the last button) and press Enter.
- The page: the last button is an icon with no visible text. It closes the dialog. Focus goes back to "Open dialog".
- The reader says: the button's name "Close" (not "button" alone, and not "x"), and after Enter the focus announcement "Open dialog", button.

### Alert dialog

**5. Open an alert dialog**
- Keys: Both readers: Tab to "Open alert" and press Enter.
- The page: a dialog opens with the title "Delete this deal?" and the text "This can't be undone." Focus is on the button "Cancel". There is no icon close button.
- The reader says: the name "Delete this deal?", the role alert dialog (some readers say "alert dialog"; at least "dialog" must be said), the description "This can't be undone.", and the focused button "Cancel". The description is spoken once, as part of the opening, and not again.

**6. Cancel, Escape and the destructive button**
- Keys: Both readers: press Tab once (to "Delete"), then Shift+Tab, then Escape. Press Enter on "Open alert" again, Tab to "Delete" and press Enter.
- The page: Escape closes the alert dialog and focus returns to "Open alert". "Delete" closes it too, and focus returns to "Open alert". Clicking outside would not close it.
- The reader says: "Delete", button, on Tab and "Cancel", button, on Shift+Tab. After Escape and after Delete: "Open alert", button. Nothing is said about a deletion, because this page deletes nothing.

### Sheet and drawer

**7. Open a sheet**
- Keys: Both readers: Tab to "Open sheet" and press Enter.
- The page: a panel slides in from the right with the title "A sheet" and the text "Sheet content". Focus goes to the footer button "Close", because the panel has no field.
- The reader says: the name "A sheet", the role dialog, and the focused button "Close". There is no description to speak. Read on in browse mode (NVDA) or with VO+Right arrow (VoiceOver): the heading "A sheet", the text "Sheet content", the button "Close" and a second button "Close" (the icon button). Both close the sheet.

**8. Close the sheet, then a drawer**
- Keys: Both readers: press Escape. Tab to "Open drawer", press Enter, read the same way, and press Escape.
- The page: the sheet closes and focus returns to "Open sheet". The drawer has the title "A drawer" and the text "Drawer content", and focus goes to its "Close" button. Escape returns focus to "Open drawer".
- The reader says: after the sheet closes, "Open sheet", button. For the drawer: the name "A drawer", the role dialog, "Close", button; and after Escape, "Open drawer", button.

### Popover

**9. Open a popover**
- Keys: Both readers: Tab to "Open popover" and press Enter.
- The page: a small box opens under the button and takes focus.
- The reader says: on the button before Enter, "Open popover", button, and that it opens a dialog or a popup. After Enter: the popover's name "More" and the role dialog, with focus on it. If the reader says expanded or collapsed for the button, it must say expanded now. The text "Popover content." can be read in browse mode (NVDA) or with VO+Right arrow (VoiceOver).

**10. Close the popover**
- Keys: Both readers: press Escape. Then press Enter on "Open popover" again and press Escape again.
- The page: the popover closes and focus goes back to the button "Open popover".
- The reader says: "Open popover", button, and collapsed if the reader says it. The popover's name is not spoken again.

### Tooltip

**11. A tooltip on focus**
- Keys: Both readers: Tab to "Tooltip" and wait one second without pressing anything.
- The page: after about a third of a second a small label "Says more" appears above the button.
- The reader says: "Tooltip", button, and its description "Says more", once. The description comes from the markup and the label that appears is the same text, so the words "Says more" must be spoken once, not twice. If the reader says them a second time when the label appears, write it in the notes and mark the step failed.

**12. Hide the tooltip**
- Keys: Both readers: press Escape. Press Tab, then Shift+Tab.
- The page: Escape hides the label and focus stays on the button. Tab and Shift+Tab bring the focus back, and the label shows again.
- The reader says: nothing after Escape (focus does not move). On coming back, "Tooltip", button, "Says more", once.

### Hover card

**13. A hover card from the keyboard**
- Keys: Both readers: Tab to the link "Hover card" and wait one second. Then read the next text: NVDA: NVDA+Space for browse mode, Down arrow. VoiceOver: VO+Right arrow.
- The page: after about a third of a second a card with the text "More about this link." opens beside the link. Escape hides it.
- The reader says: "Hover card", link. The card has no role, no name and no description tied to the link, so the reader does not announce it by itself: that is expected. The text "More about this link." comes next in reading order while the link keeps focus (NVDA). With VoiceOver, if its setting "Keyboard focus follows VoiceOver cursor" is on, moving off the link closes the card; write what happened in the notes.

### Accordion and collapsible

**14. An accordion**
- Keys: Both readers: Tab to "First" (the first title of the accordion below the buttons), press Space to close it and Space to open it again, then Tab to "Second" and press Enter. Then Shift+Tab to "First".
- The page: "First" is open at the start. The accordion lets one item be open at a time, so opening "Second" closes "First".
- The reader says: "First", button (or the reader's word for a disclosure), expanded, then collapsed after the first Space and expanded after the second. "Second", collapsed, then expanded after Enter. Back on "First": collapsed. The content "First content" and "Second content" is read only for the item that is open.

**15. A collapsible**
- Keys: Both readers: Tab to "Collapsible" and press Enter.
- The page: the content "Collapsible content" shows below the heading.
- The reader says: "Collapsible", button, collapsed; after Enter, expanded. In browse mode the text "Collapsible content" can be read only when it is open.

### Details in a dialog, and a dialog with pickers

**16. A dialog filled from a table row**
- Keys: Both readers: scroll to the section "Commands" (NVDA: NVDA+F7 and choose the table "Deals", or press T; VoiceOver: VO+U, then Tables). Move to the "Details" button in the row "Gulati expansion" and press Enter. Read the dialog. Press Escape.
- The page: a dialog titled "Deal" opens with the text "Gulati expansion, worth 48,000." Focus goes to its Close button, since it has no field. Escape closes it and focus returns to that "Details" button.
- The reader says: on the button, "Details", button, and its column "Actions" where the reader says column headers. On opening: the name "Deal", the role dialog, and the button "Close". The text "Gulati expansion, worth 48,000." is available by reading on. After Escape, "Details", button. Both rows have a button called "Details". Move along the row with the table keys (NVDA: Ctrl+Alt+Left arrow; VoiceOver: VO+Left arrow inside the table) and write in the notes what you hear: the cells "Gulati expansion" and "48,000" are what tell the rows apart.

**17. Pickers in a dialog**
- Keys: Both readers: open `/select`. Tab to the button "Open the dialog" (in the Command palette section) and press Enter. In the dialog press Down arrow on "Stage", then Escape, then Escape again.
- The page: the dialog "Pickers in a dialog" opens with the description "The popups sit above the dialog; Esc closes the popup first." Focus is on the first field, "Stage". The list of the Stage select opens above the dialog. The first Escape closes only the list, the second closes the dialog and returns focus to "Open the dialog".
- The reader says: on opening, the name "Pickers in a dialog", the role dialog, the description, and "Stage", combobox, collapsed. After Down arrow: expanded and the highlighted option. After the first Escape: collapsed, and the dialog is still there. After the second: "Open the dialog", button.

### With JavaScript off

**18. Dialogs and a popover with JavaScript off**
- Keys: Both readers: turn JavaScript off and reload `/gallery`. Press Enter on "Open dialog", then Escape. Press Enter on "Open popover", then Escape. Turn JavaScript on again afterwards.
- The page: dialogs and popovers open and close with the browser's own behaviour. A browser that does not know the `command` attribute leaves the dialog buttons dead; write the browser and its version in the notes and mark the dialog part n/a.
- The reader says: the same name, role and description as in steps 1 and 9, and focus back on the button after Escape.

**19. Tooltip and hover card with JavaScript off**
- Keys: Both readers: with JavaScript off, Tab to "Tooltip" and then to "Hover card". Turn JavaScript on again afterwards.
- The page: with no script the label and the card never show. The button keeps its link to the label's text.
- The reader says: "Tooltip", button, "Says more", once, even though nothing shows. "Hover card", link. No card is announced.

## Results

| | |
|---|---|
| Script | overlays.md |
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
| 1 | A dialog, dialog, description, Name field; each once | | | |
| 2 | Only dialog content read; Tab never reaches the page | | | |
| 3 | Escape closes; focus on Open dialog | | | |
| 4 | Icon button named Close; focus returns | | | |
| 5 | Delete this deal?, alert dialog, description, Cancel focused | | | |
| 6 | Delete and Cancel buttons; focus returns to Open alert | | | |
| 7 | A sheet, dialog, Close focused; two Close buttons | | | |
| 8 | Sheet and drawer: focus returns to their buttons | | | |
| 9 | Popover named More, dialog, has focus | | | |
| 10 | Escape closes; focus on Open popover | | | |
| 11 | Tooltip button with "Says more" once | | | |
| 12 | Escape hides the label; no extra speech | | | |
| 13 | Hover card link; card not announced; text reachable | | | |
| 14 | Expanded and collapsed states; one open at a time | | | |
| 15 | Collapsible: collapsed, then expanded | | | |
| 16 | Deal dialog; Close focused; row cells identify the button | | | |
| 17 | Dialog with pickers; first Escape closes the list only | | | |
| 18 | Same names without script; focus returns | | | |
| 19 | "Says more" still the description; no card announced | | | |

## Overall result

- [ ] Every step passed in NVDA with Firefox.
- [ ] Every step passed in NVDA with Chrome.
- [ ] Every step passed in VoiceOver with Safari.
- [ ] Every failed step has an issue, with the module name and the step number.
- [ ] Every n/a has a reason in the notes.

Signed off by: ______________________  Date: ______________
