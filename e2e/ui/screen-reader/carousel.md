# Carousel screen reader script

## Purpose

A carousel is a set of slides in a scroller. It has Previous and Next buttons, an optional row of pickers (tabs that go to a slide), and optional automatic rotation with a pause button. This script settles how a reader names the carousel and its slides ("3 of 4"), what it says when a button is used and when a button reaches the end, how the pickers speak as tabs, whether automatic rotation stays quiet, and what a person gets with JavaScript off. The page writes no text when a slide changes, so a step checks that the new slide can be reached and read, not that it is announced.

## Page to open

The UI proof app, http://127.0.0.1:8090/carousel. See [Start the proof apps](README.md#start-the-proof-apps).

The page has nine carousels, each under a heading:

| Heading | Carousel name | What it has |
|---|---|---|
| Buttons | Offers from Ludhiana | Four slides, Previous and Next; the third slide has an "Order now" link |
| Pickers | Offers with pickers | Four slides and a row of pickers named for the slides; a button "Before the carousel" stands above it |
| Going round | Mandi days | Three slides; Next on the last goes to the first |
| Rotating | Festival deals | Three slides, pickers, a pause button, turns every 2 seconds |
| Two in view | Six shops | Six slides, two in view at once |
| Vertical | News from the mandi | Four slides, top to bottom |
| One slide | A single notice | One slide |
| Right to left | an Urdu name | Four slides and pickers, right to left |
| Hostile text | the text `<img src=x onerror=window.__pwned=1>` | Three slides, and a Previous button named `<b>Back</b>` |

## Before you start

- JavaScript on (step 20 turns it off; turn it on again afterwards). Window at least 1200 px wide, browser zoom 100%.
- Do not use the mouse on the page. Keep the pointer at the edge of the screen: a pointer over a carousel holds its rotation.
- Check that your system is not set to reduce motion. If it is, the "Festival deals" carousel does not rotate and steps 12 and 15 cannot be run.
- Open the speech viewer (NVDA) or caption panel (VoiceOver).
- Reader mode: browse mode to read slides, focus on the controls with Tab. Pressing Tab in browse mode moves focus to the next control after the reading cursor.
- Slides move with a smooth scroll that takes about half a second. After a button press, wait one second before you read the next state.
- Reload the page before you begin.

## Steps

### Finding the carousels

**1. List the carousels**
- Keys: NVDA: NVDA+F7, choose Landmarks. VoiceOver: VO+U, then the Left and Right arrows to Landmarks.
- The page: each carousel is a region with a name.
- The reader says: among the page's other landmarks (such as navigation and main), nine carousels, named as in the table above, in that order. For each, the reader conveys the name and the word "carousel" (the page gives the region the role description "carousel"). The extra word "region" or "landmark" is allowed; the word "carousel" must be there.

### Buttons

**2. Reach the Buttons carousel**
- Keys: NVDA: H until the heading "Buttons", then Tab. VoiceOver: VO+U, Headings, choose "Buttons", then VO+Right to the heading and Tab.
- The page: the first controls of the carousel are its two buttons. They come before the slides.
- The reader says: first "Previous slide", button, and that it is unavailable (NVDA "unavailable"; VoiceOver "dimmed"), because the carousel is on its first slide. Tab again: "Next slide", button, available. Tab again: "Slides", group. The names come from the page and are spoken once each.

**3. Move to the next slide**
- Keys: Both readers: Shift+Tab to "Next slide" and press Enter (VoiceOver: VO+Space), then wait one second.
- The page: the track scrolls to slide 2. Focus stays on "Next slide".
- The reader says: nothing is required to be spoken for the move. The page writes no text for it, so silence is expected; the reader may say "Next slide, button" again. Then Shift+Tab to "Previous slide": it is now available, and the reader no longer says "unavailable" or "dimmed". If it still does, that is a failure.

**4. Read the slides**
- Keys: NVDA: Tab to "Slides", then Down arrow in browse mode. VoiceOver: VO+Right.
- The page: all four slides are in the page and none is hidden, so they can all be read, in order.
- The reader says: for each slide, its role description "slide" and its name "1 of 4", "2 of 4", "3 of 4", "4 of 4", then its content: a heading, such as "Phulkari dupatta", and a line, such as "Hand-embroidered in Patiala. ₹2,499." The name "n of 4" is said once per slide, not twice.

**5. Reach the end, and press a button that is unavailable**
- Keys: Both readers: Tab back to "Next slide", press Enter three times (waiting one second between), then press Enter once more.
- The page: the track ends at slide 4. "Next slide" becomes unavailable and keeps focus. The last press does nothing.
- The reader says: nothing is required while pressing. Then Shift+Tab and Tab again to re-read the button: "Next slide, button, unavailable" (NVDA) or "dimmed" (VoiceOver). The extra press makes no sound from the page and moves nothing.

**6. Focus inside a slide that is out of view**
- Keys: Both readers: press Previous slide until slide 1 shows (or reload). Tab from "Slides" into the slides.
- The page: the third slide has a link "Order now". When it takes focus, the track scrolls to bring slide 3 into view.
- The reader says: the link "Order now", role link, and the slide it is in: "slide", "3 of 4" (a reader reports the enclosing group when focus enters it). The step fails if the link is announced with no way to tell which slide holds it, or if "Order now" is never reached by Tab.

### Pickers

**7. Reach the pickers**
- Keys: NVDA: H to the heading "Pickers", then Tab until the reader says "Phulkari dupatta". VoiceOver: VO+U, Headings, "Pickers", Tab.
- The page: Tab order is "Before the carousel", "Previous slide", "Next slide", "Slides", then one picker. Only the picker of the open slide is a tab stop.
- The reader says: on the picker, the group name "Choose a slide" (tab list), then "Phulkari dupatta", tab, selected. The reader may add a position such as "1 of 4"; record it. The name is the slide's title, not "Slide 1".

**8. Use the picker keys**
- Keys: Both readers: Right arrow; then End; then Right arrow; then Left arrow; then Home. Wait half a second between keys.
- The page: Right moves focus to the next picker and opens its slide. End goes to the last. Right on the last goes round to the first. Left on the first goes to the last. Home goes to the first. The address does not change.
- The reader says: after each key, the picker that now has focus: "Basmati rice, 5 kg", then "Pashmina shawl", then "Phulkari dupatta", then "Pashmina shawl", then "Phulkari dupatta". Each is spoken as a tab and as selected.

**9. Read a slide that a picker opened**
- Keys: NVDA: Home to open the first picker, then in browse mode move the cursor with the arrow keys until you reach the slide "Phulkari dupatta" (it is the tab panel). VoiceOver: VO+Right through the slides.
- The page: with pickers, each slide is a tab panel named by its picker.
- The reader says: the slide's role description "slide" and its name, which is the title of the picker ("Phulkari dupatta", "Basmati rice, 5 kg" and so on), not "1 of 4".

### Going round

**10. Next on the last slide**
- Keys: NVDA: H to "Going round", Tab to "Previous slide". VoiceOver: VO+U, Headings, "Going round", Tab.
- The page: this carousel goes round. On the first slide "Previous slide" still works. Next on the last slide goes to the first.
- The reader says: "Previous slide" and "Next slide" are never unavailable. Press Next three times: the third press returns to slide 1. Press Previous once: it goes to slide 3. The buttons say nothing about the move.

### Rotating

**11. The pause button**
- Keys: NVDA: H to "Rotating", Tab. VoiceOver: VO+U, Headings, "Rotating", Tab.
- The page: the first stop in this carousel is the pause button, before Previous and Next.
- The reader says: "Pause automatic rotation", button. Tab again: "Previous slide", then "Next slide", then "Slides", then a picker named "Slide 1", "Slide 2" or "Slide 3" as a tab, selected.

**12. Rotation is silent**
- Keys: Both readers: press Tab once to leave the "Festival deals" carousel (focus goes to the "Two in view" carousel). NVDA: H (or Shift+H) to move the cursor onto the heading "Rotating", and press nothing else. VoiceOver: put the VO cursor on that heading. Wait ten seconds.
- The page: focus is outside the carousel and the pointer is away, so the slides turn about every 2 seconds and the pickers change which one is selected. The track is not a live region while it rotates.
- The reader says: nothing during the ten seconds. If the reader speaks the slide content or a picker change while you wait, that is a failure. If you can see the screen, confirm that the slides do turn; otherwise Tab into the picker twice at different times and notice that the selected one differs.

**13. Stop the rotation**
- Keys: Both readers: Tab to "Pause automatic rotation" and press Enter (VoiceOver: VO+Space). Then Shift+Tab and Tab to re-read the button. Then Tab four times to the picker and note which "Slide n" is selected (the four presses are Previous, Next, Slides, the picker). Tab once more to leave the carousel, wait six seconds, and Shift+Tab back to the picker.
- The page: the button's name changes to "Start automatic rotation". The rotation stops. The same picker is selected before and after the six seconds.
- The reader says: on the re-read, "Start automatic rotation", button. The name change may or may not be spoken at the moment of the press; write down what you heard. The picker you return to says the same "Slide n", tab, selected as before.

**14. Start the rotation again**
- Keys: Both readers: Shift+Tab four times from the picker to the pause button (now "Start automatic rotation") and press Enter. Shift+Tab, then Tab to re-read it.
- The page: the rotation resumes when focus and the pointer are away.
- The reader says: "Pause automatic rotation", button, on the re-read.

**15. Focus inside holds the rotation**
- Keys: Both readers: Tab four times to the picker and note the "Slide n" that is selected. Wait six seconds with focus still in the carousel, then Shift+Tab and Tab to re-read the picker.
- The page: rotation does not turn the slides while focus is in the carousel.
- The reader says: the same "Slide n", tab, selected as before the wait.

### Other layouts

**16. Two in view, and vertical**
- Keys: NVDA: H to "Two in view", Tab. Then H to "Vertical", Tab. VoiceOver: use the rotor as in step 2.
- The page: "Six shops" shows two slides at once and has no pickers. "News from the mandi" runs top to bottom. Both have Previous and Next buttons.
- The reader says: for "Six shops": "Previous slide" (unavailable at the start), "Next slide", then "Slides"; no tab list. Reading the slides: "1 of 6" up to "6 of 6". After Next, "Previous slide" is available. For "News from the mandi": the same two button names (not "Up" or "Down"), and slides "1 of 4" up to "4 of 4".

**17. One slide**
- Keys: NVDA: H to "One slide", Tab. VoiceOver: use the rotor as in step 2.
- The page: with one slide there is nothing to move to, and no pickers are drawn.
- The reader says: "Previous slide", button, unavailable; "Next slide", button, unavailable; "Slides", group; the slide "1 of 1" with "Closed on Gurpurab". No tab list.

**18. Right to left**
- Keys: NVDA: H to "Right to left", Tab to the picker. Both readers: press the Left arrow, then the Right arrow.
- The page: the first slide is at the right. In a right-to-left page the arrow that goes on is Left.
- The reader says: the Urdu names are spoken if the voice has Urdu; otherwise mark the names n/a in the notes. The structure is checked either way: a tab list of four tabs, the first selected. Left moves selection to the second tab; Right moves it back to the first.

**19. Hostile text stays text**
- Keys: NVDA: H to "Hostile text", Tab. VoiceOver: use the rotor as in step 2.
- The page: the carousel's name is the text `<img src=x onerror=window.__pwned=1>` and the Previous button is named `<b>Back</b>`. Neither becomes markup.
- The reader says: the carousel name and the first button name are the text itself (a reader may drop the symbols, depending on its punctuation level). It does not say "graphic" or "image", and the button is not simply "Back". Note what you heard.

### JavaScript off

**20. Pickers and track without script**
- Keys: Turn JavaScript off in the browser and reload `/carousel`. NVDA: H to "Pickers", Tab. VoiceOver: use the rotor as in step 2. Then Tab through the carousel, and press Enter on the third picker.
- The page: the Previous, Next and pause buttons are not shown. The track can be focused and scrolled with the arrow keys. The pickers are links to the slides, and every one can be reached by Tab. Following the third picker scrolls the track to slide 3. The picker states are written by the server and are not updated without script.
- The reader says: no "Previous slide" or "Next slide" anywhere. "Slides", group, is reachable by Tab. Each picker is spoken as a tab with its slide title ("Phulkari dupatta", "Basmati rice, 5 kg", "Desi ghee, 1 kg", "Pashmina shawl"); Tab reaches all four, not only one. After Enter on the third, the reader moves to or reads slide 3 ("Desi ghee, 1 kg"). Write down which picker the reader calls selected after the move; without script only the first is marked, and that is not a failure. Turn JavaScript on again.

## Results

| | |
|---|---|
| Script | carousel.md |
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
| 1 | Nine carousels listed by name, each called "carousel" | | | |
| 2 | Previous slide unavailable; Next slide; Slides group | | | |
| 3 | Move is silent; Previous slide is now available | | | |
| 4 | Slides: role "slide", "1 of 4" to "4 of 4", content, said once | | | |
| 5 | Next slide unavailable at the end, keeps focus; extra press does nothing | | | |
| 6 | Order now link reached; slide 3 of 4 conveyed | | | |
| 7 | Tab list "Choose a slide"; tab named by title, selected | | | |
| 8 | Arrow, End, Home keys move tabs; each new tab selected | | | |
| 9 | Slide panel named by its picker title | | | |
| 10 | Buttons never unavailable; Next on last goes to first | | | |
| 11 | Pause automatic rotation first; then buttons, track, Slide n tab | | | |
| 12 | Nothing spoken for ten seconds while it rotates | | | |
| 13 | Start automatic rotation; same picker after six seconds | | | |
| 14 | Pause automatic rotation again | | | |
| 15 | Same Slide n selected after six seconds with focus inside | | | |
| 16 | Six shops: no tabs, 1 of 6 to 6 of 6; vertical: same button names | | | |
| 17 | Both buttons unavailable; slide 1 of 1; no tabs | | | |
| 18 | Four tabs; Left selects the second, Right the first | | | |
| 19 | Hostile names spoken as text; no image or bold | | | |
| 20 | No buttons; all four picker links reached; slide 3 read | | | |

## Overall result

- [ ] Every step passed in NVDA with Firefox.
- [ ] Every step passed in NVDA with Chrome.
- [ ] Every step passed in VoiceOver with Safari.
- [ ] Every failed step has an issue, with the module name and the step number.
- [ ] Every n/a has a reason in the notes.

Signed off by: ______________________  Date: ______________

Reviewed by: ______________________  Date: ______________
